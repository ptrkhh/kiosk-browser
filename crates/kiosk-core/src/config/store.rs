//! Last-known-good config persistence (spec cfg-06). Exactly one artifact:
//! `config-lastgood.json`, the most recent successfully-applied remote config,
//! stored verbatim (sig + revision included) so it can be re-verified and so the
//! last-applied revision can never disagree with the stored document.

use crate::error::ConfigError;
use serde_json::Value;
use std::io::Write as _;
use std::path::PathBuf;

pub const LAST_GOOD_FILE: &str = "config-lastgood.json";
/// A store file that exists but cannot be parsed is renamed here so the event is
/// durable and the bad bytes are not re-read on every boot.
pub const CORRUPT_FILE: &str = "config-lastgood.corrupt";

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

    fn corrupt_path(&self) -> PathBuf {
        self.dir.join(CORRUPT_FILE)
    }

    /// Load the last-known-good config. A missing OR corrupt file is treated as
    /// "none" — a kiosk must boot even with a damaged store.
    pub fn load_last_good(&self) -> Option<StoredConfig> {
        self.load_last_good_checked().0
    }

    /// As `load_last_good`, plus a warning when the file existed but could not be
    /// parsed. In that case the file is QUARANTINED (renamed to
    /// `config-lastgood.corrupt`, best-effort) so it cannot be silently re-read on
    /// every boot and so an operator can recover it. The caller MUST surface the
    /// warning: a vanished store zeroes the on-disk anti-rollback floor.
    pub fn load_last_good_checked(&self) -> (Option<StoredConfig>, Option<String>) {
        let path = self.path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return (None, None); // absent (or unreadable) — the normal first-boot case
        };
        let parsed = serde_json::from_str::<Value>(&text).ok().and_then(|raw| {
            let revision = raw.get("revision").and_then(Value::as_i64)?;
            Some(StoredConfig { raw, revision })
        });
        match parsed {
            Some(stored) => (Some(stored), None),
            None => {
                // best-effort quarantine; a rename failure must never stop the boot
                let quarantined = std::fs::rename(&path, self.corrupt_path()).is_ok();
                (
                    None,
                    Some(format!(
                        "{} is corrupt (unparseable or missing an integer `revision`); {}",
                        path.display(),
                        if quarantined {
                            format!("quarantined as {}", self.corrupt_path().display())
                        } else {
                            "quarantine rename failed, leaving it in place".to_string()
                        }
                    )),
                )
            }
        }
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
                return Err(ConfigError::RevisionMismatch {
                    document: None,
                    verified: revision,
                });
            }
            Some(r) if r != revision => {
                return Err(ConfigError::RevisionMismatch {
                    document: Some(r),
                    verified: revision,
                });
            }
            Some(_) => {}
        }

        std::fs::create_dir_all(&self.dir)
            .map_err(|e| ConfigError::Io(format!("create {}: {e}", self.dir.display())))?;

        let text = serde_json::to_string_pretty(raw)
            .map_err(|e| ConfigError::Io(format!("serialize config: {e}")))?;

        // Durable + atomic: write, fsync the FILE, then rename. Without the fsync the
        // rename can land while the data blocks have not, leaving a zero-length or
        // partial store after a power cut — and a kiosk gets its power yanked routinely.
        let tmp = self.dir.join(format!("{LAST_GOOD_FILE}.tmp"));
        {
            let mut f = std::fs::File::create(&tmp)
                .map_err(|e| ConfigError::Io(format!("create {}: {e}", tmp.display())))?;
            f.write_all(text.as_bytes())
                .map_err(|e| ConfigError::Io(format!("write {}: {e}", tmp.display())))?;
            f.sync_all()
                .map_err(|e| ConfigError::Io(format!("fsync {}: {e}", tmp.display())))?;
        }
        std::fs::rename(&tmp, self.path())
            .map_err(|e| ConfigError::Io(format!("rename into {}: {e}", self.path().display())))?;

        // Best-effort: fsync the directory so the rename itself is durable. Not
        // meaningful on every platform (Windows returns an error for a directory
        // handle opened this way), so the result is deliberately ignored — std::fs
        // only, no per-OS dependency (layering rule, spec §4).
        let _ = std::fs::File::open(&self.dir).and_then(|d| d.sync_all());

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
    fn a_corrupt_store_is_quarantined_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LAST_GOOD_FILE), b"{ this is not json").unwrap();
        let s = ConfigStore::new(dir.path());

        let (stored, warning) = s.load_last_good_checked();
        assert_eq!(stored, None);
        let w = warning.expect("a corrupt store must be reported, never silently ignored");
        assert!(w.contains("corrupt"), "got {w:?}");

        assert!(
            !dir.path().join(LAST_GOOD_FILE).exists(),
            "the corrupt file must be moved aside"
        );
        assert!(
            dir.path().join(CORRUPT_FILE).exists(),
            "the corrupt file must be preserved for the operator"
        );
    }

    #[test]
    fn a_truncated_store_missing_a_revision_is_also_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        // Parses as JSON, but has no integer `revision` — equally unusable as a floor.
        std::fs::write(dir.path().join(LAST_GOOD_FILE), br#"{"content":{}}"#).unwrap();
        let s = ConfigStore::new(dir.path());
        let (stored, warning) = s.load_last_good_checked();
        assert_eq!(stored, None);
        assert!(warning.is_some());
        assert!(dir.path().join(CORRUPT_FILE).exists());
    }

    #[test]
    fn a_healthy_store_is_not_quarantined_and_reports_no_warning() {
        let dir = tempfile::tempdir().unwrap();
        let s = ConfigStore::new(dir.path());
        s.save_last_good(&doc(4), 4).unwrap();
        let (stored, warning) = s.load_last_good_checked();
        assert_eq!(stored.unwrap().revision, 4);
        assert_eq!(warning, None);
        assert!(!dir.path().join(CORRUPT_FILE).exists());
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
            matches!(
                err,
                ConfigError::RevisionMismatch {
                    document: None,
                    verified: 7
                }
            ),
            "a security-control rejection must not masquerade as io: {err:?}"
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
            matches!(
                err,
                ConfigError::RevisionMismatch {
                    document: Some(2),
                    verified: 8
                }
            ),
            "error must name both revisions and not be an io error: {err:?}"
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
