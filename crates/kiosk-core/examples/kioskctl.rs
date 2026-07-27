//! kioskctl — fleet config signing tool (smoke/ops use).
//!
//! Signs a remote-config JSON with the SAME recipe `signature::verify_signed` checks:
//! JCS-canonicalize the object WITHOUT `sig`, Ed25519-sign those bytes, insert
//! `sig = "ed25519:" + base64(sig64)`. Reuses kiosk-core's own crypto path, so a
//! doc it signs is byte-compatible with the on-device verifier by construction.
//!
//!   cargo run -p kiosk-core --example kioskctl -- keygen
//!     → prints KIOSK_CONFIG_PUBKEY_B64 (bake into the build) + the private seed
//!   KIOSK_SIGNING_KEY_B64=<seed> \
//!     cargo run -p kiosk-core --example kioskctl -- sign config.json > signed.json
//!   cargo run -p kiosk-core --example kioskctl -- selftest    # keygen→sign→verify roundtrip
//!
//! ponytail: hand-rolled args (matches the project's no-clap convention); Linux-runnable
//! (no Tauri), so it is the one smoke-harness piece verifiable off a Windows host.

use base64::Engine as _;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

fn seed_from_b64(b64: &str) -> [u8; 32] {
    B64.decode(b64.trim())
        .expect("signing key is not valid base64")
        .as_slice()
        .try_into()
        .expect("signing key must be 32 bytes")
}

/// The signing recipe. Mirrors `signature.rs`'s test `sign` (and thus `verify_signed`).
fn sign_doc(doc: &Value, sk: &SigningKey) -> Value {
    let mut obj = doc
        .as_object()
        .expect("config root must be a JSON object")
        .clone();
    obj.remove("sig");
    // Required by verify_signed / §5.2 — fail early rather than ship an unverifiable config.
    assert!(
        obj.contains_key("revision"),
        "config must carry `revision` (inside the signed payload)"
    );
    assert!(
        obj.contains_key("device_id"),
        "config must carry `device_id` (device binding, §8/SEC-11)"
    );
    let canonical =
        serde_jcs::to_string(&Value::Object(obj.clone())).expect("JCS canonicalization");
    let sig = sk.sign(canonical.as_bytes());
    obj.insert(
        "sig".into(),
        Value::from(format!("ed25519:{}", B64.encode(sig.to_bytes()))),
    );
    Value::Object(obj)
}

fn keygen() -> (SigningKey, String, String) {
    use rand::RngCore;
    let mut seed = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    (
        sk.clone(),
        B64.encode(seed),
        B64.encode(sk.verifying_key().to_bytes()),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let (_sk, seed_b64, pub_b64) = keygen();
            eprintln!(
                "# PRIVATE signing seed — keep secret, never commit, never bake into the binary:"
            );
            println!("KIOSK_SIGNING_KEY_B64={seed_b64}");
            eprintln!("# PUBLIC pinned key — bake into the build:");
            println!("KIOSK_CONFIG_PUBKEY_B64={pub_b64}");
        }
        Some("sign") => {
            let path = args
                .get(1)
                .expect("usage: sign <config.json>  (seed in KIOSK_SIGNING_KEY_B64)");
            let seed = std::env::var("KIOSK_SIGNING_KEY_B64")
                .expect("set KIOSK_SIGNING_KEY_B64 (from keygen)");
            let sk = SigningKey::from_bytes(&seed_from_b64(&seed));
            let doc: Value =
                serde_json::from_str(&std::fs::read_to_string(path).expect("read config"))
                    .expect("config is not valid JSON");
            println!(
                "{}",
                serde_json::to_string_pretty(&sign_doc(&doc, &sk)).unwrap()
            );
        }
        Some("selftest") => {
            let (sk, _seed, pub_b64) = keygen();
            let doc = serde_json::json!({ "revision": 42, "device_id": "lobby-01",
                "content": { "url": "https://app.example.com/kiosk" } });
            let signed = sign_doc(&doc, &sk);
            let vk =
                kiosk_core::config::signature::VerifyingKey::from_bytes(&seed_from_b64(&pub_b64))
                    .expect("pubkey");
            let rev = kiosk_core::config::signature::verify_signed(&signed, &vk)
                .expect("signed doc must verify against its own key");
            assert_eq!(rev, 42, "verify_signed must return the signed revision");
            // Tamper → must fail.
            let mut bad = signed.as_object().unwrap().clone();
            bad.insert("revision".into(), Value::from(43));
            assert!(
                kiosk_core::config::signature::verify_signed(&Value::Object(bad), &vk).is_err(),
                "a tampered body must fail verification"
            );
            println!("selftest OK — sign→verify_signed roundtrip green, tamper rejected");
        }
        _ => {
            eprintln!("usage: kioskctl <keygen|sign <config.json>|selftest>");
            std::process::exit(2);
        }
    }
}
