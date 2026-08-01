#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod boot;
mod clear;
mod cli;
mod driver;
mod effect;
mod egress;
mod fetch;
mod gesture;
mod hardening;
mod health;
mod heartbeat;
mod idle;
mod inject;
mod maintenance;
mod nav;
mod nav_policy;
mod pinpad;
mod probe;
mod recovery;
mod scheme_guard;
mod shortcuts;
mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use driver::{Driver, EffectSink};
use effect::PageTarget;
use kiosk_core::app::state::{Effect, Event as AppEvent, Machine};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::net::prober::Prober;
use kiosk_core::net::reach::resolve_probe_url;
use nav_policy::{NavPolicy, SharedNavPolicy};
use sysinfo::{Disks, System};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

const WINDOW_LABEL: &str = "kiosk";

/// The Windows/`wry` app-origin workaround for the `tauri://` custom scheme: WebView2
/// cannot navigate the top-level frame to a custom scheme, so Tauri serves bundled
/// assets at this `http://` host instead on Windows (confirmed against tauri 2.11.5,
/// `AppManager::tauri_protocol_url`: `cfg!(windows) => "http://tauri.localhost"` when
/// `use_https_scheme` is unset, which this app never sets). Revisit if/when a
/// Linux/macOS target ships (spec P2/P3), where the origin is the literal
/// `tauri://localhost`.
const APP_ORIGIN: &str = "http://tauri.localhost";

/// Generous relative to the event rate (one per probe flip / config poll / navigation);
/// sized so a burst never makes `try_send` the reason an `AppEvent` is dropped.
const EVENT_CHANNEL_CAPACITY: usize = 64;

fn bundled_url(page: &str) -> String {
    format!("{APP_ORIGIN}/{page}")
}

/// Minimal query-string percent-encoding (P1-F1 Task 2: `safe.html`'s `?device=&err=`
/// params). No dependency pulls in a full `url`/`urlencoding` crate for two values —
/// unreserved chars (RFC 3986: alnum, `-`, `_`, `.`, `~`) pass through, everything else
/// (including `&`, `=`, `?`, newlines from a multi-line panic message) is `%XX`-encoded,
/// so the receiving page's `URLSearchParams` decodes it back exactly.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod url_encode_tests {
    use super::url_encode;

    #[test]
    fn unreserved_chars_pass_through() {
        assert_eq!(url_encode("device-01_A.B~C"), "device-01_A.B~C");
    }

    #[test]
    fn reserved_and_control_chars_are_percent_encoded() {
        assert_eq!(url_encode("a b&c=d\n"), "a%20b%26c%3Dd%0A");
    }

    /// The 500-char cap the `--safe` path puts on `crash-panic.txt` before encoding
    /// (main.rs `setup`). Guards the two ways that cap could break `safe.html`:
    /// a cut landing mid-UTF-8 (byte slicing would panic / emit an undecodable
    /// %-sequence — `.chars()` cannot), and unbounded 3x encode expansion.
    #[test]
    fn capped_error_text_stays_valid_utf8_and_bounded() {
        let huge: String = "パニック💥".repeat(10_000);
        let capped: String = huge.chars().take(500).collect();
        assert_eq!(capped.chars().count(), 500);
        // 3 bytes/char worst case, x3 for percent-encoding: comfortably under any
        // browser URL limit, which is the whole point of the cap.
        assert!(url_encode(&capped).len() <= 500 * 4 * 3);
        // Round-trips: every escape is a well-formed %XX of the original bytes.
        assert_eq!(url_encode(&capped).matches('%').count(), capped.len());
    }
}

/// Pure index-vs-count decision for `display.monitor` (spec §5.2): `requested`
/// is the configured index, `count` is `available_monitors().len()`. `Some`
/// is the in-range index to place the window on; `None` means fall back to
/// the primary monitor and emit `config.warn`. Split out from the Tauri
/// wiring in `setup` so this branch is host-testable without a real display.
fn resolve_monitor_index(requested: u32, count: usize) -> Option<usize> {
    let requested = requested as usize;
    (requested < count).then_some(requested)
}

#[cfg(test)]
mod monitor_index_tests {
    use super::resolve_monitor_index;

    #[test]
    fn in_range_index_is_kept() {
        assert_eq!(resolve_monitor_index(0, 2), Some(0));
        assert_eq!(resolve_monitor_index(1, 2), Some(1));
    }

    #[test]
    fn out_of_range_falls_back() {
        assert_eq!(resolve_monitor_index(5, 1), None);
    }

    #[test]
    fn index_equal_to_count_falls_back() {
        assert_eq!(resolve_monitor_index(1, 1), None);
    }

    #[test]
    fn zero_monitors_always_falls_back() {
        assert_eq!(resolve_monitor_index(0, 0), None);
    }
}

/// The install dir `kiosk.ini`/the credential file/the offline mp4 live in (spec §4):
/// next to the running exe, unless `--config <dir>` overrides it.
fn resolve_config_dir(override_dir: Option<&str>) -> PathBuf {
    match override_dir {
        Some(dir) => PathBuf::from(dir),
        None => std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from(".")),
    }
}

/// The data dir (cache, spool, last-good) — `%ProgramData%\kiosk\` (spec §4). Never
/// operator-overridden (unlike the install dir): this is not something a `kiosk.ini`
/// deployment ever needs to relocate.
fn resolve_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kiosk")
}

/// File-only breadcrumb, installed before telemetry exists (see call site in `main`).
/// Takes/chains the existing hook first (same discipline as `install_panic_hook`
/// below) so the stdlib default hook — which prints the panic message/backtrace to
/// stderr — is preserved rather than discarded; that stderr output is the only
/// panic diagnostics dev/debug console builds get.
/// `std::panic::take_hook`/`set_hook` compose: `install_panic_hook` below calls
/// `take_hook()` when it runs, which returns THIS closure and chains it as its
/// `default_hook`, so once telemetry comes up both the file write and `telem.panic`
/// fire on every panic — this one is never replaced, only wrapped.
fn install_panic_hook_file_only(data_dir: PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let path = data_dir.join("crash-panic.txt");
        if let Ok(mut f) = std::fs::File::create(&path) {
            use std::io::Write;
            let _ = writeln!(f, "{info}");
            let _ = f.sync_all();
        }
    }));
}

/// Best-effort crash telemetry (spec TEL-10, brief step 4).
///
/// The async `Logger` is owned by the logger thread (`telemetry::run`); a
/// `std::panic::set_hook` closure runs synchronously on the panicking thread and
/// cannot reach into that thread to call it directly. `Telemetry::panic` is the one
/// channel this closure CAN safely reach: `try_send` never blocks and never panics,
/// so installing this hook cannot itself become a second panic.
///
/// Durability is *mostly* already covered by the time this fires: `Logger::log`
/// fsyncs every WARNING+ entry synchronously as it is processed
/// (`kiosk_core::logging::spool::Spool::append`), so every event logged before the
/// crash (`net.offline`, `config.error`, …) is already durable on disk. The gap this
/// leaves is narrow: the crash entry itself is only enqueued, not yet fsynced, at the
/// moment this hook returns. In the common case — a panic inside one `tokio::spawn`ed
/// task, caught by that task's own unwind boundary — the process keeps running and the
/// still-alive logger thread drains and fsyncs it on its very next scheduling. The
/// entry is genuinely at risk only if the panic unwinds out of `main` itself (e.g. from
/// inside `.setup()`), tearing the whole runtime down before the logger thread gets that
/// next turn. Documented here rather than solved: a synchronous, reentrant-safe spool
/// write from inside a panic hook (racing the very same spool the logger thread also
/// holds open, violating the "one writer per segment" invariant — spec arch-01) would
/// be the "fragile mechanism" the brief warns against, not a fix.
fn install_panic_hook(telem: telemetry::Telemetry, data_dir: PathBuf) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        telem.panic(&info.to_string());

        // Best-effort durable breadcrumb for the launcher (P1-E) to attach to
        // watchdog.restart. No allocation-heavy / re-entrant work — a panic in
        // this hook must not cascade into a second panic. `File::create`
        // truncates: on a second panic (or a panic on another thread racing
        // this one) the file is silently overwritten, so it only ever holds
        // the MOST RECENT panic, not a history. That's fine for the launcher's
        // use case (attach the breadcrumb to the restart it's currently
        // handling) but means two panics in quick succession without an
        // intervening restart lose the first message.
        let path = data_dir.join("crash-panic.txt");
        if let Ok(mut f) = std::fs::File::create(&path) {
            use std::io::Write;
            let _ = writeln!(f, "{info}"); // message + location (Display of PanicHookInfo)
            let _ = f.sync_all(); // fsync the file
        }
    }));
}

/// Drives the FSM's effects into the live webview (spec §Architecture actor-spine).
/// The `Effect` → page decision itself is `effect::page_for` (pure, host-tested); this
/// only carries out what it decides.
struct TauriSink {
    app: AppHandle,
    tx: mpsc::Sender<AppEvent>,
    refetch: Arc<Notify>,
    telem: telemetry::Telemetry,
    cancel: CancellationToken,
}

impl TauriSink {
    fn navigate(&self, url: &str) {
        let Some(window) = self.app.get_webview_window(WINDOW_LABEL) else {
            eprintln!("TauriSink: window {WINDOW_LABEL:?} missing, cannot navigate to {url}");
            return;
        };
        match url.parse() {
            Ok(parsed) => {
                if let Err(e) = window.navigate(parsed) {
                    eprintln!("TauriSink: navigate({url}) failed: {e}");
                }
            }
            Err(e) => eprintln!("TauriSink: {url:?} is not a valid URL: {e}"),
        }
    }

    /// `ShowErrorPage`'s retry countdown is FSM-driven, not JS-driven (the bundled
    /// error page is static): a one-shot timer here re-injects `CountdownExpired`,
    /// cancel-aware so it never outlives shutdown.
    fn arm_countdown(&self, retry_after_seconds: u64) {
        let tx = self.tx.clone();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = cancel.cancelled() => {}
                _ = tokio::time::sleep(Duration::from_secs(retry_after_seconds)) => {
                    let _ = tx.send(AppEvent::CountdownExpired).await;
                }
            }
        });
    }
}

impl EffectSink for TauriSink {
    fn dispatch(&mut self, effect: Effect) {
        if let Some(target) = effect::page_for(&effect) {
            match target {
                PageTarget::Remote(url) => self.navigate(&url),
                PageTarget::Offline => self.navigate(&bundled_url("offline.html")),
                PageTarget::Splash => self.navigate(&bundled_url("splash.html")),
                PageTarget::Error {
                    retry_after_seconds,
                } => {
                    self.navigate(&bundled_url("error.html"));
                    self.arm_countdown(retry_after_seconds);
                }
            }
            return;
        }
        match effect {
            Effect::RefetchConfig => self.refetch.notify_one(),
            // D2c: executes the real WebView2 clear (see `crate::clear`) and always
            // sends `AppEvent::ProfileCleared` back, releasing the P1-D1 `Clearing`
            // privacy gate — on the success path AND on any cast/call failure (never
            // strand the kiosk on the gate). `full` has no partial-clear counterpart in
            // the FSM (rule 9 only ever emits `{full: true}`), so it is intentionally
            // unused here rather than threaded into a data-kind choice that has no
            // caller.
            Effect::ClearProfile { full: _ } => {
                let Some(window) = self.app.get_webview_window(WINDOW_LABEL) else {
                    eprintln!("TauriSink: window {WINDOW_LABEL:?} missing, cannot clear profile");
                    // Never strand the Clearing gate even when the window is gone.
                    let _ = self.tx.try_send(AppEvent::ProfileCleared);
                    return;
                };
                clear::clear(&window, self.tx.clone(), self.telem.clone());
            }
            other => unreachable!(
                "effect::page_for only returns None for RefetchConfig/ClearProfile, got {other:?}"
            ),
        }
    }
}

#[tokio::main]
async fn main() {
    // P1-D2e Task 2: process-start instant for `health.sample`'s `uptime_secs` —
    // taken as early as possible so uptime reflects the whole process lifetime,
    // not just the time since the health task was spawned.
    let process_started = Instant::now();
    let args = cli::Args::parse(std::env::args());
    let config_dir = resolve_config_dir(args.config.as_deref());

    // P1-D2e final-review Fix A: `data_dir` is pure/CLI-independent (spec §4), so it can
    // be resolved before the two panic sites below (bad `--config` path / malformed
    // `kiosk.ini`) rather than after them. Installing a file-only breadcrumb hook this
    // early means BOTH panics still leave `crash-panic.txt` for the P1-E launcher —
    // telemetry doesn't exist yet at this point, so there is nothing to send it to.
    let data_dir = resolve_data_dir();
    // Best-effort: %ProgramData%\kiosk\ isn't created until spool.rs/store.rs run
    // later, both after the two early panic sites below. Without this, File::create
    // in the hook fails silently on a fresh install and there's no breadcrumb at all.
    let _ = std::fs::create_dir_all(&data_dir);
    install_panic_hook_file_only(data_dir.clone());

    // P1-F1 Task 2: safe mode's last-error breadcrumb, read now (before this run's own
    // panic hook could ever overwrite it) — `Err` (missing file, first-ever boot) reads
    // as "unknown" at render time. Read only when `--safe` needs it: the common non-safe
    // boot never touches this file.
    let last_error = args.safe.then(|| data_dir.join("crash-panic.txt"));

    // KNOWN LIMITATION (P1-F1, to fix in P1-F2): safe mode does NOT cover config
    // faults. Both panic sites below run on the `--safe` path too, so a device with
    // an unreadable or invalid `kiosk.ini` (or a bad credential) crash-loops:
    // escalate to `--safe` → panic here → `SAFE_FAIL_LIMIT` → `safe_mode_failed`,
    // and the operator gets a 60 s black-screen loop with no safe page at all.
    // P1-F2 owns rendering `safe.html` BEFORE config is parsed.
    let ini_path = config_dir.join("kiosk.ini");
    let ini_text = std::fs::read_to_string(&ini_path).unwrap_or_else(|e| {
        panic!(
            "kiosk-main: cannot read {} ({e}); pass --config <dir> in dev",
            ini_path.display()
        )
    });

    let booted = boot::boot(&ini_text, &data_dir).unwrap_or_else(|e| {
        panic!(
            "kiosk-main: {} is not a valid kiosk.ini: {e}",
            ini_path.display()
        )
    });

    // ---- Extract-before-move: every field this main needs from `booted.manager` is
    // read out HERE, before `booted.manager` moves into `fetch::run` below. ----
    let bootstrap = booted.manager.bootstrap().clone();
    let device_id = booted.manager.device_id().to_string();
    // `device_id` itself is moved into the telemetry thread's closure below; safe
    // mode needs its own copy to put in the safe.html query string, read much later
    // in `.setup()`.
    let device_id_safe = device_id.clone();
    let revision = booted.manager.revision();
    let home_url = booted.manager.home_url();
    let network = booted.manager.current().network.clone();
    // P1-D2b Task 6: read once, at boot, for the document-start injection + zoom lock.
    // These are baked into the webview at BUILD time (`initialization_script`/
    // `SetZoomFactor` below) — a later config fetch that changes any of the three
    // does NOT take effect until the next process restart (see `inject`'s module doc).
    let display = booted.manager.current().display.clone();
    let content_zoom = booted.manager.current().content.zoom;
    // P1-D2c Task 3: read once, at boot, like the zoom/injection fields above — a
    // later config fetch changing `idle_reset_seconds` takes effect only on the next
    // process restart (this loop is spawned once, below, and never re-reads config).
    let idle_reset_seconds = booted.manager.current().content.idle_reset_seconds;
    let allow_text_selection = booted.manager.current().input.allow_text_selection;
    // P1-F1 Task 3: same "read once, next-restart to change" convention as the fields
    // above — the nightly-reload timer is spawned once, below, and never re-reads
    // config (a later config fetch changing either field takes effect only on the
    // next process restart).
    let nightly_reload = booted.manager.current().maintenance.nightly_reload.clone();
    let maintenance_timezone = booted.manager.current().maintenance.timezone.clone();
    // P1-D2e Task 2: same "read once, next-restart to change" convention as the
    // fields above — the health-sample timer is spawned once, below, and never
    // re-reads config.
    let health_sample_s = booted.manager.current().logging.health_sample_s;
    // P1-D2c Task 4: same "read once, next-restart to change" convention as the
    // three fields above — remote `input.exit_gesture` wins over bootstrap
    // `[exit_gesture]` (cfg-12), resolved once here via `gesture::effective_gesture`
    // and handed to both trigger paths (`shortcuts::install`'s chord,
    // `gesture::install`'s tap capture) below.
    let exit_gesture = gesture::effective_gesture(
        booted.manager.current().input.exit_gesture.as_ref(),
        bootstrap.exit_gesture.as_ref(),
    );
    let credential_path = config_dir.join(&bootstrap.credential);
    let config_url = bootstrap.config_url.clone();
    let poll_s = network.config_poll_s;
    let probe_url = resolve_probe_url(&network.connectivity_check_url, &home_url);
    let machine_cfg = booted.machine_cfg;
    let first_event = booted.first_event;
    let warnings = booted.warnings;

    // Live-swappable nav policy (P1-D2b Task 1): built from the just-booted config so
    // the very first navigation is already judged by it. `fetch::run` (below) stores a
    // fresh one on every successful config apply; the guard install (Task 2) reads it
    // lock-free via `nav_policy.load()`.
    let nav_policy: SharedNavPolicy = Arc::new(ArcSwap::from_pointee(NavPolicy::from_config(
        &booted.manager.current().content,
        &home_url,
    )));
    let nav_policy_fetch = nav_policy.clone();

    // TEL-01: ONE clock, cloned into both the logger stack and the prober. Two
    // independent clocks would give each its own, disagreeing view of the
    // Date-header bootstrap.
    let clock = TrustedClock::new();

    let (tx, rx) = mpsc::channel::<AppEvent>(EVENT_CHANNEL_CAPACITY);
    // `refetch` carries `Effect::RefetchConfig` (TauriSink `notify_one`) → `fetch::run`'s
    // immediate poll. I1: in D2a that effect only ever comes from FSM rule 10
    // `(Offline, Reconnected)`, and nothing emits `Reconnected` (the probe emits only
    // `LinkChanged` — see `probe::run`). So this handle is forward-wired but dormant;
    // reconnect recovery runs via rule 4 + the periodic poll. Not dead code.
    let refetch = Arc::new(Notify::new());
    let cancel = CancellationToken::new();
    let prober = Prober::new(clock.clone());

    // The P1-B logger `Transport` is `reqwest::blocking`, which panics if it is built
    // OR driven inside a tokio runtime (it stands up and drops its own internal
    // runtime, forbidden in an async context). So the ENTIRE logger stack — `build`
    // (which constructs the blocking client) and the drain loop `run` — lives on a
    // DEDICATED OS THREAD, off this `#[tokio::main]` runtime, talking over a
    // `std::sync::mpsc` channel. The thread hands the `Telemetry` handle back for the
    // async tasks to clone.
    //
    // A telemetry-init failure (missing/malformed kiosk-credential.json) must NOT stop
    // the kiosk from showing content — the whole point of the device is the screen. On
    // error we log to stderr and run WITHOUT telemetry: `Telemetry::disabled()` is a
    // handle whose every helper silently no-ops, so every clone handed to
    // fetch/probe/driver/TauriSink/nav keeps working unchanged.
    let app_version = kiosk_core::app_version().to_string();
    let cancel_log = cancel.clone();
    // Published by `telemetry::run` on the logger thread, read by `health::run`
    // (spawned below) — see `telemetry::run`'s doc comment for why this atomic,
    // rather than a direct cross-thread `Logger::dropped_expired()` call, is the
    // plumbing: the `Logger` lives on this dedicated OS thread and is never `Sync`
    // across it.
    let dropped_expired = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let dropped_expired_log = dropped_expired.clone();
    let (handle_tx, handle_rx) = std::sync::mpsc::channel::<Option<telemetry::Telemetry>>();
    let panic_hook_data_dir = data_dir.clone();
    let spawned = std::thread::Builder::new()
        .name("telemetry".into())
        .spawn(move || {
            match telemetry::build(
                &bootstrap,
                &credential_path,
                &device_id,
                clock,
                app_version,
                revision,
                &data_dir,
            ) {
                Ok((telem, logger, log_rx)) => {
                    let _ = handle_tx.send(Some(telem));
                    telemetry::run(logger, log_rx, cancel_log, dropped_expired_log);
                }
                Err(e) => {
                    eprintln!("kiosk-main: telemetry disabled ({e}); continuing without it");
                    let _ = handle_tx.send(None);
                }
            }
        });
    // A thread-spawn failure must not black-screen the kiosk either: the moved
    // `handle_tx` drops with the un-spawned closure, so `handle_rx.recv()` below fails
    // through to `Telemetry::disabled()`. Same degrade as a build failure.
    if let Err(e) = spawned {
        eprintln!("kiosk-main: telemetry thread spawn failed ({e}); continuing without it");
    }
    let telem = handle_rx
        .recv()
        .ok()
        .flatten()
        .unwrap_or_else(telemetry::Telemetry::disabled);

    install_panic_hook(telem.clone(), panic_hook_data_dir);
    telem.app_start();
    telem.config_applied(revision, &warnings);

    // P1-F1 Task 2: `--safe` must make no remote request — no config fetch, no
    // prober, no remote content navigation. `booted.manager`/`network`/`prober` are
    // simply left unmoved and drop here; everything `.setup()` needs from them was
    // already extracted above.
    if !args.safe {
        tokio::spawn(fetch::run(
            booted.manager, // MOVES — every field needed above was extracted first.
            config_url,
            poll_s,
            tx.clone(),
            telem.clone(),
            refetch.clone(),
            cancel.clone(),
            nav_policy_fetch,
        ));
        tokio::spawn(probe::run(
            prober,
            network,
            probe_url,
            tx.clone(),
            telem.clone(),
            cancel.clone(),
        ));
    }
    // P1-D2c Task 3: emits `IdleExpired` UNCONDITIONALLY — the FSM (rule 9) already
    // no-ops it outside `Online`, so no state check belongs here too.
    tokio::spawn(idle::run(idle_reset_seconds, tx.clone(), cancel.clone()));

    // P1-D2e Task 2: periodic `health.sample`. `resolve_data_dir()` is a pure
    // function of `%ProgramData%`, cheap to call again here — the `data_dir` bound
    // at the top of `main` was already moved into the telemetry thread's closure
    // above (same pattern as `pinpad_state` below).
    tokio::spawn(health::run(
        System::new(),
        Disks::new_with_refreshed_list(),
        resolve_data_dir(),
        process_started,
        health_sample_s,
        Arc::new(move || dropped_expired.load(std::sync::atomic::Ordering::Relaxed)),
        telem.clone(),
        cancel.clone(),
    ));

    // P1-E2 Task 5: heartbeat client. `ready` is pulsed by `nav::install` on the
    // first committed navigation (arch-03); the client then sends `Frame::Ready`
    // and pings the launcher. No `KIOSK_HEARTBEAT_PIPE` → nobody is supervising
    // us (developer / direct launch) → no heartbeat, kiosk runs unchanged.
    let ready = Arc::new(Notify::new());
    match heartbeat::pipe_name_from_env() {
        Some(pipe_name) => {
            tokio::spawn(heartbeat::run(pipe_name, ready.clone(), cancel.clone()));
        }
        None => eprintln!(
            "kiosk-main: {} unset; running standalone with no launcher heartbeat",
            heartbeat::PIPE_ENV
        ),
    }

    // Keep-awake (spec §7, display.keep_awake): asserted once at startup, for the
    // life of the process — WebView2/tao has no per-window "don't sleep" flag, so
    // this is the process-wide Win32 mechanism instead. `ES_CONTINUOUS` makes the
    // state persist until explicitly cleared or the process exits; there is no
    // matching "undo" call because the kiosk process is expected to hold this for
    // its entire lifetime (Windows itself resets a thread's execution state to
    // `ES_CONTINUOUS`-off when the thread exits).
    #[cfg(windows)]
    if display.keep_awake {
        use windows::Win32::System::Power::{
            SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
        };
        // Safety: `SetThreadExecutionState` is a plain Win32 call with no invariants
        // beyond a valid flags value, which the three constants above are.
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
        }
    }

    let windowed = args.windowed;
    let safe = args.safe;
    let tx_setup = tx.clone();
    let refetch_setup = refetch.clone();
    let telem_setup = telem.clone();
    let cancel_setup = cancel.clone();
    let nav_policy_setup = nav_policy.clone();
    let exit_gesture_setup = exit_gesture.clone();
    let ready_setup = ready.clone();

    // P1-D2c Task 5: the `verify_pin` command's state. `resolve_data_dir()` is a
    // pure function of `%ProgramData%`, cheap to call again here — the `data_dir`
    // bound at the top of `main` was already moved into the telemetry thread's
    // closure above. `PinPadState::new` seeds the authoritative in-memory lockout
    // from disk once, here at startup.
    let pinpad_state = pinpad::PinPadState::new(
        exit_gesture.as_ref().map(|g| g.pin_hash.clone()),
        resolve_data_dir(),
    );

    tauri::Builder::default()
        .manage(pinpad_state)
        .invoke_handler(tauri::generate_handler![pinpad::verify_pin])
        // Serve the runtime, user-replaceable `kiosk-offline.mp4` (spec §3.4: sits next
        // to the binaries, NOT build-embedded) to the bundled offline.html at a fixed
        // origin. A custom scheme rather than the built-in asset protocol because the
        // latter's `scope` is static config and cannot cleanly cover a runtime install
        // dir. Windows origin form is `http://<scheme>.localhost/<path>` (tauri
        // 2.11.5 `Builder::register_uri_scheme_protocol` doc + `AppManager`'s own
        // `tauri.localhost` derivation) → `http://kioskasset.localhost/kiosk-offline.mp4`.
        .register_uri_scheme_protocol("kioskasset", move |_ctx, _req| {
            let mp4 = config_dir.join("kiosk-offline.mp4");
            match std::fs::read(&mp4) {
                Ok(bytes) => tauri::http::Response::builder()
                    .header(tauri::http::header::CONTENT_TYPE, "video/mp4")
                    .body(bytes)
                    .expect("static video/mp4 response builds"),
                // Absent/unreadable → 404; offline.html's arch-09 handlers degrade to
                // the black splash rather than hanging.
                Err(_) => tauri::http::Response::builder()
                    .status(tauri::http::StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .expect("static 404 response builds"),
            }
        })
        .setup(move |app| {
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                WINDOW_LABEL,
                tauri::WebviewUrl::App("splash.html".into()),
            );
            // Built hidden, shown after `display.monitor` placement below. Two reasons,
            // both load-bearing:
            //   1. No flash — without this the window would be visible at its default
            //      size/position for the moment between `build()` and the fullscreen
            //      call, which on a kiosk is an ugly boot artifact.
            //   2. `fullscreen(true)` is NOT set on the builder (it used to be). A
            //      window that is already fullscreen when `set_position` runs keeps the
            //      size it captured from whatever monitor it was born on: the
            //      2026-07-28 smoke measured 1920x1200 (the 125%-scaled primary's
            //      physical extent) on a 1920x1080 external panel — the move worked, the
            //      size did not follow. `set_fullscreen(true)` AFTER `set_position`
            //      re-evaluates the monitor the window is currently on, so it sizes to
            //      the target. Order is: build hidden → position → fullscreen → show.
            builder = builder.visible(false);
            builder = if windowed {
                builder.inner_size(1280.0, 800.0).decorations(true)
            } else {
                builder
                    .decorations(false)
                    .always_on_top(true)
                    .focused(true)
            };
            // P1-D2b Task 6: the ONE `initialization_script` call for this webview
            // (a second call elsewhere would clobber this one — see
            // `nav_policy::csp_policy`'s doc comment on why CSP is NOT injected here,
            // and `inject`'s module doc on why this is build-time-only, next-restart
            // to change).
            builder = builder.initialization_script(inject::build_injection(
                display.cursor_autohide_seconds,
                allow_text_selection,
            ));
            let window = builder.build()?;

            // display.monitor (spec §5.2): an out-of-range index must never
            // leave the kiosk without a window or panic at startup — a
            // failed monitor query or a bad config index both fall back to
            // the primary monitor. `available_monitors`/`primary_monitor`
            // return `Result`/`Option`, so every failure path below is
            // `Ok`/`Some`-checked, never `unwrap`ed.
            if let Ok(monitors) = window.available_monitors() {
                let target = resolve_monitor_index(display.monitor, monitors.len())
                    .and_then(|i| monitors.get(i));
                match target {
                    Some(m) => {
                        let _ = window.set_position(*m.position());
                    }
                    None => {
                        telem_setup.config_warn(
                            "display.monitor",
                            "index beyond available displays; using primary",
                        );
                        if let Ok(Some(primary)) = window.primary_monitor() {
                            let _ = window.set_position(*primary.position());
                        }
                        // No primary monitor resolvable either: leave the window
                        // wherever Tauri's own default placement put it rather
                        // than failing startup.
                    }
                }
            }

            // AFTER positioning — see the builder comment above. Unconditional (not
            // inside the `available_monitors` block) so a failed monitor query still
            // yields a fullscreen kiosk on whatever monitor Tauri picked, rather than a
            // small floating window.
            if !windowed {
                let _ = window.set_fullscreen(true);
            }
            let _ = window.show();
            let _ = window.set_focus();

            nav::install(
                &window,
                tx_setup.clone(),
                telem_setup.clone(),
                nav_policy_setup.clone(),
                ready_setup.clone(),
            );
            scheme_guard::install(&window, telem_setup.clone(), nav_policy_setup.clone());
            egress::install(&window, telem_setup.clone(), nav_policy_setup.clone());
            hardening::apply(
                &window,
                nav_policy_setup.clone(),
                content_zoom,
                telem_setup.clone(),
            );
            shortcuts::install(&window, app.handle().clone(), exit_gesture_setup.clone());
            gesture::install(&window, app.handle().clone(), exit_gesture_setup.clone());
            recovery::install(&window, telem_setup.clone(), nav_policy_setup.clone());

            // focus.lost (spec §7): best-effort, not a security boundary — a kiosk
            // that gets alt-tabbed away logs it (spec taxonomy `focus.lost`, WARNING)
            // and immediately asks the window manager to give it the foreground back.
            // `set_focus` here can itself lose again (e.g. a genuinely modal system
            // dialog); that just re-fires this same handler, which tries again on the
            // next loss — no special-casing needed.
            let window_focus = window.clone();
            let telem_focus = telem_setup.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::Focused(false) = event {
                    telem_focus.focus_lost();
                    if let Err(e) = window_focus.set_focus() {
                        eprintln!("kiosk-main: set_focus (focus.lost reassert) failed: {e}");
                    }
                }
            });

            let sink = TauriSink {
                app: app.handle().clone(),
                tx: tx_setup.clone(),
                refetch: refetch_setup.clone(),
                telem: telem_setup.clone(),
                cancel: cancel_setup.clone(),
            };

            // P1-F1 Task 2: `--safe` never drives the FSM (`AppState::Safe` has no
            // `Event` transitions in) or spawns the remote-content driver — it just
            // renders the bundled safe page once, directly, with the device id +
            // last crash breadcrumb. Must never fail to render: a missing/unreadable
            // `crash-panic.txt` degrades to "unknown", never a panic or a blank
            // screen.
            if safe {
                // Capped at 500 chars BEFORE encoding: percent-encoding expands up to
                // 3x, and a multi-hundred-KB panic Display would make a URL WebView2
                // refuses outright — i.e. safe.html renders NOTHING, defeating the one
                // invariant this page has. `.chars()` (not byte slicing) so the cut
                // never lands mid-UTF-8 and produces an undecodable %-sequence.
                let last_error_text: String = last_error
                    .as_deref()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_else(|| "unknown".to_string())
                    .chars()
                    .take(500)
                    .collect();
                let safe_url = format!(
                    "{}?device={}&err={}",
                    bundled_url("safe.html"),
                    url_encode(&device_id_safe),
                    url_encode(&last_error_text),
                );
                sink.navigate(&safe_url);
            } else {
                // P1-F1 Task 3: nightly-reload timer. Fires `IdleExpired` INTO the FSM
                // rather than navigating the webview directly, so the reload obeys the
                // machine's rules (state.rs rule 9): Online + idle_clear clears the
                // profile first and re-navigates only after `ProfileCleared` (the
                // privacy gate), Online without idle_clear reloads home, and any other
                // state (Offline, ErrorPage, Clearing) is a no-op — a 04:00 reload must
                // not replace the offline video with a doomed remote load, nor paint
                // home over an in-progress clear. `--safe` never spawns this (same
                // `if safe {} else {}` split as the FSM driver above).
                let tx_reload = tx_setup.clone();
                let maint_telem = telem_setup.clone();
                tokio::spawn(maintenance::run(
                    nightly_reload,
                    maintenance_timezone,
                    move || {
                        let _ = tx_reload.try_send(AppEvent::IdleExpired);
                    },
                    move || {
                        maint_telem.config_warn(
                            "maintenance.nightly_reload",
                            "unparseable HH:MM or unknown timezone; nightly reload disabled",
                        )
                    },
                    cancel_setup.clone(),
                ));

                tokio::spawn(driver::run(
                    rx,
                    Driver {
                        machine: Machine::new(machine_cfg),
                    },
                    Box::new(sink),
                    cancel_setup.clone(),
                ));

                let tx_first = tx_setup.clone();
                tokio::spawn(async move {
                    let _ = tx_first.send(first_event).await;
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("kiosk-main: failed to start")
        // The graceful-exit path: on a locked-down kiosk tao usually tears the process
        // down without ever reaching here, so this is best-effort only — WARNING+
        // durability already rests on `Spool::append`'s synchronous fsync (see
        // `install_panic_hook`), not on this `app.stop`.
        .run(move |_app, event| {
            if let tauri::RunEvent::Exit = event {
                telem.app_stop();
                cancel.cancel();
            }
        });
}
