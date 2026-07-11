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
    #[error("io: {0}")]
    Io(String),
    #[error("parse: {0}")]
    Parse(String),
}
