//! Last-known-good config persistence (spec cfg-06). Exactly one artifact:
//! `config-lastgood.json`, the most recent successfully-applied remote config,
//! stored verbatim (sig + revision included) so it can be re-verified and so the
//! last-applied revision can never disagree with the stored document.

use crate::error::ConfigError;
use serde_json::Value;
use std::path::{Path, PathBuf};

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
    pub fn save_last_good(&self, raw: &Value, revision: i64) -> Result<(), ConfigError> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| ConfigError::Io(format!("create {}: {e}", self.dir.display())))?;

        let text = serde_json::to_string_pretty(raw)
            .map_err(|e| ConfigError::Io(format!("serialize config: {e}")))?;

        let tmp = self.dir.join(format!("{LAST_GOOD_FILE}.tmp"));
        std::fs::write(&tmp, text.as_bytes())
            .map_err(|e| ConfigError::Io(format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, self.path())
            .map_err(|e| ConfigError::Io(format!("rename into {}: {e}", self.path().display())))?;

        debug_assert_eq!(
            raw.get("revision").and_then(Value::as_i64),
            Some(revision),
            "stored document's revision must match the applied revision"
        );
        Ok(())
    }
}

/// Convenience for callers that hold a path rather than a store.
pub fn last_good_path(dir: &Path) -> PathBuf {
    dir.join(LAST_GOOD_FILE)
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
}
