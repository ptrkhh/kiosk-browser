//! The single error vocabulary for the config subsystem.

use serde::Serialize;

/// One offending field in a rejected config. The `config.error` telemetry
/// payload carries a list of these (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub reason: String,
    pub value: String,
}

impl FieldError {
    pub fn new(
        field: impl Into<String>,
        reason: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        FieldError {
            field: field.into(),
            reason: reason.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// Whole-document rejection: the config is never applied partially (spec cfg-11).
    #[error("invalid config: {} field error(s)", .errors.len())]
    Invalid {
        errors: Vec<FieldError>,
        rejected_revision: Option<i64>,
    },
    #[error("signature: {0}")]
    Signature(String),
    #[error("anti-rollback: revision {got} <= last applied {last}")]
    Rollback { got: i64, last: i64 },
    #[error("unsupported_version: {0}")]
    UnsupportedVersion(u32),
    /// A security-control rejection, NOT a disk problem: the revision the signature
    /// authoritatively verified disagrees with (or is missing from) the document body.
    /// Persisting such a document would collapse the anti-rollback floor (spec §8/SEC-11).
    #[error("revision mismatch: document says {document:?}, signature verified {verified}")]
    RevisionMismatch {
        document: Option<i64>,
        verified: i64,
    },
    /// The document is genuinely signed, but for a DIFFERENT device (or for none at all).
    /// A signed config is bound to exactly one device: without this gate, any principal
    /// who can read the bucket (SEC-04) or influence which object we fetch (SEC-08) could
    /// replay kiosk B's genuinely-signed, higher-revision config — including its
    /// `content.url` and `inject_js` — at kiosk A (spec §8/SEC-11).
    #[error("device binding: config is for device {got:?}, this device is {expected}")]
    DeviceMismatch {
        expected: String,
        got: Option<String>,
    },
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
}
