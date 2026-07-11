//! Device identity (spec §4, cfg-09). The effective device_id is used BOTH as the
//! `{device_id}` URL template value and as the Cloud Logging `device_id` label, so
//! URL identity and log identity are identical by construction.

use crate::error::{ConfigError, FieldError};

/// Resolve the effective device id: `[kiosk] device_id` if non-empty, else the
/// caller-supplied machine id. Errors when neither is available — a kiosk with no
/// identity cannot be told apart in telemetry, so we refuse to guess.
pub fn effective_device_id(
    configured: Option<&str>,
    machine_id: Option<&str>,
) -> Result<String, ConfigError> {
    let pick = configured
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| machine_id.map(str::trim).filter(|s| !s.is_empty()));

    match pick {
        Some(id) => Ok(id.to_string()),
        None => Err(ConfigError::Invalid {
            errors: vec![FieldError::new(
                "kiosk.device_id",
                "empty and no machine id could be resolved",
                "",
            )],
            rejected_revision: None,
        }),
    }
}

/// Expand every `{device_id}` placeholder in a URL (spec cfg-09).
pub fn expand_device_id_template(url: &str, device_id: &str) -> String {
    url.replace("{device_id}", device_id)
}

/// True when the id looks like a machine GUID rather than a human-assigned name.
/// Callers warn on this at apply time: an opaque id makes fleet triage painful
/// (spec cfg-09).
pub fn is_opaque_guid(id: &str) -> bool {
    let hex: String = id.chars().filter(|c| *c != '-').collect();
    hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_id_wins_over_machine_id() {
        let id = effective_device_id(Some("lobby-01"), Some("9f8e7d6c")).unwrap();
        assert_eq!(id, "lobby-01");
    }

    #[test]
    fn falls_back_to_machine_id_when_unconfigured() {
        let id = effective_device_id(None, Some("9f8e7d6c")).unwrap();
        assert_eq!(id, "9f8e7d6c");
    }

    #[test]
    fn blank_configured_id_is_treated_as_unset() {
        let id = effective_device_id(Some("   "), Some("machine-1")).unwrap();
        assert_eq!(id, "machine-1");
    }

    #[test]
    fn errors_when_neither_source_yields_an_id() {
        let err = effective_device_id(None, None).expect_err("must fail");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "kiosk.device_id");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn expands_every_device_id_placeholder() {
        let url = expand_device_id_template(
            "https://app.example.com/k?device={device_id}&d={device_id}",
            "lobby-01",
        );
        assert_eq!(url, "https://app.example.com/k?device=lobby-01&d=lobby-01");
    }

    #[test]
    fn leaves_url_untouched_when_no_placeholder() {
        let url = expand_device_id_template("https://app.example.com/k", "lobby-01");
        assert_eq!(url, "https://app.example.com/k");
    }

    #[test]
    fn detects_opaque_guids() {
        // Machine GUIDs are opaque: a human cannot tell which kiosk this is.
        assert!(is_opaque_guid("6f9619ff-8b86-d011-b42d-00c04fc964ff"));
        assert!(is_opaque_guid("6F9619FF8B86D011B42D00C04FC964FF"));
        // Human-assigned names are not.
        assert!(!is_opaque_guid("lobby-01"));
        assert!(!is_opaque_guid("jakarta-hq-entrance"));
    }
}
