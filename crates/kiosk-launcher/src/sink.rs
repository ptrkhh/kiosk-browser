//! `LauncherSink` — the real `ActionSink`: it executes the `Action`s the E1
//! watchdog FSM returns and nothing else. Every restart/backoff/safe-mode/
//! exit-86 decision belongs to `kiosk_core::watchdog`; this module only spawns
//! processes, drains a dead main's orphaned spool, logs `watchdog.*` and exits.
//!
//! It also owns the launcher's Logger stack (the same P1-B primitives kiosk-main
//! wires in its `telemetry` module, over the launcher's OWN spool partition —
//! `<data>/spool/launcher`, spec arch-01: one writer per partition, so no
//! cross-process lock is needed). Telemetry is `try`-based throughout: a logging
//! failure is swallowed, never propagated, and never takes down the supervisor.

use std::io;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use kiosk_core::config::bootstrap::BootstrapConfig;
use kiosk_core::config::schema::Logging;
use kiosk_core::logging::auth::{ServiceAccount, TokenSource};
use kiosk_core::logging::client::GclClient;
use kiosk_core::logging::entry::EntryContext;
use kiosk_core::logging::spool::{Spool, SpoolConfig};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::logging::transport::{ReqwestTransport, Transport};
use kiosk_core::logging::{Logger, MAX_BATCH};
use kiosk_core::watchdog::{Action, Event, WatchdogEvent};
use serde_json::{Map, Value};

use crate::clock::now;
use crate::loop_::ActionSink;
use crate::spawn::spawn_main;

/// The launcher's telemetry: its own `Logger` (spooling to `spool/launcher`) plus
/// a second `GclClient` used only to drain a dead main's orphaned spool. Both share
/// one `TrustedClock` so a skew observed on either path is known to both.
struct Telemetry {
    logger: Logger,
    /// Deliberately NOT the Logger's client: `Logger` owns its own, and the orphan
    /// drain writes entries kiosk-main already stamped (its device/site context,
    /// its insertIds) rather than anything this process authored.
    drain_client: GclClient,
}

/// The launcher's spool partition (spec arch-01). `Spool::open` appends its own
/// `spool/` component, so the partition root is what's passed here.
const LAUNCHER_PARTITION: &str = "launcher";
/// kiosk-main's live partition. NEVER drained in place — it is renamed first, so
/// each partition keeps exactly one writer and one drainer.
const MAIN_PARTITION: &str = "main";
/// Where a dead main's partition is moved to before it is drained.
const ORPHAN_DIR: &str = "spool.orphaned";

/// Assembles the launcher's Logger stack. Mirrors kiosk-main's `telemetry::build`
/// (same P1-B primitives, same spec defaults) rather than extracting a shared
/// abstraction across the two binaries: the two stacks differ in partition,
/// context and lifecycle, and one of them is a Tauri app.
///
/// `Err` is not fatal to the launcher — see [`LauncherSink::new`].
fn build_telemetry(
    bootstrap: &BootstrapConfig,
    config_dir: &Path,
    data_dir: &Path,
) -> Result<Telemetry, Box<dyn std::error::Error>> {
    // The ini's `credential` names a file next to `kiosk.ini` (spec §4), not
    // inline JSON — same resolution kiosk-main does.
    let credential_json = std::fs::read_to_string(config_dir.join(&bootstrap.credential))?;
    let service_account = ServiceAccount::from_json(&credential_json)?;
    let device_id =
        kiosk_core::identity::effective_device_id(bootstrap.device_id.as_deref(), None)?;

    let clock = TrustedClock::new();
    let transport: Arc<dyn Transport> = Arc::new(ReqwestTransport::new(Duration::from_secs(10))?);
    let token_source = TokenSource::new(service_account, transport.clone(), clock.clone());
    let client = GclClient::new(token_source, transport.clone(), clock.clone());
    let drain_client = GclClient::new(
        TokenSource::new(
            ServiceAccount::from_json(&credential_json)?,
            transport.clone(),
            clock.clone(),
        ),
        transport,
        clock.clone(),
    );

    // Spec defaults, single-sourced from kiosk-core's `Logging` exactly as
    // kiosk-main does: the launcher never fetches remote config.
    let logging = Logging::default();
    let spool = Spool::open(
        &data_dir.join("spool").join(LAUNCHER_PARTITION),
        SpoolConfig::from_logging(&logging),
    )?;
    let ctx = EntryContext {
        project_id: bootstrap.project_id.clone(),
        device_id,
        site: bootstrap.site.clone(),
        region: bootstrap
            .region
            .clone()
            .unwrap_or_else(|| bootstrap.site.clone()),
        app_version: kiosk_core::app_version().to_string(),
        config_revision: None,
        url_detail: logging.url_detail,
    };
    let logger = Logger::new(
        ctx,
        spool,
        client,
        kiosk_core::logging::ratelimit::RateLimiter::new(clock.clone()),
        clock,
    );
    Ok(Telemetry {
        logger,
        drain_client,
    })
}

/// The enumerated jsonPayload for a `watchdog.*` entry. Only the fields the FSM
/// actually carries — no free-form content ever reaches telemetry.
pub fn fields_for(e: &WatchdogEvent) -> Map<String, Value> {
    let mut f = Map::new();
    match e {
        WatchdogEvent::Arm { time_to_ready_s } => {
            f.insert("time_to_ready_s".into(), Value::from(*time_to_ready_s));
        }
        WatchdogEvent::Restart {
            code,
            backoff_s,
            cause,
        } => {
            f.insert("code".into(), Value::from(*code));
            f.insert("backoff_s".into(), Value::from(*backoff_s));
            f.insert("cause".into(), Value::from(*cause));
        }
        WatchdogEvent::Hang
        | WatchdogEvent::ChannelReset
        | WatchdogEvent::SafeMode
        | WatchdogEvent::SafeModeFailed => {}
    }
    f
}

fn spool_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// Drains one already-renamed orphan partition, removing it once it is empty.
/// Returns the number of entries delivered.
fn drain_dir(dir: &Path, client: &mut GclClient) -> io::Result<usize> {
    let mut spool =
        Spool::open(dir, SpoolConfig::from_logging(&Logging::default())).map_err(spool_err)?;
    let mut delivered = 0usize;
    loop {
        let batch = spool.drain_batch(MAX_BATCH).map_err(spool_err)?;
        if batch.is_empty() {
            break;
        }
        client.write(&batch).map_err(spool_err)?;
        let committed = spool.commit_drained(&batch).map_err(spool_err)?;
        delivered += batch.len();
        if committed == 0 {
            // A blocked commit prefix (see `Logger::flush`'s poison-entry notes)
            // would otherwise re-drain the same batch forever. The partition is
            // left in place; the next restart retries it.
            break;
        }
    }
    // Fully drained: the partition has served its purpose.
    std::fs::remove_dir_all(dir)?;
    Ok(delivered)
}

/// Move kiosk-main's dead spool partition aside and deliver whatever it still
/// holds (spec arch-01 / TEL-10: main's pre-death context reaches Cloud Logging
/// even though main is gone).
///
/// `<data>/spool/main` is kiosk-main's LIVE partition and is never drained in
/// place — it is renamed to `<data>/spool.orphaned` first, so every partition
/// keeps exactly one writer and one drainer and no cross-process lock is needed.
/// A leftover `spool.orphaned` (a previous drain that failed midway) is drained
/// first, before the rename can need its name.
///
/// A missing partition is `Ok(0)`, not an error.
pub fn drain_orphan(data_dir: &Path, client: &mut GclClient) -> io::Result<usize> {
    let live = data_dir.join("spool").join(MAIN_PARTITION);
    let orphan = data_dir.join(ORPHAN_DIR);

    let mut delivered = 0usize;
    if orphan.exists() {
        delivered += drain_dir(&orphan, client)?;
    }
    if !live.exists() {
        return Ok(delivered);
    }
    std::fs::rename(&live, &orphan)?;
    delivered += drain_dir(&orphan, client)?;
    Ok(delivered)
}

/// Executes the FSM's `Action`s: spawn, drain, log, exit.
pub struct LauncherSink {
    exe: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    pipe_name: String,
    tx: Sender<Event>,
    /// The supervised child's handle, so a still-live predecessor can be killed
    /// before it is replaced (dropping a `Child` does NOT kill a Windows process,
    /// and an orphan keeps holding the single-instance mutex and the pipe).
    child: Option<Child>,
    /// Shared with `pipe::serve`; 0 means "no live child". This sink is its only
    /// writer — see `pipe`'s module docs for what it gates.
    child_pid: Arc<AtomicU32>,
    /// `None` when the credential/config could not be read: the supervisor runs
    /// blind rather than refusing to start (a black screen is worse than a
    /// telemetry gap — the same trade kiosk-main makes with `Telemetry::disabled`).
    telemetry: Option<Telemetry>,
}

impl LauncherSink {
    pub fn new(
        exe: PathBuf,
        config_dir: PathBuf,
        data_dir: PathBuf,
        pipe_name: String,
        tx: Sender<Event>,
        child_pid: Arc<AtomicU32>,
        bootstrap: Option<&BootstrapConfig>,
    ) -> LauncherSink {
        let telemetry = bootstrap.and_then(|b| match build_telemetry(b, &config_dir, &data_dir) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("kiosk-launcher: telemetry disabled ({e}); supervising without it");
                None
            }
        });
        LauncherSink {
            exe,
            config_dir,
            data_dir,
            pipe_name,
            tx,
            child: None,
            child_pid,
            telemetry,
        }
    }

    /// Kill whatever child is still held, so a hang-restart never leaves an
    /// orphaned kiosk-main holding the single-instance mutex and the pipe.
    fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
        }
    }

    fn spawn(&mut self, safe: bool) {
        self.kill_child();
        match spawn_main(
            &self.exe,
            &self.config_dir,
            safe,
            &self.pipe_name,
            self.tx.clone(),
        ) {
            Ok(child) => {
                // Publish the PID BEFORE storing the handle: `pipe::serve` may
                // already be waiting for it to authenticate the child's connect.
                self.child_pid.store(child.id(), Ordering::Relaxed);
                self.child = Some(child);
            }
            Err(e) => {
                eprintln!(
                    "kiosk-launcher: spawning {} failed: {e}",
                    self.exe.display()
                );
                // `spawn_main`'s `Err` contract: it sent no exit event and no
                // supervised child exists. EXACTLY ONE synthetic exit per `Err`,
                // so the FSM's backoff governs the retry and never busy-loops.
                self.child_pid.store(0, Ordering::Relaxed);
                let _ = self.tx.send(Event::ChildExited {
                    code: -1,
                    at: now(),
                });
            }
        }
    }

    fn log(&mut self, e: &WatchdogEvent) {
        let Some(t) = self.telemetry.as_mut() else {
            return;
        };
        t.logger.log(e.log_event(), fields_for(e));
        // ponytail: flush on every watchdog event instead of running a dedicated
        // 10 s tick thread. `watchdog.*` entries are rare and all WARNING+, and a
        // supervisor that only delivered on exit would never report the restart
        // loop it is living through. Add a tick thread if the launcher ever logs
        // anything high-volume.
        let _ = t.logger.flush();
    }
}

impl ActionSink for LauncherSink {
    fn dispatch(&mut self, action: Action) -> ControlFlow<i32> {
        match action {
            Action::SpawnMain => self.spawn(false),
            Action::SpawnSafe => self.spawn(true),
            Action::DrainOrphanedSpool => {
                // The child that owned that partition is gone: nothing lives on
                // the shared PID until the next spawn publishes one.
                self.child_pid.store(0, Ordering::Relaxed);
                if let Some(t) = self.telemetry.as_mut() {
                    if let Err(e) = drain_orphan(&self.data_dir, &mut t.drain_client) {
                        // A lost drain is not fatal (design.md, "Error handling").
                        eprintln!("kiosk-launcher: orphan spool drain failed: {e}");
                    }
                }
            }
            Action::Log(e) => self.log(&e),
            Action::ExitLauncher { code } => {
                self.kill_child();
                self.child_pid.store(0, Ordering::Relaxed);
                if let Some(t) = self.telemetry.as_mut() {
                    let _ = t.logger.flush();
                }
                return ControlFlow::Break(code);
            }
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::UrlDetail;
    use kiosk_core::logging::client::ENTRIES_WRITE_URL;
    use kiosk_core::logging::event::Event as LogEvent;
    use kiosk_core::logging::ratelimit::RateLimiter;
    use kiosk_core::logging::transport::{HttpResponse, TransportError};
    use std::sync::Mutex;

    /// A fake `Transport` that records every body posted to `entries:write` and
    /// answers the token endpoint with a canned bearer token — mirrors kiosk-main's
    /// `telemetry::tests::FakeTransport` (URL-routed, no live network).
    struct FakeTransport {
        writes: Mutex<Vec<String>>,
    }

    impl FakeTransport {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                writes: Mutex::new(Vec::new()),
            })
        }
        /// Every `insertId` that reached `entries:write`, in post order.
        fn insert_ids(&self) -> Vec<String> {
            self.writes
                .lock()
                .unwrap()
                .iter()
                .flat_map(|body| {
                    let v: Value = serde_json::from_str(body).unwrap();
                    v["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|e| e["insertId"].as_str().unwrap().to_string())
                        .collect::<Vec<_>>()
                })
                .collect()
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
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: r#"{"access_token":"ya29.TEST","expires_in":3600}"#.into(),
            })
        }
    }

    /// A real, runtime-generated RSA keypair — never a committed fixture (a
    /// `-----BEGIN PRIVATE KEY-----` in the repo is the highest-signal pattern
    /// secret scanners hunt for). It has to be a key that actually PARSES and
    /// SIGNS: the drain path calls `GclClient::write`, which mints a service-
    /// account JWT locally (`TokenSource::mint` -> `sign_assertion`) before the
    /// fake transport ever sees a request. Generated ONCE per test binary —
    /// 2048-bit keygen in a debug build costs tens of seconds.
    static TEST_KEY_PEM: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        use rsa::RsaPrivateKey;

        RsaPrivateKey::new(&mut rand::thread_rng(), 2048)
            .expect("generate test RSA key")
            .to_pkcs8_pem(LineEnding::LF)
            .expect("encode private pem")
            .to_string()
    });

    fn test_service_account() -> ServiceAccount {
        let private_pem = &*TEST_KEY_PEM;
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

    fn fake_client(transport: Arc<FakeTransport>) -> GclClient {
        let clock = established_clock();
        GclClient::new(
            TokenSource::new(test_service_account(), transport.clone(), clock.clone()),
            transport,
            clock,
        )
    }

    /// Writes `n` entries into `<data>/spool/main`, exactly as a live kiosk-main
    /// would (a real `Logger` over that partition), and leaves them uncommitted.
    fn spool_main_entries(data_dir: &Path, n: usize) {
        let clock = established_clock();
        let spool = Spool::open(
            &data_dir.join("spool").join(MAIN_PARTITION),
            SpoolConfig::from_logging(&Logging::default()),
        )
        .expect("main partition opens");
        let mut logger = Logger::new(
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
            fake_client(FakeTransport::new()),
            RateLimiter::new(clock.clone()),
            clock,
        );
        for _ in 0..n {
            logger.log(LogEvent::AppStart, Map::new());
        }
    }

    /// Brief Step 1: a dead main's partition is moved aside and delivered, and
    /// `spool/main` is gone afterwards — never drained in place.
    #[test]
    fn drain_orphan_renames_main_and_delivers_its_entries() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        spool_main_entries(data, 3);

        let transport = FakeTransport::new();
        let mut client = fake_client(transport.clone());
        let n = drain_orphan(data, &mut client).expect("drain succeeds");

        assert_eq!(n, 3, "every spooled entry must be delivered");
        assert!(
            !data.join("spool").join(MAIN_PARTITION).exists(),
            "main's live partition must be renamed away, never drained in place"
        );
        assert_eq!(
            transport.insert_ids(),
            vec![
                "lobby-01-1".to_string(),
                "lobby-01-2".to_string(),
                "lobby-01-3".to_string()
            ],
            "the entries that reached the wire must be main's own, insertIds intact"
        );
        assert!(
            !data.join(ORPHAN_DIR).exists(),
            "a fully drained orphan partition is removed"
        );
    }

    /// A device that never had a main partition (first boot) must not error.
    #[test]
    fn drain_orphan_with_no_main_partition_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let transport = FakeTransport::new();
        let mut client = fake_client(transport.clone());

        assert_eq!(drain_orphan(dir.path(), &mut client).unwrap(), 0);
        assert!(transport.insert_ids().is_empty());
    }

    /// `fields_for` shapes the enumerated payload the taxonomy expects.
    #[test]
    fn fields_for_carries_the_restart_triple_and_nothing_else() {
        let f = fields_for(&WatchdogEvent::Restart {
            code: 137,
            backoff_s: 8,
            cause: "hang",
        });
        assert_eq!(f["code"], Value::from(137));
        assert_eq!(f["backoff_s"], Value::from(8u64));
        assert_eq!(f["cause"], Value::from("hang"));
        assert_eq!(f.len(), 3);

        assert_eq!(
            fields_for(&WatchdogEvent::Arm { time_to_ready_s: 4 })["time_to_ready_s"],
            Value::from(4u64)
        );
        assert!(fields_for(&WatchdogEvent::SafeMode).is_empty());
    }
}
