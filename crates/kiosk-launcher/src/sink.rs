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
use crate::credential_acl;
use crate::job::Job;
use crate::loop_::ActionSink;
use crate::spawn::{spawn_main, ChildHandle};

/// The exact message SEC-09's launcher gate reports (mirrors kiosk-main's boot
/// gate — same wording, same fault, two separate binaries reading the same
/// credential). Surfaces via `build_telemetry`'s `Err`, which `LauncherSink::new`
/// already treats exactly like any other degraded-telemetry start: breadcrumb,
/// `telemetry: None`, supervision continues unaffected.
const CREDENTIAL_PERMISSIONS_MESSAGE: &str =
    "credential file permissions are not owner-only — refusing to load";

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
    let credential_path = config_dir.join(&bootstrap.credential);

    // SEC-09 launcher gate: verify the DACL before ever reading the credential's
    // contents. `Ok(false)`/`Err` (fail closed) surfaces as an `Err` here, which
    // `LauncherSink::new` already treats exactly like a missing/malformed
    // credential — `telemetry: None`, a breadcrumb, and supervision (spawn/
    // watch/restart, all in `dispatch`) is untouched because it never reads
    // `self.telemetry` in the first place.
    if credential_acl::is_violation(credential_acl::credential_is_owner_only(&credential_path)) {
        return Err(CREDENTIAL_PERMISSIONS_MESSAGE.into());
    }

    let credential_json = std::fs::read_to_string(&credential_path)?;
    let service_account = ServiceAccount::from_json(&credential_json)?;
    let device_id =
        kiosk_core::identity::effective_device_id(bootstrap.device_id.as_deref(), None)?;

    let clock = TrustedClock::new();
    // 3s, deliberately SHORTER than kiosk-main's 10s. `LauncherSink::log`
    // calls `Logger::flush` synchronously on the single supervise thread, and
    // `flush` does not honour the `retry_after` backoff that `Logger::tick`
    // does — so an OFFLINE device would pay the full timeout twice (token +
    // entries:write) on every watchdog event, parking the supervisor for ~20s
    // per restart and delaying both respawns and the exit-86 handoff. The
    // launcher's entries are rare and spooled: a dropped delivery attempt costs
    // nothing (the next flush retries from the spool), an unresponsive
    // supervisor costs the screen. P2: give the sink a flush thread, or route
    // it through `Logger::tick` so the backoff applies, and this can go back up.
    let transport: Arc<dyn Transport> = Arc::new(ReqwestTransport::new(Duration::from_secs(3))?);
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

/// Operator-facing breadcrumb file for a degraded start (`<data>/startup-degraded.txt`).
///
/// Under a Scheduled Task or an MSI-installed service there is NO console, so the
/// `eprintln!`s on these paths go nowhere and a telemetry-dead device is
/// indistinguishable from a healthy one. This file is the signal that survives
/// having no console — same discipline as kiosk-main's `crash-panic.txt`: data
/// dir, plain text, `File::create` (truncating, last-writer-wins), and every
/// failure swallowed, because diagnostics must never be able to stop the
/// supervisor from starting.
///
/// Format: exactly one line, `<unix_seconds> <reason>: <detail>`.
/// `reason` is a stable enumerated token (`config`, `telemetry`); `detail` is the
/// underlying error's Display. Readers must tolerate the file being missing,
/// empty, or truncated.
///
/// Lifecycle: written on any degraded start, removed on the next fully-healthy
/// start (bootstrap config parsed AND telemetry built). Presence therefore means
/// "the LAST boot was degraded", not "some boot, once, was degraded" — a fleet
/// check must read it that way.
pub fn breadcrumb(data_dir: &Path, reason: &str, detail: &str) {
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(mut f) = std::fs::File::create(data_dir.join(DEGRADED_FILE)) {
        use std::io::Write;
        let _ = writeln!(f, "{} {reason}: {detail}", now());
        let _ = f.sync_all();
    }
}

/// See [`breadcrumb`]. Operator-facing contract — do not rename.
pub const DEGRADED_FILE: &str = "startup-degraded.txt";

/// Like [`breadcrumb`], but never overwrites one already written this boot.
///
/// The file is a single line and `breadcrumb` truncates, so it is
/// last-writer-wins. The supervision-hardening warnings (`job`, `mutex`) are
/// the LEAST severe things that write it — a device that cannot read its
/// `kiosk.ini` has no telemetry at all and no way to be told so — and must not
/// bury a louder one that a `config`/`telemetry`/`pipe` failure already left.
///
/// Best-effort, like everything on this path: the check and the write are not
/// atomic against `pipe::serve`'s own breadcrumb on another thread. Losing that
/// race costs one diagnostic line, never correctness.
pub fn breadcrumb_if_absent(data_dir: &Path, reason: &str, detail: &str) {
    if !data_dir.join(DEGRADED_FILE).exists() {
        breadcrumb(data_dir, reason, detail);
    }
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
        // Trust `commit_drained`, never `batch.len()`: an acked entry sitting
        // behind a still-unacked one stays on disk, so only the committed count
        // is honest about what actually left this device.
        let committed = spool.commit_drained(&batch).map_err(spool_err)?;
        delivered += committed;
        if committed == 0 {
            // A blocked commit prefix (see `Logger::flush`'s poison-entry notes)
            // would otherwise re-drain the same batch forever. Return WITHOUT
            // removing the directory: everything behind the poison prefix is
            // still undelivered, and the next restart retries the partition.
            return Ok(delivered);
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
    /// and an orphan keeps holding the heartbeat pipe — `nMaxInstances` is 1, so
    /// the new child could never connect). Two kiosk-mains from two LAUNCHERS is
    /// now prevented upstream, by `job::acquire_single_instance` in `main`.
    child: Option<ChildHandle>,
    /// The kill-on-close Job Object every spawned child is assigned to, so an
    /// unexpected launcher death (taskkill, panic, fast shutdown) takes
    /// kiosk-main with it instead of leaving an unsupervised full-screen orphan.
    ///
    /// Owned HERE because this struct lives for the launcher's whole run: the
    /// job fires when its last handle closes, so a `Job` dropped early kills the
    /// kiosk on the spot rather than merely disabling the feature.
    ///
    /// `None` only when `job::create` failed at startup (WARNING + continue —
    /// see `main`): supervision still works, the child just outlives an
    /// unexpected launcher death, i.e. exactly the pre-P1-F1 behaviour.
    job: Option<Job>,
    /// Shared with `pipe::serve`; 0 means "no live child". This sink is its only
    /// writer — see `pipe`'s module docs for what it gates.
    child_pid: Arc<AtomicU32>,
    /// `None` when the credential/config could not be read: the supervisor runs
    /// blind rather than refusing to start (a black screen is worse than a
    /// telemetry gap — the same trade kiosk-main makes with `Telemetry::disabled`).
    telemetry: Option<Telemetry>,
}

impl LauncherSink {
    // 8 flat arguments, one over clippy's threshold. They are the launcher's
    // whole wiring, every one is a distinct type, and there is exactly ONE
    // production caller (`main`) plus the test harnesses — a params struct here
    // would be a second name for `LauncherSink`'s own fields and nothing else.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        exe: PathBuf,
        config_dir: PathBuf,
        data_dir: PathBuf,
        pipe_name: String,
        tx: Sender<Event>,
        child_pid: Arc<AtomicU32>,
        bootstrap: Option<&BootstrapConfig>,
        job: Option<Job>,
    ) -> LauncherSink {
        let telemetry = bootstrap.and_then(|b| match build_telemetry(b, &config_dir, &data_dir) {
            Ok(t) => {
                // Bootstrap parsed AND telemetry built: this boot took no
                // degraded path, so any breadcrumb left by a PAST degraded boot
                // is now stale. Best-effort, same as `breadcrumb` itself — a
                // failed delete must never affect startup.
                let _ = std::fs::remove_file(data_dir.join(DEGRADED_FILE));
                Some(t)
            }
            Err(e) => {
                eprintln!("kiosk-launcher: telemetry disabled ({e}); supervising without it");
                breadcrumb(&data_dir, "telemetry", &e.to_string());
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
            job,
        }
    }

    /// Kill whatever child is still held, so a hang-restart never leaves an
    /// orphaned kiosk-main holding the heartbeat pipe's single instance.
    ///
    /// Kills AND WAITS (bounded — see `spawn::kill_and_wait`): `TerminateProcess`
    /// returns before the kernel closes the dying process's handles, and the very
    /// next thing the `DrainOrphanedSpool` arm does is rename `spool/main` out
    /// from under it. Without the wait that rename races kiosk-main's cached
    /// spool segment file and fails `ERROR_SHARING_VIOLATION`, silently dropping
    /// the dead child's pre-death telemetry (TEL-10) on exactly the hang /
    /// no_ready / channel restarts whose diagnostics matter most. Waiting also
    /// closes the `ChannelFault`-after-kill race: the pipe reader's read error
    /// can no longer beat the `child_pid.store(0)` that follows.
    fn kill_child(&mut self) {
        if let Some(mut child) = self.child.take() {
            crate::spawn::kill_and_wait(&mut child);
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
                // Assign to the job FIRST, before anything else this arm does.
                //
                // ACCEPTED RACE (P1): `std::process::Command` starts the child
                // running, so between `spawn_main` returning and this call there
                // is a window — one syscall wide — in which the child is not yet
                // in the job and would survive a launcher death. Closing it
                // properly means CREATE_SUSPENDED plus a resume, and
                // `std::process::Child` exposes no thread handle to resume
                // (`ResumeThread` needs one; the alternatives are undocumented
                // `NtResumeProcess` or a thread-table walk). Not worth that for
                // a window whose worst case is the pre-P1-F1 status quo — an
                // orphaned kiosk-main — rather than a regression. Revisit if the
                // spawn path ever moves to raw `CreateProcessW`.
                if let Some(job) = self.job.as_ref() {
                    if let Err(e) = job.assign(&child) {
                        // WARNING + continue: assignment can legitimately fail
                        // under some CI/container/debugger job configurations,
                        // and a supervised-but-unkillable kiosk still beats no
                        // kiosk. Same operator channel as every other degraded
                        // start — there is no console on a deployed device.
                        eprintln!(
                            "kiosk-launcher: could not assign kiosk-main (pid {}) to the job object ({e}); it will survive an unexpected launcher death",
                            child.id()
                        );
                        breadcrumb_if_absent(&self.data_dir, "job", &e.to_string());
                    }
                }
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
                // `Watchdog::restart` emits this for ALL FOUR causes, and only
                // `exit` means the child is already dead: on `hang`, `no_ready`
                // and `channel` it is still running and still WRITING to
                // `spool/main`. Renaming and deleting a live partition would
                // destroy the one-writer-per-partition invariant arch-01 exists
                // to provide, so the child dies here, before the PID is zeroed
                // and before anything is moved. On the `exit` path this kills an
                // already-exited `Child` — a no-op that emits nothing (the
                // waiter thread already sent its one `ChildExited`).
                self.kill_child();
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
    use kiosk_core::logging::entry::LogEntry;
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

    /// A poisoned commit prefix must NOT cost the operator the entries behind it.
    ///
    /// The poison is built the way `Spool` actually produces one: `drain_batch`
    /// takes up to `MAX_BATCH` from EACH ring, merges, sorts by `(timestamp,
    /// insert_id)` and truncates back to `MAX_BATCH`. Give each ring 100 entries
    /// whose FIRST on-disk line is the newest, and the global truncate cuts both
    /// heads — so `Ring::commit` stops dead at line 0 in both rings and
    /// `commit_drained` returns 0 for a full 100-entry batch.
    #[test]
    fn a_blocked_commit_prefix_keeps_the_partition_and_reports_zero_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        {
            let mut spool = Spool::open(
                &data.join("spool").join(MAIN_PARTITION),
                SpoolConfig::from_logging(&Logging::default()),
            )
            .expect("main partition opens");
            let ctx = EntryContext {
                project_id: "proj".into(),
                device_id: "x".into(),
                site: "hq".into(),
                region: "hq".into(),
                app_version: "0.1.0".into(),
                config_revision: None,
                url_detail: UrlDetail::Path,
            };
            let clock = established_clock();
            let mut seq = 1u64;
            // NavBlocked is WARNING (high ring), AppStart is INFO (low ring).
            for event in [LogEvent::NavBlocked, LogEvent::AppStart] {
                for i in 0..MAX_BATCH {
                    let mut e = LogEntry::new(event, &ctx, seq, &clock, Map::new());
                    seq += 1;
                    e.timestamp = Some(if i == 0 {
                        "2026-07-12T09:00:00+00:00".into() // the head: newest, so it sorts out of the batch
                    } else {
                        format!("2026-07-12T08:30:{:02}+00:00", i % 60)
                    });
                    spool.append(&e).expect("append");
                }
            }
        }

        let transport = FakeTransport::new();
        let mut client = fake_client(transport.clone());
        let n = drain_orphan(data, &mut client).expect("drain succeeds");

        assert_eq!(
            n, 0,
            "the count must reflect committed entries only, not the batch that was posted"
        );
        assert!(
            data.join(ORPHAN_DIR).exists(),
            "a partition with undelivered entries behind a poison prefix must survive"
        );
        assert_eq!(
            transport.insert_ids().len(),
            MAX_BATCH,
            "one batch was posted, then the drain stopped instead of re-posting it forever"
        );
    }

    /// The degraded-start breadcrumb: one line, `<unix_seconds> <reason>: <detail>`,
    /// in the data dir — the operator-facing contract the P1-F runbook reads.
    /// A data dir that does not exist yet is created; an unwritable one is silent.
    #[test]
    fn a_degraded_start_leaves_a_readable_breadcrumb() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("kiosk"); // deliberately not created yet
        breadcrumb(
            &data,
            "config",
            "D:\\kiosk\\kiosk.ini is not a valid kiosk.ini: boom",
        );

        let text = std::fs::read_to_string(data.join(DEGRADED_FILE)).expect("breadcrumb written");
        let (ts, rest) = text
            .trim_end()
            .split_once(' ')
            .expect("`<ts> <reason>: <detail>`");
        assert!(
            ts.parse::<u64>().unwrap() > 1_700_000_000,
            "unix seconds, got {ts}"
        );
        assert_eq!(
            rest,
            "config: D:\\kiosk\\kiosk.ini is not a valid kiosk.ini: boom"
        );
        assert_eq!(text.lines().count(), 1, "exactly one line");
    }

    /// The hardening warnings are the least severe things that write the
    /// single-line breadcrumb, so they must claim it when it is free and leave
    /// it alone when a louder failure already has it — otherwise a device with
    /// an unreadable `kiosk.ini` (no telemetry AT ALL) reports itself to the
    /// operator as merely missing kill-on-close.
    #[test]
    fn breadcrumb_if_absent_never_buries_a_louder_warning() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();

        breadcrumb_if_absent(data, "job", "kill-on-close unavailable");
        let text = std::fs::read_to_string(data.join(DEGRADED_FILE)).unwrap();
        assert!(
            text.contains("job:"),
            "an unclaimed breadcrumb must be written, got {text:?}"
        );

        // A more severe failure claims the file...
        breadcrumb(data, "config", "cannot read kiosk.ini");
        // ...and a later hardening warning must not overwrite it.
        breadcrumb_if_absent(data, "mutex", "access denied");

        let text = std::fs::read_to_string(data.join(DEGRADED_FILE)).unwrap();
        assert!(
            text.contains("config:"),
            "the louder warning must survive, got {text:?}"
        );
    }

    /// A fully-healthy start (bootstrap parses AND telemetry builds) must clear a
    /// breadcrumb left by a PAST degraded boot — otherwise presence of the file
    /// stops meaning "the last boot was degraded" and a fixed device looks
    /// permanently sick to any fleet check shaped as "does this file exist".
    #[test]
    fn a_healthy_start_clears_a_pre_existing_breadcrumb() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();

        // A breadcrumb from some earlier, degraded boot.
        breadcrumb(
            &data_dir,
            "config",
            "stale, from a boot that has since healed",
        );
        assert!(data_dir.join(DEGRADED_FILE).exists());

        // A real, parseable credential next to `kiosk.ini`, exactly as
        // `build_telemetry` expects to find it. Forced owner-only (see
        // `force_owner_only_acl`'s doc): a fresh temp file's inherited ACL is not
        // guaranteed owner-only, and this test needs the SEC-09 gate to pass, not
        // merely the JSON to parse.
        let cred_path = config_dir.join("cred.json");
        std::fs::write(&cred_path, test_service_account_json()).unwrap();
        #[cfg(windows)]
        force_owner_only_acl(&cred_path);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&cred_path, std::fs::Permissions::from_mode(0o600)).unwrap();
            if !credential_acl::credential_is_owner_only(&cred_path).unwrap_or(false) {
                eprintln!("skipping owner-only credential test on a mode-insensitive filesystem");
                return;
            }
        }
        let bootstrap = BootstrapConfig::parse(
            "[kiosk]\nconfig_url = https://e/c.json\nsite = hq\nproject_id = p\n\
             credential = cred.json\ndevice_id = lobby-01\n\n[bootstrap]\nurl = https://app.example.com/\n",
        )
        .expect("valid ini");

        let (tx, _rx) = std::sync::mpsc::channel();
        let sink = LauncherSink::new(
            PathBuf::from("kiosk-main.exe"),
            config_dir,
            data_dir.clone(),
            "test-pipe".into(),
            tx,
            Arc::new(AtomicU32::new(0)),
            Some(&bootstrap),
            None, // no job object: these tests only exercise the breadcrumb lifecycle
        );

        assert!(
            sink.telemetry.is_some(),
            "telemetry must have built successfully for this to be the healthy path under test"
        );
        assert!(
            !data_dir.join(DEGRADED_FILE).exists(),
            "a fully-healthy start must clear the stale breadcrumb"
        );
    }

    /// The degraded telemetry path (unreadable credential) must leave a
    /// pre-existing breadcrumb in place — only a fully-healthy start clears it.
    #[test]
    fn a_degraded_telemetry_start_leaves_a_pre_existing_breadcrumb() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        // No credential file written: `build_telemetry` fails reading it.

        breadcrumb(&data_dir, "config", "stale, still unresolved");
        assert!(data_dir.join(DEGRADED_FILE).exists());

        let bootstrap = BootstrapConfig::parse(
            "[kiosk]\nconfig_url = https://e/c.json\nsite = hq\nproject_id = p\n\
             credential = missing.json\ndevice_id = lobby-01\n\n[bootstrap]\nurl = https://app.example.com/\n",
        )
        .expect("valid ini");

        let (tx, _rx) = std::sync::mpsc::channel();
        let sink = LauncherSink::new(
            PathBuf::from("kiosk-main.exe"),
            config_dir,
            data_dir.clone(),
            "test-pipe".into(),
            tx,
            Arc::new(AtomicU32::new(0)),
            Some(&bootstrap),
            None, // no job object: these tests only exercise the breadcrumb lifecycle
        );

        assert!(
            sink.telemetry.is_none(),
            "telemetry must have failed to build for this to be the degraded path under test"
        );
        assert!(
            data_dir.join(DEGRADED_FILE).exists(),
            "a degraded start must not erase a live warning"
        );
    }

    /// Forces `path` to an owner-only DACL (current user only) — mirrors
    /// `kiosk-main`'s `credential_acl::tests::credential_is_owner_only_reflects_real_dacl`
    /// fixture setup: a freshly created temp file's inherited ACL is NOT
    /// owner-only on a typical dev/CI host, so a test that needs a genuinely
    /// TRUSTED credential (or a controlled starting point before widening it)
    /// must force it rather than assume it.
    #[cfg(windows)]
    fn force_owner_only_acl(path: &Path) {
        use std::process::Command;

        let out = Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("failed to spawn whoami");
        let sid = String::from_utf8_lossy(&out.stdout)
            .trim()
            .rsplit(',')
            .next()
            .expect("whoami /user csv output should have a SID field")
            .trim_matches('"')
            .to_string();

        assert!(Command::new("icacls")
            .arg(path)
            .arg("/setowner")
            .arg(format!("*{sid}"))
            .output()
            .expect("failed to spawn icacls /setowner")
            .status
            .success());
        assert!(Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("*{sid}:(F)"))
            .output()
            .expect("failed to spawn icacls /inheritance:r")
            .status
            .success());
    }

    /// Windows-host: SEC-09 launcher gate. A credential whose DACL grants read to
    /// `BUILTIN\Users` (a non-owner principal) must refuse before the JSON is ever
    /// read, with the distinct owner-only message — not the generic
    /// `std::io::Error` a missing/malformed file would produce.
    #[cfg(windows)]
    #[test]
    fn build_telemetry_fails_closed_on_a_bad_credential_dacl() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        let cred_path = config_dir.join("cred.json");
        std::fs::write(&cred_path, test_service_account_json()).unwrap();
        force_owner_only_acl(&cred_path);

        let out = Command::new("icacls")
            .arg(&cred_path)
            .arg("/grant")
            .arg("*S-1-5-32-545:(R)")
            .output()
            .expect("failed to spawn icacls");
        assert!(
            out.status.success(),
            "icacls /grant failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let bootstrap = BootstrapConfig::parse(
            "[kiosk]\nconfig_url = https://e/c.json\nsite = hq\nproject_id = p\n\
             credential = cred.json\ndevice_id = lobby-01\n\n[bootstrap]\nurl = https://app.example.com/\n",
        )
        .expect("valid ini");

        match build_telemetry(&bootstrap, &config_dir, &data_dir) {
            Err(e) => assert_eq!(e.to_string(), CREDENTIAL_PERMISSIONS_MESSAGE),
            Ok(_) => panic!("a bad credential DACL must refuse to build telemetry"),
        }
    }

    /// The same bad-DACL credential, driven through `LauncherSink::new` (not just
    /// `build_telemetry` directly): telemetry degrades to `None` and a breadcrumb
    /// is written — the launcher itself starts and would keep supervising exactly
    /// as the missing-credential path already does (`dispatch`'s `spawn`/
    /// `kill_child` never touch `self.telemetry`).
    #[cfg(windows)]
    #[test]
    fn a_bad_credential_dacl_degrades_telemetry_but_still_constructs_the_sink() {
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        let cred_path = config_dir.join("cred.json");
        std::fs::write(&cred_path, test_service_account_json()).unwrap();
        force_owner_only_acl(&cred_path);
        let out = Command::new("icacls")
            .arg(&cred_path)
            .arg("/grant")
            .arg("*S-1-5-32-545:(R)")
            .output()
            .expect("failed to spawn icacls");
        assert!(
            out.status.success(),
            "icacls /grant failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let bootstrap = BootstrapConfig::parse(
            "[kiosk]\nconfig_url = https://e/c.json\nsite = hq\nproject_id = p\n\
             credential = cred.json\ndevice_id = lobby-01\n\n[bootstrap]\nurl = https://app.example.com/\n",
        )
        .expect("valid ini");

        let (tx, _rx) = std::sync::mpsc::channel();
        let sink = LauncherSink::new(
            PathBuf::from("kiosk-main.exe"),
            config_dir,
            data_dir.clone(),
            "test-pipe".into(),
            tx,
            Arc::new(AtomicU32::new(0)),
            Some(&bootstrap),
            None,
        );

        assert!(
            sink.telemetry.is_none(),
            "a bad credential DACL must skip building the GCL client"
        );
        assert!(
            data_dir.join(DEGRADED_FILE).exists(),
            "the refusal must leave the same degraded-start breadcrumb a missing credential does"
        );
    }

    /// JSON for a real, runtime-generated RSA keypair, in the shape
    /// `ServiceAccount::from_json` expects — reused by the breadcrumb-lifecycle
    /// tests above, which need `build_telemetry` to actually succeed.
    fn test_service_account_json() -> String {
        let private_pem = &*TEST_KEY_PEM;
        serde_json::json!({
            "private_key": private_pem,
            "client_email": "kiosk-logger@test-project.iam.gserviceaccount.com",
            "token_uri": "https://oauth2.googleapis.com/token",
        })
        .to_string()
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
