//! The event taxonomy and its severity mapping (spec §6, TEL-06).
//! The mapping is table-driven and asserted by a test: a severity that drifts
//! silently changes which events are protected in the spool and which are
//! write-through-fsynced, so it is pinned deliberately.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    Debug,
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    /// WARNING and above. These go to the protected spool ring and are
    /// write-through-fsynced at creation (spec TEL-07/TEL-10).
    pub fn is_high(&self) -> bool {
        matches!(
            self,
            Severity::Warning | Severity::Error | Severity::Critical
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    AppStart,
    AppStop,
    ConfigApplied,
    ConfigError,
    ConfigWarn,
    ConfigReverted,
    NetOnline,
    NetOffline,
    NavError,
    NavBlocked,
    WebviewCrash,
    MediaError,
    WatchdogRestart,
    WatchdogHang,
    WatchdogChannelReset,
    WatchdogArm,
    WatchdogSafeMode,
    WatchdogSafeModeFailed,
    FocusLost,
    ClockSkew,
    TokenError,
    HealthSample,
    CrashPanic,
}

impl Event {
    /// The dotted wire name written to `jsonPayload.event`.
    pub fn name(&self) -> &'static str {
        match self {
            Event::AppStart => "app.start",
            Event::AppStop => "app.stop",
            Event::ConfigApplied => "config.applied",
            Event::ConfigError => "config.error",
            Event::ConfigWarn => "config.warn",
            Event::ConfigReverted => "config.reverted",
            Event::NetOnline => "net.online",
            Event::NetOffline => "net.offline",
            Event::NavError => "nav.error",
            Event::NavBlocked => "nav.blocked",
            Event::WebviewCrash => "webview.crash",
            Event::MediaError => "media.error",
            Event::WatchdogRestart => "watchdog.restart",
            Event::WatchdogHang => "watchdog.hang",
            Event::WatchdogChannelReset => "watchdog.channel_reset",
            Event::WatchdogArm => "watchdog.arm",
            Event::WatchdogSafeMode => "watchdog.safe_mode",
            Event::WatchdogSafeModeFailed => "watchdog.safe_mode_failed",
            Event::FocusLost => "focus.lost",
            Event::ClockSkew => "clock.skew",
            Event::TokenError => "token.error",
            Event::HealthSample => "health.sample",
            Event::CrashPanic => "crash.panic",
        }
    }

    pub fn severity(&self) -> Severity {
        match self {
            Event::AppStart
            | Event::AppStop
            | Event::ConfigApplied
            | Event::NetOnline
            | Event::WatchdogArm
            | Event::HealthSample => Severity::Info,

            Event::ConfigWarn
            | Event::ConfigReverted
            | Event::NetOffline
            | Event::NavError
            | Event::NavBlocked
            | Event::MediaError
            | Event::WatchdogChannelReset
            | Event::FocusLost
            | Event::ClockSkew
            | Event::TokenError => Severity::Warning,

            Event::ConfigError
            | Event::WebviewCrash
            | Event::WatchdogRestart
            | Event::WatchdogHang => Severity::Error,

            Event::WatchdogSafeMode | Event::WatchdogSafeModeFailed | Event::CrashPanic => {
                Severity::Critical
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spec's table, verbatim. If you change this, you are changing the
    /// contract with the fleet's log-based metrics and alerting.
    const TAXONOMY: &[(Event, &str, Severity)] = &[
        (Event::AppStart, "app.start", Severity::Info),
        (Event::AppStop, "app.stop", Severity::Info),
        (Event::ConfigApplied, "config.applied", Severity::Info),
        (Event::ConfigError, "config.error", Severity::Error),
        (Event::ConfigWarn, "config.warn", Severity::Warning),
        (Event::ConfigReverted, "config.reverted", Severity::Warning),
        (Event::NetOnline, "net.online", Severity::Info),
        (Event::NetOffline, "net.offline", Severity::Warning),
        (Event::NavError, "nav.error", Severity::Warning),
        (Event::NavBlocked, "nav.blocked", Severity::Warning),
        (Event::WebviewCrash, "webview.crash", Severity::Error),
        (Event::MediaError, "media.error", Severity::Warning),
        (Event::WatchdogRestart, "watchdog.restart", Severity::Error),
        (Event::WatchdogHang, "watchdog.hang", Severity::Error),
        (
            Event::WatchdogChannelReset,
            "watchdog.channel_reset",
            Severity::Warning,
        ),
        (Event::WatchdogArm, "watchdog.arm", Severity::Info),
        (
            Event::WatchdogSafeMode,
            "watchdog.safe_mode",
            Severity::Critical,
        ),
        (
            Event::WatchdogSafeModeFailed,
            "watchdog.safe_mode_failed",
            Severity::Critical,
        ),
        (Event::FocusLost, "focus.lost", Severity::Warning),
        (Event::ClockSkew, "clock.skew", Severity::Warning),
        (Event::TokenError, "token.error", Severity::Warning),
        (Event::HealthSample, "health.sample", Severity::Info),
        (Event::CrashPanic, "crash.panic", Severity::Critical),
    ];

    #[test]
    fn every_event_maps_to_its_spec_name_and_severity() {
        for (event, name, severity) in TAXONOMY {
            assert_eq!(event.name(), *name, "wire name for {event:?}");
            assert_eq!(event.severity(), *severity, "severity for {event:?}");
        }
    }

    #[test]
    fn taxonomy_table_still_covers_all_23_spec_events() {
        // Adding an Event variant is already caught by the compiler: name() and
        // severity() match exhaustively with no catch-all arm, so a new variant
        // fails to compile (E0004). This test covers the other direction — a row
        // silently deleted from TAXONOMY would shrink the loop above's coverage
        // without any compile error, so the table's size is pinned to the spec's
        // 23 rows. Update it only when the spec's §6 table changes.
        assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23 events");
    }

    #[test]
    fn severity_serializes_to_cloud_logging_strings() {
        assert_eq!(
            serde_json::to_string(&Severity::Warning).unwrap(),
            "\"WARNING\""
        );
        assert_eq!(
            serde_json::to_string(&Severity::Critical).unwrap(),
            "\"CRITICAL\""
        );
        assert_eq!(serde_json::to_string(&Severity::Info).unwrap(), "\"INFO\"");
    }

    #[test]
    fn is_high_selects_warning_and_above() {
        assert!(!Severity::Debug.is_high());
        assert!(!Severity::Info.is_high());
        assert!(Severity::Warning.is_high());
        assert!(Severity::Error.is_high());
        assert!(Severity::Critical.is_high());
    }
}
