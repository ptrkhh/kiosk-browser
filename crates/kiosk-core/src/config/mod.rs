//! Configuration subsystem (spec §5, §8).
//!
//! Validation order for a fetched config is fixed and non-negotiable:
//! parse (strict JSON) → signature → anti-rollback → schema/ranges → persist → adopt.
//! A failure at any step leaves the currently-running config untouched.

pub mod bootstrap;
pub mod schema;
pub mod signature;
pub mod store;
pub mod validate;

use crate::error::ConfigError;
use crate::identity::{expand_device_id_template, is_opaque_guid};
use schema::RemoteConfig;
use signature::VerifyingKey;

/// Where the currently-running config came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    LastGood,
    Bootstrap,
    Fetched,
}

/// The outcome of a boot or an apply — what the caller logs as
/// `config.applied` (spec §6).
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub config: RemoteConfig,
    pub revision: Option<i64>,
    pub warnings: Vec<String>,
    pub source: Source,
}

pub struct ConfigManager {
    bootstrap: bootstrap::BootstrapConfig,
    device_id: String,
    store: store::ConfigStore,
    /// `None` ⇒ no pinned key compiled in ⇒ every fetched config is rejected
    /// (fail closed, spec §8).
    key: Option<VerifyingKey>,
    current: RemoteConfig,
    revision: Option<i64>,
}

impl ConfigManager {
    /// BOOT (spec §5.2): apply `config-lastgood.json` if present — it is used directly
    /// when the network is down — else fall back to the bootstrap url. Last-good is NOT
    /// re-signature-checked (it was verified when first applied) but IS re-validated for
    /// schema/ranges and clamped, so a legacy document can never disable polling.
    pub fn boot(
        bootstrap: bootstrap::BootstrapConfig,
        device_id: String,
        store: store::ConfigStore,
        key: Option<VerifyingKey>,
    ) -> (ConfigManager, Applied) {
        let (mut config, revision, mut warnings, source) = match store.load_last_good() {
            Some(stored) => match serde_json::from_value::<RemoteConfig>(stored.raw) {
                Ok(cfg) => {
                    let warnings = validate::validate(&cfg).unwrap_or_else(|e| {
                        vec![format!(
                            "last-good failed re-validation, using it anyway: {e}"
                        )]
                    });
                    (cfg, Some(stored.revision), warnings, Source::LastGood)
                }
                Err(e) => (
                    RemoteConfig::default(),
                    None,
                    vec![format!(
                        "last-good is unreadable, falling back to bootstrap: {e}"
                    )],
                    Source::Bootstrap,
                ),
            },
            None => (RemoteConfig::default(), None, Vec::new(), Source::Bootstrap),
        };

        validate::clamp_effective(&mut config);
        warnings.extend(Self::identity_warnings(&config, &device_id));

        let m = ConfigManager {
            bootstrap,
            device_id,
            store,
            key,
            current: config.clone(),
            revision,
        };
        let applied = Applied {
            config,
            revision,
            warnings,
            source,
        };
        (m, applied)
    }

    /// REFETCH (spec §5.2): validate a downloaded body in strict order and, only if every
    /// step passes, persist it as the new last-good and adopt it. On ANY failure the
    /// currently-running config is untouched.
    pub fn apply_fetched(&mut self, body: &[u8]) -> Result<Applied, ConfigError> {
        // 0. strict JSON (cfg-13 — comments are not accepted)
        let raw: serde_json::Value = serde_json::from_slice(body)
            .map_err(|e| ConfigError::Parse(format!("config is not strict JSON: {e}")))?;

        // 1. signature (returns the revision inside the signed payload)
        let key = self.key.as_ref().ok_or_else(|| {
            ConfigError::Signature(
                "no pinned public key compiled in — refusing every fetched config".to_string(),
            )
        })?;
        let revision = signature::verify_signed(&raw, key)?;

        // 2. anti-rollback
        let last = self.store.last_applied_revision();
        if revision <= last {
            return Err(ConfigError::Rollback {
                got: revision,
                last,
            });
        }

        // 3. schema & ranges (whole-document)
        let config: RemoteConfig = serde_json::from_value(raw.clone())
            .map_err(|e| ConfigError::Parse(format!("config does not match the schema: {e}")))?;
        let mut warnings = validate::validate(&config)?;

        // 4. persist, then adopt
        self.store.save_last_good(&raw, revision)?;

        let mut effective = config;
        validate::clamp_effective(&mut effective);
        warnings.extend(Self::identity_warnings(&effective, &self.device_id));

        self.current = effective.clone();
        self.revision = Some(revision);

        Ok(Applied {
            config: effective,
            revision: Some(revision),
            warnings,
            source: Source::Fetched,
        })
    }

    /// The URL to navigate to: `content.url` with `{device_id}` expanded, or the
    /// `[bootstrap]` url when the config supplies none (spec cfg-05).
    pub fn home_url(&self) -> String {
        match self.current.content.url.as_deref() {
            Some(url) => expand_device_id_template(url, &self.device_id),
            None => self.bootstrap.bootstrap_url.clone(),
        }
    }

    pub fn current(&self) -> &RemoteConfig {
        &self.current
    }

    pub fn revision(&self) -> Option<i64> {
        self.revision
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn bootstrap(&self) -> &bootstrap::BootstrapConfig {
        &self.bootstrap
    }

    /// cfg-09: templating with an opaque machine GUID makes fleet triage painful.
    fn identity_warnings(config: &RemoteConfig, device_id: &str) -> Vec<String> {
        let mut w = Vec::new();
        let uses_template = config
            .content
            .url
            .as_deref()
            .is_some_and(|u| u.contains("{device_id}"));
        if uses_template && is_opaque_guid(device_id) {
            w.push(format!(
                "content.url uses {{device_id}} but it resolves to an opaque GUID ({device_id}) \
                 — set [kiosk] device_id to a human-readable name"
            ));
        }
        w
    }
}

#[cfg(test)]
mod manager_tests {
    use super::*;
    use crate::config::signature::VerifyingKey;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::Value;

    fn keys() -> (SigningKey, VerifyingKey) {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    fn sign(doc: &Value, sk: &SigningKey) -> Vec<u8> {
        use base64::Engine as _;
        let mut unsigned = doc.clone();
        unsigned.as_object_mut().unwrap().remove("sig");
        let canonical = serde_jcs::to_string(&unsigned).unwrap();
        let sig = sk.sign(canonical.as_bytes());
        let b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());
        let mut signed = unsigned;
        signed
            .as_object_mut()
            .unwrap()
            .insert("sig".into(), Value::String(format!("ed25519:{b64}")));
        serde_json::to_vec(&signed).unwrap()
    }

    fn boot_config() -> bootstrap::BootstrapConfig {
        bootstrap::BootstrapConfig {
            config_url: "https://example.com/c.json".into(),
            device_id: Some("lobby-01".into()),
            site: "hq".into(),
            region: None,
            project_id: "proj".into(),
            credential: "cred.json".into(),
            startup_grace_s: 90,
            healthy_run_s: 120,
            channel_grace_s: 30,
            demo_mode: false,
            bootstrap_url: "https://boot.example.com/".into(),
            exit_gesture: None,
        }
    }

    fn manager(dir: &std::path::Path, key: Option<VerifyingKey>) -> (ConfigManager, Applied) {
        ConfigManager::boot(
            boot_config(),
            "lobby-01".to_string(),
            store::ConfigStore::new(dir),
            key,
        )
    }

    #[test]
    fn boots_on_bootstrap_url_when_no_last_good_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::Bootstrap);
        assert_eq!(applied.revision, None);
        assert_eq!(m.home_url(), "https://boot.example.com/");
    }

    #[test]
    fn applies_a_valid_signed_config_and_persists_it() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(
            &serde_json::json!({
                "revision": 5,
                "content": { "url": "https://app.example.com/k?d={device_id}" }
            }),
            &sk,
        );
        let applied = m.apply_fetched(&body).expect("must apply");
        assert_eq!(applied.source, Source::Fetched);
        assert_eq!(applied.revision, Some(5));
        assert_eq!(m.home_url(), "https://app.example.com/k?d=lobby-01");

        // Persisted: a fresh manager on the same dir boots from last-good.
        let (m2, applied2) = manager(dir.path(), Some(vk));
        assert_eq!(applied2.source, Source::LastGood);
        assert_eq!(applied2.revision, Some(5));
        assert_eq!(m2.home_url(), "https://app.example.com/k?d=lobby-01");
    }

    #[test]
    fn rejects_an_unsigned_config_and_keeps_current() {
        let dir = tempfile::tempdir().unwrap();
        let (_, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = serde_json::to_vec(
            &serde_json::json!({ "revision": 5, "content": { "url": "https://evil/" } }),
        )
        .unwrap();
        let err = m
            .apply_fetched(&body)
            .expect_err("unsigned must be rejected");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
        assert_eq!(
            m.home_url(),
            "https://boot.example.com/",
            "current must survive"
        );
    }

    #[test]
    fn rejects_a_replayed_or_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let good = sign(&serde_json::json!({ "revision": 5, "content": {} }), &sk);
        m.apply_fetched(&good).expect("rev 5 applies");

        let stale = sign(&serde_json::json!({ "revision": 5, "content": {} }), &sk);
        let err = m
            .apply_fetched(&stale)
            .expect_err("rev <= last must be rejected");
        match err {
            ConfigError::Rollback { got, last } => {
                assert_eq!(got, 5);
                assert_eq!(last, 5);
            }
            other => panic!("expected Rollback, got {other:?}"),
        }
    }

    #[test]
    fn rejects_out_of_range_values_whole_document_and_keeps_current() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(
            &serde_json::json!({
                "revision": 9,
                "content": { "url": "https://app/", "zoom": 99.0 },
                "network": { "config_poll_s": 0 }
            }),
            &sk,
        );
        let err = m
            .apply_fetched(&body)
            .expect_err("out of range must be rejected");
        match err {
            ConfigError::Invalid {
                errors,
                rejected_revision,
            } => {
                assert_eq!(rejected_revision, Some(9));
                assert!(errors.iter().any(|e| e.field == "content.zoom"));
                assert!(errors.iter().any(|e| e.field == "network.config_poll_s"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
        assert_eq!(
            m.home_url(),
            "https://boot.example.com/",
            "current must survive"
        );
    }

    #[test]
    fn a_signed_empty_body_is_valid_and_falls_back_to_the_bootstrap_url() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let body = sign(&serde_json::json!({ "revision": 1 }), &sk);
        let applied = m.apply_fetched(&body).expect("signed {} is schema-valid");
        assert_eq!(applied.revision, Some(1));
        assert_eq!(
            m.home_url(),
            "https://boot.example.com/",
            "content.url absent => bootstrap"
        );
    }

    #[test]
    fn without_a_pinned_key_every_fetch_is_rejected_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, _) = keys();
        let (mut m, _) = manager(dir.path(), None); // no pinned key compiled in

        let body = sign(&serde_json::json!({ "revision": 1, "content": {} }), &sk);
        let err = m.apply_fetched(&body).expect_err("no key => reject");
        assert!(matches!(err, ConfigError::Signature(_)));
    }

    #[test]
    fn rejects_comments_in_the_fetched_body_strict_json() {
        let dir = tempfile::tempdir().unwrap();
        let (_, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));
        let err = m
            .apply_fetched(b"{ /* comment */ \"revision\": 1 }")
            .expect_err("comments are not strict JSON");
        assert!(matches!(err, ConfigError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn warns_when_device_id_template_resolves_to_an_opaque_guid() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = ConfigManager::boot(
            boot_config(),
            "6f9619ff-8b86-d011-b42d-00c04fc964ff".to_string(),
            store::ConfigStore::new(dir.path()),
            Some(vk),
        );
        let body = sign(
            &serde_json::json!({ "revision": 2, "content": { "url": "https://a/?d={device_id}" } }),
            &sk,
        );
        let applied = m.apply_fetched(&body).expect("applies");
        assert!(
            applied.warnings.iter().any(|w| w.contains("device_id")),
            "got {:?}",
            applied.warnings
        );
    }

    #[test]
    fn a_legacy_last_good_with_a_bad_poll_interval_is_clamped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        // Simulate a stored doc from an older build carrying an out-of-range value.
        let stored = serde_json::json!({
            "revision": 3,
            "sig": "ed25519:AAAA",
            "network": { "config_poll_s": 1 }
        });
        store::ConfigStore::new(dir.path())
            .save_last_good(&stored, 3)
            .unwrap();

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::LastGood);
        assert_eq!(
            m.current().network.config_poll_s,
            30,
            "clamped so polling can never be disabled (cfg-01)"
        );
    }
}
