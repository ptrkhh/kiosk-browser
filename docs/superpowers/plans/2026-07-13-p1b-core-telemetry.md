# P1-B — `kiosk-core`: Telemetry (Google Cloud Logging)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the telemetry subsystem in `kiosk-core` — the event taxonomy with its severity mapping, NTP-independent trusted time, `LogEntry` construction (resource/labels/`insertId`/URL redaction), a severity-tiered crash-durable disk spool, per-event rate limiting with coalescing, service-account auth (RS256 JWT → OAuth2 token), and the batching Google Cloud Logging client that drains the spool.

**Architecture:** Pure Rust in `crates/kiosk-core/src/logging/`, no Tauri and no per-OS API (spec §4). **All network I/O goes through a `Transport` trait**, so every component is unit-testable against a fake with zero live servers; the real implementation wraps `reqwest::blocking` with rustls. The spool is the source of truth and the network is a drain — WARNING and above are written through to disk with `fsync` at creation, because a watchdog `SIGKILL` runs neither the panic hook nor `Drop` (spec TEL-10). The subsystem is synchronous and designed to run on its own thread, independent of the (frequently hung) webview thread.

**Tech Stack:** Rust stable; `reqwest` (blocking, rustls), `jsonwebtoken` (RS256), `chrono` (RFC 3339 out, RFC 2822 HTTP-`Date` in), `sha2` (URL hashing), `url` (redaction), `serde`/`serde_json`. Tests: `tempfile`, plus an in-crate fake `Transport`.

## Global Constraints

- Spec of record: `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (§6 telemetry, §4 layering). **On conflict, the spec wins over this plan.**
- **⚠️ This plan's sample code is a STARTING POINT, NOT GOSPEL.** In the previous plan (P1-A), three genuine security/correctness defects were bugs *in the plan's own sample code* that implementers transcribed faithfully — including a `debug_assert` guard that vanished in release builds and a rollback floor that silently collapsed. **If the code here looks wrong, it may well be wrong. Say so and stop rather than implementing something you believe is broken.** Reviewers: a defect is a defect even when the plan mandated it — report it at its true severity, labeled plan-mandated.
- **Layering rule (spec §4): `kiosk-core` must NEVER depend on Tauri or any per-OS API.** `std::fs`, `std::thread`, and cross-platform crates are fine; `windows`/`gtk`/`jni`/`tauri` are not.
- **All network I/O goes through the `Transport` trait** (Task 6). No component may call `reqwest` directly except the one adapter. This is what keeps the subsystem testable.
- **The OAuth access token is a bearer secret: it is cached in memory ONLY and must NEVER be written to the spool or any log** (spec TEL-05).
- **`insertId` is assigned once at entry creation and reused byte-identically on every retry** (spec TEL-03) — it is Cloud Logging's only dedup key. A regenerated `insertId` on retry duplicates entries.
- Rust stable, edition 2021. Lint gates before every commit: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Commit: conventional prefix + trailer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. The repo-local git identity is configured — just `git commit`; do NOT pass `-c user.email=...`. Stage explicit paths; never `git add -A`. Nothing under `.superpowers/` or `FROM_GH/`. Any dependency change must commit the regenerated `Cargo.lock`.

## What exists already (from P1-A — consume, do not reimplement)

- `kiosk_core::error::{ConfigError, FieldError}`.
- `kiosk_core::config::bootstrap::BootstrapConfig` — supplies `project_id`, `credential` (path to the service-account JSON), `site`, `region: Option<String>` (defaults to `site` when unset), `device_id: Option<String>`.
- `kiosk_core::config::schema::{Logging, UrlDetail}` — `level`, `health_sample_s`, `spool_max_mb`, `spool_reserve_high_mb`, `url_detail`.
- `kiosk_core::identity::effective_device_id`.
- `kiosk_core::app_version()`.

## File structure

All paths under `crates/kiosk-core/src/logging/`:

| File | Responsibility |
|---|---|
| `mod.rs` | `Logger` facade: batching, flush triggers, spool-on-failure, drain |
| `event.rs` | event taxonomy enum + severity mapping (spec TEL-06) |
| `time.rs` | trusted time: `time_offset` from HTTP `Date`, `trusted_utc()` (TEL-01) |
| `entry.rs` | `LogEntry` construction: resource, labels, timestamps, `insertId`, redaction (TEL-02/03/04/08) |
| `spool.rs` | severity-tiered JSONL rings, drop-oldest, drain interleave, `seq` persistence (TEL-07/09/10) |
| `ratelimit.rs` | per-event token buckets + coalescing summaries (TEL-09) |
| `auth.rs` | service-account JSON → RS256 JWT → OAuth2 token, cache + refresh (TEL-05) |
| `transport.rs` | `Transport` trait + `ReqwestTransport` adapter (the ONLY place `reqwest` is used) |
| `client.rs` | `entries:write` REST call, 401 refresh-and-retry (TEL-05) |

---

### Task 1: Event taxonomy + severity mapping

**Files:**
- Create: `crates/kiosk-core/src/logging/mod.rs` (module declarations only in this task)
- Create: `crates/kiosk-core/src/logging/event.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod logging;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `logging::event::Severity` enum: `Debug | Info | Warning | Error | Critical` (serializes to the exact Cloud Logging strings `DEBUG`/`INFO`/`WARNING`/`ERROR`/`CRITICAL`), with `Severity::is_high(&self) -> bool` (true for Warning/Error/Critical — this is what selects the protected spool ring and write-through fsync).
  - `logging::event::Event` enum with one variant per row of the spec §6 taxonomy, and `Event::name(&self) -> &'static str` returning the dotted wire name (`"app.start"`, `"config.applied"`, …) and `Event::severity(&self) -> Severity`.

**The full taxonomy, verbatim from spec §6 — name → severity:**

| name | severity |
|---|---|
| `app.start` | INFO |
| `app.stop` | INFO |
| `config.applied` | INFO |
| `config.error` | ERROR |
| `config.warn` | WARNING |
| `config.reverted` | WARNING |
| `net.online` | INFO |
| `net.offline` | WARNING |
| `nav.error` | WARNING |
| `nav.blocked` | WARNING |
| `webview.crash` | ERROR |
| `media.error` | WARNING |
| `watchdog.restart` | ERROR |
| `watchdog.hang` | ERROR |
| `watchdog.channel_reset` | WARNING |
| `watchdog.arm` | INFO |
| `watchdog.safe_mode` | CRITICAL |
| `watchdog.safe_mode_failed` | CRITICAL |
| `focus.lost` | WARNING |
| `clock.skew` | WARNING |
| `token.error` | WARNING |
| `health.sample` | INFO |
| `crash.panic` | CRITICAL |

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/logging/event.rs`:

```rust
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
        matches!(self, Severity::Warning | Severity::Error | Severity::Critical)
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
        (Event::WatchdogChannelReset, "watchdog.channel_reset", Severity::Warning),
        (Event::WatchdogArm, "watchdog.arm", Severity::Info),
        (Event::WatchdogSafeMode, "watchdog.safe_mode", Severity::Critical),
        (Event::WatchdogSafeModeFailed, "watchdog.safe_mode_failed", Severity::Critical),
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
    fn taxonomy_covers_every_event_variant() {
        // Guards against adding an Event variant without adding it to the spec
        // table above. Update BOTH when the spec's §6 table changes.
        assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23 events");
    }

    #[test]
    fn severity_serializes_to_cloud_logging_strings() {
        assert_eq!(serde_json::to_string(&Severity::Warning).unwrap(), "\"WARNING\"");
        assert_eq!(serde_json::to_string(&Severity::Critical).unwrap(), "\"CRITICAL\"");
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
```

Create `crates/kiosk-core/src/logging/mod.rs`:

```rust
//! Telemetry — Google Cloud Logging (spec §6).

pub mod event;
```

Add `pub mod logging;` to `crates/kiosk-core/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core logging::event`
Expected: FAIL to compile — ``no method named `name` found for enum `Event` ``.

- [ ] **Step 3: Implement**

Add to `crates/kiosk-core/src/logging/event.rs`, above the `#[cfg(test)]` module:

```rust
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core logging::event`
Expected: 4 tests PASS.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): telemetry event taxonomy and severity mapping

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Trusted time (NTP-independent)

**Files:**
- Create: `crates/kiosk-core/src/logging/time.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs` (add `pub mod time;`)
- Modify: `crates/kiosk-core/Cargo.toml` (add `chrono`)

**Why this exists (spec TEL-01):** a kiosk with a dead CMOS battery, on a LAN that blocks NTP (UDP/123) but allows 443, cannot mint a JWT — Google rejects an `iat`/`exp` outside its tolerance. Every HTTP response carries an authoritative UTC `Date` header (RFC 9110 §6.6.1 makes it mandatory), and the connectivity prober already makes a request every 10–30 s. So we harvest the clock from traffic we are already sending. NTP remains the documented primary; this is the always-available fallback.

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `logging::time::TrustedClock` — `new()`, `observe_http_date(&self, header_value: &str) -> Result<(), TimeError>` (parses an RFC 2822 / HTTP-date value and updates the offset), `trusted_utc(&self) -> Option<DateTime<Utc>>` (`None` until an offset has been established), `offset_seconds(&self) -> Option<i64>`, `is_skewed(&self) -> bool` (|offset| > 30 s → the caller emits `clock.skew`).
  - `TrustedClock` must be cheap to clone and shareable across threads (it is read by the logger thread and written by the prober thread) — use `Arc<Mutex<..>>` or atomics internally and implement `Clone`.
- `logging::time::SKEW_THRESHOLD_SECONDS: i64 = 30` (the JWT tolerance, spec TEL-01).

- [ ] **Step 1: Add the dependency**

Add to `crates/kiosk-core/Cargo.toml` under `[dependencies]`:

```toml
chrono = { version = "0.4", default-features = false, features = ["std", "clock", "serde"] }
```

- [ ] **Step 2: Write the failing tests**

Create `crates/kiosk-core/src/logging/time.rs`:

```rust
//! Trusted time (spec TEL-01). A kiosk with a dead CMOS battery on a
//! 443-only LAN cannot reach NTP, and a JWT with a bad `iat`/`exp` is rejected
//! by Google. Every HTTP response carries a mandatory `Date` header, and the
//! prober already makes a request every 10-30 s — so we take the clock from
//! traffic we are already sending. This is a FALLBACK; NTP stays the primary.

use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

/// Beyond this, the JWT `iat`/`exp` are outside Google's tolerance and the
/// caller emits `clock.skew` (WARNING).
pub const SKEW_THRESHOLD_SECONDS: i64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum TimeError {
    #[error("unparseable HTTP Date header: {0}")]
    BadDate(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixed, known-good HTTP-date (RFC 7231 IMF-fixdate).
    const HTTP_DATE: &str = "Sun, 12 Jul 2026 08:30:00 GMT";

    fn expected() -> DateTime<Utc> {
        DateTime::parse_from_rfc2822("Sun, 12 Jul 2026 08:30:00 +0000")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn no_offset_until_a_date_is_observed() {
        let c = TrustedClock::new();
        assert_eq!(c.offset_seconds(), None);
        assert_eq!(c.trusted_utc(), None, "must not guess before it knows");
        assert!(!c.is_skewed(), "unknown skew is not skewed");
    }

    #[test]
    fn observing_a_date_establishes_the_offset() {
        let c = TrustedClock::new();
        c.observe_http_date(HTTP_DATE).expect("valid HTTP date");
        assert!(c.offset_seconds().is_some());
        let t = c.trusted_utc().expect("clock is established");
        // Trusted time must be within a second or two of the observed instant.
        let delta = (t - expected()).num_seconds().abs();
        assert!(delta <= 2, "trusted_utc drifted from the observed Date: {delta}s");
    }

    #[test]
    fn a_malformed_date_is_rejected_and_does_not_corrupt_the_offset() {
        let c = TrustedClock::new();
        c.observe_http_date(HTTP_DATE).unwrap();
        let before = c.offset_seconds();
        c.observe_http_date("not a date").expect_err("must reject");
        assert_eq!(
            c.offset_seconds(),
            before,
            "a bad header must never move an established clock"
        );
    }

    #[test]
    fn a_later_observation_replaces_the_earlier_offset() {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT").unwrap();
        let first = c.offset_seconds().unwrap();
        c.observe_http_date("Sun, 12 Jul 2026 09:30:00 GMT").unwrap();
        let second = c.offset_seconds().unwrap();
        assert_eq!(second - first, 3600, "the newer Date must win");
    }

    #[test]
    fn is_skewed_trips_past_the_jwt_tolerance() {
        let c = TrustedClock::new();
        // Force a large offset by observing a Date far from the local clock.
        c.observe_http_date("Sun, 12 Jul 2020 08:30:00 GMT").unwrap();
        assert!(
            c.is_skewed(),
            "a multi-year offset must be reported as skew (offset {:?})",
            c.offset_seconds()
        );
    }

    #[test]
    fn clock_is_shareable_across_threads() {
        let c = TrustedClock::new();
        let c2 = c.clone();
        std::thread::spawn(move || c2.observe_http_date(HTTP_DATE).unwrap())
            .join()
            .unwrap();
        assert!(
            c.offset_seconds().is_some(),
            "an observation on the prober thread must be visible to the logger thread"
        );
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core logging::time`
Expected: FAIL to compile — `cannot find type TrustedClock in this scope`.

- [ ] **Step 4: Implement**

Add to `crates/kiosk-core/src/logging/time.rs`, above the `#[cfg(test)]` module:

```rust
#[derive(Debug, Clone, Default)]
pub struct TrustedClock {
    /// `server_utc - local_utc`, in seconds. `None` until a Date is observed.
    offset: Arc<Mutex<Option<i64>>>,
}

impl TrustedClock {
    pub fn new() -> Self {
        TrustedClock {
            offset: Arc::new(Mutex::new(None)),
        }
    }

    /// Parse an HTTP `Date` header and update the offset. The header is
    /// IMF-fixdate, which is RFC 2822-compatible for parsing purposes.
    pub fn observe_http_date(&self, header_value: &str) -> Result<(), TimeError> {
        let server = DateTime::parse_from_rfc2822(header_value)
            .map_err(|e| TimeError::BadDate(format!("{header_value:?}: {e}")))?
            .with_timezone(&Utc);
        let local = Utc::now();
        let offset = server.timestamp() - local.timestamp();
        *self.offset.lock().expect("offset mutex poisoned") = Some(offset);
        Ok(())
    }

    pub fn offset_seconds(&self) -> Option<i64> {
        *self.offset.lock().expect("offset mutex poisoned")
    }

    /// `None` until an offset is established — the caller must NOT guess a
    /// timestamp it does not know (spec TEL-02: an entry created before trusted
    /// time existed has its timestamp omitted so Logging assigns receive time).
    pub fn trusted_utc(&self) -> Option<DateTime<Utc>> {
        let offset = self.offset_seconds()?;
        Some(Utc::now() + chrono::Duration::seconds(offset))
    }

    /// True when the device clock is outside Google's JWT tolerance.
    pub fn is_skewed(&self) -> bool {
        matches!(self.offset_seconds(), Some(o) if o.abs() > SKEW_THRESHOLD_SECONDS)
    }
}
```

**Note for the implementer:** `DateTime::parse_from_rfc2822` on an IMF-fixdate string ending in `GMT` may or may not parse depending on the `chrono` version — RFC 2822 expects a numeric zone (`+0000`). The test pins the behavior. **If it does not parse, do not weaken the test:** normalize the header (e.g. replace a trailing ` GMT` with ` +0000`) or use a dedicated HTTP-date parser, and record the deviation. Getting this wrong means a device with a dead clock silently never sends telemetry.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core logging::time`
Expected: 6 tests PASS.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/ Cargo.lock
git commit -m "feat(core): NTP-independent trusted time from HTTP Date headers

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `LogEntry` — resource, labels, insertId, URL redaction

**Files:**
- Create: `crates/kiosk-core/src/logging/entry.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs` (add `pub mod entry;`)
- Modify: `crates/kiosk-core/Cargo.toml` (add `sha2`, `url`)

**Interfaces:**
- Consumes: `event::{Event, Severity}` (T1), `time::TrustedClock` (T2), `config::schema::UrlDetail` (P1-A).
- Produces:
  - `logging::entry::EntryContext` — the per-device identity carried on every entry: `{ project_id, device_id, site, region, app_version, config_revision: Option<i64>, url_detail: UrlDetail }`.
  - `logging::entry::LogEntry` (Serialize + Deserialize — it is persisted to the spool verbatim) with the Cloud Logging wire shape:
    - `log_name: String` = `projects/{project_id}/logs/kiosk`
    - `resource: { type: "generic_node", labels: { project_id, node_id: device_id, namespace: site, location: region } }` — **schema keys ONLY** (spec TEL-04)
    - `labels: { app_version, config_revision, device_id, site }` — non-schema identity
    - `severity: Severity`
    - `timestamp: Option<String>` (RFC 3339; `None` when trusted time is not yet established — spec TEL-02)
    - `insert_id: String` = `{device_id}-{seq}`
    - `json_payload: { event: &str, device_ts_raw: String, ...fields }`
  - `LogEntry::new(event, ctx, seq, clock, fields: serde_json::Map<String, Value>) -> LogEntry`
  - `logging::entry::redact_url(raw: &str, detail: UrlDetail) -> (String, String)` → `(redacted, url_sha256_8)`.

**URL redaction (spec TEL-08), the exact contract:**
- `UrlDetail::Path` (default): `scheme://host/path` — query and fragment STRIPPED.
- `UrlDetail::Host`: `scheme://host`.
- `UrlDetail::Full`: the URL unchanged.
- In all three cases also return `url_sha256_8` = the first 8 hex chars of the SHA-256 of the **full original** URL, so two distinct offenders remain distinguishable without exposing their query strings.
- A URL that fails to parse must NOT panic and must NOT be logged verbatim (it may itself be hostile) — return `("<unparseable>", hash_of_raw)`.

**Why this matters:** the query string is where tokens, session IDs, and PII live. `nav.blocked` fires precisely when something unexpected happened, which is exactly when a URL is most likely to carry a secret. Default-strip, hash for correlation.

- [ ] **Step 1: Add dependencies**

Add to `crates/kiosk-core/Cargo.toml` under `[dependencies]`:

```toml
sha2 = "0.10"
url = "2"
```

- [ ] **Step 2: Write the failing tests**

Create `crates/kiosk-core/src/logging/entry.rs`:

```rust
//! LogEntry construction (spec TEL-02/03/04/08).

use crate::config::schema::UrlDetail;
use crate::logging::event::{Event, Severity};
use crate::logging::time::TrustedClock;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct EntryContext {
    pub project_id: String,
    pub device_id: String,
    pub site: String,
    /// Defaults to `site` when `[kiosk] region` is unset (spec TEL-04).
    pub region: String,
    pub app_version: String,
    pub config_revision: Option<i64>,
    pub url_detail: UrlDetail,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EntryContext {
        EntryContext {
            project_id: "proj".into(),
            device_id: "lobby-01".into(),
            site: "hq".into(),
            region: "asia-southeast1".into(),
            app_version: "0.1.0+abc1234".into(),
            config_revision: Some(42),
            url_detail: UrlDetail::Path,
        }
    }

    fn established_clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT").unwrap();
        c
    }

    #[test]
    fn entry_carries_the_generic_node_resource_with_schema_keys_only() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        assert_eq!(e.log_name, "projects/proj/logs/kiosk");
        assert_eq!(e.resource.r#type, "generic_node");
        assert_eq!(e.resource.labels.project_id, "proj");
        assert_eq!(e.resource.labels.node_id, "lobby-01");
        assert_eq!(e.resource.labels.namespace, "hq");
        assert_eq!(e.resource.labels.location, "asia-southeast1");
    }

    #[test]
    fn non_schema_identity_goes_in_entry_labels() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        assert_eq!(e.labels.get("app_version").unwrap(), "0.1.0+abc1234");
        assert_eq!(e.labels.get("config_revision").unwrap(), "42");
        assert_eq!(e.labels.get("device_id").unwrap(), "lobby-01");
        assert_eq!(e.labels.get("site").unwrap(), "hq");
    }

    #[test]
    fn insert_id_is_device_id_and_seq() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 7, &established_clock(), Map::new());
        assert_eq!(e.insert_id, "lobby-01-7");
    }

    #[test]
    fn severity_and_event_name_come_from_the_taxonomy() {
        let e = LogEntry::new(Event::WatchdogSafeMode, &ctx(), 1, &established_clock(), Map::new());
        assert_eq!(e.severity, Severity::Critical);
        assert_eq!(e.json_payload.get("event").unwrap(), "watchdog.safe_mode");
    }

    #[test]
    fn timestamp_is_omitted_when_trusted_time_is_not_established() {
        // spec TEL-02: do not guess. Logging assigns receive time instead.
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &TrustedClock::new(), Map::new());
        assert_eq!(e.timestamp, None);
    }

    #[test]
    fn timestamp_is_rfc3339_when_trusted_time_is_established() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        let ts = e.timestamp.expect("established clock => timestamp");
        chrono::DateTime::parse_from_rfc3339(&ts).expect("must be RFC3339");
    }

    #[test]
    fn raw_device_clock_is_preserved_for_forensics() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 1, &established_clock(), Map::new());
        let raw = e.json_payload.get("device_ts_raw").unwrap().as_str().unwrap();
        chrono::DateTime::parse_from_rfc3339(raw).expect("device_ts_raw must be RFC3339");
    }

    #[test]
    fn custom_fields_land_in_the_json_payload() {
        let mut f = Map::new();
        f.insert("exit_code".into(), Value::from(86));
        let e = LogEntry::new(Event::WatchdogRestart, &ctx(), 1, &established_clock(), f);
        assert_eq!(e.json_payload.get("exit_code").unwrap(), 86);
    }

    #[test]
    fn entry_round_trips_through_json_for_the_spool() {
        let e = LogEntry::new(Event::AppStart, &ctx(), 3, &established_clock(), Map::new());
        let text = serde_json::to_string(&e).unwrap();
        let back: LogEntry = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.insert_id, e.insert_id,
            "insertId MUST survive the spool byte-identically (TEL-03)"
        );
        assert_eq!(back.severity, e.severity);
        assert_eq!(back.timestamp, e.timestamp);
    }

    // --- URL redaction (TEL-08) ---

    #[test]
    fn path_detail_strips_query_and_fragment() {
        let (red, hash) = redact_url(
            "https://app.example.com/k/page?token=SECRET&id=9#frag",
            UrlDetail::Path,
        );
        assert_eq!(red, "https://app.example.com/k/page");
        assert!(!red.contains("SECRET"), "the token must never reach the log");
        assert_eq!(hash.len(), 8);
    }

    #[test]
    fn host_detail_keeps_only_scheme_and_host() {
        let (red, _) = redact_url("https://app.example.com/k/page?x=1", UrlDetail::Host);
        assert_eq!(red, "https://app.example.com");
    }

    #[test]
    fn full_detail_keeps_everything() {
        let raw = "https://app.example.com/k?x=1#f";
        let (red, _) = redact_url(raw, UrlDetail::Full);
        assert_eq!(red, raw);
    }

    #[test]
    fn distinct_urls_get_distinct_hashes_even_when_redacted_identically() {
        let (a_red, a_hash) = redact_url("https://x.test/p?token=A", UrlDetail::Path);
        let (b_red, b_hash) = redact_url("https://x.test/p?token=B", UrlDetail::Path);
        assert_eq!(a_red, b_red, "redaction hides the difference...");
        assert_ne!(a_hash, b_hash, "...but the hash must still distinguish them");
    }

    #[test]
    fn an_unparseable_url_is_not_logged_verbatim_and_does_not_panic() {
        let (red, hash) = redact_url("::: not a url :::", UrlDetail::Path);
        assert_eq!(red, "<unparseable>");
        assert_eq!(hash.len(), 8);
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core logging::entry`
Expected: FAIL to compile — `cannot find type LogEntry in this scope`.

- [ ] **Step 4: Implement**

Add to `crates/kiosk-core/src/logging/entry.rs`, above the `#[cfg(test)]` module:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceLabels {
    pub project_id: String,
    pub node_id: String,
    pub namespace: String,
    pub location: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    /// Always `generic_node` (spec TEL-04).
    pub r#type: String,
    pub labels: ResourceLabels,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub log_name: String,
    pub resource: Resource,
    pub labels: Map<String, Value>,
    pub severity: Severity,
    /// `None` when trusted time was not established at creation (spec TEL-02).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Cloud Logging's ONLY dedup key. Assigned once, reused verbatim on retry.
    pub insert_id: String,
    pub json_payload: Map<String, Value>,
}

impl LogEntry {
    pub fn new(
        event: Event,
        ctx: &EntryContext,
        seq: u64,
        clock: &TrustedClock,
        mut fields: Map<String, Value>,
    ) -> LogEntry {
        let mut labels = Map::new();
        labels.insert("app_version".into(), Value::from(ctx.app_version.clone()));
        labels.insert(
            "config_revision".into(),
            match ctx.config_revision {
                Some(r) => Value::from(r.to_string()),
                None => Value::from(""),
            },
        );
        labels.insert("device_id".into(), Value::from(ctx.device_id.clone()));
        labels.insert("site".into(), Value::from(ctx.site.clone()));

        fields.insert("event".into(), Value::from(event.name()));
        // The raw device clock is preserved even when it is wrong — it is the
        // evidence that tells an operator the device's clock is broken.
        fields.insert(
            "device_ts_raw".into(),
            Value::from(chrono::Utc::now().to_rfc3339()),
        );

        LogEntry {
            log_name: format!("projects/{}/logs/kiosk", ctx.project_id),
            resource: Resource {
                r#type: "generic_node".into(),
                labels: ResourceLabels {
                    project_id: ctx.project_id.clone(),
                    node_id: ctx.device_id.clone(),
                    namespace: ctx.site.clone(),
                    location: ctx.region.clone(),
                },
            },
            labels,
            severity: event.severity(),
            timestamp: clock.trusted_utc().map(|t| t.to_rfc3339()),
            insert_id: format!("{}-{}", ctx.device_id, seq),
            json_payload: fields,
        }
    }
}

/// Reduce a URL for logging and return `(redacted, url_sha256_8)` (spec TEL-08).
/// The query string is where tokens and PII live, and `nav.blocked` fires exactly
/// when a URL is most likely to carry one — so strip by default, hash for correlation.
pub fn redact_url(raw: &str, detail: UrlDetail) -> (String, String) {
    use sha2::{Digest, Sha256};

    let hash = {
        let mut h = Sha256::new();
        h.update(raw.as_bytes());
        let digest = h.finalize();
        hex8(&digest)
    };

    let redacted = match url::Url::parse(raw) {
        Err(_) => "<unparseable>".to_string(),
        Ok(u) => match detail {
            UrlDetail::Full => raw.to_string(),
            UrlDetail::Host => match u.host_str() {
                Some(h) => format!("{}://{}", u.scheme(), h),
                None => "<unparseable>".to_string(),
            },
            UrlDetail::Path => match u.host_str() {
                Some(h) => format!("{}://{}{}", u.scheme(), h, u.path()),
                None => "<unparseable>".to_string(),
            },
        },
    };

    (redacted, hash)
}

fn hex8(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(4)
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core logging::entry`
Expected: 14 tests PASS.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/ Cargo.lock
git commit -m "feat(core): LogEntry construction with resource labels and URL redaction

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Severity-tiered, crash-durable disk spool

**Files:**
- Create: `crates/kiosk-core/src/logging/spool.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs` (add `pub mod spool;`)

**Why this is the hardest task (spec TEL-07/TEL-09/TEL-10):** the spool, not the network, is the source of truth. A watchdog `SIGKILL` or an OS OOM-kill runs neither the panic hook nor `Drop`, so anything only in memory is gone. Therefore WARNING-and-above are **written through to disk and fsynced at creation**. And an INFO flood (a redirect loop spraying `nav.blocked`) must never be able to evict the `watchdog.safe_mode` entry that explains why the device died — hence two independent rings with independent drop-oldest.

**Interfaces:**
- Consumes: `entry::LogEntry` (T3), `event::Severity` (T1).
- Produces:
  - `logging::spool::Spool` — `open(dir, SpoolConfig) -> Result<Spool, SpoolError>`
  - `Spool::append(&mut self, entry: &LogEntry) -> Result<(), SpoolError>` — routes by `entry.severity.is_high()`; **fsyncs before returning for high severity**.
  - `Spool::next_seq(&mut self) -> Result<u64, SpoolError>` — the per-device monotonic counter, persisted (spec TEL-03). It MUST NOT restart at 0 after a restart, or `insertId`s collide and Cloud Logging silently dedups away real entries.
  - `Spool::drain_batch(&mut self, max: usize) -> Result<Vec<LogEntry>, SpoolError>` — oldest-first, **interleaving both rings by `(timestamp, insert_id)`**.
  - `Spool::commit_drained(&mut self, entries: &[LogEntry]) -> Result<(), SpoolError>` — removes them only after the network confirmed the write. A drain that is never committed must be re-delivered (at-least-once + `insertId` dedup = effectively-once).
  - `Spool::dropped_expired(&self) -> u64` — surfaced in the next `health.sample` so loss is visible, never silent.
  - `logging::spool::SpoolConfig { max_mb: u64, reserve_high_mb: u64, segment_mb: u64 }` — from `config::schema::Logging` (`spool_max_mb` default 50, `spool_reserve_high_mb` default 10, segments 5 MB).

**Layout on disk:** `<dir>/spool/high/*.jsonl` and `<dir>/spool/low/*.jsonl`, plus `<dir>/spool/seq` holding the counter. One JSON object per line.

**Rules that MUST hold:**
1. The high ring is sized `reserve_high_mb`; the low ring gets `max_mb - reserve_high_mb`. Drop-oldest applies **within each ring independently** — a low-ring flood can never evict a high-ring entry.
2. High-severity `append` fsyncs before returning.
3. `next_seq` is persisted and monotonic across process restarts.
4. `drain_batch` returns entries oldest-first across BOTH rings, ordered by `(timestamp, insert_id)`; a `None` timestamp sorts first (it is older than anything with a clock).
5. `commit_drained` is the only thing that removes entries.
6. Dropping an entry (ring full) increments a counter that is reported, not swallowed.

- [ ] **Step 1: Write the failing tests**

Create `crates/kiosk-core/src/logging/spool.rs` with the type stubs and this test module. (Write the `SpoolConfig`, `SpoolError`, and `Spool` struct declarations first so the tests name real types; leave the methods unimplemented — `todo!()` — so the tests compile and fail at runtime, which is the RED state for this task.)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::entry::{EntryContext, LogEntry};
    use crate::logging::event::Event;
    use crate::logging::time::TrustedClock;
    use crate::config::schema::UrlDetail;
    use serde_json::Map;

    fn ctx() -> EntryContext {
        EntryContext {
            project_id: "p".into(),
            device_id: "d1".into(),
            site: "s".into(),
            region: "r".into(),
            app_version: "0.1.0".into(),
            config_revision: None,
            url_detail: UrlDetail::Path,
        }
    }

    fn clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT").unwrap();
        c
    }

    fn entry(event: Event, seq: u64) -> LogEntry {
        LogEntry::new(event, &ctx(), seq, &clock(), Map::new())
    }

    fn cfg() -> SpoolConfig {
        SpoolConfig { max_mb: 50, reserve_high_mb: 10, segment_mb: 5 }
    }

    #[test]
    fn appended_entries_drain_back_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();
        s.append(&entry(Event::WatchdogSafeMode, 2)).unwrap();

        let batch = s.drain_batch(10).unwrap();
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn seq_is_monotonic_and_survives_a_restart() {
        // TEL-03: if seq restarts at 0, insertIds collide across restarts and
        // Cloud Logging silently dedups away real entries.
        let dir = tempfile::tempdir().unwrap();
        let a = {
            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            let _ = s.next_seq().unwrap();
            s.next_seq().unwrap()
        };
        let b = {
            let mut s = Spool::open(dir.path(), cfg()).unwrap();
            s.next_seq().unwrap()
        };
        assert!(b > a, "seq must not restart after a reopen: {a} then {b}");
    }

    #[test]
    fn drained_entries_are_not_removed_until_committed() {
        // At-least-once delivery: a drain whose network write never lands must
        // be re-delivered. insertId then makes the retry effectively-once.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();

        let first = s.drain_batch(10).unwrap();
        assert_eq!(first.len(), 1);
        drop(s);

        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        let again = s.drain_batch(10).unwrap();
        assert_eq!(again.len(), 1, "an uncommitted drain must be re-delivered");
        assert_eq!(again[0].insert_id, first[0].insert_id, "insertId reused verbatim");

        s.commit_drained(&again).unwrap();
        assert!(s.drain_batch(10).unwrap().is_empty(), "committed entries are gone");
    }

    #[test]
    fn a_low_severity_flood_cannot_evict_high_severity_entries() {
        // The whole point of the tiered ring (TEL-07): a redirect loop spraying
        // nav.blocked must not push out the watchdog.safe_mode entry that
        // explains why the device died.
        let dir = tempfile::tempdir().unwrap();
        // Tiny rings so the flood actually overflows within the test.
        let mut s = Spool::open(
            dir.path(),
            SpoolConfig { max_mb: 2, reserve_high_mb: 1, segment_mb: 1 },
        )
        .unwrap();

        s.append(&entry(Event::WatchdogSafeMode, 1)).unwrap(); // CRITICAL -> high ring

        // Flood the low ring well past its capacity.
        for seq in 2..20_000u64 {
            s.append(&entry(Event::AppStart, seq)).unwrap(); // INFO -> low ring
        }

        let drained = s.drain_batch(100_000).unwrap();
        assert!(
            drained.iter().any(|e| e.insert_id == "d1-1"),
            "the CRITICAL entry must survive an INFO flood"
        );
        assert!(s.dropped_expired() > 0 || drained.len() < 20_000, "the flood must have dropped low entries");
    }

    #[test]
    fn drain_is_oldest_first_across_both_rings() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        // Interleave severities; ordering must be by (timestamp, insert_id),
        // NOT by which ring they landed in.
        s.append(&entry(Event::AppStart, 1)).unwrap();
        s.append(&entry(Event::WatchdogSafeMode, 2)).unwrap();
        s.append(&entry(Event::AppStart, 3)).unwrap();

        let batch = s.drain_batch(10).unwrap();
        let ids: Vec<&str> = batch.iter().map(|e| e.insert_id.as_str()).collect();
        assert_eq!(ids, vec!["d1-1", "d1-2", "d1-3"], "got {ids:?}");
    }

    #[test]
    fn a_corrupt_spool_line_is_skipped_not_fatal() {
        // A torn write from a power cut must not brick telemetry forever.
        let dir = tempfile::tempdir().unwrap();
        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        s.append(&entry(Event::AppStart, 1)).unwrap();
        drop(s);

        // Append a torn line to the low ring's segment.
        let seg = std::fs::read_dir(dir.path().join("spool").join("low"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let mut f = std::fs::OpenOptions::new().append(true).open(&seg).unwrap();
        use std::io::Write;
        writeln!(f, "{{ this is a torn writ").unwrap();
        drop(f);

        let mut s = Spool::open(dir.path(), cfg()).unwrap();
        let batch = s.drain_batch(10).unwrap();
        assert_eq!(batch.len(), 1, "the good entry survives; the torn line is skipped");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core logging::spool`
Expected: FAIL — the `todo!()` stubs panic (`not yet implemented`). This is RED.

- [ ] **Step 3: Implement the spool**

Implement `Spool` to satisfy the six rules above. Guidance rather than literal code, because this is the task where a transcribed bug would be most costly — **you are expected to think, and to push back if a rule looks wrong**:

- Represent each ring as a directory of `NNNNN.jsonl` segments, rotating at `segment_mb`.
- `append`: serialize the entry as one line; pick the ring by `entry.severity.is_high()`; if the ring is over budget, delete whole oldest segments until it fits, incrementing `dropped_expired` by the number of entries discarded. **For a high-severity entry, `File::sync_all()` before returning** — this is the TEL-10 durability guarantee and a test must pin it.
- `next_seq`: read `<dir>/spool/seq`, increment, write + fsync, return. Must be correct across a reopen. Consider that a crash between increment and use merely skips a seq — harmless. A crash that *reuses* a seq is not.
- `drain_batch`: read entries from both rings, skip unparseable lines (a torn write must not be fatal), sort by `(timestamp, insert_id)` with `None` timestamps first, take `max`.
- `commit_drained`: remove exactly the committed entries. The simplest correct approach is to track, per segment, how many leading lines have been committed, and delete a segment once fully committed; do NOT rewrite segments in place on the drain path (a crash mid-rewrite loses data).
- Never let a spool error take down the app: the caller logs and continues.

**Add a test that pins the fsync guarantee** — assert that after `append` of a CRITICAL entry the data is readable from a *freshly opened* handle without any explicit flush, and state in the report how you verified the fsync actually happens (an fsync cannot be observed directly from safe Rust; verifying that `sync_all` is called on the write path and that the entry is durable on reopen is the achievable bar — say so honestly rather than claiming more).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core logging::spool`
Expected: all spool tests PASS (6 above + your fsync test).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/kiosk-core/
git commit -m "feat(core): severity-tiered crash-durable telemetry spool

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Rate limiting + coalescing

**Files:**
- Create: `crates/kiosk-core/src/logging/ratelimit.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs` (add `pub mod ratelimit;`)

**Why (spec TEL-09):** a redirect loop or a crash loop is *signal*, but 10,000 identical entries a minute is a firehose that costs money and buries the diagnosis. Cap each event type with a token bucket and coalesce the overflow into ONE summary entry per window carrying the suppressed count plus a sample — so the loop is still visible, and still bounded.

**Interfaces:**
- Consumes: `event::Event` (T1).
- Produces:
  - `logging::ratelimit::RateLimiter::new(clock: TrustedClock) -> RateLimiter`
  - `RateLimiter::admit(&mut self, event: Event) -> Admit` where `Admit` is `Allow | Suppress`.
  - `RateLimiter::take_summaries(&mut self) -> Vec<(Event, u64)>` — the (event, suppressed_count) pairs whose window has closed; the caller emits one coalesced entry per pair carrying the count.
  - `logging::ratelimit::caps() -> &'static [(Event, u32 /* per minute */, u32 /* burst */)]`

**Defaults, verbatim from spec TEL-09:** `nav.blocked` and `nav.error` → 10/min, burst 20. `webview.crash` → 6/min (burst 6). Every other event is **uncapped** (they are low-volume by construction; capping `watchdog.safe_mode` would hide the very thing you need).

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::event::Event;
    use crate::logging::time::TrustedClock;

    fn limiter() -> RateLimiter {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT").unwrap();
        RateLimiter::new(c)
    }

    #[test]
    fn an_uncapped_event_is_always_allowed() {
        let mut r = limiter();
        // Capping a CRITICAL would hide the thing you most need to see.
        for _ in 0..1000 {
            assert!(matches!(r.admit(Event::WatchdogSafeMode), Admit::Allow));
        }
    }

    #[test]
    fn a_capped_event_is_allowed_up_to_its_burst_then_suppressed() {
        let mut r = limiter();
        let mut allowed = 0;
        for _ in 0..100 {
            if matches!(r.admit(Event::NavBlocked), Admit::Allow) {
                allowed += 1;
            }
        }
        assert_eq!(allowed, 20, "nav.blocked burst is 20 (spec TEL-09)");
    }

    #[test]
    fn suppressed_events_are_counted_and_surface_as_a_summary() {
        // The loop must remain VISIBLE — bounded, not hidden.
        let mut r = limiter();
        for _ in 0..100 {
            r.admit(Event::NavBlocked);
        }
        let summaries = r.take_summaries();
        let (event, count) = summaries
            .iter()
            .find(|(e, _)| *e == Event::NavBlocked)
            .expect("a suppressed event must produce a summary");
        assert_eq!(*event, Event::NavBlocked);
        assert_eq!(*count, 80, "100 attempts - 20 admitted = 80 suppressed");
    }

    #[test]
    fn taking_summaries_clears_them() {
        let mut r = limiter();
        for _ in 0..100 {
            r.admit(Event::NavBlocked);
        }
        assert!(!r.take_summaries().is_empty());
        assert!(r.take_summaries().is_empty(), "summaries are drained, not repeated");
    }

    #[test]
    fn caps_match_the_spec() {
        let c = caps();
        let nav = c.iter().find(|(e, _, _)| *e == Event::NavBlocked).unwrap();
        assert_eq!((nav.1, nav.2), (10, 20));
        let crash = c.iter().find(|(e, _, _)| *e == Event::WebviewCrash).unwrap();
        assert_eq!(crash.1, 6);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p kiosk-core logging::ratelimit`
Expected: FAIL to compile — `cannot find type RateLimiter in this scope`.

- [ ] **Step 3: Implement**

Token bucket per capped event: capacity = burst, refill = `per_minute / 60` tokens per second, using `TrustedClock` (fall back to the local monotonic clock when trusted time is unavailable — a rate limiter must work before the clock is established). Count every `Suppress` per event; `take_summaries` returns and clears the non-zero counts.

**Think about this before you write it:** refilling from wall-clock time means a backwards clock jump could stall the limiter. Prefer `std::time::Instant` (monotonic, immune to clock changes) for the refill schedule, and keep `TrustedClock` only for stamping entries. If you agree, do that and record the deviation from this plan's suggestion — that is exactly the kind of push-back this plan asks for.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p kiosk-core logging::ratelimit`
Expected: 5 tests PASS.

- [ ] **Step 5: Lint and commit**

```bash
git add crates/kiosk-core/
git commit -m "feat(core): per-event rate limiting with coalesced summaries

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: `Transport` trait + service-account auth (RS256 JWT → OAuth2)

**Files:**
- Create: `crates/kiosk-core/src/logging/transport.rs`
- Create: `crates/kiosk-core/src/logging/auth.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs`
- Modify: `crates/kiosk-core/Cargo.toml` (add `reqwest`, `jsonwebtoken`)

**Interfaces:**
- Produces:
  - `logging::transport::HttpResponse { status: u16, headers: Vec<(String, String)>, body: String }`
  - `logging::transport::Transport` trait: `fn post(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<HttpResponse, TransportError>` — `Send + Sync`.
  - `logging::transport::ReqwestTransport` — the ONLY place `reqwest` may be referenced.
  - `logging::auth::ServiceAccount::from_json(&str) -> Result<ServiceAccount, AuthError>` — parses `client_email`, `private_key` (PEM), `token_uri`.
  - `logging::auth::TokenSource::new(sa, transport, clock)` with `TokenSource::token(&mut self) -> Result<String, AuthError>` — mints via RS256 JWT, caches in memory, refreshes proactively at `exp − 5 min`, and `TokenSource::invalidate(&mut self)` (called on a 401 to force one refresh-and-retry).

**Non-negotiable (spec TEL-05):**
- The access token is a **bearer secret**: in-memory only. It must never be written to the spool, a log, an error message, or `Debug` output. **Implement `Debug` manually to redact it**, and add a test asserting the token string does not appear in `format!("{:?}", token_source)`.
- The JWT `iat`/`exp` use **trusted time**, not the local clock (that is the whole point of Task 2 — a dead-CMOS device must still authenticate).
- Do NOT mint per flush (~8,640 calls/device/day). Cache and reuse.
- JWT claims: `iss` = `client_email`, `scope` = `https://www.googleapis.com/auth/logging.write`, `aud` = `token_uri`, `exp` = `iat + 3600` (must be ≤ 3600).

- [ ] **Step 1: Add dependencies**

```toml
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls", "json"] }
jsonwebtoken = "9"
```

(`default-features = false` + `rustls-tls` avoids linking the platform's OpenSSL — keeps the crate portable and the layering rule honest.)

- [ ] **Step 2: Write the failing tests**

Write a `FakeTransport` in the test module that records requests and returns canned responses. Tests to write:

1. `service_account_json_parses_the_fields_we_need` — `client_email`, `private_key`, `token_uri`.
2. `jwt_claims_match_googles_server_to_server_contract` — decode the JWT the `TokenSource` sends and assert `iss`, `scope` contains `logging.write`, `aud` == `token_uri`, `exp - iat <= 3600`. **This is the test that catches a typo'd `aud`/`scope` in CI instead of in production.**
3. `jwt_iat_and_exp_use_trusted_time_not_the_local_clock` — give the clock a large offset and assert the JWT's `iat` reflects the trusted time, not `Utc::now()`.
4. `the_token_is_cached_and_not_reminted_on_every_call` — call `token()` twice, assert the fake transport saw exactly ONE token request.
5. `a_token_near_expiry_is_refreshed_proactively` — a token whose `exp` is inside the 5-minute window triggers a re-mint.
6. `invalidate_forces_exactly_one_refresh` — models the 401 path.
7. `the_token_never_appears_in_debug_output` — `assert!(!format!("{:?}", ts).contains(SECRET_TOKEN))`.
8. `a_token_endpoint_failure_is_reported_not_panicked` — 500/429/network error → `Err`, no panic.

For the RS256 signing key, generate a test RSA key once and embed the PEM as a test constant (or generate deterministically in the test) — do NOT reach the network in a unit test.

- [ ] **Step 3–5: RED → implement → GREEN, then lint and commit**

Run: `cargo test -p kiosk-core logging::auth`

```bash
git add crates/kiosk-core/ Cargo.lock
git commit -m "feat(core): Transport trait and service-account OAuth token source

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: Cloud Logging client (`entries:write`)

**Files:**
- Create: `crates/kiosk-core/src/logging/client.rs`
- Modify: `crates/kiosk-core/src/logging/mod.rs`

**Interfaces:**
- Consumes: `transport::Transport` (T6), `auth::TokenSource` (T6), `entry::LogEntry` (T3), `time::TrustedClock` (T2).
- Produces:
  - `logging::client::GclClient::new(token_source, transport, clock)`
  - `GclClient::write(&mut self, entries: &[LogEntry]) -> Result<(), ClientError>` — POSTs to `https://logging.googleapis.com/v2/entries:write` with `{ "entries": [...], "partialSuccess": true }`.

**Requirements:**
- **A 401 forces exactly ONE `invalidate` + refresh + retry** (spec TEL-05). Not a loop — a persistently-401ing credential must surface as an error, not spin.
- Harvest the `Date` header from EVERY response (success or failure) into the `TrustedClock` — this is a free clock sample (spec TEL-01).
- On any failure (network, 5xx, 429), return `Err` so the caller spools and backs off. Emit `token.error` semantics via the error type.
- **Timestamp hygiene on drain (spec TEL-02):** before sending, clamp each entry's timestamp into `[now − retention + 1h, now]`; an entry with `timestamp: None` is sent without one (Logging assigns receive time). Cloud Logging **rejects an entire batch** containing any timestamp more than 24 h in the future — so a device with a wildly-fast clock would otherwise poison every batch forever. Test this.

**Tests (with `FakeTransport`):**
1. `a_successful_write_posts_every_entry_with_partial_success`.
2. `the_bearer_token_is_sent_in_the_authorization_header`.
3. `a_401_triggers_exactly_one_refresh_and_retry` — assert the transport saw 2 write attempts and the token source re-minted once.
4. `a_persistent_401_gives_up_rather_than_looping` — second 401 → `Err`, and assert a bounded number of attempts.
5. `a_5xx_is_an_error_so_the_caller_spools` .
6. `the_date_header_of_every_response_updates_the_trusted_clock` — including on a failure response.
7. `a_future_timestamp_is_clamped_so_it_cannot_poison_the_batch` — an entry stamped +48 h is clamped to ≤ now.
8. `an_entry_without_a_timestamp_is_sent_without_one`.

- [ ] RED → implement → GREEN → lint → commit

```bash
git commit -m "feat(core): Cloud Logging entries:write client with 401 refresh-and-retry

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 8: `Logger` facade — batching, flush, spool-on-failure, drain

**Files:**
- Modify: `crates/kiosk-core/src/logging/mod.rs` (the facade lives here, below the module declarations)

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `logging::Logger::new(ctx: EntryContext, spool: Spool, client: GclClient, limiter: RateLimiter, clock: TrustedClock) -> Logger`
  - `Logger::log(&mut self, event: Event, fields: Map<String, Value>)` — **infallible from the caller's perspective**: telemetry must never take down the kiosk. It rate-limits, assigns `seq`/`insertId`, builds the entry, write-throughs to the spool if high-severity, and buffers.
  - `Logger::flush(&mut self) -> Result<(), LoggerError>` — drains the spool and writes via the client; on success `commit_drained`, on failure leaves the spool intact and backs off.
  - `Logger::tick(&mut self)` — called on a timer by the owning thread; flushes when 10 s elapsed OR 100 entries buffered (spec TEL-07), and emits any coalesced rate-limit summaries.

**Requirements:**
- Flush triggers: **every 10 s or 100 entries, whichever first.**
- A failed flush must NOT lose entries: they stay spooled and are retried with the SAME `insertId`.
- Rate-limit summaries are emitted as real entries (event = the suppressed event, with a `suppressed_count` field and a sample).
- `log()` never panics and never propagates an error to the caller.
- The `Logger` is designed to be owned by a dedicated thread (spec TEL-10: independent of the webview thread). It does not spawn that thread itself — the caller does. Keep it `Send`.

**Tests:**
1. `a_high_severity_event_is_on_disk_before_log_returns` — the SIGKILL-durability guarantee (TEL-10).
2. `flush_writes_the_spooled_entries_and_commits_them`.
3. `a_failed_flush_keeps_the_entries_spooled_for_retry` — and the retry reuses the same `insertId`.
4. `a_flush_fires_after_100_entries`.
5. `logging_never_panics_even_when_the_spool_is_broken` — point the spool at an unwritable dir; `log()` still returns.
6. `rate_limited_events_produce_a_coalesced_summary_entry_on_tick`.
7. `an_end_to_end_pass_reaches_the_transport` — `log()` → `tick()` → the `FakeTransport` received a request containing the entry.

- [ ] RED → implement → GREEN → lint → commit

```bash
git commit -m "feat(core): Logger facade with batching, spool-on-failure, and drain

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Plan self-review (run at write time)

**Spec coverage.** TEL-01 trusted time → T2. TEL-02 timestamps (omit/clamp/drop-expired) → T3 + T7. TEL-03 insertId + persisted seq → T3 + T4. TEL-04 resource/labels → T3. TEL-05 token lifecycle → T6 + T7. TEL-06 severity mapping → T1. TEL-07 batching + tiered spool → T4 + T8. TEL-08 URL redaction → T3. TEL-09 rate limiting + coalescing → T5. TEL-10 SIGKILL durability → T4 (write-through fsync) + T8.

**Deliberately out of scope, with the owner named:**
- **Emitting the events** (`config.applied`, `net.online`, `watchdog.*`, …). This plan builds the pipe; P1-C (prober) and P1-D (`kiosk-main`) call `Logger::log`. The `ConfigError`/`Applied` shapes from P1-A were designed to slot straight in.
- **`health.sample` content** (CPU/mem/disk/RSS) — needs `sysinfo` and platform-ish sampling; belongs with the `kiosk-main` plan. The event and its severity exist here.
- **The launcher draining main's orphaned spool** (TEL-10) — the `Spool` API supports it (open a directory, drain, commit); the wiring is the launcher plan (P1-E).
- **`spool.dropped_expired` surfacing in `health.sample`** — the counter exists (T4); the reporting is P1-D.
- **The `crash.panic` hook** — the event exists; installing the panic hook is P1-D.
- **The dedicated logger thread** — `Logger` is `Send` and designed for it; the thread is spawned by P1-D/P1-E.

**Known risks in this plan (flagged, not hidden):**
- **T4 (spool) is the hardest task and the one where a transcribed bug would be most expensive.** It is deliberately specified as rules + tests rather than literal code, so the implementer must think. Expect a review loop.
- **The HTTP-`Date` parse (T2)** may need a normalization step depending on `chrono`'s RFC 2822 strictness about a `GMT` zone. The test pins it; do not weaken the test to make it pass.
- The `partialSuccess: true` flag means Cloud Logging can accept some entries and reject others. This plan treats the response as all-or-nothing. **If the response body reports per-entry errors, that is a real gap** — surface it in review rather than silently dropping the rejected entries.

**Type consistency.** `Severity`/`Event` (T1) are used by `LogEntry` (T3), `Spool` (T4), `RateLimiter` (T5). `TrustedClock` (T2) is used by T3, T5, T6, T7. `Transport` (T6) is the only network seam, used by T6's `TokenSource` and T7's `GclClient`. `LogEntry` (T3) is the unit that T4 persists, T7 sends, and T8 orchestrates.

## Follow-on plans (unchanged order)

1. **P1-C** — connectivity prober FSM + navigation allowlist matcher (both pure `kiosk-core`). The prober is also what feeds `TrustedClock::observe_http_date` on its 10–30 s cadence.
2. **P1-D** — `kiosk-main` integration: state machine, offline video, webview hardening (folds in the P0 gate verdict), the `display.monitor` topology check that `kiosk-core` cannot do, the panic hook, and `health.sample` content.
3. **P1-E** — `kiosk-launcher` watchdog: heartbeat, READY arming, backoff, safe mode, orphaned-spool drain.
4. **P1-F** — packaging: WiX MSI (Authenticode-signed, credential ACL), §7.2 Windows OS-lockdown docs.
