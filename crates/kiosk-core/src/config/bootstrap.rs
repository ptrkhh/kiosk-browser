//! `kiosk.ini` — the local, per-device bootstrap config written at install time
//! (spec §5.1). This is the only config that exists before any network fetch.

use crate::error::{ConfigError, FieldError};

/// Bootstrap exit gesture, used before the first remote fetch (spec cfg-12).
/// If absent here AND in remote config, the exit gesture is DISABLED.
#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapExitGesture {
    pub pin_hash: String,
    pub taps: u8,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BootstrapConfig {
    pub config_url: String,
    /// `None` when the ini value is empty → caller resolves the machine id.
    pub device_id: Option<String>,
    pub site: String,
    /// `None` when empty → falls back to `site` (spec §6 TEL-04).
    pub region: Option<String>,
    pub project_id: String,
    pub credential: String,
    pub startup_grace_s: u64,
    pub healthy_run_s: u64,
    pub channel_grace_s: u64,
    /// Android-only, local-only. NEVER settable from remote config (spec §5.1).
    pub demo_mode: bool,
    /// `[bootstrap] url` — the home URL until/unless remote config supplies `content.url`.
    pub bootstrap_url: String,
    pub exit_gesture: Option<BootstrapExitGesture>,
}

/// Strip a trailing `;`/`#` inline comment and surrounding whitespace from a raw
/// ini value. `rust-ini`'s inline-comment support is an off-by-default cargo
/// feature, so the raw value we get back still carries any trailing comment —
/// and the spec's kiosk.ini puts comments after values (spec §5.1). We therefore
/// normalize explicitly, applying the standard ini rule:
///
/// A `;`/`#` opens a comment ONLY at the start of the value or when preceded by
/// whitespace. One embedded mid-token is literal content — otherwise a URL
/// fragment (`https://app.example.com/#/kiosk`) would be silently truncated, and
/// `config_url`/`bootstrap_url` are exactly the values this parser exists to read.
fn clean(raw: &str) -> String {
    // Start-of-value behaves like "preceded by whitespace".
    let mut prev_is_ws = true;
    for (i, c) in raw.char_indices() {
        if prev_is_ws && matches!(c, ';' | '#') {
            return raw[..i].trim().to_string();
        }
        prev_is_ws = c.is_whitespace();
    }
    raw.trim().to_string()
}

fn required(v: Option<&str>, field: &str, errors: &mut Vec<FieldError>) -> Option<String> {
    match v.map(clean).filter(|s| !s.is_empty()) {
        Some(s) => Some(s),
        None => {
            errors.push(FieldError::new(
                field,
                "required field is missing or empty",
                "",
            ));
            None
        }
    }
}

fn optional(v: Option<&str>) -> Option<String> {
    v.map(clean).filter(|s| !s.is_empty())
}

fn number<T: std::str::FromStr>(
    v: Option<&str>,
    field: &str,
    default: T,
    errors: &mut Vec<FieldError>,
) -> T {
    match optional(v) {
        None => default,
        Some(s) => match s.parse::<T>() {
            Ok(n) => n,
            Err(_) => {
                errors.push(FieldError::new(field, "not a valid number", &s));
                default
            }
        },
    }
}

impl BootstrapConfig {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let ini = ini::Ini::load_from_str(text)
            .map_err(|e| ConfigError::Parse(format!("kiosk.ini: {e}")))?;
        let mut errors: Vec<FieldError> = Vec::new();

        let k = ini.section(Some("kiosk"));
        let get = |key: &str| k.and_then(|s| s.get(key));

        let config_url = required(get("config_url"), "kiosk.config_url", &mut errors);
        let site = required(get("site"), "kiosk.site", &mut errors);
        let project_id = required(get("project_id"), "kiosk.project_id", &mut errors);
        let credential = required(get("credential"), "kiosk.credential", &mut errors);

        let startup_grace_s = number(
            get("startup_grace_s"),
            "kiosk.startup_grace_s",
            90u64,
            &mut errors,
        );
        let healthy_run_s = number(
            get("healthy_run_s"),
            "kiosk.healthy_run_s",
            120u64,
            &mut errors,
        );
        let channel_grace_s = number(
            get("channel_grace_s"),
            "kiosk.channel_grace_s",
            30u64,
            &mut errors,
        );

        let demo_mode = match optional(get("demo_mode")) {
            None => false,
            Some(s) => match s.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => {
                    errors.push(FieldError::new("kiosk.demo_mode", "not a boolean", &s));
                    false
                }
            },
        };

        let bootstrap_url = required(
            ini.section(Some("bootstrap")).and_then(|s| s.get("url")),
            "bootstrap.url",
            &mut errors,
        );

        let exit_gesture = ini.section(Some("exit_gesture")).and_then(|s| {
            let pin_hash = optional(s.get("pin_hash"))?;
            let taps = number(s.get("taps"), "exit_gesture.taps", 7u8, &mut errors);
            let region = optional(s.get("region")).unwrap_or_else(|| "top-left".to_string());
            Some(BootstrapExitGesture {
                pin_hash,
                taps,
                region,
            })
        });

        if !errors.is_empty() {
            return Err(ConfigError::Invalid {
                errors,
                rejected_revision: None,
            });
        }

        Ok(BootstrapConfig {
            config_url: config_url.expect("checked above"),
            device_id: optional(get("device_id")),
            site: site.expect("checked above"),
            region: optional(get("region")),
            project_id: project_id.expect("checked above"),
            credential: credential.expect("checked above"),
            startup_grace_s,
            healthy_run_s,
            channel_grace_s,
            demo_mode,
            bootstrap_url: bootstrap_url.expect("checked above"),
            exit_gesture,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[kiosk]
config_url    = https://storage.googleapis.com/kiosk/devices/lobby-01.json
device_id     =                       ; empty -> auto (machine GUID / machine-id / Android ID)
site          = jakarta-hq
region        =                       ; optional
project_id    = my-gcp-project
credential    = kiosk-credential.json
startup_grace_s = 90
healthy_run_s   = 120
channel_grace_s = 30
demo_mode       = false

[bootstrap]
url = https://app.example.com/kiosk   ; home URL until remote config arrives

[exit_gesture]
pin_hash = $argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA
taps     = 7
region   = top-left
"#;

    #[test]
    fn parses_full_ini_and_strips_inline_comments() {
        let c = BootstrapConfig::parse(FULL).expect("must parse");
        assert_eq!(
            c.config_url,
            "https://storage.googleapis.com/kiosk/devices/lobby-01.json"
        );
        // Empty value with a trailing comment must be None, NOT the comment text.
        assert_eq!(c.device_id, None);
        assert_eq!(c.region, None);
        assert_eq!(c.site, "jakarta-hq");
        assert_eq!(c.project_id, "my-gcp-project");
        assert_eq!(c.credential, "kiosk-credential.json");
        assert_eq!(c.startup_grace_s, 90);
        assert_eq!(c.healthy_run_s, 120);
        assert_eq!(c.channel_grace_s, 30);
        assert!(!c.demo_mode);
        // Trailing comment must not leak into the URL.
        assert_eq!(c.bootstrap_url, "https://app.example.com/kiosk");
        let g = c.exit_gesture.expect("exit gesture present");
        assert_eq!(g.pin_hash, "$argon2id$v=19$m=65536,t=3,p=4$c2FsdA$aGFzaA");
        assert_eq!(g.taps, 7);
        assert_eq!(g.region, "top-left");
    }

    #[test]
    fn applies_defaults_for_optional_watchdog_fields() {
        let ini = r#"
[kiosk]
config_url = https://example.com/c.json
site = s
project_id = p
credential = cred.json

[bootstrap]
url = https://app.example.com/
"#;
        let c = BootstrapConfig::parse(ini).expect("must parse");
        assert_eq!(c.startup_grace_s, 90);
        assert_eq!(c.healthy_run_s, 120);
        assert_eq!(c.channel_grace_s, 30);
        assert!(!c.demo_mode);
        assert_eq!(
            c.exit_gesture, None,
            "no [exit_gesture] section => disabled"
        );
    }

    #[test]
    fn missing_required_fields_are_reported_together() {
        let ini = "[bootstrap]\nurl = https://app.example.com/\n";
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(fields.contains(&"kiosk.config_url"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.site"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.project_id"), "got {fields:?}");
                assert!(fields.contains(&"kiosk.credential"), "got {fields:?}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn missing_bootstrap_url_is_rejected() {
        let ini = "[kiosk]\nconfig_url = https://e/c.json\nsite = s\nproject_id = p\ncredential = c.json\n";
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                assert!(errors.iter().any(|e| e.field == "bootstrap.url"));
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn non_numeric_grace_value_is_a_field_error() {
        let ini = r#"
[kiosk]
config_url = https://e/c.json
site = s
project_id = p
credential = c.json
startup_grace_s = ninety

[bootstrap]
url = https://app.example.com/
"#;
        let err = BootstrapConfig::parse(ini).expect_err("must reject");
        match err {
            ConfigError::Invalid { errors, .. } => {
                let e = errors
                    .iter()
                    .find(|e| e.field == "kiosk.startup_grace_s")
                    .expect("field error present");
                assert_eq!(e.value, "ninety");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    /// A minimal-but-valid ini parameterized on the two URL-typed fields.
    fn ini_with_urls(config_url: &str, bootstrap_url: &str) -> String {
        format!(
            "[kiosk]\nconfig_url = {config_url}\nsite = s\nproject_id = p\ncredential = c.json\n\n[bootstrap]\nurl = {bootstrap_url}\n"
        )
    }

    #[test]
    fn url_fragment_is_not_mistaken_for_an_inline_comment() {
        // No comment anywhere on either line: a `#` with no preceding whitespace is
        // literal value content and must survive verbatim (SPA hash routes).
        let ini = ini_with_urls("https://e/c.json#v2", "https://app.example.com/#/kiosk");
        let c = BootstrapConfig::parse(&ini).expect("must parse");
        assert_eq!(c.config_url, "https://e/c.json#v2");
        assert_eq!(c.bootstrap_url, "https://app.example.com/#/kiosk");
    }

    #[test]
    fn whitespace_preceded_comment_after_a_url_is_still_stripped() {
        let ini = ini_with_urls("https://e/c.json", "https://app.example.com/k   ; home url");
        let c = BootstrapConfig::parse(&ini).expect("must parse");
        assert_eq!(c.bootstrap_url, "https://app.example.com/k");
    }

    #[test]
    fn comment_opens_only_at_value_start_or_after_whitespace() {
        // Preceded by whitespace => comment.
        assert_eq!(clean("value ; trailing comment"), "value");
        assert_eq!(clean("value\t# trailing comment"), "value");
        // At the very start of the value => the whole value is a comment.
        assert_eq!(clean("; empty -> auto"), "");
        assert_eq!(clean("   # optional"), "");
        // NOT preceded by whitespace => literal content, kept verbatim.
        assert_eq!(
            clean("https://app.example.com/#/kiosk"),
            "https://app.example.com/#/kiosk"
        );
        assert_eq!(clean("a;b#c"), "a;b#c");
        // Only the comment is removed; the value keeps its own inner punctuation.
        assert_eq!(clean("a#b ; note"), "a#b");
        // Surrounding whitespace is still trimmed (contract unchanged).
        assert_eq!(clean("   spaced   "), "spaced");
    }
}
