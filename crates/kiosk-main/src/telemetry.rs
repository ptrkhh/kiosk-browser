//! Telemetry task (spec §6, P1-D2a Task 5): wires kiosk-core's P1-B logger stack
//! (GCL client, RS256 service-account auth, disk spool, rate limiter, trusted
//! clock) behind a cheap, cloneable, `Send` handle so the rest of kiosk-main can
//! fire an event without ever blocking or panicking on a logging failure. This
//! module only WIRES the stack; every logging behavior it exercises (spooling,
//! batching, retry, rate-limiting) already lives in `kiosk_core::logging` and is
//! not reimplemented here.
//!
//! Wired into `main.rs` (Task 6): `main` calls [`build`] from a real `Booted` and
//! runs [`run`] on a dedicated OS thread (the blocking `reqwest` transport cannot
//! live on the tokio runtime — see [`run`]).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::config::schema::{Logging, UrlDetail};
use kiosk_core::logging::auth::{ServiceAccount, TokenSource};
use kiosk_core::logging::client::GclClient;
use kiosk_core::logging::entry::{redact_url, EntryContext, LogEntry};
use kiosk_core::logging::event::Event as LogEvent;
use kiosk_core::logging::ratelimit::RateLimiter;
use kiosk_core::logging::spool::{Spool, SpoolConfig};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::logging::transport::{ReqwestTransport, Transport};
use kiosk_core::logging::{Logger, FLUSH_INTERVAL};
use serde_json::{Map, Value};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use tokio_util::sync::CancellationToken;

/// How many in-flight requests the handle-to-task channel holds before
/// `try_send` starts dropping. A burst during a config reload or a reconnect
/// should not be the thing that drops telemetry; the disk spool (not this
/// channel) is the real durability layer (spec TEL-10).
const CHANNEL_CAPACITY: usize = 256;

/// One request from a [`Telemetry`] handle to the logger task.
pub struct LogReq {
    pub event: LogEvent,
    pub fields: Map<String, Value>,
}

/// A cheap, `Clone + Send` handle onto the logger task's channel. Every helper
/// is fire-and-forget.
#[derive(Clone)]
pub struct Telemetry {
    tx: SyncSender<LogReq>,
}

impl Telemetry {
    /// A telemetry handle that silently drops everything. Used when [`build`] fails
    /// (missing/malformed credential) so the kiosk still shows content — telemetry is
    /// never worth a black screen. The receiver is dropped immediately, so every
    /// `emit`/`try_send` returns `Err(Closed)` and is discarded, exactly like a full
    /// queue. No logger task is spawned for this handle.
    pub fn disabled() -> Telemetry {
        let (tx, _rx) = mpsc::sync_channel(1);
        Telemetry { tx }
    }

    /// `try_send`, never `send().await`: telemetry must never block or panic
    /// the caller. A full queue silently drops the event — the Logger's own
    /// spool is telemetry's durability layer, not this in-memory hop.
    fn emit(&self, event: LogEvent, fields: Map<String, Value>) {
        let _ = self.tx.try_send(LogReq { event, fields });
    }

    pub fn net_online(&self) {
        self.emit(LogEvent::NetOnline, Map::new());
    }

    pub fn net_offline(&self) {
        self.emit(LogEvent::NetOffline, Map::new());
    }

    pub fn app_start(&self) {
        self.emit(LogEvent::AppStart, Map::new());
    }

    pub fn app_stop(&self) {
        self.emit(LogEvent::AppStop, Map::new());
    }

    pub fn config_error(&self, reason: &str) {
        let mut f = Map::new();
        f.insert("error".into(), Value::from(reason));
        self.emit(LogEvent::ConfigError, f);
    }

    pub fn config_applied(&self, revision: Option<i64>, warnings: &[String]) {
        let mut f = Map::new();
        f.insert(
            "revision".into(),
            Value::from(revision.map(|r| r.to_string()).unwrap_or_default()),
        );
        if !warnings.is_empty() {
            f.insert("warnings".into(), Value::from(warnings.join("; ")));
        }
        self.emit(LogEvent::ConfigApplied, f);
    }

    pub fn panic(&self, msg: &str) {
        let mut f = Map::new();
        f.insert("message".into(), Value::from(msg));
        self.emit(LogEvent::CrashPanic, f);
    }

    /// A navigation failed to load (spec §6 taxonomy: `nav.error`, WARNING, rate-capped
    /// at 10/min burst 20). `reason` is a short diagnostic (Task 6's `nav` module passes
    /// WebView2's `WebErrorStatus`) — never the full page body or headers.
    pub fn nav_error(&self, reason: &str) {
        let mut f = Map::new();
        f.insert("error".into(), Value::from(reason));
        self.emit(LogEvent::NavError, f);
    }

    /// A navigation was cancelled by the native guard (main-frame on Windows; any frame on
    /// Linux — see `nav.rs`). Spec §3.6/§6: `nav.blocked`, WARNING, rate-capped by the same
    /// Logger bucket as every other event — no second limiter here. `reason` is
    /// `BlockReason::as_str()` (a stable, greppable label); `url` is the navigation that got
    /// cancelled, redacted via `UrlDetail::Host` — never logged raw. `url_sha256` lets an
    /// operator correlate repeated blocks of the same URL without ever seeing the URL
    /// itself.
    pub fn nav_blocked(&self, reason: &str, url: &str) {
        let (redacted, hash) = redact_url(url, UrlDetail::Host);
        let mut f = Map::new();
        f.insert("reason".into(), reason.into());
        f.insert("url".into(), redacted.into());
        f.insert("url_sha256".into(), hash.into());
        self.emit(LogEvent::NavBlocked, f);
    }

    /// The kiosk window lost OS focus (spec §6 taxonomy: `focus.lost`, WARNING) — e.g.
    /// an alt-tab or a system dialog stealing the foreground. Not a security boundary;
    /// `main.rs`'s handler that calls this also re-asserts focus immediately after.
    pub fn focus_lost(&self) {
        self.emit(LogEvent::FocusLost, Map::new());
    }

    /// The renderer process failed (spec §6 taxonomy: `webview.crash`, ERROR,
    /// rate-capped like every other event). `kind` is `recovery::kind_label`'s stable,
    /// greppable label (e.g. `render_process_unresponsive`) — never a raw WebView2 enum
    /// debug-format, which isn't guaranteed stable across `webview2-com-sys` versions.
    pub fn webview_crash(&self, kind: &str) {
        let mut f = Map::new();
        f.insert("kind".into(), Value::from(kind));
        self.emit(LogEvent::WebviewCrash, f);
    }

    /// A periodic `health.sample` (spec §6, P1-D2e Task 2): BASIC host metrics
    /// (`kiosk_core::metrics::sample`/`to_fields`) — no free-form content.
    pub fn health(&self, fields: Map<String, Value>) {
        self.emit(LogEvent::HealthSample, fields);
    }

    /// A config value was out of range / invalid and a safe fallback was used
    /// instead (spec §6 taxonomy: `config.warn`, WARNING). `field` is a stable
    /// dotted label identifying the affected setting — a real config key where
    /// one exists (e.g. `display.monitor`), or a shipped smoke-checklist label
    /// for a setting with no single key (e.g. `hardening.autofill`, reused by
    /// P1-D2e Task 5).
    pub fn config_warn(&self, field: &str, reason: &str) {
        let mut f = Map::new();
        f.insert("field".into(), Value::from(field));
        f.insert("reason".into(), Value::from(reason));
        self.emit(LogEvent::ConfigWarn, f);
    }
}

/// Assembles the P1-B logger stack for this device.
///
/// Two inputs are deliberately NOT read straight off `bootstrap`, because the
/// raw ini value is not what the logger needs:
///
/// * `credential_path` — `[kiosk] credential` in `kiosk.ini` is an opaque
///   `String` (spec §5.1 example: `credential = kiosk-credential.json`) that
///   names a file living NEXT TO `kiosk.ini` (design.md §4 "File & directory
///   conventions": install dir, not `data_dir`), not inline service-account
///   JSON — `kiosk_core::config::bootstrap::BootstrapConfig::credential`
///   (crates/kiosk-core/src/config/bootstrap.rs:24) is documented and tested
///   only as that filename, and `ServiceAccount::from_json` (auth.rs:163)
///   parses a JSON *document*, not a path. So the caller resolves the real
///   on-disk location (install dir joined with the ini value, or `--config`'s
///   directory) and hands it here; `build` reads the file and parses it.
/// * `device_id` — the raw `bootstrap.device_id` is `Option::None` whenever
///   the ini leaves it blank for auto-resolution (bootstrap.rs:19); the value
///   telemetry must stamp into every entry's `insertId` and `resource.labels`
///   is the one `kiosk_core::identity::effective_device_id` already resolved
///   at boot (see `boot.rs`) / `ConfigManager::device_id()`. Reading the raw
///   optional field here would silently stamp an empty device id on exactly
///   the devices that rely on auto-resolution.
pub fn build(
    bootstrap: &BootstrapConfig,
    credential_path: &Path,
    device_id: &str,
    clock: TrustedClock,
    app_version: String,
    revision: Option<i64>,
    data_dir: &Path,
) -> Result<(Telemetry, Logger, Receiver<LogReq>), Box<dyn std::error::Error>> {
    let credential_json = std::fs::read_to_string(credential_path)?;
    let service_account = ServiceAccount::from_json(&credential_json)?;

    let transport: Arc<dyn Transport> = Arc::new(ReqwestTransport::new(Duration::from_secs(10))?);
    let token_source = TokenSource::new(service_account, transport.clone(), clock.clone());
    let client = GclClient::new(token_source, transport, clock.clone());

    // Spec defaults (schema.rs pins `spool_max_mb == 50`, `spool_reserve_high_mb
    // == 10`, `url_detail == Path`) until the first remote config is fetched and
    // its `[logging]` block can be threaded through here. Both the spool config
    // and `url_detail` are single-sourced from this one `Logging` via kiosk-core's
    // own `SpoolConfig::from_logging`, so a spec-default change tracks
    // automatically and nothing is re-derived (the layering rule: config logic
    // lives in kiosk-core, not here). Task 6 replaces `Logging::default()` with
    // the fetched config's `[logging]`.
    let logging = Logging::default();
    // Spec arch-01: kiosk-main owns the `main` spool partition, never the bare
    // `<data>/spool` root — the launcher's `drain_orphan` renames exactly
    // `<data>/spool/main` aside when this process dies, so the two must agree
    // on that path. `Spool::open` appends its own `spool/` component, so the
    // partition ROOT (not the final on-disk dir) is what's passed here —
    // mirrors kiosk-launcher's `LAUNCHER_PARTITION` in `sink.rs`.
    let spool = Spool::open(
        &data_dir.join("spool").join("main"),
        SpoolConfig::from_logging(&logging),
    )?;
    let limiter = RateLimiter::new(clock.clone());

    let ctx = entry_context(
        bootstrap,
        device_id,
        app_version,
        revision,
        logging.url_detail,
    );

    let logger = Logger::new(ctx, spool, client, limiter, clock);
    let (tx, rx) = mpsc::sync_channel(CHANNEL_CAPACITY);
    Ok((Telemetry { tx }, logger, rx))
}

/// The one place `EntryContext` is assembled from a `BootstrapConfig` — shared by
/// [`build`] (the live, GCL-backed path) and [`spool_boot_config_error`] (the
/// credential-less boot-gate path), so a `config.error` written by either produces the
/// byte-identical label/context shape.
fn entry_context(
    bootstrap: &BootstrapConfig,
    device_id: &str,
    app_version: String,
    revision: Option<i64>,
    url_detail: kiosk_core::config::schema::UrlDetail,
) -> EntryContext {
    EntryContext {
        project_id: bootstrap.project_id.clone(),
        device_id: device_id.to_string(),
        site: bootstrap.site.clone(),
        region: bootstrap
            .region
            .clone()
            .unwrap_or_else(|| bootstrap.site.clone()),
        app_version,
        config_revision: revision,
        url_detail,
    }
}

/// SEC-09 boot gate durability (Critical 1 fix): `build` above is never called when the
/// boot gate trips on a bad credential DACL — that would be the very credential read the
/// gate exists to prevent — so there is no live `Logger`/`GclClient` for `telem.config_error`
/// to reach. This writes the SAME event shape (`ConfigError`, `reason` field, identical
/// `EntryContext`) directly to the local [`Spool`], which needs no credential and no
/// network — exactly what must still work when the credential itself is refused. Best-
/// effort: a spool that fails to open/append here is no worse than the boot fault it is
/// trying to report, and must never turn into a second reason the kiosk fails to boot.
///
/// `logging` (SEC-09 final review, minor): the caller's LAST-GOOD `[logging]` block, not
/// a fresh `Logging::default()` — `Spool::open` reopens the SAME on-disk partition
/// `telemetry::build`'s live `Logger` would, so a site that tuned `max_mb`/`segment_mb`
/// away from spec defaults gets the SAME `SpoolConfig` here as everywhere else this
/// partition is opened, not a mismatched one for this one write. The boot-fault caller
/// (`main`) always still has a `ConfigManager` at this point — even the credential-DACL
/// fault path parses `kiosk.ini` fine and only refuses the credential — so its last-good
/// `current().logging` (spec defaults on a genuinely first boot, same as before) is
/// always available to pass here.
// One over clippy's 7-arg threshold, same call as `fetch::run`'s: every parameter is a
// distinct, already-resolved piece of the one boot-fault write this function makes —
// there is exactly one production caller (`main`) — and a params struct here would
// just be a second name for the same seven-plus-one fields.
#[allow(clippy::too_many_arguments)]
pub fn spool_boot_config_error(
    data_dir: &Path,
    bootstrap: &BootstrapConfig,
    device_id: &str,
    app_version: String,
    revision: Option<i64>,
    clock: &TrustedClock,
    reason: &str,
    logging: &Logging,
) {
    let Ok(mut spool) = Spool::open(
        &data_dir.join("spool").join("main"),
        SpoolConfig::from_logging(logging),
    ) else {
        return;
    };
    let ctx = entry_context(
        bootstrap,
        device_id,
        app_version,
        revision,
        logging.url_detail,
    );
    let mut fields = Map::new();
    fields.insert("error".into(), Value::from(reason));
    let Ok(seq) = spool.next_seq() else {
        return;
    };
    let entry = LogEntry::new(LogEvent::ConfigError, &ctx, seq, clock, fields);
    let _ = spool.append(&entry);
}

/// The logger loop — run on a DEDICATED OS THREAD (`std::thread`), never a tokio
/// task. The P1-B `Transport` is `reqwest::blocking`, which panics if it is built
/// OR driven inside a tokio runtime: `reqwest::blocking` stands up and tears down
/// its own internal runtime, and dropping a runtime inside an async context is
/// forbidden ("Cannot drop a runtime in a context where blocking is not allowed").
/// So the whole logger stays off the async runtime: this is a plain blocking loop
/// over a `std::sync::mpsc` channel, draining `LogReq`s and ticking the logger on
/// its own `FLUSH_INTERVAL` (spec TEL-07) when idle. Shutdown is best-effort: a
/// cancel or a disconnected channel flushes and exits (WARNING+ entries are already
/// fsynced synchronously as logged, so durability never depends on this flush).
///
/// The tick is DEADLINE-based, not a fresh `recv_timeout(FLUSH_INTERVAL)` per message.
/// `Logger::tick` is the only path that delivers batched entries and emits the
/// rate-limit summaries (TEL-07/TEL-09); `Logger::log` only appends. A naive
/// `recv_timeout(FLUSH_INTERVAL)` resets on every message, so a steady sub-interval
/// stream (e.g. a nav-error flood) would starve the tick forever — nothing delivered,
/// `pending` growing until the ring evicts it, no flood summary. Ticking on a fixed
/// `next_tick` deadline fires ~every FLUSH_INTERVAL regardless of load, matching the
/// old `tokio::time::interval` cadence.
///
/// `dropped_expired` is published here, into a shared atomic, on every pass: the
/// `Logger` lives on this dedicated thread and is never reachable from another
/// thread (see the doc above), so `health::run` (P1-D2e Task 2, a tokio task on
/// the async runtime) cannot call `Logger::dropped_expired(&self)` directly. This
/// is the smallest correct plumbing — reusing the existing counter rather than
/// inventing a second one — at the cost of the health sample reading a value that
/// is at most one loop-iteration stale, which is fine for a 10s+ heartbeat metric.
pub fn run(
    mut logger: Logger,
    rx: Receiver<LogReq>,
    cancel: CancellationToken,
    dropped_expired: Arc<AtomicU64>,
) {
    let mut next_tick = Instant::now() + FLUSH_INTERVAL;
    loop {
        dropped_expired.store(logger.dropped_expired(), Ordering::Relaxed);
        if cancel.is_cancelled() {
            let _ = logger.flush();
            break;
        }
        match rx.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
            Ok(req) => {
                logger.log(req.event, req.fields); // infallible; WARNING+ fsynced
                                                   // A relentless flood keeps `recv_timeout(0)` returning `Ok`; tick on
                                                   // the deadline anyway so delivery/summaries never starve.
                if Instant::now() >= next_tick {
                    logger.tick();
                    next_tick = Instant::now() + FLUSH_INTERVAL;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                logger.tick();
                next_tick = Instant::now() + FLUSH_INTERVAL;
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = logger.flush();
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::UrlDetail;
    use kiosk_core::logging::client::ENTRIES_WRITE_URL;
    use kiosk_core::logging::transport::{HttpResponse, TransportError};
    use std::sync::Mutex;

    /// A fake `Transport` that records every body posted to `entries:write`
    /// and answers the token endpoint with a canned bearer token — mirrors
    /// kiosk-core's own `logging::mod::tests::FakeTransport` (URL-routed, no
    /// live network), per the task brief's instruction to reuse that pattern
    /// rather than invent a new one.
    struct FakeTransport {
        writes: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                writes: Mutex::new(Vec::new()),
            })
        }
    }

    impl Transport for FakeTransport {
        fn post(
            &self,
            url: &str,
            _headers: &[(&str, &str)],
            body: &str,
        ) -> Result<HttpResponse, TransportError> {
            if url == ENTRIES_WRITE_URL {
                self.writes.lock().unwrap().push(body.to_string());
                return Ok(HttpResponse {
                    status: 200,
                    headers: vec![],
                    body: "{}".into(),
                });
            }
            // The token endpoint.
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: r#"{"access_token":"ya29.TEST","expires_in":3600}"#.into(),
            })
        }
    }

    /// A real, runtime-generated RSA keypair — never a committed fixture (see
    /// `kiosk_core::logging::auth`'s test docs on why: a committed
    /// `-----BEGIN PRIVATE KEY-----` trips every secret scanner, and the
    /// signing path (`TokenSource::mint` -> `sign_assertion`) genuinely needs a
    /// key that parses, since `Logger::flush` mints a token for real.
    fn test_service_account() -> ServiceAccount {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::RsaPrivateKey;

        let mut rng = rand::thread_rng();
        let private_pem = RsaPrivateKey::new(&mut rng, 2048)
            .expect("generate test RSA key")
            .to_pkcs8_pem(LineEnding::LF)
            .expect("encode private pem")
            .to_string();

        ServiceAccount::from_json(
            &serde_json::json!({
                "private_key": private_pem,
                "client_email": "kiosk-logger@test-project.iam.gserviceaccount.com",
                "token_uri": "https://oauth2.googleapis.com/token",
            })
            .to_string(),
        )
        .expect("fixture service account JSON parses")
    }

    fn established_clock() -> TrustedClock {
        let c = TrustedClock::new();
        c.observe_http_date("Sun, 12 Jul 2026 08:30:00 GMT")
            .expect("valid HTTP date");
        c
    }

    fn logger_with(dir: &std::path::Path, transport: Arc<FakeTransport>) -> Logger {
        let clock = established_clock();
        let client = GclClient::new(
            TokenSource::new(test_service_account(), transport.clone(), clock.clone()),
            transport,
            clock.clone(),
        );
        let spool = Spool::open(
            dir,
            SpoolConfig {
                max_mb: 50,
                reserve_high_mb: 10,
                segment_mb: 5,
            },
        )
        .expect("spool opens");
        Logger::new(
            EntryContext {
                project_id: "proj".into(),
                device_id: "lobby-01".into(),
                site: "hq".into(),
                region: "hq".into(),
                app_version: "0.1.0".into(),
                config_revision: None,
                url_detail: UrlDetail::Path,
            },
            spool,
            client,
            RateLimiter::new(clock.clone()),
            clock,
        )
    }

    /// Step 1 (TDD RED first): drive one `Telemetry::net_offline()`, hand its
    /// `LogReq` to a real `Logger` wired to a fake `Transport`, flush, and
    /// assert the event that actually reached (the fake) `entries:write` is
    /// `net.offline` — not merely that the channel accepted a message.
    ///
    /// `flush()` is used instead of `tick()`: `Logger::tick` only flushes once
    /// 10 s have elapsed or 100 entries are pending (spec TEL-07), so calling
    /// it immediately after `Logger::new` would not actually attempt a flush
    /// and the assertion below would be vacuous.
    #[test]
    fn net_offline_helper_emits_a_net_offline_event() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, rx) = mpsc::sync_channel(8);
        let telemetry = Telemetry { tx };
        telemetry.net_offline();

        let req = rx
            .try_recv()
            .expect("net_offline() must hand the logger task a LogReq");
        assert_eq!(req.event, LogEvent::NetOffline);
        assert!(req.fields.is_empty());

        logger.log(req.event, req.fields);
        logger
            .flush()
            .expect("flush against the fake transport must succeed");

        let writes = transport.writes.lock().unwrap();
        assert_eq!(
            writes.len(),
            1,
            "net.offline must have reached entries:write exactly once"
        );
        let posted: Value = serde_json::from_str(&writes[0]).unwrap();
        assert_eq!(posted["entries"][0]["jsonPayload"]["event"], "net.offline");
        assert_eq!(posted["entries"][0]["severity"], "WARNING");
    }

    /// Same shape as `net_offline_helper_emits_a_net_offline_event`, for the helper
    /// Task 6 added: `nav::install`'s `NavigationCompleted` handler is the only caller
    /// that can ever observe a failed navigation, so this pins the mapping onto the
    /// `nav.error` taxonomy entry (spec §6) it must reach.
    #[test]
    fn nav_error_helper_emits_a_nav_error_event_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, rx) = mpsc::sync_channel(8);
        let telemetry = Telemetry { tx };
        telemetry.nav_error("COREWEBVIEW2_WEB_ERROR_STATUS(12)");

        let req = rx
            .try_recv()
            .expect("nav_error() must hand the logger task a LogReq");
        assert_eq!(req.event, LogEvent::NavError);
        assert_eq!(
            req.fields["error"],
            Value::from("COREWEBVIEW2_WEB_ERROR_STATUS(12)")
        );

        logger.log(req.event, req.fields);
        logger
            .flush()
            .expect("flush against the fake transport must succeed");

        let writes = transport.writes.lock().unwrap();
        assert_eq!(writes.len(), 1, "nav.error must have reached entries:write");
        let posted: Value = serde_json::from_str(&writes[0]).unwrap();
        assert_eq!(posted["entries"][0]["jsonPayload"]["event"], "nav.error");
        assert_eq!(posted["entries"][0]["severity"], "WARNING");
    }

    /// Same shape as `nav_error_helper_emits_a_nav_error_event_with_the_reason`: pins
    /// `nav_blocked`'s mapping onto the `nav.blocked` taxonomy entry (spec §6) and
    /// checks the URL actually reaching the fake transport is redacted, never raw.
    #[test]
    fn nav_blocked_helper_emits_a_nav_blocked_event_with_reason_and_redacted_url() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, rx) = mpsc::sync_channel(8);
        let telemetry = Telemetry { tx };
        telemetry.nav_blocked("not_allowlisted", "https://evil.test/steal?token=secret");

        let req = rx
            .try_recv()
            .expect("nav_blocked() must hand the logger task a LogReq");
        assert_eq!(req.event, LogEvent::NavBlocked);
        assert_eq!(req.fields["reason"], Value::from("not_allowlisted"));
        assert_eq!(req.fields["url"], Value::from("https://evil.test"));
        assert!(!req.fields["url"].as_str().unwrap().contains("token"));

        logger.log(req.event, req.fields);
        logger
            .flush()
            .expect("flush against the fake transport must succeed");

        let writes = transport.writes.lock().unwrap();
        assert_eq!(
            writes.len(),
            1,
            "nav.blocked must have reached entries:write"
        );
        let posted: Value = serde_json::from_str(&writes[0]).unwrap();
        assert_eq!(posted["entries"][0]["jsonPayload"]["event"], "nav.blocked");
        assert_eq!(posted["entries"][0]["severity"], "WARNING");
        assert!(
            !writes[0].contains("token"),
            "raw query string must never reach the wire"
        );
    }

    /// Same shape as `nav_error_helper_emits_a_nav_error_event_with_the_reason`: pins
    /// `webview_crash`'s mapping onto the `webview.crash` taxonomy entry (spec §6,
    /// ERROR severity) that `recovery::install`'s `ProcessFailed` handler reaches.
    #[test]
    fn webview_crash_helper_emits_a_webview_crash_event_with_the_kind() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, rx) = mpsc::sync_channel(8);
        let telemetry = Telemetry { tx };
        telemetry.webview_crash("render_process_unresponsive");

        let req = rx
            .try_recv()
            .expect("webview_crash() must hand the logger task a LogReq");
        assert_eq!(req.event, LogEvent::WebviewCrash);
        assert_eq!(
            req.fields["kind"],
            Value::from("render_process_unresponsive")
        );

        logger.log(req.event, req.fields);
        logger
            .flush()
            .expect("flush against the fake transport must succeed");

        let writes = transport.writes.lock().unwrap();
        assert_eq!(
            writes.len(),
            1,
            "webview.crash must have reached entries:write"
        );
        let posted: Value = serde_json::from_str(&writes[0]).unwrap();
        assert_eq!(
            posted["entries"][0]["jsonPayload"]["event"],
            "webview.crash"
        );
        assert_eq!(posted["entries"][0]["severity"], "ERROR");
    }

    /// The disabled handle (used when telemetry init fails) must swallow every helper
    /// call with no panic and no logger task — its receiver is already dropped.
    #[test]
    fn disabled_telemetry_swallows_calls_without_panicking() {
        let telem = Telemetry::disabled();
        telem.app_start();
        telem.net_offline();
        telem.config_applied(Some(7), &["w".into()]);
        telem.nav_error("boom");
        telem.panic("boom");
        // No assertion beyond "did not panic": the whole contract is that a dropped
        // receiver turns every `try_send` into a discarded `Err(Closed)`.
    }

    fn test_bootstrap() -> BootstrapConfig {
        BootstrapConfig {
            config_url: "https://cfg.test/c.json".into(),
            device_id: None,
            site: "hq".into(),
            region: None,
            project_id: "proj".into(),
            credential: "cred.json".into(),
            startup_grace_s: 0,
            healthy_run_s: 0,
            channel_grace_s: 0,
            demo_mode: false,
            bootstrap_url: "https://site.test/".into(),
            exit_gesture: None,
        }
    }

    /// Critical 1 (SEC-09 review): `telem.config_error(reason)` at boot is a
    /// guaranteed no-op — `telem` is always `Telemetry::disabled()` in the exact
    /// scenario the boot gate exists for, since `telemetry::build` (the only thing
    /// that would ever produce a live `Logger`) is never called. This asserts the
    /// event actually lands on DISK — draining the same on-disk `Spool` a fresh
    /// process would reopen — not merely that some call was made.
    #[test]
    fn spool_boot_config_error_writes_a_durable_config_error_entry() {
        let dir = tempfile::tempdir().unwrap();
        let bootstrap = test_bootstrap();
        let clock = established_clock();

        spool_boot_config_error(
            dir.path(),
            &bootstrap,
            "lobby-01",
            "0.1.0".into(),
            None,
            &clock,
            "credential_permissions",
            &Logging::default(),
        );

        let mut spool = Spool::open(
            &dir.path().join("spool").join("main"),
            SpoolConfig {
                max_mb: 50,
                reserve_high_mb: 10,
                segment_mb: 5,
            },
        )
        .expect("spool reopens");
        let batch = spool.drain_batch(10).expect("drain succeeds");
        assert_eq!(
            batch.len(),
            1,
            "the config.error entry must actually be on disk, not just attempted"
        );
        assert_eq!(batch[0].json_payload["event"], Value::from("config.error"));
        assert_eq!(
            batch[0].json_payload["error"],
            Value::from("credential_permissions")
        );
        assert_eq!(batch[0].labels["device_id"], Value::from("lobby-01"));
    }

    /// A spool that cannot be opened (e.g. an unwritable path) must degrade silently —
    /// this is a best-effort durability write at boot, not a second reason to fail
    /// startup.
    #[test]
    fn spool_boot_config_error_never_panics_on_an_unusable_path() {
        let bootstrap = test_bootstrap();
        let clock = established_clock();
        // A file, not a directory, as the "data dir" — `Spool::open`'s `create_dir_all`
        // must fail against it.
        let file = tempfile::NamedTempFile::new().unwrap();
        spool_boot_config_error(
            file.path(),
            &bootstrap,
            "lobby-01",
            "0.1.0".into(),
            None,
            &clock,
            "credential_permissions",
            &Logging::default(),
        );
        // No assertion beyond "did not panic".
    }
}
