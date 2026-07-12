//! Whole-document validation (spec §5.2 cfg-07/cfg-11/cfg-03, RT-08).
//! Any invalid value rejects the ENTIRE config — never a partial apply.

use crate::config::schema::RemoteConfig;
use crate::error::{ConfigError, FieldError};

/// The schema MAJOR version this build supports (spec cfg-03).
pub const SCHEMA_MAJOR: u32 = 1;

/// Severity levels recognized by `logging.level` (spec §6).
const VALID_LOG_LEVELS: &[&str] = &["debug", "info", "warning", "error", "critical"];

/// Config fields whose runtime feature is not implemented in this build.
/// Setting them to a non-default value is accepted but warned about (spec RT-08).
const UNIMPLEMENTED: &[(&str, &str)] = &[
    ("content.inject_css", "P2"),
    ("content.inject_js", "P2"),
    ("content.pdf_view", "P1"),
    ("maintenance.max_webview_mem_mb", "P2"),
    ("maintenance.restart_app", "P2"),
];

fn range_u64(value: u64, lo: u64, hi: u64, field: &str, errors: &mut Vec<FieldError>) {
    if value < lo || value > hi {
        errors.push(FieldError::new(
            field,
            format!("out of range [{lo}, {hi}]"),
            value.to_string(),
        ));
    }
}

/// Validate the whole document. `Ok(warnings)` or a rejection carrying EVERY
/// offending field (spec cfg-11: never a partial apply).
pub fn validate(cfg: &RemoteConfig) -> Result<Vec<String>, ConfigError> {
    if cfg.version > SCHEMA_MAJOR {
        return Err(ConfigError::UnsupportedVersion(cfg.version));
    }

    let mut errors: Vec<FieldError> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // content
    if !(0.5..=3.0).contains(&cfg.content.zoom) {
        errors.push(FieldError::new(
            "content.zoom",
            "out of range [0.5, 3.0]",
            cfg.content.zoom.to_string(),
        ));
    }

    // display — monitor is device-local: warn and fall back to primary, never reject.
    if cfg.display.monitor > 0 {
        warnings.push(format!(
            "display.monitor = {} — falls back to primary if that display is absent",
            cfg.display.monitor
        ));
    }
    range_u64(
        cfg.display.cursor_autohide_seconds,
        0,
        3600,
        "display.cursor_autohide_seconds",
        &mut errors,
    );

    // input
    if let Some(g) = &cfg.input.exit_gesture {
        if !(3..=10).contains(&g.taps) {
            errors.push(FieldError::new(
                "input.exit_gesture.taps",
                "out of range [3, 10]",
                g.taps.to_string(),
            ));
        }
        if g.pin_hash.trim().is_empty() {
            errors.push(FieldError::new(
                "input.exit_gesture.pin_hash",
                "must not be empty when an exit gesture is configured",
                "",
            ));
        }
    }

    // network
    range_u64(
        cfg.network.probe_online_s,
        5,
        3600,
        "network.probe_online_s",
        &mut errors,
    );
    range_u64(
        cfg.network.probe_offline_s,
        5,
        3600,
        "network.probe_offline_s",
        &mut errors,
    );
    // 0 is INVALID: polling is the only remote lever (cfg-01).
    range_u64(
        cfg.network.config_poll_s,
        30,
        3600,
        "network.config_poll_s",
        &mut errors,
    );

    // maintenance — {0} ∪ [256, 8192]
    let mem = cfg.maintenance.max_webview_mem_mb;
    if mem != 0 && !(256..=8192).contains(&mem) {
        errors.push(FieldError::new(
            "maintenance.max_webview_mem_mb",
            "must be 0 (off) or within [256, 8192]",
            mem.to_string(),
        ));
    }

    // logging
    range_u64(
        cfg.logging.health_sample_s,
        10,
        3600,
        "logging.health_sample_s",
        &mut errors,
    );
    range_u64(
        cfg.logging.spool_max_mb,
        5,
        1024,
        "logging.spool_max_mb",
        &mut errors,
    );
    if cfg.logging.spool_reserve_high_mb > cfg.logging.spool_max_mb {
        warnings.push(format!(
            "logging.spool_reserve_high_mb ({}) > spool_max_mb ({}) — clamped",
            cfg.logging.spool_reserve_high_mb, cfg.logging.spool_max_mb
        ));
    }
    if !VALID_LOG_LEVELS
        .iter()
        .any(|lvl| lvl.eq_ignore_ascii_case(&cfg.logging.level))
    {
        errors.push(FieldError::new(
            "logging.level",
            format!("must be one of {VALID_LOG_LEVELS:?}"),
            cfg.logging.level.clone(),
        ));
    }

    // unknown fields → warnings (spec §5.2: unknown fields warn, never reject)
    let unknowns: [(&str, &serde_json::Map<String, serde_json::Value>); 7] = [
        ("(root)", &cfg.unknown),
        ("content", &cfg.content.unknown),
        ("display", &cfg.display.unknown),
        ("input", &cfg.input.unknown),
        ("network", &cfg.network.unknown),
        ("maintenance", &cfg.maintenance.unknown),
        ("logging", &cfg.logging.unknown),
    ];
    for (section, map) in unknowns {
        for key in map.keys() {
            warnings.push(format!("unknown field {section}.{key} — ignored"));
        }
    }
    // Nested sections that don't fit the flat (section, map) shape above.
    for key in cfg.content.permissions.unknown.keys() {
        warnings.push(format!("unknown field content.permissions.{key} — ignored"));
    }
    if let Some(g) = &cfg.input.exit_gesture {
        for key in g.unknown.keys() {
            warnings.push(format!("unknown field input.exit_gesture.{key} — ignored"));
        }
    }

    // capability set (RT-08)
    let defaults = RemoteConfig::default();
    for (field, phase) in UNIMPLEMENTED {
        let set = match *field {
            "content.inject_css" => cfg.content.inject_css != defaults.content.inject_css,
            "content.inject_js" => cfg.content.inject_js != defaults.content.inject_js,
            "content.pdf_view" => cfg.content.pdf_view != defaults.content.pdf_view,
            "maintenance.max_webview_mem_mb" => {
                cfg.maintenance.max_webview_mem_mb != defaults.maintenance.max_webview_mem_mb
            }
            "maintenance.restart_app" => {
                cfg.maintenance.restart_app != defaults.maintenance.restart_app
            }
            _ => false,
        };
        if set {
            warnings.push(format!(
                "field {field} accepted but feature unavailable in this build (introduced {phase})"
            ));
        }
    }

    if !errors.is_empty() {
        return Err(ConfigError::Invalid {
            errors,
            rejected_revision: cfg.revision,
        });
    }
    Ok(warnings)
}

/// Runtime clamps applied to the EFFECTIVE config, so a legacy last-good document can
/// never disable polling or over-reserve the spool (spec cfg-01).
pub fn clamp_effective(cfg: &mut RemoteConfig) {
    cfg.network.config_poll_s = cfg.network.config_poll_s.clamp(30, 3600);
    cfg.network.probe_online_s = cfg.network.probe_online_s.clamp(5, 3600);
    cfg.network.probe_offline_s = cfg.network.probe_offline_s.clamp(5, 3600);
    cfg.content.zoom = cfg.content.zoom.clamp(0.5, 3.0);
    if cfg.logging.spool_reserve_high_mb > cfg.logging.spool_max_mb {
        cfg.logging.spool_reserve_high_mb = cfg.logging.spool_max_mb;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(json: &str) -> RemoteConfig {
        serde_json::from_str(json).expect("test json must parse")
    }

    #[test]
    fn empty_config_is_valid_with_no_warnings() {
        let warnings = validate(&cfg("{}")).expect("defaults are valid");
        assert!(warnings.is_empty(), "got {warnings:?}");
    }

    #[test]
    fn collects_every_out_of_range_field_in_one_rejection() {
        let c = cfg(r#"{
          "content": { "zoom": 9.0 },
          "network": { "probe_online_s": 1, "config_poll_s": 0 },
          "logging": { "spool_max_mb": 0, "health_sample_s": 1 }
        }"#);
        let err = validate(&c).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let f: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(f.contains(&"content.zoom"), "got {f:?}");
                assert!(f.contains(&"network.probe_online_s"), "got {f:?}");
                assert!(f.contains(&"network.config_poll_s"), "got {f:?}");
                assert!(f.contains(&"logging.spool_max_mb"), "got {f:?}");
                assert!(f.contains(&"logging.health_sample_s"), "got {f:?}");
                assert_eq!(errors.len(), 5, "whole-document: all errors at once");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn config_poll_zero_is_invalid_cannot_disable_the_only_remote_lever() {
        let err = validate(&cfg(r#"{"network":{"config_poll_s":0}}"#)).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "network.config_poll_s");
                assert_eq!(errors[0].value, "0");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn max_webview_mem_allows_zero_meaning_off_but_rejects_between() {
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":0}}"#)).is_ok());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":256}}"#)).is_ok());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":100}}"#)).is_err());
        assert!(validate(&cfg(r#"{"maintenance":{"max_webview_mem_mb":9000}}"#)).is_err());
    }

    #[test]
    fn exit_gesture_taps_range_is_enforced() {
        let bad = r#"{"input":{"exit_gesture":{"taps":99,"region":"top-left","pin_hash":"$h"}}}"#;
        let err = validate(&cfg(bad)).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert_eq!(errors[0].field, "input.exit_gesture.taps");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn future_major_version_is_rejected_as_unsupported() {
        let err = validate(&cfg(r#"{"version":2}"#)).expect_err("must reject");
        match err {
            ConfigError::UnsupportedVersion(v) => assert_eq!(v, 2),
            other => panic!("expected UnsupportedVersion, got {other:?}"),
        }
    }

    #[test]
    fn out_of_range_monitor_warns_instead_of_rejecting() {
        let warnings = validate(&cfg(r#"{"display":{"monitor":9}}"#))
            .expect("monitor must warn, not reject — display topology is device-local");
        assert!(
            warnings.iter().any(|w| w.contains("display.monitor")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn unknown_fields_produce_warnings_not_errors() {
        let warnings = validate(&cfg(r#"{"content":{"future_knob":1},"nonsense":2}"#))
            .expect("unknown fields must not reject");
        assert!(
            warnings.iter().any(|w| w.contains("future_knob")),
            "got {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("nonsense")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn unimplemented_features_warn_when_set_to_non_default() {
        // RT-08: knobs whose feature is not in this build's capability set must be
        // visible in telemetry, not silent no-ops.
        let warnings = validate(&cfg(r#"{"content":{"inject_js":"alert(1)"}}"#))
            .expect("accepted but unavailable");
        assert!(
            warnings.iter().any(|w| w.contains("content.inject_js")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn spool_reserve_is_clamped_not_rejected() {
        let mut c = cfg(r#"{"logging":{"spool_max_mb":20,"spool_reserve_high_mb":50}}"#);
        let warnings = validate(&c).expect("clamped, not rejected");
        assert!(warnings.iter().any(|w| w.contains("spool_reserve_high_mb")));
        clamp_effective(&mut c);
        assert_eq!(c.logging.spool_reserve_high_mb, 20);
    }

    #[test]
    fn clamp_effective_restores_a_sane_poll_interval() {
        // A legacy last-good doc could carry an out-of-range value; the runtime clamp
        // guarantees polling can never be disabled (cfg-01).
        let mut c = cfg("{}");
        c.network.config_poll_s = 0;
        clamp_effective(&mut c);
        assert_eq!(c.network.config_poll_s, 30);
    }

    // --- Extra scope 1: nested unknown-field capture for Permissions and ExitGesture ---

    #[test]
    fn unknown_field_in_permissions_warns_not_rejects() {
        let warnings = validate(&cfg(r#"{"content":{"permissions":{"midi":true}}}"#))
            .expect("unknown permissions field must not reject");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("content.permissions.midi")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn unknown_field_in_exit_gesture_warns_not_rejects() {
        let json = r#"{"input":{"exit_gesture":{
            "taps": 5, "region": "top-left", "pin_hash": "$h", "cooldown_ms": 500
        }}}"#;
        let warnings = validate(&cfg(json)).expect("unknown exit_gesture field must not reject");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("input.exit_gesture.cooldown_ms")),
            "got {warnings:?}"
        );
    }

    // --- Extra scope 2: logging.level validation ---

    #[test]
    fn bad_logging_level_is_rejected() {
        let err = validate(&cfg(r#"{"logging":{"level":"OOPS"}}"#)).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let f: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(f.contains(&"logging.level"), "got {f:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn each_valid_logging_level_is_accepted() {
        for level in ["debug", "info", "warning", "error", "critical"] {
            let json = format!(r#"{{"logging":{{"level":"{level}"}}}}"#);
            validate(&cfg(&json)).unwrap_or_else(|e| {
                panic!("level {level:?} should be valid, got {e:?}");
            });
        }
    }
}
