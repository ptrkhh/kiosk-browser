//! Config integrity (spec §8/SEC-11): detached Ed25519 over the RFC 8785 (JCS)
//! canonicalization of the config object with `sig` removed, verified against a
//! pinned public key baked into the binary. GCS IAM is access control, not
//! authenticity — this is what stops a bucket-write attacker owning the fleet.

use crate::error::ConfigError;
use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier};
use serde_json::Value;

pub use ed25519_dalek::VerifyingKey;

const SIG_PREFIX: &str = "ed25519:";

fn sig_err(msg: impl Into<String>) -> ConfigError {
    ConfigError::Signature(msg.into())
}

/// Verify the detached signature and return the document's `revision`.
/// Both `sig` and `revision` are REQUIRED on every fetched config (spec §5.2).
pub fn verify_signed(raw: &Value, key: &VerifyingKey) -> Result<i64, ConfigError> {
    let obj = raw
        .as_object()
        .ok_or_else(|| sig_err("config root is not a JSON object"))?;

    let sig_str = obj
        .get("sig")
        .and_then(Value::as_str)
        .ok_or_else(|| sig_err("missing required field `sig`"))?;

    let b64 = sig_str
        .strip_prefix(SIG_PREFIX)
        .ok_or_else(|| sig_err(format!("signature must start with `{SIG_PREFIX}`")))?;

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| sig_err(format!("signature is not valid base64: {e}")))?;
    let sig_bytes: [u8; 64] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| sig_err(format!("signature must be 64 bytes, got {}", bytes.len())))?;
    let signature = Signature::from_bytes(&sig_bytes);

    // Canonicalize the document WITHOUT `sig` (RFC 8785 JCS).
    let mut unsigned = obj.clone();
    unsigned.remove("sig");
    let canonical = serde_jcs::to_string(&Value::Object(unsigned))
        .map_err(|e| sig_err(format!("JCS canonicalization failed: {e}")))?;

    key.verify(canonical.as_bytes(), &signature)
        .map_err(|_| sig_err("signature does not verify against the pinned key"))?;

    // A signed document without a revision cannot be anti-rollback checked, so it is
    // rejected here rather than silently applied.
    obj.get("revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| sig_err("missing required field `revision`"))
}

/// The pinned public key, baked in at build time. Fails closed when absent: a build
/// with no pinned key rejects every fetched config (spec §8).
pub fn pinned_key() -> Result<VerifyingKey, ConfigError> {
    let b64 = option_env!("KIOSK_CONFIG_PUBKEY_B64").ok_or_else(|| {
        sig_err("no pinned public key compiled in (set KIOSK_CONFIG_PUBKEY_B64 at build)")
    })?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| sig_err(format!("pinned key is not valid base64: {e}")))?;
    let key_bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| sig_err(format!("pinned key must be 32 bytes, got {}", bytes.len())))?;
    VerifyingKey::from_bytes(&key_bytes)
        .map_err(|e| sig_err(format!("pinned key is not a valid Ed25519 key: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Deterministic test key — no RNG needed, so tests are reproducible.
    fn test_keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    /// Sign a document the way the fleet signing tool must: JCS-canonicalize the
    /// object WITHOUT `sig`, sign those bytes, then insert `sig`.
    fn sign(doc: &Value, sk: &SigningKey) -> Value {
        let mut unsigned = doc.clone();
        unsigned.as_object_mut().unwrap().remove("sig");
        let canonical = serde_jcs::to_string(&unsigned).expect("jcs");
        let sig = sk.sign(canonical.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let mut signed = unsigned;
        signed
            .as_object_mut()
            .unwrap()
            .insert("sig".to_string(), Value::String(format!("ed25519:{b64}")));
        signed
    }

    #[test]
    fn accepts_a_correctly_signed_document_and_returns_its_revision() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 42, "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        let rev = verify_signed(&signed, &vk).expect("must verify");
        assert_eq!(rev, 42);
    }

    #[test]
    fn signature_survives_key_reordering_that_is_jcs_canonicalization_working() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 7, "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        // Re-serialize through a BTreeMap-ish round trip to shuffle key order.
        let reordered: Value =
            serde_json::from_str(&serde_json::to_string(&signed).unwrap()).unwrap();
        assert_eq!(verify_signed(&reordered, &vk).unwrap(), 7);
    }

    #[test]
    fn rejects_a_tampered_body() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1, "content": { "url": "https://good/" } });
        let mut signed = sign(&doc, &sk);
        signed["content"]["url"] = Value::String("https://evil/".to_string());
        let err = verify_signed(&signed, &vk).expect_err("tampered body must fail");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
    }

    #[test]
    fn rejects_a_missing_signature() {
        let (_, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1 });
        let err = verify_signed(&doc, &vk).expect_err("unsigned must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_a_signature_from_the_wrong_key() {
        let (sk, _) = test_keys();
        let other_vk = SigningKey::from_bytes(&[9u8; 32]).verifying_key();
        let doc = serde_json::json!({ "revision": 1 });
        let signed = sign(&doc, &sk);
        let err = verify_signed(&signed, &other_vk).expect_err("wrong key must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_a_missing_revision_even_when_signed() {
        // spec §5.2: sig AND revision are both required on every fetched config.
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "content": { "url": "https://a/" } });
        let signed = sign(&doc, &sk);
        let err = verify_signed(&signed, &vk).expect_err("missing revision must fail");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
    }

    #[test]
    fn rejects_a_malformed_sig_prefix() {
        let (sk, vk) = test_keys();
        let doc = serde_json::json!({ "revision": 1 });
        let mut signed = sign(&doc, &sk);
        signed["sig"] = Value::String("rsa:AAAA".to_string());
        let err = verify_signed(&signed, &vk).expect_err("wrong algorithm must fail");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    /// Operator helper: `cargo test -p kiosk-core print_signing_keypair -- --ignored --nocapture`
    /// prints a fresh keypair. The PUBLIC half goes into KIOSK_CONFIG_PUBKEY_B64 at
    /// build time; the PRIVATE half stays in the signing service, never on a device.
    #[test]
    #[ignore]
    fn print_signing_keypair() {
        let sk = SigningKey::from_bytes(&[42u8; 32]);
        let b64 = base64::engine::general_purpose::STANDARD;
        println!("PRIVATE (seed, keep secret): {}", b64.encode(sk.to_bytes()));
        println!(
            "KIOSK_CONFIG_PUBKEY_B64={}",
            b64.encode(sk.verifying_key().to_bytes())
        );
    }
}
