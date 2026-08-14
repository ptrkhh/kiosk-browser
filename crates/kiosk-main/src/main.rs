#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod boot;
mod clear;
mod cli;
mod credential_acl;
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
use driver::{Driver, EffectSink, SafeLatchedSink, SafeModeLatch};
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

/// The app origin for bundled pages. Windows/`wry` cannot navigate the top-level frame
/// to a custom scheme, so Tauri serves bundled assets at an `http://` host there; on
/// Linux/WebKitGTK the origin is the literal custom scheme. Same compile-time switch
/// Tauri uses internally (`tauri-2.11.5/src/manager/mod.rs:340-345`,
/// `AppManager::tauri_protocol_url`).
const APP_ORIGIN: &str = if cfg!(windows) {
    "http://tauri.localhost"
} else {
    "tauri://localhost"
};

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
    /// before building its diagnostic URL. Guards the two ways that cap could break `safe.html`:
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

/// SEC-09 reload gate, Critical 2 fix: `fetch::run` cannot navigate directly (it was
/// spawned before the window existed), so it reports the violation message once over
/// `credential_violation_rx`; this is the task that performs the actual `safe.html`
/// navigation — the exact same call `--safe` uses — and then holds that state.
///
/// Cancelling `fetch_probe_cancel` alone narrows the race but does not close it:
/// `probe::run`'s in-flight `probe_once` and its unconditional `tx.send` aren't raced
/// against cancellation, and `driver::run`'s `select!` has no `biased;`, so an
/// `AppEvent` already buffered in the channel at the moment the token fires can still
/// be dispatched. `safe_mode_latch.trip()` closes that: it trips the SAME latch
/// `driver::run`'s `SafeLatchedSink` checks on every `dispatch`, so once tripped, no
/// buffered event, in-flight probe result, or future producer can push a
/// `Navigate`/`ShowVideo` through to the webview, however `select!` happens to
/// schedule. Tripped before the cancel (belt-and-braces: the latch, not the token, is
/// what makes this race-free).
///
/// SEC-09 final review, FIX 2: `fetch_probe_cancel` cancels ONLY `fetch::run` and
/// `probe::run` — see its doc comment at the call site in `main` — never
/// `driver::run`, which is handed the top-level shutdown `cancel` instead and stays
/// alive so `Effect::ClearProfile` (the idle-clear privacy gate) keeps reaching
/// `TauriSink` while the kiosk sits in safe mode. The `SafeLatchedSink` latch alone is
/// what keeps the webview from navigating away — narrowing to `fetch`/`probe` doesn't
/// weaken that: neither task is the one this function's own `navigate` calls route
/// through, and every OTHER navigating producer (`driver::run`'s dispatch) already
/// checks the latch on every call, cancelled or not.
///
/// SEC-09 final review, FIX 3: `trip()` and `navigate(&url)` are not atomic with a
/// `driver::run` dispatch that is already in flight — a dispatch that loaded
/// `latched == false` microseconds before `trip()` runs can still complete
/// `inner.dispatch` (and thus its own navigation) AFTER this function's first
/// `navigate(&url)` call lands, leaving the kiosk showing remote content while
/// everything after it is latched. Navigating to the SAME url a second time, after
/// the latch is tripped and `fetch`/`probe` are cancelled, makes this call the last
/// write in that vanishingly rare ordering — simple last-write-wins, not a handshake
/// protocol.
///
/// Extracted from `.setup()` so this is host-testable without a real Tauri window.
/// One-shot: once tripped there is nothing left to watch for (`fetch::run` has already
/// stopped polling).
async fn hold_safe_after_credential_violation(
    mut credential_violation_rx: mpsc::Receiver<String>,
    device_id: String,
    navigate: impl Fn(&str),
    fetch_probe_cancel: CancellationToken,
    safe_mode_latch: SafeModeLatch,
) {
    if let Some(message) = credential_violation_rx.recv().await {
        let url = format!(
            "{}?device={}&err={}",
            bundled_url("safe.html"),
            url_encode(&device_id),
            url_encode(&message),
        );
        safe_mode_latch.trip();
        navigate(&url);
        fetch_probe_cancel.cancel();
        // FIX 3: last-write-wins against the residual race described above.
        navigate(&url);
    }
}

#[cfg(test)]
mod hold_safe_after_credential_violation_tests {
    use super::*;
    use std::sync::Mutex;

    /// A no-op `EffectSink`, only so tests can build a `SafeLatchedSink`/`SafeModeLatch`
    /// pair without a real Tauri window.
    #[derive(Default)]
    struct NoopSink;
    impl EffectSink for NoopSink {
        fn dispatch(&mut self, _effect: Effect) {}
    }

    /// TDD RED (Critical 2, before the fix): the prior implementation navigated to
    /// `safe.html` but never cancelled anything shared with `driver`/`probe` — so this
    /// assertion on `fetch_probe_cancel.is_cancelled()` failed against that code, proving
    /// the exact gap the review flagged (a subsequent `LinkChanged` could still reach the
    /// FSM and repaint over `safe.html`). GREEN once `fetch_probe_cancel.cancel()` was
    /// added above. `safe_mode_latch.is_tripped()`-equivalent (`.latch_handle()` off the
    /// same `SafeLatchedSink` `driver::run` would box) closes the residual race the review
    /// found even after that first cancel fix — see `driver::tests::
    /// a_buffered_event_never_reaches_the_sink_once_safe_mode_latches` for the mechanism
    /// itself proven directly against `driver::run`.
    #[tokio::test]
    async fn a_reported_violation_navigates_and_holds_safe_mode() {
        let (tx, rx) = mpsc::channel::<String>(1);
        let navigated = Arc::new(Mutex::new(Vec::new()));
        let navigated_task = navigated.clone();
        let fetch_probe_cancel = CancellationToken::new();
        let fetch_probe_cancel_task = fetch_probe_cancel.clone();
        let latched_sink = SafeLatchedSink::new(NoopSink);
        let safe_mode_latch = latched_sink.latch_handle();

        let handle = tokio::spawn(hold_safe_after_credential_violation(
            rx,
            "lobby-01".into(),
            move |url: &str| navigated_task.lock().unwrap().push(url.to_string()),
            fetch_probe_cancel_task,
            safe_mode_latch,
        ));

        tx.send("credential file permissions are not owner-only".into())
            .await
            .unwrap();
        handle.await.unwrap();

        let urls = navigated.lock().unwrap();
        // FIX 3 (SEC-09 final review): navigates TWICE, last-write-wins against a
        // driver dispatch that raced the latch — see this function's doc comment.
        assert_eq!(urls.len(), 2, "must navigate exactly twice (last-write-wins)");
        for url in urls.iter() {
            assert!(url.contains("safe.html"));
            assert!(url.contains("device=lobby-01"));
        }
        assert!(
            fetch_probe_cancel.is_cancelled(),
            "fetch/probe must be cancelled so no later LinkChanged/config-fetch can \
             produce another navigating AppEvent"
        );
        // `SafeModeLatch` holds its own `Arc` clone of the flag, so `safe_mode_latch`
        // stays meaningful regardless of `latched_sink`'s lifetime; this drop only
        // silences the unused-binding warning.
        drop(latched_sink);
    }

    /// A channel closed without ever reporting a violation (e.g. `fetch::run` exited via
    /// `cancel` at shutdown, not via the credential gate) must not spuriously trip safe
    /// mode.
    #[tokio::test]
    async fn a_closed_channel_without_a_report_does_not_cancel() {
        let (tx, rx) = mpsc::channel::<String>(1);
        let fetch_probe_cancel = CancellationToken::new();
        let fetch_probe_cancel_task = fetch_probe_cancel.clone();
        let latched_sink = SafeLatchedSink::new(NoopSink);
        let safe_mode_latch = latched_sink.latch_handle();
        drop(tx);

        hold_safe_after_credential_violation(
            rx,
            "lobby-01".into(),
            |_| {},
            fetch_probe_cancel_task,
            safe_mode_latch,
        )
        .await;

        assert!(!fetch_probe_cancel.is_cancelled());
    }

    /// SEC-09 final review, FIX 2: proves the actual assembly, not just
    /// `SafeLatchedSink` in isolation (`driver::tests`) or
    /// `hold_safe_after_credential_violation` in isolation (the tests above). Wires a
    /// real `driver::run` task on a cancel token this test NEVER cancels — mirroring
    /// `main`'s `cancel_setup`, distinct from the `fetch_probe_cancel` this function
    /// DOES cancel — and drives a full violation-then-idle-clear sequence through it:
    /// after the violation, `Effect::ClearProfile` (rule 9: Online + IdleExpired +
    /// idle_clear) must still reach the inner sink, and the `Navigate` rule 9 emits
    /// once `ProfileCleared` completes the gate must NOT. Fails against eadca54, where
    /// `driver::run` was handed the SAME token this function cancels and the task
    /// exited outright on violation — dispatching NOTHING at all afterward,
    /// `ClearProfile` included, contradicting that commit's own message ("so idle
    /// profile-clear still runs").
    #[tokio::test]
    async fn a_reported_violation_leaves_the_driver_running_but_latched() {
        use kiosk_core::app::state::{MachineConfig, DEFAULT_ERROR_RETRY_SECONDS};
        use kiosk_core::config::schema::Fallback;

        #[derive(Clone, Default)]
        struct SharedRecordingSink(Arc<Mutex<Vec<Effect>>>);
        impl EffectSink for SharedRecordingSink {
            fn dispatch(&mut self, effect: Effect) {
                self.0.lock().unwrap().push(effect);
            }
        }

        let cfg = MachineConfig {
            fallback: Fallback::Video,
            error_max_retries: 5,
            idle_clear: true,
            error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
        };
        let (app_tx, app_rx) = mpsc::channel::<AppEvent>(8);
        let inner = SharedRecordingSink::default();
        let effects = inner.0.clone();
        let latched_sink = SafeLatchedSink::new(inner);
        let safe_mode_latch = latched_sink.latch_handle();

        // Mirrors `main`: `driver::run` is handed a token this test never cancels —
        // the point under test is that the violation handler below does not reach it
        // either (it only ever touches its OWN `fetch_probe_cancel`, below).
        let driver_never_cancelled = CancellationToken::new();
        let driver_handle = tokio::spawn(driver::run(
            app_rx,
            Driver {
                machine: Machine::new(cfg),
            },
            Box::new(latched_sink),
            driver_never_cancelled,
        ));

        // Bring the FSM to `Online` before the violation, same as a real boot. This
        // legitimately dispatches one `Navigate` — the boot navigation, well before
        // any latch trips — so it's drained and cleared below rather than being
        // mistaken for a post-violation leak.
        app_tx
            .send(AppEvent::ConfigApplied {
                url: "https://home.test/".into(),
            })
            .await
            .unwrap();
        let boot_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while effects.lock().unwrap().is_empty() && std::time::Instant::now() < boot_deadline {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            *effects.lock().unwrap(),
            vec![Effect::Navigate("https://home.test/".into())],
            "sanity: the boot event must navigate before the violation is ever reported"
        );
        effects.lock().unwrap().clear();

        // Drive the violation exactly as `main` wires it: its own
        // `fetch_probe_cancel`, distinct from `driver_never_cancelled` above.
        let (violation_tx, violation_rx) = mpsc::channel::<String>(1);
        let fetch_probe_cancel = CancellationToken::new();
        let navigated: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let navigated_task = navigated.clone();
        let handler = tokio::spawn(hold_safe_after_credential_violation(
            violation_rx,
            "lobby-01".into(),
            move |url: &str| navigated_task.lock().unwrap().push(url.to_string()),
            fetch_probe_cancel,
            safe_mode_latch,
        ));
        violation_tx
            .send("credential file permissions are not owner-only".into())
            .await
            .unwrap();
        handler.await.unwrap();
        assert_eq!(
            navigated.lock().unwrap().len(),
            2,
            "the violation itself must still navigate (twice, FIX 3's last-write-wins)"
        );

        // Post-violation: idle-clear must still fire through the SAME latched sink
        // the (still-alive) driver task dispatches through.
        app_tx.send(AppEvent::IdleExpired).await.unwrap();
        // Post-violation: releasing the privacy gate would normally re-navigate home
        // (rule 9) — that Navigate must be blocked by the latch.
        app_tx.send(AppEvent::ProfileCleared).await.unwrap();
        // Dropping the only sender closes the channel: `driver::run`'s `rx.recv()`
        // then returns `None` and the loop breaks on its own, WITHOUT this test ever
        // cancelling `driver_never_cancelled` — guaranteeing both events above were
        // fully processed (in order) before the join below returns, no sleep-and-hope
        // polling needed.
        drop(app_tx);
        tokio::time::timeout(Duration::from_secs(2), driver_handle)
            .await
            .expect("driver::run must drain and exit once its channel closes")
            .unwrap();

        let got = effects.lock().unwrap();
        assert_eq!(
            *got,
            vec![Effect::ClearProfile { full: true }],
            "ClearProfile must still reach the inner sink after the violation, and no \
             Navigate may reach it, even though driver::run was never cancelled: {got:?}"
        );
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

/// The data dir (cache, spool, last-good) — `%ProgramData%\kiosk\` on Windows,
/// `/var/lib/kiosk/` on Linux (spec §4). Never operator-overridden (unlike the install
/// dir): this is not something a `kiosk.ini` deployment ever needs to relocate.
///
/// The launcher's `resolve_data_dir` must return the identical path — it drains the
/// `spool/main` partition written here (P2-C C16).
#[cfg(windows)]
fn resolve_data_dir() -> PathBuf {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kiosk")
}

#[cfg(not(windows))]
fn resolve_data_dir() -> PathBuf {
    PathBuf::from("/var/lib/kiosk")
}

#[cfg(windows)]
fn machine_id() -> Option<String> {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let mut bytes = 0;
    // Safety: fixed registry paths; first call sizes the buffer, second writes only that size.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let mut value = vec![0u16; bytes as usize / 2];
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Cryptography"),
            w!("MachineGuid"),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        value.truncate(value.iter().position(|&c| c == 0).unwrap_or(value.len()));
        String::from_utf16(&value).ok()
    }
}

/// Pure, host-tested: the `/etc/machine-id` contents → a device id, or `None` when the
/// file is empty/whitespace. Split out of `machine_id` so the trimming rule is testable
/// without an `/etc` fixture.
#[cfg(not(windows))]
fn parse_machine_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// systemd's `/etc/machine-id` (spec §4). Absent or unreadable degrades exactly as the
/// Windows missing-MachineGuid path does — `None`, no panic, boot continues.
#[cfg(not(windows))]
fn machine_id() -> Option<String> {
    parse_machine_id(&std::fs::read_to_string("/etc/machine-id").ok()?)
}

#[cfg(all(test, not(windows)))]
mod data_dir_tests {
    use super::{parse_machine_id, resolve_data_dir};

    #[test]
    fn machine_id_is_trimmed() {
        assert_eq!(
            parse_machine_id("2c4a1b6e8f9d4c3b8a7e6f5d4c3b2a19\n"),
            Some("2c4a1b6e8f9d4c3b8a7e6f5d4c3b2a19".to_string())
        );
    }

    /// An empty or whitespace-only `/etc/machine-id` degrades exactly as the Windows
    /// no-MachineGuid path does: `None`, no panic, boot continues with the fallback id.
    #[test]
    fn an_empty_machine_id_file_degrades_to_none() {
        assert_eq!(parse_machine_id(""), None);
        assert_eq!(parse_machine_id("   \n"), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_linux_data_dir_is_var_lib_kiosk() {
        assert_eq!(
            resolve_data_dir(),
            std::path::PathBuf::from("/var/lib/kiosk")
        );
    }
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
#[derive(Clone)]
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

    // `data_dir` is pure/CLI-independent (spec §4), so resolve it before config load.
    let data_dir = resolve_data_dir();
    // Best-effort: %ProgramData%\kiosk\ isn't created until spool.rs/store.rs run
    // later, both after the two early panic sites below. Without this, File::create
    // in the hook fails silently on a fresh install and there's no breadcrumb at all.
    let _ = std::fs::create_dir_all(&data_dir);
    install_panic_hook_file_only(data_dir.clone());

    let ini_path = config_dir.join("kiosk.ini");
    let machine_id = machine_id();
    let (booted, config_error, boot_fault_reason) =
        boot::load(&ini_path, &data_dir, machine_id.as_deref()).into_parts();
    let safe = args.safe || config_error.is_some();
    // Config faults win over an older crash breadcrumb. Missing breadcrumb degrades to
    // "unknown"; neither case can abort the safe renderer.
    let safe_error = config_error.or_else(|| {
        args.safe.then(|| {
            std::fs::read_to_string(data_dir.join("crash-panic.txt"))
                .unwrap_or_else(|_| "unknown".to_string())
        })
    });

    // ---- Extract-before-move: every field this main needs from `booted.manager` is
    // read out HERE, before `booted.manager` moves into `fetch::run` below. ----
    let bootstrap = booted.manager.bootstrap().clone();
    let device_id = booted.manager.device_id().to_string();
    // SEC-09 reload gate: cloned before `device_id` moves into the telemetry
    // thread's closure below — needed to build the `safe.html?device=` url if a
    // credential-DACL violation trips mid-run (see `credential_violation_sink`
    // in `.setup()`).
    let device_id_for_reload = device_id.clone();
    // Build once, before `device_id` moves into telemetry. Recovery uses this same
    // diagnostic URL, never the configured remote home, while safe mode is active.
    let safe_url = safe.then(|| {
        // Cap before encoding: percent expansion cannot create an unusably long URL.
        let error: String = safe_error
            .as_deref()
            .unwrap_or("unknown")
            .chars()
            .take(500)
            .collect();
        format!(
            "{}?device={}&err={}",
            bundled_url("safe.html"),
            url_encode(&device_id),
            url_encode(&error),
        )
    });
    let revision = booted.manager.revision();
    let home_url = safe_url
        .clone()
        .unwrap_or_else(|| booted.manager.home_url());
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
    // SEC-09 reload gate: fetch::run re-checks the DACL on its own poll cadence
    // and needs its own copy — `credential_path` itself moves into the
    // telemetry thread's closure below.
    let credential_path_for_reload = credential_path.clone();
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
    let policy = if safe {
        NavPolicy::from_config(&kiosk_core::config::schema::Content::default(), &home_url)
    } else {
        NavPolicy::from_config(&booted.manager.current().content, &home_url)
    };
    let nav_policy: SharedNavPolicy = Arc::new(ArcSwap::from_pointee(policy));
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
    // SEC-09 (Critical 2 fix, narrowed further by the final review's FIX 2): once
    // the reload gate reports a credential-DACL violation and the kiosk has
    // navigated to `safe.html`, NOTHING may navigate the webview away from it
    // again — but `probe::run` keeps emitting `LinkChanged` and `fetch::run` keeps
    // polling, because `AppState::Safe` is entered out-of-band (no `Event`
    // transitions into it, by design — see `kiosk_core::app::state`) and neither
    // task was ever told to stop. A CHILD of `cancel` scoped to exactly
    // `probe`/`fetch` (never `driver`, and never `idle`/`health`/`heartbeat`,
    // which must keep running and reporting regardless of safe mode) lets the
    // credential-violation handler in `.setup()` (below) permanently silence the
    // only two tasks that can PRODUCE a navigating `AppEvent`, without touching
    // the rest of the process. `driver::run` is deliberately NOT cancelled by
    // this token (see its spawn site below, and `driver::SafeLatchedSink`'s doc
    // comment): the driver task must stay alive so a later `IdleExpired` can
    // still dispatch `Effect::ClearProfile` — the FSM's privacy gate — through to
    // `TauriSink`. The `SafeLatchedSink` latch, not task cancellation, is what
    // stops a `Navigate`/`ShowVideo` effect from reaching the webview once
    // tripped. A normal shutdown still cancels this token too, since it is a
    // child of `cancel`.
    let fetch_probe_cancel = cancel.child_token();
    let prober = Prober::new(clock.clone());
    // SEC-09 reload gate: `fetch::run` detects a credential-DACL violation on its
    // own poll cadence but has no window/`AppHandle` yet at the point it's spawned
    // (below, before `tauri::Builder` even runs `.setup()`), so it cannot navigate
    // directly. It reports the fault message once over this channel; the
    // `.setup()`-spawned receiver below (which DOES have a `TauriSink`) does the
    // actual `safe.html` navigation — the same navigate path boot uses, not a
    // second mechanism.
    let (credential_violation_tx, credential_violation_rx) = mpsc::channel::<String>(1);

    // The P1-B logger `Transport` is `reqwest::blocking`, which panics if it is built
    // OR driven inside a tokio runtime (it stands up and drops its own internal
    // runtime, forbidden in an async context). So the ENTIRE logger stack — `build`
    // (which constructs the blocking client) and the drain loop `run` — lives on a
    // DEDICATED OS THREAD, off this `#[tokio::main]` runtime, talking over a
    // `std::sync::mpsc` channel. The thread hands the `Telemetry` handle back for the
    // async tasks to clone.
    //
    // Credential read/parse was validated by `boot::load`; any later telemetry setup
    // fault degrades to `Telemetry::disabled()` without replacing the visible page.
    let app_version = kiosk_core::app_version().to_string();
    // SEC-09 boot gate durability (Critical 1 fix): `telemetry::build` is never called
    // below when `boot_fault_reason` is `credential_permissions` (see the thread
    // closure), so `telem` resolves to `Telemetry::disabled()` and its later
    // `config_error` call is a guaranteed no-op — nothing would ever reach disk. Write
    // the SAME event directly to the local `Spool` here, BEFORE `bootstrap`/`device_id`/
    // `clock` move into the thread closure: this needs no GCL client and no credential —
    // exactly what must still work when the credential itself is refused.
    if let Some(reason) = boot_fault_reason {
        telemetry::spool_boot_config_error(
            &data_dir,
            &bootstrap,
            &device_id,
            app_version.clone(),
            revision,
            &clock,
            reason,
            &booted.manager.current().logging,
        );
    }
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
            // SEC-09 boot gate: a credential-permissions violation was already
            // detected in `boot::load` — never call `telemetry::build` in that
            // case (it would be the very credential read this gate exists to
            // prevent). Degrade exactly like a build failure: no telemetry, kiosk
            // still shows content via the safe path.
            if boot_fault_reason == Some(boot::CREDENTIAL_PERMISSIONS_REASON) {
                let _ = handle_tx.send(None);
                return;
            }
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
    // SEC-09 boot gate: the durable `config.error{credential_permissions}` write
    // already happened above, directly against the local `Spool`, BEFORE `telem` even
    // existed (`telem` is always `Telemetry::disabled()` in this exact scenario — see
    // `spool_boot_config_error`'s call site). No `telem.config_error` call belongs
    // here: it would be a guaranteed no-op against a disabled handle.

    // P1-F1 Task 2: `--safe` must make no remote request — no config fetch, no
    // prober, no remote content navigation. `booted.manager`/`network`/`prober` are
    // simply left unmoved and drop here; everything `.setup()` needs from them was
    // already extracted above.
    if !safe {
        tokio::spawn(fetch::run(
            booted.manager, // MOVES — every field needed above was extracted first.
            config_url,
            poll_s,
            tx.clone(),
            telem.clone(),
            refetch.clone(),
            fetch_probe_cancel.clone(),
            nav_policy_fetch,
            credential_path_for_reload,
            credential_violation_tx,
        ));
        tokio::spawn(probe::run(
            prober,
            network,
            probe_url,
            tx.clone(),
            telem.clone(),
            fetch_probe_cancel.clone(),
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
    let tx_setup = tx.clone();
    let refetch_setup = refetch.clone();
    let telem_setup = telem.clone();
    let cancel_setup = cancel.clone();
    let fetch_probe_cancel_setup = fetch_probe_cancel.clone();
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
        // `tauri.localhost` derivation) → `http://kioskasset.localhost/kiosk-offline.mp4`;
        // Linux/WebKitGTK serves the same scheme at its literal custom-scheme origin →
        // `kioskasset://localhost/kiosk-offline.mp4`. `offline.html` picks the mp4 URL by
        // `location.protocol` (page-local JS, no serve-time templating).
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

            // Linux nav guard (P2-A): wry already installs the `decide-policy` handler this drives
            // (`wry-0.55.1/src/webkitgtk/mod.rs:547-576`) — NavigationAction only, every frame,
            // correct return value. Do NOT hand-write a `decide-policy` handler.
            //
            // The `true` third argument is a deliberate Linux decision — enforce on ALL frames —
            // not a transfer of the Windows justification. A blocked sub-frame therefore reports
            // `nav.blocked{reason: "not_allowlisted"}` where Windows reports `"egress"`.
            #[cfg(not(windows))]
            {
                let guard_policy = nav_policy_setup.clone();
                let guard_telem = telem_setup.clone();
                builder = builder.on_navigation(move |url| {
                    match nav::should_block(&guard_policy.load(), url.as_str(), true) {
                        Some(reason) => {
                            guard_telem.nav_blocked(reason.as_str(), url.as_str());
                            false
                        }
                        None => true,
                    }
                });
                // §7: "new windows navigate in place". Hand the URL back to the main webview and
                // THEN deny: `navigate` is a dispatcher-proxied non-blocking send, safe from the
                // event-loop thread, and the resulting load re-enters `on_navigation` above and
                // faces the same guard — exactly Windows' `SetHandled(true)` + `Navigate`. Deny
                // explicitly rather than relying on wry not connecting `connect_create`.
                let popup_handle = app.handle().clone();
                builder = builder.on_new_window(move |url, _features| {
                    if let Some(w) = popup_handle.get_webview_window(WINDOW_LABEL) {
                        let _ = w.navigate(url);
                    }
                    tauri::webview::NewWindowResponse::Deny
                });
            }

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
            // SEC-09 Critical 2 fix: the ONE `EffectSink` `driver::run` ever dispatches
            // through — see `latch_handle()`'s use below at the credential-violation
            // spawn, and `driver::SafeLatchedSink`'s doc comment for why wrapping here
            // (rather than hardening `probe::run`/`driver::run`'s `select!`
            // separately) closes every producer, present or future, in one place.
            let latched_sink = SafeLatchedSink::new(sink.clone());

            // P1-F1 Task 2: `--safe` never drives the FSM (`AppState::Safe` has no
            // `Event` transitions in) or spawns the remote-content driver — it just
            // renders the bundled safe page once, directly, with the device id plus
            // config fault or last crash breadcrumb. Missing detail degrades to
            // "unknown", never a panic or blank screen.
            if let Some(safe_url) = safe_url.as_deref() {
                sink.navigate(safe_url);
            } else {
                // SEC-09 reload gate: `fetch::run` cannot navigate directly (it was
                // spawned before this window existed — see the channel's doc comment
                // at its creation), so it reports the violation message once here,
                // where a real `TauriSink` exists, and THIS task performs the actual
                // `safe.html` navigation — the exact same call `--safe` uses above,
                // never a second mechanism. One-shot: once tripped there is nothing
                // left to watch for (`fetch::run` has already stopped polling).
                // SEC-09 Critical 2 fix: `driver::run` below is handed a
                // `SafeLatchedSink` wrapping `sink`, not `sink` directly — the choke
                // point every `Navigate`/`ShowVideo` effect funnels through. This
                // `latch_handle()` is a second, `Clone`+`Send` handle onto that SAME
                // latch, so this task (which navigates through its own unwrapped
                // `sink` clone, never through `dispatch`) can still trip it. Tripping
                // it makes every later `dispatch` on the driver's sink a no-op,
                // regardless of whether the event behind it was already buffered in
                // the channel or produced by an in-flight probe that lands after
                // `fetch_probe_cancel.cancel()` — closing the residual race
                // cancellation alone left open.
                let credential_violation_sink = sink.clone();
                let fetch_probe_cancel_violation = fetch_probe_cancel_setup.clone();
                let safe_mode_latch = latched_sink.latch_handle();
                tokio::spawn(hold_safe_after_credential_violation(
                    credential_violation_rx,
                    device_id_for_reload,
                    move |url| credential_violation_sink.navigate(url),
                    fetch_probe_cancel_violation,
                    safe_mode_latch,
                ));

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

                // SEC-09 final review, FIX 2: `driver::run` is handed the top-level
                // shutdown `cancel`, NOT `fetch_probe_cancel_setup` — the credential-
                // violation handler cancels only `fetch::run`/`probe::run` (see
                // `hold_safe_after_credential_violation`'s doc comment), leaving the
                // driver task alive so idle-triggered `ClearProfile` keeps reaching
                // `TauriSink` even after a reload-gate violation latches navigation.
                // The `SafeLatchedSink` wrapping `latched_sink` above is the sole
                // choke point that must (and does — see this module's
                // `a_reported_violation_leaves_the_driver_running_but_latched` test)
                // close the navigation race; cancelling the driver task itself would
                // reopen the exact idle-clear gap eadca54's commit message claimed to
                // fix but did not (the driver task would simply exit and dispatch
                // nothing at all, `ClearProfile` included).
                tokio::spawn(driver::run(
                    rx,
                    Driver {
                        machine: Machine::new(machine_cfg),
                    },
                    Box::new(latched_sink),
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
