//! RT-13 (spec §10): headless integration — the REAL launcher supervising a
//! scriptable mock kiosk-main.
//!
//! Everything under test is production code: `kiosk_core::watchdog`'s FSM, the
//! real `pipe::serve` named-pipe server, the real `spawn::spawn_main` process
//! spawn + waiter thread, the real `timer` tick source, the real `loop_::run`,
//! and the real `sink::LauncherSink`. The only test-owned pieces are the
//! scriptable child (`rt13-mock-main`) and a spy that WRAPS the real sink.
//!
//! # Observability
//! Assertions are made against the `Action`s the FSM produced, recorded by a
//! spy that forwards every one of them to a real `LauncherSink` — so the real
//! sink still spawns, kills, publishes the child PID and drives the exit path.
//! The alternative (asserting on delivered spool entries) would need a service
//! account, a transport and the whole telemetry stack for no extra coverage of
//! the supervise behaviour this test exists to prove. `LauncherSink` is built
//! with `bootstrap: None`, i.e. `telemetry: None` — the log/drain calls are
//! then no-ops, which `sink`'s own unit tests already cover directly.
//!
//! # Wiring differences from `main.rs`
//! * The pipe name is per-scenario unique, not `pipe::instance_name()`: that
//!   derives from the LAUNCHER's PID, and here one process hosts every
//!   scenario, so they would all collide on one name.
//! * `WatchdogConfig` uses test timings (see `test_config`), not the ini's.
//! * `LauncherSink` is wrapped by `Spy`, and the process does not `exit()`.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use kiosk_core::watchdog::{Action, Watchdog, WatchdogConfig, WatchdogEvent};
use kiosk_launcher::loop_::{self, ActionSink};
use kiosk_launcher::sink::LauncherSink;
use kiosk_launcher::{pipe, timer};

/// Timings for the test. `startup_grace_s` only has to cover "spawn a tiny exe
/// and let it open a pipe"; `healthy_run_s` is deliberately far beyond any
/// scenario's runtime so the rule-6 healthy-reset never perturbs a scenario;
/// `channel_grace_s` is short so a channel loss could not be mistaken for the
/// slow path.
///
/// None of these can shorten scenario 2: the heartbeat miss window is
/// `MISS_LIMIT_S = 15`, a hard-coded FSM constant rather than config, so a hang
/// cannot be detected in less than ~15 s of real wall clock. That scenario is
/// kept honest and slow rather than weakened.
fn test_config() -> WatchdogConfig {
    WatchdogConfig {
        startup_grace_s: 10,
        healthy_run_s: 3600,
        channel_grace_s: 5,
    }
}

/// How long scenario 1 watches a healthy child. Comfortably past the 15 s miss
/// window, so a heartbeat path that silently stopped working would restart
/// inside the observation rather than after it.
const HEALTHY_OBSERVE: Duration = Duration::from_secs(20);

/// Records every `Action` the FSM emits, then hands it to the REAL sink.
///
/// `stop` is the harness's way to end a `loop_::run` that would otherwise
/// supervise forever. On stop it dispatches a real `ExitLauncher` to the inner
/// sink first, so the supervised child is killed exactly the way production
/// kills it — a leaked mock would outlive the test binary.
struct Spy {
    inner: LauncherSink,
    actions: Arc<Mutex<Vec<Action>>>,
    stop: Arc<AtomicBool>,
}

impl ActionSink for Spy {
    fn dispatch(&mut self, action: Action) -> ControlFlow<i32> {
        self.actions.lock().unwrap().push(action.clone());
        if let ControlFlow::Break(code) = self.inner.dispatch(action) {
            return ControlFlow::Break(code);
        }
        if self.stop.load(Ordering::Relaxed) {
            let _ = self.inner.dispatch(Action::ExitLauncher { code: 0 });
            return ControlFlow::Break(0);
        }
        ControlFlow::Continue(())
    }
}

struct Harness {
    actions: Arc<Mutex<Vec<Action>>>,
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    child_pid: Arc<AtomicU32>,
    code_rx: mpsc::Receiver<i32>,
    _config_dir: tempfile::TempDir,
    _data_dir: tempfile::TempDir,
    /// Set once teardown (via `stop()` or `Drop`) has run, so the two never
    /// double-act on the same harness.
    torn_down: AtomicBool,
}

/// A transport name unique to this scenario. `pipe::instance_name()` is
/// per-launcher PID, which is not unique when one test binary hosts several
/// launchers.
fn unique_transport(tag: &str, data_dir: &Path) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    #[cfg(windows)]
    {
        let _ = data_dir;
        format!(
            r"\\.\pipe\kiosk-heartbeat-rt13-{tag}-{}-{}",
            std::process::id(),
            n
        )
    }
    #[cfg(unix)]
    {
        data_dir
            .join(format!("hb-{tag}-{}-{n}.sock", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }
}

impl Harness {
    /// Starts a real launcher supervising `rt13-mock-main` running `script`.
    /// Mirrors `main.rs`'s assembly (timer thread + pipe thread + one loop).
    fn start(tag: &str, script: &str) -> Harness {
        let config_dir = tempfile::tempdir().expect("config tempdir");
        let data_dir = tempfile::tempdir().expect("data tempdir");
        std::fs::write(config_dir.path().join("script.txt"), script).expect("write script");

        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(AtomicU32::new(0));
        let pipe_name = unique_transport(tag, data_dir.path());

        timer::spawn_timer(tx.clone(), cancel.clone());
        {
            let (name, data, tx, cancel, pid) = (
                pipe_name.clone(),
                data_dir.path().to_path_buf(),
                tx.clone(),
                cancel.clone(),
                child_pid.clone(),
            );
            std::thread::spawn(move || pipe::serve(&name, &data, tx, cancel, pid));
        }

        let sink = LauncherSink::new(
            Path::new(env!("CARGO_BIN_EXE_rt13-mock-main")).to_path_buf(),
            config_dir.path().to_path_buf(),
            data_dir.path().to_path_buf(),
            pipe_name,
            tx,
            child_pid.clone(),
            None, // no bootstrap => no telemetry; see the module note
            // No job object: `Harness` already reaps the mock explicitly (see
            // `kill_child`/`Drop`), and a kill-on-close job scoped to a single
            // scenario would fire whenever this sink was dropped — killing a
            // mock the scenario still expects to be alive. `job::Job`'s own
            // unit test covers kill-on-close directly.
            None,
        );
        let actions = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let mut spy = Spy {
            inner: sink,
            actions: actions.clone(),
            stop: stop.clone(),
        };

        let (wd, initial) = Watchdog::new(test_config());
        let (code_tx, code_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let code = loop_::run(rx, wd, initial, &mut spy);
            let _ = code_tx.send(code);
        });

        Harness {
            actions,
            stop,
            cancel,
            child_pid,
            code_rx,
            _config_dir: config_dir,
            _data_dir: data_dir,
            torn_down: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> Vec<Action> {
        self.actions.lock().unwrap().clone()
    }

    /// Polls until some recorded `Action` matches `pred`, or `within` elapses.
    /// Every wait in this test is bounded through here.
    fn wait_for(&self, what: &str, within: Duration, pred: impl Fn(&Action) -> bool) {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if self.snapshot().iter().any(&pred) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "timed out after {within:?} waiting for {what}; actions so far: {:?}",
            self.snapshot()
        );
    }

    /// Polls until `pred` has matched at least `count` recorded `Action`s, or
    /// `within` elapses. Used to prove something happened a SECOND time (e.g.
    /// re-arming after a restart), not just once.
    fn wait_for_count(
        &self,
        what: &str,
        within: Duration,
        count: usize,
        pred: impl Fn(&Action) -> bool,
    ) {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if self.snapshot().iter().filter(|a| pred(a)).count() >= count {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "timed out after {within:?} waiting for {count}x {what}; actions so far: {:?}",
            self.snapshot()
        );
    }

    /// Signals the stop flag and kills the mock child, if one is still alive.
    /// Shared by `stop()` and `Drop` so both tear down the same way.
    fn kill_child(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let pid = self.child_pid.load(Ordering::Relaxed);
        if pid != 0 {
            #[cfg(windows)]
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output(); // captured, so the suite's output stays clean
            #[cfg(unix)]
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .output();
        }
    }

    /// Ends a supervise loop that would otherwise run forever, and waits for it.
    /// The kill is what wakes a loop whose child is perfectly healthy (a healthy
    /// Armed FSM emits no `Action` at all, so the stop flag alone is never seen).
    fn stop(&self) {
        self.kill_child();
        let code = self
            .code_rx
            .recv_timeout(Duration::from_secs(30))
            .expect("the supervise loop must exit once stopped");
        assert_eq!(code, 0, "a harness-stopped loop exits 0");
        self.cancel.store(true, Ordering::Relaxed);
        self.torn_down.store(true, Ordering::Relaxed);
    }

    /// Waits for `loop_::run` to return on its own (scenario 4).
    fn wait_for_exit(&self, within: Duration) -> i32 {
        let code = self
            .code_rx
            .recv_timeout(within)
            .expect("the launcher must exit on its own");
        self.cancel.store(true, Ordering::Relaxed);
        self.torn_down.store(true, Ordering::Relaxed);
        code
    }
}

/// Reaps the mock child even when a scenario panics before reaching `stop()`
/// or `wait_for_exit()` — e.g. a `wait_for` timeout at the assertion above.
/// The child is owned by a detached thread inside `LauncherSink`, which
/// Windows will not reap on its own, so a leaked test failure otherwise
/// leaves a permanent orphan `rt13-mock-main.exe` process behind.
///
/// Idempotent: if `stop()`/`wait_for_exit()` already ran, `torn_down` is
/// already set and this is a no-op. Must never panic — `Drop` runs during
/// unwinding, and a panic here would abort the process instead of reporting
/// the original test failure.
impl Drop for Harness {
    fn drop(&mut self) {
        if self.torn_down.swap(true, Ordering::Relaxed) {
            return;
        }
        self.kill_child();
        self.cancel.store(true, Ordering::Relaxed);
        // Best-effort: give the loop a moment to notice, but never block a
        // panicking test forever and never turn a failure into a hang.
        let _ = self.code_rx.recv_timeout(Duration::from_secs(5));
    }
}

fn is_restart(a: &Action) -> bool {
    matches!(a, Action::Log(WatchdogEvent::Restart { .. }))
}

/// Scenario 1 — healthy: `Ready` then `Ping`s forever. The FSM arms and never
/// restarts.
#[test]
fn healthy_child_arms_the_watchdog_and_is_never_restarted() {
    let h = Harness::start("healthy", "healthy");
    h.wait_for("watchdog.arm", Duration::from_secs(20), |a| {
        matches!(a, Action::Log(WatchdogEvent::Arm { .. }))
    });

    std::thread::sleep(HEALTHY_OBSERVE);
    let observed = h.snapshot();
    h.stop();

    assert!(
        !observed.iter().any(is_restart),
        "a pinging child must never be restarted; saw {observed:?}"
    );
    assert_eq!(
        observed
            .iter()
            .filter(|a| matches!(a, Action::SpawnMain | Action::SpawnSafe))
            .count(),
        1,
        "exactly the one initial spawn; saw {observed:?}"
    );
}

/// Scenario 2 — hang: `Ready`, then silence while staying alive and connected.
///
/// The FSM must log `watchdog.hang` and restart with cause `hang` inside the
/// miss window. It then ALSO logs a `watchdog.restart{cause:"exit"}`: the sink's
/// own deliberate kill of the hung child comes back as `Event::ChildExited`,
/// which carries no PID and so is indistinguishable from a crash. That is the
/// known, accepted behaviour recorded in Task 4's review, asserted here as what
/// actually happens rather than papered over.
#[test]
fn a_hung_child_is_detected_and_restarted_within_the_miss_window() {
    let h = Harness::start("hang", "hang");
    h.wait_for("watchdog.arm", Duration::from_secs(20), |a| {
        matches!(a, Action::Log(WatchdogEvent::Arm { .. }))
    });

    // 15 s miss window + a tick's slack + the FSM's 1 s backoff.
    h.wait_for("watchdog.hang", Duration::from_secs(40), |a| {
        matches!(a, Action::Log(WatchdogEvent::Hang))
    });
    h.wait_for("a hang restart", Duration::from_secs(5), |a| {
        matches!(a, Action::Log(WatchdogEvent::Restart { cause: "hang", .. }))
    });
    h.wait_for(
        "the follow-on exit restart from the sink's own kill",
        Duration::from_secs(20),
        |a| matches!(a, Action::Log(WatchdogEvent::Restart { cause: "exit", .. })),
    );
    h.stop();
}

/// Scenario 3 — crash: the child exits 7 after `Ready`. The restart carries the
/// real exit code, and the restarted child re-arms.
///
/// The second `Arm` is the one assertion in this suite that proves the FSM
/// recovered a full cycle, not just logged an event. Immediately after a
/// restart, `pipe::serve` can still hold the OLD child's PID in its
/// pre-connect `expected` snapshot when the NEW child connects (`pipe.rs`),
/// so the server rejects the first `Ready` write and the new mock's write
/// fails with `ERROR_NO_DATA`; the real client's write-retry (per
/// `kiosk-main/src/heartbeat.rs`) backs off ~100ms and resends. That recovery
/// path is real production behaviour with nothing else in this suite
/// asserting on it — miss it here and a restarted kiosk could restart-loop
/// forever in the field while every scenario still passed.
#[test]
fn a_crashing_child_is_restarted_with_its_real_exit_code() {
    let h = Harness::start("crash", "exit:7");
    h.wait_for("watchdog.restart{code:7}", Duration::from_secs(30), |a| {
        matches!(a, Action::Log(WatchdogEvent::Restart { code: 7, .. }))
    });
    // Bounded generously: it must tolerate the ~100ms-1s pipe-rejection retry
    // cycle, but 30s is still a real bound, not effectively infinite.
    h.wait_for_count(
        "a second watchdog.arm (the restarted child re-arming)",
        Duration::from_secs(30),
        2,
        |a| matches!(a, Action::Log(WatchdogEvent::Arm { .. })),
    );
    // Not asserted separately: a second `Arm` cannot happen without a second
    // `SpawnMain` having run first (the mock only reaches `Ready` after being
    // spawned), so this already closes Finding 4 ("restart logged but nothing
    // was spawned"). A separate exact spawn-count assertion would be racy
    // here anyway — `exit:7` crash-loops the mock, so more restarts can land
    // between the second `Arm` and `stop()` taking effect.
    h.stop();
}

/// Scenario 4 — exit 86: the technician exit. The launcher exits 86 itself and
/// never restarts the child.
#[test]
fn exit_86_stops_the_launcher_without_a_restart() {
    let h = Harness::start("exit86", "exit:86");
    let code = h.wait_for_exit(Duration::from_secs(30));

    assert_eq!(code, 86, "the launcher propagates the technician exit code");
    let observed = h.snapshot();
    assert!(
        !observed.iter().any(is_restart),
        "exit 86 must never restart the child; saw {observed:?}"
    );
    assert_eq!(
        observed
            .iter()
            .filter(|a| matches!(a, Action::SpawnMain | Action::SpawnSafe))
            .count(),
        1,
        "only the initial spawn; saw {observed:?}"
    );
}
