//! Last-known-good config persistence (spec cfg-06). Exactly one artifact:
//! `config-lastgood.json`, the most recent successfully-applied remote config,
//! stored verbatim (sig + revision included) so it can be re-verified and so the
//! last-applied revision can never disagree with the stored document.

use crate::error::ConfigError;
use serde_json::Value;
use std::path::PathBuf;

pub const LAST_GOOD_FILE: &str = "config-lastgood.json";

#[derive(Debug, Clone, PartialEq)]
pub struct StoredConfig {
    pub raw: Value,
    pub revision: i64,
}

#[derive(Debug, Clone)]
pub struct ConfigStore {
    dir: PathBuf,
}

impl ConfigStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        ConfigStore { dir: dir.into() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join(LAST_GOOD_FILE)
    }

    /// Load the last-known-good config. A missing OR corrupt file is treated as
    /// "none" — a kiosk must boot even with a damaged store.
    pub fn load_last_good(&self) -> Option<StoredConfig> {
        let text = std::fs::read_to_string(self.path()).ok()?;
        let raw: Value = serde_json::from_str(&text).ok()?;
        let revision = raw.get("revision").and_then(Value::as_i64)?;
        Some(StoredConfig { raw, revision })
    }

    /// The highest revision ever applied; `0` when nothing has been applied.
    /// Used for the anti-rollback check (spec §8/SEC-11).
    pub fn last_applied_revision(&self) -> i64 {
        self.load_last_good().map(|c| c.revision).unwrap_or(0)
    }

    /// Persist verbatim, atomically (temp file + rename) so a power cut mid-write
    /// cannot leave a half-written config that bricks the next boot.
    ///
    /// `revision` is the revision that signature verification authoritatively
    /// extracted from the signed payload. It is cross-checked against the
    /// document's own `revision` field BEFORE anything is written, in ALL build
    /// profiles: a document whose revision is absent, non-integer, or divergent
    /// is rejected outright. Persisting one would make `load_last_good` unable to
    /// recover a revision, so `last_applied_revision()` would report `0` while a
    /// config was in fact stored — collapsing the anti-rollback floor and letting
    /// an older, validly-signed config be replayed (spec §8/SEC-11).
    pub fn save_last_good(&self, raw: &Value, revision: i64) -> Result<(), ConfigError> {
        match raw.get("revision").and_then(Value::as_i64) {
            None => {
                return Err(ConfigError::Io(format!(
                    "refusing to store config: document has no integer `revision` field \
                     (applied revision {revision}); storing it would zero the anti-rollback floor"
                )));
            }
            Some(r) if r != revision => {
                return Err(ConfigError::Io(format!(
                    "refusing to store config: document `revision` is {r} but the applied \
                     (signature-verified) revision is {revision}"
                )));
            }
            Some(_) => {}
        }

        std::fs::create_dir_all(&self.dir)
            .map_err(|e| ConfigError::Io(format!("create {}: {e}", self.dir.display())))?;

        let text = serde_json::to_string_pretty(raw)
            .map_err(|e| ConfigError::Io(format!("serialize config: {e}")))?;

        let tmp = self.dir.join(format!("{LAST_GOOD_FILE}.tmp"));
        std::fs::write(&tmp, text.as_bytes())
            .map_err(|e| ConfigError::Io(format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, self.path())
            .map_err(|e| ConfigError::Io(format!("rename into {}: {e}", self.path().display())))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(rev: i64) -> Value {
        serde_json::json!({ "revision": rev, "sig": "ed25519:AAAA", "content": { "url": "https://a/" } })
    }

    #[test]
    fn no_store_yields_no_last_good_and_revision_zero() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        assert_eq!(s.load_last_good(), None);
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn saves_and_reloads_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(42), 42).unwrap();

        let got = s.load_last_good().expect("must load");
        assert_eq!(got.revision, 42);
        // Verbatim: the signature must survive, or the document could never be re-verified.
        assert_eq!(got.raw["sig"], Value::String("ed25519:AAAA".into()));
        assert_eq!(
            got.raw["content"]["url"],
            Value::String("https://a/".into())
        );
        assert_eq!(s.last_applied_revision(), 42);
    }

    #[test]
    fn overwrite_advances_the_revision() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(1), 1).unwrap();
        s.save_last_good(&doc(9), 9).unwrap();
        assert_eq!(s.last_applied_revision(), 9);
        assert_eq!(s.load_last_good().unwrap().revision, 9);
    }

    #[test]
    fn corrupt_store_is_treated_as_absent_not_a_crash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LAST_GOOD_FILE), b"{ this is not json").unwrap();
        let s = ConfigStore::new(dir.path());
        assert_eq!(
            s.load_last_good(),
            None,
            "a corrupt file must not panic the kiosk"
        );
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn save_creates_the_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("does").join("not").join("exist");
        let s = ConfigStore::new(&nested);
        s.save_last_good(&doc(3), 3).unwrap();
        assert_eq!(s.last_applied_revision(), 3);
    }

    #[test]
    fn save_rejects_a_document_with_no_revision_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        let no_rev =
            serde_json::json!({ "sig": "ed25519:AAAA", "content": { "url": "https://a/" } });

        let err = s.save_last_good(&no_rev, 7).unwrap_err();
        assert!(
            matches!(err, ConfigError::Io(ref m) if m.contains("no integer `revision`")),
            "unexpected error: {err:?}"
        );
        // A stored doc with no recoverable revision would report a floor of 0
        // while holding a config. It must never reach disk.
        assert!(
            !dir.path().join(LAST_GOOD_FILE).exists(),
            "a rejected save must not write the store"
        );
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn save_rejects_a_revision_that_disagrees_with_the_document_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());

        // Document says 2, but signature verification authoritatively said 8.
        let err = s.save_last_good(&doc(2), 8).unwrap_err();
        assert!(
            matches!(err, ConfigError::Io(ref m) if m.contains("is 2") && m.contains("is 8")),
            "error must name both revisions: {err:?}"
        );
        assert!(
            !dir.path().join(LAST_GOOD_FILE).exists(),
            "a rejected save must not write the store"
        );
        assert_eq!(s.last_applied_revision(), 0);
    }

    #[test]
    fn a_rejected_save_does_not_clobber_the_existing_rollback_floor() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(5), 5).unwrap();

        // Both rejection paths, against a store that already holds a good config.
        let no_rev = serde_json::json!({ "sig": "ed25519:BBBB" });
        assert!(s.save_last_good(&no_rev, 6).is_err());
        assert!(s.save_last_good(&doc(2), 6).is_err());

        // The rollback floor and the stored document must both be intact.
        assert_eq!(s.last_applied_revision(), 5);
        let got = s.load_last_good().expect("the good config must survive");
        assert_eq!(got.revision, 5);
        assert_eq!(got.raw["sig"], Value::String("ed25519:AAAA".into()));
    }
}
