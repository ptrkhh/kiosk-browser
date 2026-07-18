//! Telemetry task (spec §6, P1-D2a Task 5): wires kiosk-core's P1-B logger stack
//! (GCL client, RS256 service-account auth, disk spool, rate limiter, trusted
//! clock) behind a cheap, cloneable, `Send` handle so the rest of kiosk-main can
//! fire an event without ever blocking or panicking on a logging failure. This
//! module only WIRES the stack; every logging behavior it exercises (spooling,
//! batching, retry, rate-limiting) already lives in `kiosk_core::logging` and is
//! not reimplemented here.
//!
//! Wired into `main.rs` (Task 6): `main` calls [`build`] from a real `Booted` and
//! spawns [`run`].

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::config::schema::Logging;
use kiosk_core::logging::auth::{ServiceAccount, TokenSource};
use kiosk_core::logging::client::GclClient;
use kiosk_core::logging::entry::EntryContext;
use kiosk_core::logging::event::Event as LogEvent;
use kiosk_core::logging::ratelimit::RateLimiter;
use kiosk_core::logging::spool::{Spool, SpoolConfig};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::logging::transport::{ReqwestTransport, Transport};
use kiosk_core::logging::{Logger, FLUSH_INTERVAL};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
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
    tx: mpsc::Sender<LogReq>,
}

impl Telemetry {
    /// A telemetry handle that silently drops everything. Used when [`build`] fails
    /// (missing/malformed credential) so the kiosk still shows content — telemetry is
    /// never worth a black screen. The receiver is dropped immediately, so every
    /// `emit`/`try_send` returns `Err(Closed)` and is discarded, exactly like a full
    /// queue. No logger task is spawned for this handle.
    pub fn disabled() -> Telemetry {
        let (tx, _rx) = mpsc::channel(1);
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
) -> Result<(Telemetry, Logger, mpsc::Receiver<LogReq>), Box<dyn std::error::Error>> {
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
    let spool = Spool::open(data_dir, SpoolConfig::from_logging(&logging))?;
    let limiter = RateLimiter::new(clock.clone());

    let ctx = EntryContext {
        project_id: bootstrap.project_id.clone(),
        device_id: device_id.to_string(),
        site: bootstrap.site.clone(),
        region: bootstrap
            .region
            .clone()
            .unwrap_or_else(|| bootstrap.site.clone()),
        app_version,
        config_revision: revision,
        url_detail: logging.url_detail,
    };

    let logger = Logger::new(ctx, spool, client, limiter, clock);
    let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
    Ok((Telemetry { tx }, logger, rx))
}

/// The logger task: owns the `Logger`, drains `LogReq`s, and ticks it on the
/// logger's own `FLUSH_INTERVAL` (spec TEL-07) so a steady trickle of events
/// still flushes on schedule even when nothing new arrives.
pub async fn run(mut logger: Logger, mut rx: mpsc::Receiver<LogReq>, cancel: CancellationToken) {
    let mut tick = tokio::time::interval(FLUSH_INTERVAL);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = logger.flush();
                break;
            }
            _ = tick.tick() => logger.tick(),
            maybe = rx.recv() => match maybe {
                Some(req) => logger.log(req.event, req.fields), // infallible; WARNING+ fsynced
                None => {
                    let _ = logger.flush();
                    break;
                }
            },
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
    #[tokio::test]
    async fn net_offline_helper_emits_a_net_offline_event() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, mut rx) = mpsc::channel(8);
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
    #[tokio::test]
    async fn nav_error_helper_emits_a_nav_error_event_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut logger = logger_with(dir.path(), transport.clone());

        let (tx, mut rx) = mpsc::channel(8);
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
}
