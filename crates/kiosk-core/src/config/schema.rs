//! The remote config document (spec §5.2). Every *content* field has a default, so a
//! signed body of `{}` is schema-valid. Unknown fields are captured, not rejected —
//! validation turns them into warnings.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn d_true() -> bool {
    true
}
fn d_zoom() -> f64 {
    1.0
}
fn d_version() -> u32 {
    1
}
fn d_error_max_retries() -> u32 {
    5
}
fn d_idle_reset() -> u64 {
    180
}
fn d_cursor_autohide() -> u64 {
    5
}
fn d_check_url() -> String {
    "https://www.gstatic.com/generate_204".to_string()
}
fn d_probe_online() -> u64 {
    30
}
fn d_probe_offline() -> u64 {
    10
}
fn d_config_poll() -> u64 {
    300
}
fn d_max_mem() -> u64 {
    1500
}
fn d_level() -> String {
    "info".to_string()
}
fn d_health_sample() -> u64 {
    60
}
fn d_spool_max() -> u64 {
    50
}
fn d_spool_reserve() -> u64 {
    10
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fallback {
    Video,
    ErrorPage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchKeyboard {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlDetail {
    Path,
    Host,
    Full,
}

/// Web-permission policy, default-deny (spec §7 M9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Permissions {
    #[serde(default)]
    pub camera: bool,
    #[serde(default)]
    pub microphone: bool,
    #[serde(default)]
    pub geolocation: bool,
    #[serde(default)]
    pub notifications: bool,
    #[serde(default)]
    pub clipboard_read: bool,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    /// `None` ⇒ fall back to the `[bootstrap]` url (spec cfg-05).
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub allowlist: Vec<String>,
    #[serde(default)]
    pub frame_allowlist: Option<Vec<String>>,
    #[serde(default)]
    pub scheme_allowlist: Vec<String>,
    #[serde(default = "d_fallback")]
    pub fallback: Fallback,
    #[serde(default = "d_error_max_retries")]
    pub error_max_retries: u32,
    #[serde(default = "d_zoom")]
    pub zoom: f64,
    #[serde(default)]
    pub inject_css: String,
    #[serde(default)]
    pub inject_js: String,
    #[serde(default = "d_idle_reset")]
    pub idle_reset_seconds: u64,
    #[serde(default = "d_true")]
    pub clear_data_on_reset: bool,
    #[serde(default)]
    pub pdf_view: bool,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_fallback() -> Fallback {
    Fallback::Video
}

impl Default for Content {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Content defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Display {
    #[serde(default = "d_cursor_autohide")]
    pub cursor_autohide_seconds: u64,
    #[serde(default)]
    pub monitor: u32,
    #[serde(default = "d_true")]
    pub keep_awake: bool,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Display {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Display defaults must deserialize")
    }
}

/// The screen corner (or centre) the exit-gesture taps must land in. Spec §5.2
/// enumerates exactly these — a free string would let a signed-but-wrong value
/// reach the platform layer with no field-level `config.error` for the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GestureRegion {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitGesture {
    pub taps: u8,
    pub region: GestureRegion,
    #[serde(default)]
    pub min_len: Option<u8>,
    #[serde(default)]
    pub alphanumeric: bool,
    pub pin_hash: String,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    #[serde(default = "d_touch_keyboard")]
    pub touch_keyboard: TouchKeyboard,
    #[serde(default)]
    pub allow_context_menu: bool,
    #[serde(default)]
    pub allow_text_selection: bool,
    #[serde(default)]
    pub exit_gesture: Option<ExitGesture>,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_touch_keyboard() -> TouchKeyboard {
    TouchKeyboard::Auto
}

impl Default for Input {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Input defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Network {
    #[serde(default = "d_check_url")]
    pub connectivity_check_url: String,
    #[serde(default = "d_probe_online")]
    pub probe_online_s: u64,
    #[serde(default = "d_probe_offline")]
    pub probe_offline_s: u64,
    #[serde(default = "d_config_poll")]
    pub config_poll_s: u64,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Network {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Network defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Maintenance {
    #[serde(default)]
    pub nightly_reload: Option<String>,
    #[serde(default)]
    pub restart_app: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default = "d_max_mem")]
    pub max_webview_mem_mb: u64,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for Maintenance {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Maintenance defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default = "d_level")]
    pub level: String,
    #[serde(default = "d_health_sample")]
    pub health_sample_s: u64,
    #[serde(default = "d_spool_max")]
    pub spool_max_mb: u64,
    #[serde(default = "d_spool_reserve")]
    pub spool_reserve_high_mb: u64,
    #[serde(default = "d_url_detail")]
    pub url_detail: UrlDetail,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

fn d_url_detail() -> UrlDetail {
    UrlDetail::Path
}

impl Default for Logging {
    fn default() -> Self {
        serde_json::from_str("{}").expect("Logging defaults must deserialize")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "d_version")]
    pub version: u32,
    /// REQUIRED on every *fetched* config (spec §5.2); absent is legal only for
    /// locally-sourced config, which never passes through signature/rollback checks.
    #[serde(default)]
    pub revision: Option<i64>,
    /// REQUIRED on every *fetched* config (spec §5.2/§8 SEC-11): it binds the signed
    /// document to one device and must equal the effective device_id. `Option` on the
    /// type only so `RemoteConfig::default()` and locally-sourced config still work —
    /// the requirement is enforced on the fetched/last-good paths, not by the type.
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub sig: Option<String>,
    #[serde(default)]
    pub content: Content,
    #[serde(default)]
    pub display: Display,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub network: Network,
    #[serde(default)]
    pub maintenance: Maintenance,
    #[serde(default)]
    pub logging: Logging,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        serde_json::from_str("{}").expect("RemoteConfig defaults must deserialize")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_body_deserializes_to_all_defaults() {
        let c: RemoteConfig = serde_json::from_str("{}").expect("empty body is valid");
        assert_eq!(c.version, 1);
        assert_eq!(c.revision, None);
        assert_eq!(c.sig, None);
        assert_eq!(c.content.url, None);
        assert_eq!(c.content.allowlist, Vec::<String>::new());
        assert_eq!(c.content.fallback, Fallback::Video);
        assert_eq!(c.content.error_max_retries, 5);
        assert_eq!(c.content.zoom, 1.0);
        assert_eq!(c.content.idle_reset_seconds, 180);
        assert!(c.content.clear_data_on_reset);
        assert!(!c.content.pdf_view);
        assert!(!c.content.permissions.camera);
        assert_eq!(c.display.cursor_autohide_seconds, 5);
        assert_eq!(c.display.monitor, 0);
        assert!(c.display.keep_awake);
        assert_eq!(c.input.touch_keyboard, TouchKeyboard::Auto);
        assert!(!c.input.allow_context_menu);
        assert!(!c.input.allow_text_selection);
        assert_eq!(c.input.exit_gesture, None);
        assert_eq!(
            c.network.connectivity_check_url,
            "https://www.gstatic.com/generate_204"
        );
        assert_eq!(c.network.probe_online_s, 30);
        assert_eq!(c.network.probe_offline_s, 10);
        assert_eq!(c.network.config_poll_s, 300);
        assert_eq!(c.maintenance.nightly_reload, None);
        assert_eq!(c.maintenance.max_webview_mem_mb, 1500);
        assert_eq!(c.logging.level, "info");
        assert_eq!(c.logging.health_sample_s, 60);
        assert_eq!(c.logging.spool_max_mb, 50);
        assert_eq!(c.logging.spool_reserve_high_mb, 10);
        assert_eq!(c.logging.url_detail, UrlDetail::Path);
    }

    #[test]
    fn parses_a_populated_document() {
        let json = r#"{
          "version": 1,
          "revision": 42,
          "sig": "ed25519:AAAA",
          "content": {
            "url": "https://app.example.com/kiosk?device={device_id}",
            "allowlist": ["https://app.example.com/*"],
            "fallback": "error_page",
            "zoom": 1.25,
            "idle_reset_seconds": 0
          },
          "input": { "touch_keyboard": "off",
                     "exit_gesture": { "taps": 5, "region": "top-right", "pin_hash": "$argon2id$x" } },
          "logging": { "url_detail": "full" }
        }"#;
        let c: RemoteConfig = serde_json::from_str(json).expect("valid");
        assert_eq!(c.revision, Some(42));
        assert_eq!(c.sig.as_deref(), Some("ed25519:AAAA"));
        assert_eq!(
            c.content.url.as_deref(),
            Some("https://app.example.com/kiosk?device={device_id}")
        );
        assert_eq!(c.content.fallback, Fallback::ErrorPage);
        assert_eq!(c.content.zoom, 1.25);
        assert_eq!(c.content.idle_reset_seconds, 0);
        assert_eq!(c.input.touch_keyboard, TouchKeyboard::Off);
        let g = c.input.exit_gesture.expect("gesture");
        assert_eq!(g.taps, 5);
        assert_eq!(g.region, GestureRegion::TopRight);
        assert_eq!(c.logging.url_detail, UrlDetail::Full);
        // Untouched sections still default.
        assert_eq!(c.network.config_poll_s, 300);
    }

    #[test]
    fn unknown_fields_are_captured_not_rejected() {
        let json = r#"{ "content": { "url": "https://a/", "future_knob": 7 },
                        "brand_new_section_ignored_by_serde": 1 }"#;
        let c: RemoteConfig = serde_json::from_str(json).expect("unknown fields must not reject");
        assert!(c.content.unknown.contains_key("future_knob"));
        assert!(c.unknown.contains_key("brand_new_section_ignored_by_serde"));
    }

    #[test]
    fn rejects_comments_strict_json_only() {
        // spec cfg-13: the on-device parser is strict JSON.
        let json = "{ /* nope */ \"version\": 1 }";
        assert!(serde_json::from_str::<RemoteConfig>(json).is_err());
    }

    #[test]
    fn exit_gesture_region_is_an_enum_not_a_free_string() {
        // Spec §5.2 enumerates exactly five regions. A signed-but-wrong value must be
        // rejected at parse time, not passed through to the platform layer.
        for region in [
            "top-left",
            "top-right",
            "bottom-left",
            "bottom-right",
            "center",
        ] {
            let json = format!(
                r#"{{"input":{{"exit_gesture":{{"taps":5,"region":"{region}","pin_hash":"$h"}}}}}}"#
            );
            serde_json::from_str::<RemoteConfig>(&json)
                .unwrap_or_else(|e| panic!("{region} must parse: {e}"));
        }
        let bad = r#"{"input":{"exit_gesture":{"taps":5,"region":"middle-left","pin_hash":"$h"}}}"#;
        assert!(
            serde_json::from_str::<RemoteConfig>(bad).is_err(),
            "an unenumerated region must be rejected"
        );
        // Snake_case is NOT the wire form — the spec writes kebab-case.
        let snake = r#"{"input":{"exit_gesture":{"taps":5,"region":"top_left","pin_hash":"$h"}}}"#;
        assert!(serde_json::from_str::<RemoteConfig>(snake).is_err());
    }

    #[test]
    fn bad_enum_value_is_a_parse_error() {
        let json = r#"{ "content": { "fallback": "carrier_pigeon" } }"#;
        assert!(serde_json::from_str::<RemoteConfig>(json).is_err());
    }
}
