#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod boot;
mod cli;
mod driver;
mod effect;
mod fetch;
mod nav;
mod probe;
mod telemetry;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use driver::{Driver, EffectSink};
use effect::PageTarget;
use kiosk_core::app::state::{Effect, Event as AppEvent, Machine};
use kiosk_core::logging::time::TrustedClock;
use kiosk_core::net::prober::Prober;
use kiosk_core::net::reach::resolve_probe_url;
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

/// Best-effort crash telemetry (spec TEL-10, brief step 4).
///
/// The async `Logger` is owned by the logger task (`telemetry::run`); a
/// `std::panic::set_hook` closure runs synchronously on the panicking thread and
/// cannot reach into another task to call it directly. `Telemetry::panic` is the one
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
/// still-alive logger task drains and fsyncs it on its very next scheduling. The
/// entry is genuinely at risk only if the panic unwinds out of `main` itself (e.g. from
/// inside `.setup()`), tearing the whole runtime down before the logger task gets that
/// next turn. Documented here rather than solved: a synchronous, reentrant-safe spool
/// write from inside a panic hook (racing the very same spool the logger task also
/// holds open, violating the "one writer per segment" invariant — spec arch-01) would
/// be the "fragile mechanism" the brief warns against, not a fix.
fn install_panic_hook(telem: telemetry::Telemetry) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        telem.panic(&info.to_string());
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
            // D2c: profile clearing is not implemented yet. D2a never arms the idle
            // timer that would emit this in practice (plan's "deferred effects" note),
            // so this is a documented no-op, not a placeholder oversight. No `Telemetry`
            // helper names this event (D2c's job to add one) — a bare warning is the
            // honest signal without inventing a Cloud Logging taxonomy entry early.
            Effect::ClearProfile { full } => {
                eprintln!(
                    "TauriSink: Effect::ClearProfile{{full:{full}}} not implemented (D2c) — no-op"
                );
                let _ = &self.telem; // kept for the D2c implementer; unused today.
            }
            other => unreachable!(
                "effect::page_for only returns None for RefetchConfig/ClearProfile, got {other:?}"
            ),
        }
    }
}

#[tokio::main]
async fn main() {
    let args = cli::Args::parse(std::env::args());
    let config_dir = resolve_config_dir(args.config.as_deref());
    let ini_path = config_dir.join("kiosk.ini");
    let ini_text = std::fs::read_to_string(&ini_path).unwrap_or_else(|e| {
        panic!(
            "kiosk-main: cannot read {} ({e}); pass --config <dir> in dev",
            ini_path.display()
        )
    });

    let data_dir = resolve_data_dir();
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
    let revision = booted.manager.revision();
    let home_url = booted.manager.home_url();
    let network = booted.manager.current().network.clone();
    let credential_path = config_dir.join(&bootstrap.credential);
    let config_url = bootstrap.config_url.clone();
    let poll_s = network.config_poll_s;
    let probe_url = resolve_probe_url(&network.connectivity_check_url, &home_url);
    let machine_cfg = booted.machine_cfg;
    let first_event = booted.first_event;
    let warnings = booted.warnings;

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

    // A telemetry-init failure (missing/malformed kiosk-credential.json) must NOT stop
    // the kiosk from showing content — the whole point of the device is the screen. On
    // error we log to stderr and run WITHOUT telemetry: `Telemetry::disabled()` is a
    // handle whose every helper silently no-ops, so every clone handed to
    // fetch/probe/driver/TauriSink/nav keeps working unchanged.
    let telem = match telemetry::build(
        &bootstrap,
        &credential_path,
        &device_id,
        clock,
        kiosk_core::app_version().to_string(),
        revision,
        &data_dir,
    ) {
        Ok((telem, logger, log_rx)) => {
            tokio::spawn(telemetry::run(logger, log_rx, cancel.clone()));
            telem
        }
        Err(e) => {
            eprintln!("kiosk-main: telemetry disabled ({e}); continuing without it");
            telemetry::Telemetry::disabled()
        }
    };

    install_panic_hook(telem.clone());
    telem.app_start();
    telem.config_applied(revision, &warnings);

    tokio::spawn(fetch::run(
        booted.manager, // MOVES — every field needed above was extracted first.
        config_url,
        poll_s,
        tx.clone(),
        telem.clone(),
        refetch.clone(),
        cancel.clone(),
    ));
    tokio::spawn(probe::run(
        prober,
        network,
        probe_url,
        tx.clone(),
        telem.clone(),
        cancel.clone(),
    ));

    let windowed = args.windowed;
    let tx_setup = tx.clone();
    let refetch_setup = refetch.clone();
    let telem_setup = telem.clone();
    let cancel_setup = cancel.clone();

    tauri::Builder::default()
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
            builder = if windowed {
                builder.inner_size(1280.0, 800.0).decorations(true)
            } else {
                builder
                    .fullscreen(true)
                    .decorations(false)
                    .always_on_top(true)
                    .focused(true)
            };
            let window = builder.build()?;

            nav::install(&window, tx_setup.clone(), telem_setup.clone());

            let sink = TauriSink {
                app: app.handle().clone(),
                tx: tx_setup.clone(),
                refetch: refetch_setup.clone(),
                telem: telem_setup.clone(),
                cancel: cancel_setup.clone(),
            };
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
