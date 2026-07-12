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
    /// when the network is down — else fall back to the bootstrap config.
    ///
    /// A stored last-good is adopted ONLY if it clears every gate it would have to clear
    /// as a fetch, minus the network:
    ///
    /// * its signature re-verifies against the pinned key (when a key is compiled in).
    ///   "It was verified when first applied" is a claim about provenance that a file on
    ///   disk cannot prove — anyone who can write the file would otherwise get
    ///   `content.inject_js` executed on the next boot with no cryptographic gate
    ///   (spec §8/SEC-01). With NO pinned key we adopt it unverified: such a build
    ///   already rejects every fetch, and refusing to boot would brick the device.
    /// * it deserializes into this build's schema, and
    /// * it still passes `validate()` — including the `version` gate, so a downgraded
    ///   binary can never run a document it does not understand (cfg-03).
    ///
    /// Failing any gate falls back to the bootstrap config for the CONTENT, but the
    /// stored revision is still retained as the in-memory anti-rollback floor: a
    /// rejected or damaged store must never let an older signed config be replayed
    /// (spec §8/SEC-11).
    pub fn boot(
        bootstrap: bootstrap::BootstrapConfig,
        device_id: String,
        store: store::ConfigStore,
        key: Option<VerifyingKey>,
    ) -> (ConfigManager, Applied) {
        let (stored, corrupt_warning) = store.load_last_good_checked();
        let mut warnings: Vec<String> = corrupt_warning.into_iter().collect();

        // The floor survives independently of whether the CONTENT is adopted. It is read
        // from the RAW json (`StoredConfig::revision`), never from the typed document, so
        // a schema-shape mismatch this build cannot deserialize still cannot zero it.
        let revision = stored.as_ref().map(|s| s.revision);

        let (mut config, source) = match stored {
            None => (RemoteConfig::default(), Source::Bootstrap),
            Some(stored) => match Self::vet_last_good(&stored, key.as_ref()) {
                Ok((cfg, w)) => {
                    warnings.extend(w);
                    (cfg, Source::LastGood)
                }
                Err(reason) => {
                    warnings.push(format!(
                        "last-good rejected, falling back to bootstrap ({reason}); \
                         anti-rollback floor retained at revision {}",
                        stored.revision
                    ));
                    (RemoteConfig::default(), Source::Bootstrap)
                }
            },
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

    /// Gate a stored last-good document. `Ok(cfg)` ⇒ safe to adopt; `Err(reason)` ⇒ the
    /// caller falls back to bootstrap content (but keeps the rollback floor).
    fn vet_last_good(
        stored: &store::StoredConfig,
        key: Option<&VerifyingKey>,
    ) -> Result<(RemoteConfig, Vec<String>), String> {
        if let Some(key) = key {
            // SEC-01: inject_js et al. are honoured only from a signature-verified config.
            match signature::verify_signed(&stored.raw, key) {
                Ok(rev) if rev != stored.revision => {
                    return Err(format!(
                        "signed revision {rev} disagrees with the stored revision {}",
                        stored.revision
                    ));
                }
                Ok(_) => {}
                Err(e) => return Err(format!("signature does not re-verify: {e}")),
            }
        }

        let cfg: RemoteConfig = serde_json::from_value(stored.raw.clone())
            .map_err(|e| format!("does not match this build's schema: {e}"))?;
        let warnings =
            validate::validate(&cfg).map_err(|e| format!("failed re-validation: {e}"))?;
        Ok((cfg, warnings))
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

        // 2. anti-rollback. The floor is the MAXIMUM of what is on disk and what this
        // process has already applied. Reading it from disk alone would let an attacker
        // who can delete or corrupt `config-lastgood.json` (store reads a missing/corrupt
        // file as revision 0) replay a genuinely-signed OLD revision at a device that is
        // running a much newer one — no forged signature required (spec §8/SEC-11).
        let last = self
            .store
            .last_applied_revision()
            .max(self.revision.unwrap_or(0));
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

    /// Store a document verbatim as if it had been applied, without going through
    /// `apply_fetched` (which is what an attacker with write access to the data dir does).
    fn plant(dir: &std::path::Path, doc: &Value, revision: i64) {
        store::ConfigStore::new(dir)
            .save_last_good(doc, revision)
            .expect("planting the fixture must succeed");
    }

    fn signed_value(doc: &Value, sk: &SigningKey) -> Value {
        serde_json::from_slice(&sign(doc, sk)).unwrap()
    }

    #[test]
    fn an_adopted_last_good_is_still_clamped_so_polling_can_never_be_disabled() {
        let dir = tempfile::tempdir().unwrap();
        // Valid (validate() only warns about an over-reserved spool) but in need of a clamp.
        let stored = serde_json::json!({
            "revision": 3,
            "sig": "ed25519:AAAA",
            "logging": { "spool_max_mb": 20, "spool_reserve_high_mb": 500 }
        });
        plant(dir.path(), &stored, 3);

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::LastGood);
        assert_eq!(m.current().logging.spool_reserve_high_mb, 20, "clamped");
        assert_eq!(
            m.current().network.config_poll_s,
            300,
            "polling can never be disabled (cfg-01)"
        );
    }

    // --- FIX 1: the anti-rollback floor must not depend on the store file surviving ---

    #[test]
    fn the_rollback_floor_survives_deletion_of_the_store_file() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        m.apply_fetched(&sign(
            &serde_json::json!({ "revision": 5, "content": {} }),
            &sk,
        ))
        .expect("rev 5 applies");

        // The attacker deletes the store. `last_applied_revision()` now reads 0.
        std::fs::remove_file(dir.path().join(store::LAST_GOOD_FILE)).unwrap();
        assert_eq!(
            store::ConfigStore::new(dir.path()).last_applied_revision(),
            0
        );

        // A genuinely-signed but OLD revision must still be refused: the running process
        // remembers the floor. No forged signature is needed for this attack, only a
        // stale object and the ability to remove a file.
        for old in [3, 5] {
            let body = sign(
                &serde_json::json!({ "revision": old, "content": { "url": "https://evil/" } }),
                &sk,
            );
            match m.apply_fetched(&body) {
                Err(ConfigError::Rollback { got, last }) => {
                    assert_eq!(got, old);
                    assert_eq!(
                        last, 5,
                        "the floor must come from memory, not the deleted file"
                    );
                }
                Err(other) => panic!("expected Rollback for rev {old}, got {other:?}"),
                Ok(_) => panic!("rev {old} was APPLIED — the anti-rollback floor collapsed"),
            }
        }
        assert_eq!(m.revision(), Some(5));
        assert_eq!(m.current().content.url, None, "current must survive");
    }

    #[test]
    fn a_corrupt_store_does_not_collapse_the_rollback_floor_of_a_running_manager() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));
        m.apply_fetched(&sign(
            &serde_json::json!({ "revision": 500, "content": {} }),
            &sk,
        ))
        .expect("rev 500 applies");

        // Truncate the store (a power cut, or an attacker).
        std::fs::write(dir.path().join(store::LAST_GOOD_FILE), b"").unwrap();

        let replay = sign(
            &serde_json::json!({ "revision": 12, "content": { "url": "https://evil/" } }),
            &sk,
        );
        let err = m
            .apply_fetched(&replay)
            .expect_err("replay must be refused");
        match err {
            ConfigError::Rollback { got, last } => {
                assert_eq!(got, 12);
                assert_eq!(last, 500, "the in-memory floor must hold");
            }
            other => panic!("expected Rollback, got {other:?}"),
        }
        assert_eq!(m.revision(), Some(500));
        assert_eq!(m.current().content.url, None, "current must survive");
    }

    #[test]
    fn a_corrupt_store_is_quarantined_and_boot_still_succeeds_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(store::LAST_GOOD_FILE), b"{ not json").unwrap();

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(
            applied.source,
            Source::Bootstrap,
            "the kiosk must still boot"
        );
        assert_eq!(m.home_url(), "https://boot.example.com/");
        assert!(
            applied.warnings.iter().any(|w| w.contains("corrupt")),
            "a corrupt store must be surfaced, got {:?}",
            applied.warnings
        );
        assert!(
            dir.path().join(store::CORRUPT_FILE).exists(),
            "the corrupt file must be quarantined, not re-read every boot"
        );
    }

    #[test]
    fn a_schema_shape_mismatch_in_the_store_still_preserves_the_floor_across_boot() {
        let dir = tempfile::tempdir().unwrap();
        // Readable JSON with a revision, but a shape this build cannot deserialize.
        let stored = serde_json::json!({ "revision": 77, "content": { "zoom": "not-a-number" } });
        std::fs::write(
            dir.path().join(store::LAST_GOOD_FILE),
            serde_json::to_vec(&stored).unwrap(),
        )
        .unwrap();

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::Bootstrap);
        assert_eq!(
            m.revision(),
            Some(77),
            "the floor must survive a shape mismatch"
        );
    }

    // --- FIX 2 / FIX 4: a stored last-good must clear the same gates as a fetch ---

    #[test]
    fn a_tampered_last_good_is_not_adopted_when_a_key_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();

        // Legitimately signed, then edited on disk to inject JS — SEC-01.
        let mut doc = signed_value(
            &serde_json::json!({ "revision": 4, "content": { "url": "https://good/" } }),
            &sk,
        );
        doc["content"]["inject_js"] = Value::String("exfiltrate()".into());
        plant(dir.path(), &doc, 4);

        let (m, applied) = manager(dir.path(), Some(vk));
        assert_eq!(
            applied.source,
            Source::Bootstrap,
            "a last-good whose signature does not re-verify must not be adopted"
        );
        assert_eq!(m.current().content.inject_js, "");
        assert_eq!(m.home_url(), "https://boot.example.com/");
        assert!(
            applied.warnings.iter().any(|w| w.contains("signature")),
            "got {:?}",
            applied.warnings
        );
        assert_eq!(m.revision(), Some(4), "the floor is still retained");
    }

    #[test]
    fn a_validly_signed_last_good_is_adopted_when_a_key_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let doc = signed_value(
            &serde_json::json!({ "revision": 4, "content": { "url": "https://good/" } }),
            &sk,
        );
        plant(dir.path(), &doc, 4);

        let (m, applied) = manager(dir.path(), Some(vk));
        assert_eq!(applied.source, Source::LastGood);
        assert_eq!(applied.revision, Some(4));
        assert_eq!(m.home_url(), "https://good/");
    }

    #[test]
    fn a_last_good_from_a_future_schema_version_is_not_adopted_cfg_03() {
        let dir = tempfile::tempdir().unwrap();
        // A downgraded binary must never run a v2 document it does not understand.
        let stored = serde_json::json!({
            "revision": 8, "version": 2, "content": { "url": "https://v2/" }
        });
        plant(dir.path(), &stored, 8);

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::Bootstrap);
        assert_eq!(m.home_url(), "https://boot.example.com/");
        assert!(
            applied.warnings.iter().any(|w| w.contains("re-validation")),
            "got {:?}",
            applied.warnings
        );
        assert_eq!(m.revision(), Some(8), "the floor is still retained");
    }

    #[test]
    fn a_last_good_with_an_out_of_range_value_is_not_adopted() {
        let dir = tempfile::tempdir().unwrap();
        // Neither `logging.level` nor `config_poll_s` is healed by clamp_effective's set;
        // an un-healed invalid document must not become the running config.
        let stored = serde_json::json!({
            "revision": 9,
            "network": { "config_poll_s": 1 },
            "logging": { "level": "OOPS" }
        });
        plant(dir.path(), &stored, 9);

        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::Bootstrap);
        assert_eq!(m.current().logging.level, "info");
        assert_eq!(m.current().network.config_poll_s, 300);
        assert_eq!(m.revision(), Some(9), "the floor is still retained");
    }

    #[test]
    fn a_keyless_build_still_adopts_last_good_rather_than_bricking() {
        let dir = tempfile::tempdir().unwrap();
        let stored = serde_json::json!({
            "revision": 2, "sig": "ed25519:AAAA", "content": { "url": "https://kept/" }
        });
        plant(dir.path(), &stored, 2);
        let (m, applied) = manager(dir.path(), None);
        assert_eq!(applied.source, Source::LastGood);
        assert_eq!(m.home_url(), "https://kept/");
    }

    // --- FIX 8: the remaining composed-path security properties ---

    #[test]
    fn a_byte_level_tampered_body_is_rejected_at_the_manager_level() {
        let dir = tempfile::tempdir().unwrap();
        let (sk, vk) = keys();
        let (mut m, _) = manager(dir.path(), Some(vk));

        let mut doc = signed_value(
            &serde_json::json!({ "revision": 6, "content": { "url": "https://good/" } }),
            &sk,
        );
        // Keep the (valid) signature, swap the body it covers.
        doc["content"]["url"] = Value::String("https://evil/".into());
        let body = serde_json::to_vec(&doc).unwrap();

        let err = m
            .apply_fetched(&body)
            .expect_err("a tampered body must be rejected");
        assert!(matches!(err, ConfigError::Signature(_)), "got {err:?}");
        assert_eq!(m.home_url(), "https://boot.example.com/");
        assert_eq!(m.revision(), None);
        assert!(!dir.path().join(store::LAST_GOOD_FILE).exists());
    }

    #[test]
    fn a_failed_persist_does_not_adopt_the_config() {
        let (sk, vk) = keys();
        let dir = tempfile::tempdir().unwrap();
        // Point the store at a path that cannot be a directory: a FILE stands where the
        // store dir would go, so create_dir_all (and therefore the save) must fail.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"i am a file, not a directory").unwrap();
        let store_dir = blocker.join("store");

        let (mut m, _) = ConfigManager::boot(
            boot_config(),
            "lobby-01".to_string(),
            store::ConfigStore::new(&store_dir),
            Some(vk),
        );

        let body = sign(
            &serde_json::json!({ "revision": 3, "content": { "url": "https://app/" } }),
            &sk,
        );
        let err = m
            .apply_fetched(&body)
            .expect_err("persist must fail, so the apply must fail");
        assert!(matches!(err, ConfigError::Io(_)), "got {err:?}");

        // Persist-then-adopt: a config that could not be made durable must not be running,
        // or a restart would silently revert it.
        assert_eq!(m.revision(), None, "revision must be unchanged");
        assert_eq!(m.current(), &RemoteConfig::default());
        assert_eq!(m.home_url(), "https://boot.example.com/");
    }
}
