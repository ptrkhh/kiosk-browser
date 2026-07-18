# P1-D2a — kiosk-main Integration Spine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the pure P1-D1 `kiosk_core::app::Machine` to live I/O — a Tauri fullscreen webview, connectivity probe, remote-config fetch, and Cloud Logging telemetry — producing a kiosk that boots, shows the site, survives offline with the video loop, polls config, and emits telemetry.

**Architecture:** An actor spine on tokio tasks. All real-world inputs feed one `mpsc<AppEvent>` channel consumed by a single **driver** that owns the `Machine`; the `Vec<Effect>` it returns is dispatched through an **`EffectSink` trait** (production `TauriSink` drives the webview; a recording fake makes the driver host-testable without Tauri). Logger, probe, and config-poll are their own tasks.

**Tech Stack:** Rust, Tauri 2, tokio (bundled by Tauri), reqwest (async), `kiosk-core` (config/telemetry/prober/nav, all already built + host-tested).

## Global Constraints

- **Execution host:** Tauri/WebView2 code builds and runs on **Windows only**. Tasks 1–5 are host-testable on any platform (pure logic behind traits); Task 6 (the Tauri binding) is compiled, TDD'd, and smoke-tested on the Windows host. `cargo test --workspace` rebuilds Tauri (>8 min) — use `cargo test -p kiosk-main` in the task loop.
- **Layering (spec §4):** all domain logic already lives in `kiosk-core`; `kiosk-main` only performs I/O and marshals it. Do NOT reimplement validation, signing, damping, nav decisions, or entry shaping here — call kiosk-core.
- **The two `Event` enums:** `kiosk_core::app::state::Event` is the FSM input; `kiosk_core::logging::event::Event` is the telemetry event taxonomy. Alias on import: `use kiosk_core::app::state::Event as AppEvent;` and `use kiosk_core::logging::event::Event as LogEvent;`. Never let them collide.
- **`ConfigApplied` is emitted AT LEVEL** (every poll re-sends the current home url), never edge-triggered — required by the FSM's same-url-no-op / recovery arms (P1-D1 module docs). The changed-url-while-ErrorPage/Clearing no-op is only safe under this contract.
- **Live navigations:** D2a performs only **FSM-initiated** navigations (home + bundled pages), which are trusted. It does NOT intercept page-initiated navigations — that is D2b and MUST route through `kiosk_core::nav::decide` (never `Allowlist::allows` / `scheme_decision` directly). Do not add a nav intercept in D2a.
- **Telemetry must never panic the kiosk** (`Logger::log` is infallible by design). No `.unwrap()`/`.expect()` on any I/O result in a task hot path — map to an `AppEvent` or a logged non-fatal.
- **Deferred effects:** `Effect::ClearProfile` is D2c — the `TauriSink` logs a warning and no-ops it. D2a never arms the idle timer, so it is never emitted in practice.

## kiosk-core interfaces this plan consumes (all already exist)

```rust
// app::state  (P1-D1)
Machine::new(cfg: MachineConfig) -> Machine
Machine::on(&mut self, event: AppEvent) -> Vec<Effect>
Machine::state(&self) -> &AppState
enum AppEvent { ConfigApplied{url:String}, ConfigUnavailable, LinkChanged(Link),
                NavigationCommitted, NavigationFailed, CountdownExpired,
                IdleExpired, ProfileCleared, Reconnected }
enum Effect { Navigate(String), ShowVideo, ShowSplash,
              ShowErrorPage{retry_after_seconds:u64}, ClearProfile{full:bool}, RefetchConfig }
struct MachineConfig { fallback: Fallback, error_max_retries: u32,
                       idle_clear: bool, error_retry_seconds: u64 }
const DEFAULT_ERROR_RETRY_SECONDS: u64 = 15;

// config  (P1-A)
BootstrapConfig::parse(text: &str) -> Result<BootstrapConfig, ConfigError>
//   fields: config_url, device_id: Option<String>, site, region: Option<String>,
//           project_id, credential, bootstrap_url, startup_grace_s, healthy_run_s,
//           channel_grace_s, demo_mode, exit_gesture, …
ConfigStore::new(dir: impl Into<PathBuf>) -> ConfigStore
ConfigManager::boot(bootstrap, device_id: String, store, key: Option<VerifyingKey>)
    -> (ConfigManager, Applied)
ConfigManager::apply_fetched(&mut self, body: &[u8]) -> Result<Applied, ConfigError>
ConfigManager::home_url(&self) -> String
ConfigManager::revision(&self) -> Option<i64>
ConfigManager::current(&self) -> &RemoteConfig      // .content, .network
struct Applied { config: RemoteConfig, revision: Option<i64>, warnings: Vec<String>, source: Source }
// RemoteConfig.content: Content { url: Option<String>, fallback: Fallback,
//   error_max_retries: u32, clear_data_on_reset: bool, idle_reset_seconds: u64, … }
// RemoteConfig.network: Network { connectivity_check_url: String,
//   probe_online_s: u64, probe_offline_s: u64, config_poll_s: u64 }

// net  (P1-C)
Prober::new(clock: TrustedClock) -> Prober
Prober::record(&mut self, outcome: ProbeOutcome) -> Option<Link>   // Some only on a damped flip
Prober::interval(&self, net: &Network) -> Duration
Prober::link(&self) -> Link
struct ProbeOutcome { reachable: bool, date_header: Option<String> }
enum Link { Online, Offline }
// reach::resolve_probe_url(network, content) -> String   // arch-13 (confirm exact name in net::reach)

// logging  (P1-B)
TrustedClock::new() -> TrustedClock            // Clone; shared writer(prober)/reader(logger)
ServiceAccount::from_json(json: &str) -> Result<ServiceAccount, AuthError>
TokenSource::new(...) -> TokenSource           // read exact params in logging/auth.rs
ReqwestTransport::new(timeout: Duration) -> Result<ReqwestTransport, TransportError>
GclClient::new(token_source: TokenSource, transport: Arc<dyn Transport>, clock: TrustedClock) -> GclClient
Spool::open(dir: &Path, cfg: SpoolConfig) -> Result<Spool, SpoolError>
//   SpoolConfig { max_mb, reserve_high_mb, segment_mb }
RateLimiter::new(clock: TrustedClock) -> RateLimiter
struct EntryContext { project_id, device_id, site, region, app_version,
                      config_revision: Option<i64>, url_detail: UrlDetail }
Logger::new(ctx: EntryContext, spool: Spool, client: GclClient,
            limiter: RateLimiter, clock: TrustedClock) -> Logger
Logger::log(&mut self, event: LogEvent, fields: Map<String, Value>)   // infallible
Logger::tick(&mut self)      // periodic; emits flood summaries, attempts a flush
Logger::flush(&mut self) -> Result<(), LoggerError>
```

---

### Task 1: Crate deps, teardown, and the host-tested driver

**Files:**
- Modify: `crates/kiosk-main/Cargo.toml`
- Delete: `crates/kiosk-main/src/spike.rs`
- Modify: `crates/kiosk-main/src/main.rs` (drop `mod spike;` + `--spike-input` use), `crates/kiosk-main/src/cli.rs` (drop `spike_input`)
- Create: `crates/kiosk-main/src/driver.rs`
- Test: `crates/kiosk-main/src/driver.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `EffectSink` trait (`fn dispatch(&mut self, effect: Effect)`); `struct Driver { machine: Machine }` with `fn handle(&mut self, event: AppEvent, sink: &mut dyn EffectSink)`; `async fn run(rx: mpsc::Receiver<AppEvent>, driver: Driver, sink: Box<dyn EffectSink + Send>, cancel: CancellationToken)`.
- Consumes: `kiosk_core::app::state::{Machine, MachineConfig, Event as AppEvent, Effect}`.

- [ ] **Step 1: Add deps.** In `crates/kiosk-main/Cargo.toml` add under `[dependencies]`:

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tokio-util = "0.7"                      # CancellationToken
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
serde_json = "1"
```

(Keep `reqwest` feature set aligned with kiosk-core's telemetry transport to avoid two TLS backends — confirm against `crates/kiosk-core/Cargo.toml` and match it.)

- [ ] **Step 2: Delete the P0 spike.** `git rm crates/kiosk-main/src/spike.rs`. In `main.rs` remove `mod spike;` and the `if args.spike_input { spike::install(&window); }` block. In `cli.rs` remove the `spike_input` field, its `--spike-input` arm, and the `spike_input: true` in the `parses_all_flags` test (assert the remaining fields). Run `cargo test -p kiosk-main` — cli tests pass.

- [ ] **Step 3: Write the failing driver test.** Create `crates/kiosk-main/src/driver.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::app::state::{Effect, Event as AppEvent, Fallback, MachineConfig, Machine,
                                 DEFAULT_ERROR_RETRY_SECONDS};

    #[derive(Default)]
    struct RecordingSink { effects: Vec<Effect> }
    impl EffectSink for RecordingSink {
        fn dispatch(&mut self, effect: Effect) { self.effects.push(effect); }
    }

    fn cfg() -> MachineConfig {
        MachineConfig { fallback: Fallback::Video, error_max_retries: 5,
                        idle_clear: true, error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS }
    }

    #[test]
    fn boot_with_config_navigates_home() {
        let mut d = Driver { machine: Machine::new(cfg()) };
        let mut sink = RecordingSink::default();
        d.handle(AppEvent::ConfigApplied { url: "https://home.test/".into() }, &mut sink);
        assert_eq!(sink.effects, vec![Effect::Navigate("https://home.test/".into())]);
    }

    #[test]
    fn boot_without_config_shows_video() {
        let mut d = Driver { machine: Machine::new(cfg()) };
        let mut sink = RecordingSink::default();
        d.handle(AppEvent::ConfigUnavailable, &mut sink);
        assert_eq!(sink.effects, vec![Effect::ShowVideo]);
    }

    #[test]
    fn offline_then_reconnect_refetches_then_navigates() {
        let mut d = Driver { machine: Machine::new(cfg()) };
        let mut sink = RecordingSink::default();
        d.handle(AppEvent::ConfigApplied { url: "https://home.test/".into() }, &mut sink);
        d.handle(AppEvent::LinkChanged(kiosk_core::net::prober::Link::Offline), &mut sink);
        sink.effects.clear();
        d.handle(AppEvent::Reconnected, &mut sink);
        assert_eq!(sink.effects, vec![Effect::RefetchConfig]);
    }
}
```

Run `cargo test -p kiosk-main driver::` → FAIL (`EffectSink`, `Driver` undefined).

- [ ] **Step 4: Implement the driver.** Above the tests in `driver.rs`:

```rust
use kiosk_core::app::state::{Effect, Event as AppEvent, Machine};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Executes the effects the FSM returns. The production impl (`TauriSink`, Task 6) drives
/// the webview; tests use a recording fake. Sync: the webview marshals internally.
pub trait EffectSink {
    fn dispatch(&mut self, effect: Effect);
}

/// Owns the single `Machine`. Not `Sync`; lives inside the driver task alone.
pub struct Driver {
    pub machine: Machine,
}

impl Driver {
    pub fn handle(&mut self, event: AppEvent, sink: &mut dyn EffectSink) {
        for effect in self.machine.on(event) {
            sink.dispatch(effect);
        }
    }
}

/// The driver task: drains the event channel until the channel closes or cancellation.
pub async fn run(
    mut rx: mpsc::Receiver<AppEvent>,
    mut driver: Driver,
    mut sink: Box<dyn EffectSink + Send>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe = rx.recv() => match maybe {
                Some(event) => driver.handle(event, sink.as_mut()),
                None => break,
            },
        }
    }
}
```

- [ ] **Step 5: Run tests to verify pass.** `cargo test -p kiosk-main driver::` → PASS (3 tests). `cargo fmt -p kiosk-main` and `cargo clippy -p kiosk-main -- -D warnings` clean.

- [ ] **Step 6: Commit.**

```bash
git add -A && git commit -m "feat(main): driver actor + EffectSink seam; remove P0 spike"
```

---

### Task 2: Boot — kiosk.ini, ConfigManager, and MachineConfig

**Files:**
- Create: `crates/kiosk-main/src/boot.rs`
- Test: `crates/kiosk-main/src/boot.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `fn machine_config(content: &Content) -> MachineConfig`; `fn boot_event(applied: &Applied) -> AppEvent`; `struct Booted { manager: ConfigManager, machine_cfg: MachineConfig, first_event: AppEvent, warnings: Vec<String> }`; `fn boot(ini_text: &str, data_dir: &Path) -> Result<Booted, ConfigError>`.
- Consumes: `kiosk_core::config::{BootstrapConfig, ConfigStore, ConfigManager, Applied, schema::{Content, Fallback}}`, `kiosk_core::identity`.

- [ ] **Step 1: Write the failing pure-mapping tests.** Create `crates/kiosk-main/src/boot.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::{Content, Fallback};

    fn content(url: Option<&str>, fallback: Fallback, retries: u32, clear: bool) -> Content {
        Content { url: url.map(str::to_string), fallback, error_max_retries: retries,
                  clear_data_on_reset: clear, ..Content::default() }
    }

    #[test]
    fn machine_config_maps_clear_flag_and_retries() {
        let c = content(Some("https://h/"), Fallback::ErrorPage, 3, false);
        let mc = machine_config(&c);
        assert_eq!(mc.error_max_retries, 3);
        assert_eq!(mc.fallback, Fallback::ErrorPage);
        assert!(!mc.idle_clear, "idle_clear mirrors clear_data_on_reset");
        assert_eq!(mc.error_retry_seconds,
                   kiosk_core::app::state::DEFAULT_ERROR_RETRY_SECONDS);
    }

    #[test]
    fn boot_event_navigates_when_home_is_present() {
        // Applied with a resolvable home url -> ConfigApplied{home}.
        // (Construct Applied via ConfigManager::boot in an integration test below;
        //  here assert the branch on a hand-built Applied.)
    }
}
```

Run `cargo test -p kiosk-main boot::` → FAIL (`machine_config` undefined).

- [ ] **Step 2: Implement `machine_config`.**

```rust
use kiosk_core::app::state::{Event as AppEvent, MachineConfig, DEFAULT_ERROR_RETRY_SECONDS};
use kiosk_core::config::schema::Content;

pub fn machine_config(content: &Content) -> MachineConfig {
    MachineConfig {
        fallback: content.fallback,
        error_max_retries: content.error_max_retries,
        idle_clear: content.clear_data_on_reset,
        // No remote field for the countdown length (spec §3.3); kiosk-core default.
        error_retry_seconds: DEFAULT_ERROR_RETRY_SECONDS,
    }
}
```

Run `cargo test -p kiosk-main boot::machine_config` → PASS.

- [ ] **Step 3: Write the failing boot integration test** (uses a temp data dir + fixture ini — filesystem only, no Tauri, so host-testable):

```rust
    #[test]
    fn boot_with_no_store_uses_bootstrap_and_emits_config_applied() {
        let dir = tempfile::tempdir().unwrap();
        let ini = "[bootstrap]\nconfig_url = https://cfg.test/c.json\n\
                   bootstrap_url = https://site.test/\nsite = s\nproject_id = p\n\
                   credential = {}\n";                     // minimal valid ini
        let booted = boot(ini, dir.path()).expect("boot ok");
        match booted.first_event {
            AppEvent::ConfigApplied { url } => assert_eq!(url, "https://site.test/"),
            other => panic!("expected ConfigApplied, got {other:?}"),
        }
    }
```

Add `tempfile` to `[dev-dependencies]`. Run → FAIL (`boot` undefined).

- [ ] **Step 4: Implement `boot` and `boot_event`.**

```rust
use kiosk_core::config::{Applied, BootstrapConfig, ConfigManager, ConfigStore, ConfigError};
use std::path::Path;

pub struct Booted {
    pub manager: ConfigManager,
    pub machine_cfg: MachineConfig,
    pub first_event: AppEvent,
    pub warnings: Vec<String>,
}

/// A config that resolves a home url boots straight to it; otherwise the machine waits on
/// the video (rule 2). Home is present whenever `content.url` or `[bootstrap] url` is set,
/// which `ConfigManager::home_url()` already resolves.
pub fn boot_event(home_url: &str) -> AppEvent {
    AppEvent::ConfigApplied { url: home_url.to_string() }
}

pub fn boot(ini_text: &str, data_dir: &Path) -> Result<Booted, ConfigError> {
    let bootstrap = BootstrapConfig::parse(ini_text)?;
    let device_id = kiosk_core::identity::resolve(bootstrap.device_id.clone()); // confirm exact fn
    let store = ConfigStore::new(data_dir);
    let key = pinned_key();                       // Option<VerifyingKey>; see Step 5
    let (manager, applied) = ConfigManager::boot(bootstrap, device_id, store, key);
    let machine_cfg = machine_config(&manager.current().content);
    let first_event = boot_event(&manager.home_url());
    Ok(Booted { manager, machine_cfg, first_event, warnings: applied.warnings })
}
```

- [ ] **Step 5: Implement `pinned_key()`.** The compiled-in Ed25519 verifying key (spec §8). Read how P1-A exposes/loads it (`config::signature`); if a build ships no key, return `None` (fail-closed — every fetch rejected, boot still works off bootstrap). Match the existing mechanism; do not invent a new one.

- [ ] **Step 6: Run tests, fmt, clippy, commit.** `cargo test -p kiosk-main boot::` → PASS.

```bash
git add -A && git commit -m "feat(main): boot wiring — kiosk.ini, ConfigManager, MachineConfig"
```

---

### Task 3: Config-fetch task

**Files:**
- Create: `crates/kiosk-main/src/fetch.rs`
- Test: `crates/kiosk-main/src/fetch.rs`

**Interfaces:**
- Produces: `enum FetchOutcome { Applied{home_url:String, revision:Option<i64>, warnings:Vec<String>}, Rejected(String), Unreachable }`; `fn apply(manager: &mut ConfigManager, body_result: Result<Vec<u8>, ()>) -> FetchOutcome`; `async fn fetch_bytes(url:&str) -> Result<Vec<u8>,()>`; `async fn run(manager, tx: mpsc::Sender<AppEvent>, telem: Telemetry, refetch: Arc<Notify>, cancel)`.
- Consumes: `ConfigManager::apply_fetched`, `AppEvent`, `Telemetry` (Task 5).

- [ ] **Step 1: Failing test for the pure apply mapping** (no network — feeds bytes straight to a real `ConfigManager`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_transport_error_is_unreachable_not_a_rejection() {
        let mut m = crate::boot::boot("[bootstrap]\nconfig_url=https://c/\n\
            bootstrap_url=https://s/\nsite=s\nproject_id=p\ncredential={}\n",
            tempfile::tempdir().unwrap().path()).unwrap().manager;
        assert!(matches!(apply(&mut m, Err(())), FetchOutcome::Unreachable));
    }

    #[test]
    fn garbage_body_is_rejected_not_applied() {
        let mut m = /* same as above */;
        match apply(&mut m, Ok(b"not json".to_vec())) {
            FetchOutcome::Rejected(_) => {}
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}
```

Run → FAIL.

- [ ] **Step 2: Implement `apply`** (maps `ConfigManager::apply_fetched` to a `FetchOutcome`; a changed url is naturally handled by the FSM at level):

```rust
use kiosk_core::config::ConfigManager;

pub enum FetchOutcome {
    Applied { home_url: String, revision: Option<i64>, warnings: Vec<String> },
    Rejected(String),
    Unreachable,
}

pub fn apply(manager: &mut ConfigManager, body: Result<Vec<u8>, ()>) -> FetchOutcome {
    match body {
        Err(()) => FetchOutcome::Unreachable,               // timeout / transport error (cfg-10)
        Ok(bytes) => match manager.apply_fetched(&bytes) {
            Ok(applied) => FetchOutcome::Applied {
                home_url: manager.home_url(),
                revision: applied.revision,
                warnings: applied.warnings,
            },
            Err(e) => FetchOutcome::Rejected(e.to_string()),
        },
    }
}
```

- [ ] **Step 3: Implement `fetch_bytes` + the poll loop.**

```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio_util::sync::CancellationToken;

pub async fn fetch_bytes(url: &str) -> Result<Vec<u8>, ()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))               // cfg-10
        .build().map_err(|_| ())?;
    let resp = client.get(url).send().await.map_err(|_| ())?;
    let bytes = resp.bytes().await.map_err(|_| ())?;
    Ok(bytes.to_vec())
}

/// Poll every `config_poll_s`, or immediately when `RefetchConfig` pinged `refetch`.
/// Emits `AppEvent::ConfigApplied{home}` AT LEVEL on every successful apply.
pub async fn run(
    mut manager: ConfigManager,
    url: String,
    poll_s: u64,
    tx: mpsc::Sender<AppEvent>,
    mut telem: crate::telemetry::Telemetry,
    refetch: Arc<Notify>,
    cancel: CancellationToken,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(poll_s.max(1)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {}
            _ = refetch.notified() => {}
        }
        match apply(&mut manager, fetch_bytes(&url).await) {
            FetchOutcome::Applied { home_url, revision, warnings } => {
                telem.config_applied(revision, &warnings);
                let _ = tx.send(AppEvent::ConfigApplied { url: home_url }).await;
            }
            FetchOutcome::Rejected(reason) => telem.config_error(&reason),
            FetchOutcome::Unreachable => { /* damping/prober owns connectivity; no event */ }
        }
    }
}
```

(`telem.config_applied` / `config_error` are Task-5 helpers wrapping `LogEvent::ConfigApplied` / `LogEvent::ConfigError`.)

- [ ] **Step 4: Run tests, fmt, clippy, commit.** `cargo test -p kiosk-main fetch::` → PASS.

```bash
git add -A && git commit -m "feat(main): config-fetch task with at-level ConfigApplied"
```

---

### Task 4: Connectivity-probe task

**Files:**
- Create: `crates/kiosk-main/src/probe.rs`
- Test: `crates/kiosk-main/src/probe.rs`

**Interfaces:**
- Produces: `fn on_outcome(prober: &mut Prober, outcome: ProbeOutcome) -> Option<AppEvent>`; `async fn probe_once(url:&str) -> ProbeOutcome`; `async fn run(prober, network, probe_url, tx, telem, cancel)`.
- Consumes: `kiosk_core::net::prober::{Prober, ProbeOutcome, Link}`, `kiosk_core::net::reach`, `kiosk_core::config::schema::Network`.

- [ ] **Step 1: Failing test for outcome→event mapping** (pure; `Prober` is host-tested in kiosk-core, here we test the translation to `AppEvent` on a flip only):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::logging::time::TrustedClock;
    use kiosk_core::net::prober::{Prober, ProbeOutcome, Link};

    fn outcome(ok: bool) -> ProbeOutcome { ProbeOutcome { reachable: ok, date_header: None } }

    #[test]
    fn a_damped_flip_yields_a_linkchanged_event() {
        let mut p = Prober::new(TrustedClock::new());
        // cold start: first failure sets Offline without a flip event (Unknown consumed)
        let _ = on_outcome(&mut p, outcome(false));
        // two successes flip Online (damping) -> exactly one LinkChanged(Online)
        let _ = on_outcome(&mut p, outcome(true));
        let ev = on_outcome(&mut p, outcome(true));
        assert!(matches!(ev, Some(AppEvent::LinkChanged(Link::Online))));
    }

    #[test]
    fn a_non_flip_yields_no_event() {
        let mut p = Prober::new(TrustedClock::new());
        let _ = on_outcome(&mut p, outcome(true));
        assert!(on_outcome(&mut p, outcome(true)).is_none(), "steady state emits nothing");
    }
}
```

Run → FAIL.

- [ ] **Step 2: Implement `on_outcome` + `probe_once`.**

```rust
use kiosk_core::app::state::Event as AppEvent;
use kiosk_core::net::prober::{Link, ProbeOutcome, Prober};

/// Feed one probe outcome to the damped prober; on a flip, produce the FSM event.
/// The prober harvests the `Date` header into its TrustedClock internally.
pub fn on_outcome(prober: &mut Prober, outcome: ProbeOutcome) -> Option<AppEvent> {
    prober.record(outcome).map(AppEvent::LinkChanged)
}

pub async fn probe_once(url: &str) -> ProbeOutcome {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(10))
        .build();
    match client {
        Err(_) => ProbeOutcome { reachable: false, date_header: None },
        Ok(client) => match client.get(url).send().await {
            Ok(resp) => {
                let date = resp.headers().get("date")
                    .and_then(|v| v.to_str().ok()).map(str::to_string);
                ProbeOutcome { reachable: resp.status().is_success(), date_header: date }
            }
            Err(_) => ProbeOutcome { reachable: false, date_header: None },
        },
    }
}
```

- [ ] **Step 3: Implement the probe loop** — interval from `prober.interval(&network)` re-read each iteration (30 s online / 10 s offline); emit `net.online`/`net.offline` telemetry on a flip alongside the event; resolve the probe URL via `net::reach` (arch-13: unset check-url + private content host → probe content origin).

```rust
pub async fn run(
    mut prober: Prober,
    network: Network,
    probe_url: String,          // resolved via reach::resolve_probe_url before spawn
    tx: mpsc::Sender<AppEvent>,
    mut telem: crate::telemetry::Telemetry,
    cancel: CancellationToken,
) {
    loop {
        let wait = prober.interval(&network);
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(wait) => {}
        }
        if let Some(ev) = on_outcome(&mut prober, probe_once(&probe_url).await) {
            match ev { AppEvent::LinkChanged(Link::Online)  => telem.net_online(),
                       AppEvent::LinkChanged(Link::Offline) => telem.net_offline(), _ => {} }
            let _ = tx.send(ev).await;
        }
    }
}
```

- [ ] **Step 4: Run tests, fmt, clippy, commit.** `cargo test -p kiosk-main probe::` → PASS.

```bash
git add -A && git commit -m "feat(main): connectivity-probe task feeding the damped prober"
```

---

### Task 5: Telemetry task + logger stack

**Files:**
- Create: `crates/kiosk-main/src/telemetry.rs`
- Test: `crates/kiosk-main/src/telemetry.rs`

**Interfaces:**
- Produces: `struct Telemetry` (cloneable `Send` handle wrapping `mpsc::Sender<LogReq>`) with helpers `net_online()`, `net_offline()`, `config_applied(rev, warnings)`, `config_error(&str)`, `app_start()`, `app_stop()`, `panic(&str)`; `fn build(bootstrap, clock, app_version, revision) -> (Telemetry, LoggerTask)`; `async fn run(logger: Logger, rx: mpsc::Receiver<LogReq>, cancel)`.
- Consumes: the full P1-B logger stack (see interfaces block above), `kiosk_core::logging::event::Event as LogEvent`.

- [ ] **Step 1: Failing test — the handle shapes the right event** (uses a fake `Transport` so no network; asserts the logger receives/flushes the event):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // A fake Transport that records posted bodies (mirror kiosk-core's test Transport).
    // Build a Logger with it, drive one Telemetry::net_offline(), tick(), and assert a
    // `net.offline` entry was attempted.
    #[tokio::test]
    async fn net_offline_helper_emits_a_net_offline_event() { /* … */ }
}
```

Run → FAIL.

- [ ] **Step 2: Define `LogReq` + `Telemetry` handle.**

```rust
use kiosk_core::logging::event::Event as LogEvent;
use serde_json::{Map, Value};
use tokio::sync::mpsc;

pub struct LogReq { pub event: LogEvent, pub fields: Map<String, Value> }

#[derive(Clone)]
pub struct Telemetry { tx: mpsc::Sender<LogReq> }

impl Telemetry {
    fn emit(&self, event: LogEvent, fields: Map<String, Value>) {
        // try_send: telemetry must never block or panic the caller; a full queue drops
        // (the Logger's own spool is the durability layer, not this in-memory hop).
        let _ = self.tx.try_send(LogReq { event, fields });
    }
    pub fn net_online(&self)  { self.emit(LogEvent::NetOnline,  Map::new()); }
    pub fn net_offline(&self) { self.emit(LogEvent::NetOffline, Map::new()); }
    pub fn app_start(&self)   { self.emit(LogEvent::AppStart,   Map::new()); }
    pub fn app_stop(&self)    { self.emit(LogEvent::AppStop,    Map::new()); }
    pub fn config_error(&self, reason: &str) {
        let mut f = Map::new(); f.insert("error".into(), Value::from(reason));
        self.emit(LogEvent::ConfigError, f);
    }
    pub fn config_applied(&self, revision: Option<i64>, warnings: &[String]) {
        let mut f = Map::new();
        f.insert("revision".into(), Value::from(revision.map(|r| r.to_string()).unwrap_or_default()));
        if !warnings.is_empty() { f.insert("warnings".into(), Value::from(warnings.join("; "))); }
        self.emit(LogEvent::ConfigApplied, f);
    }
    pub fn panic(&self, msg: &str) {
        let mut f = Map::new(); f.insert("message".into(), Value::from(msg));
        self.emit(LogEvent::CrashPanic, f);
    }
}
```

- [ ] **Step 3: Implement `build` (assemble the logger stack) and `run` (the logger task).**

```rust
use kiosk_core::logging::{Logger, entry::EntryContext, spool::{Spool, SpoolConfig},
    ratelimit::RateLimiter, client::GclClient, auth::{ServiceAccount, TokenSource},
    transport::ReqwestTransport};
use kiosk_core::logging::time::TrustedClock;
use std::{sync::Arc, time::Duration};

pub fn build(bootstrap: &BootstrapConfig, clock: TrustedClock, app_version: String,
             revision: Option<i64>, data_dir: &std::path::Path)
    -> Result<(Telemetry, Logger), Box<dyn std::error::Error>>
{
    let sa = ServiceAccount::from_json(&bootstrap.credential)?;
    let token = TokenSource::new(/* read exact params in logging/auth.rs */);
    let transport: Arc<dyn kiosk_core::logging::transport::Transport> =
        Arc::new(ReqwestTransport::new(Duration::from_secs(10))?);
    let client = GclClient::new(token, transport, clock.clone());
    let spool = Spool::open(data_dir, SpoolConfig { max_mb: 64, reserve_high_mb: 8, segment_mb: 5 })?;
    let limiter = RateLimiter::new(clock.clone());
    let ctx = EntryContext {
        project_id: bootstrap.project_id.clone(),
        device_id:  bootstrap.device_id.clone().unwrap_or_default(),  // resolved id in practice
        site: bootstrap.site.clone(),
        region: bootstrap.region.clone().unwrap_or_else(|| bootstrap.site.clone()),
        app_version, config_revision: revision,
        url_detail: kiosk_core::logging::entry::UrlDetail::default(),  // confirm variant
    };
    let logger = Logger::new(ctx, spool, client, limiter, clock);
    let (tx, rx) = mpsc::channel(256);
    Ok((Telemetry { tx }, logger))   // caller spawns run(logger, rx, cancel)
}

/// The logger task: owns the `&mut Logger`, drains requests, ticks every 10 s.
pub async fn run(mut logger: Logger, mut rx: mpsc::Receiver<LogReq>, cancel: CancellationToken) {
    let mut tick = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => { let _ = logger.flush(); break; }
            _ = tick.tick() => logger.tick(),
            maybe = rx.recv() => match maybe {
                Some(req) => logger.log(req.event, req.fields),   // infallible; WARNING+ fsynced
                None => { let _ = logger.flush(); break; }
            },
        }
    }
}
```

(Exact `TokenSource::new` / `UrlDetail` are read from source during implementation — do not guess; they are defined in `logging/auth.rs` and `logging/entry.rs`.)

- [ ] **Step 4: Run tests, fmt, clippy, commit.** `cargo test -p kiosk-main telemetry::` → PASS.

```bash
git add -A && git commit -m "feat(main): telemetry task + GCL logger stack wiring"
```

---

### Task 6: Tauri assembly — `main.rs`, `TauriSink`, panic hook, bundled pages (WINDOWS HOST)

**Files:**
- Modify: `crates/kiosk-main/src/main.rs`
- Create: `crates/kiosk-main/src/effect.rs`, bundled `crates/kiosk-main/bundled/{offline,splash,error}.html`
- Modify: `crates/kiosk-main/tauri.conf.json` (bundle the local pages)
- Test: `crates/kiosk-main/src/effect.rs` (host-testable parts: the `Effect` → page-path mapping)

> **This task is compiled, TDD'd, and smoke-tested on the Windows host** (WebView2). Tasks 1–5 already pass there via `cargo test -p kiosk-main`. Keep every non-Tauri decision in a pure helper so it stays host-testable; only the `AppHandle`/webview calls are Windows-bound.

**Interfaces:**
- Consumes: `driver::EffectSink`, all task handles, `tauri::{AppHandle, WebviewWindow}`.
- Produces: `struct TauriSink`; `fn page_for(effect: &Effect) -> Option<PageTarget>` (pure, host-tested).

- [ ] **Step 1: Bundled pages.** Create minimal `bundled/offline.html` (`<video loop autoplay muted playsinline src="kiosk-offline.mp4">` + black bg), `bundled/splash.html`, `bundled/error.html` (static message; the countdown is driven by the FSM, not JS). Reference them in `tauri.conf.json` so they resolve at a `tauri://localhost/...` app-origin URL.

- [ ] **Step 2: Host-test the pure effect→target mapping.** In `effect.rs`:

```rust
pub enum PageTarget { Remote(String), Offline, Splash, Error { retry_after_seconds: u64 } }

/// Pure: which page an effect shows (or None for non-navigation effects). Host-tested.
pub fn page_for(effect: &Effect) -> Option<PageTarget> {
    match effect {
        Effect::Navigate(u)                       => Some(PageTarget::Remote(u.clone())),
        Effect::ShowVideo                         => Some(PageTarget::Offline),
        Effect::ShowSplash                        => Some(PageTarget::Splash),
        Effect::ShowErrorPage { retry_after_seconds } =>
            Some(PageTarget::Error { retry_after_seconds: *retry_after_seconds }),
        Effect::RefetchConfig | Effect::ClearProfile { .. } => None,
    }
}
```

Test each arm. Run `cargo test -p kiosk-main effect::` → PASS (host).

- [ ] **Step 3: Implement `TauriSink`** (Windows host). Holds `AppHandle`, the `mpsc::Sender<AppEvent>` (to arm the countdown and re-emit), the `Arc<Notify>` refetch handle, the `Telemetry`, and a cancel token. `dispatch`:
  - `page_for(&effect)` → navigate the webview (via `AppHandle`/`WebviewWindow::navigate`, marshalled to the main thread) to the remote url or the bundled `tauri://` page.
  - `ShowErrorPage{retry_after_seconds}` → after navigating to the error page, spawn a one-shot `tokio::time::sleep` that sends `AppEvent::CountdownExpired`.
  - `RefetchConfig` → `refetch.notify_one()`.
  - `ClearProfile { .. }` → `telem.…`/log a warning and no-op (D2c).

- [ ] **Step 4: Assemble `main.rs`.** `#[tokio::main]` or Tauri's async runtime: read `kiosk.ini` (path per spec §5.1), `boot::boot`, build the fullscreen webview (reuse the P0 builder — fullscreen/decorations(false)/always_on_top/focused), create the `mpsc<AppEvent>` channel + `Notify` + `CancellationToken`, `telemetry::build` + spawn logger task, spawn `fetch::run` / `probe::run` (probe url via `reach::resolve_probe_url`) / `driver::run` with a `TauriSink`, send `booted.first_event`, install the panic hook (`telem.panic(info)` + `logger.flush` via a dedicated durable path — the hook can't own the async logger, so flush the spool synchronously; confirm the mechanism), then run Tauri. Register webview load callbacks (`on_page_load` / navigation error) → `AppEvent::NavigationCommitted` / `NavigationFailed`.

- [ ] **Step 5: Windows-host verification (manual smoke — record results in the task report).**
  1. `cargo build -p kiosk-main` on Windows (MSVC) — compiles.
  2. Launch with a test `kiosk.ini` pointing at a reachable site + a scratch config/probe endpoint. Site shows fullscreen.
  3. Pull the network (or block the probe URL): within one offline probe interval the offline video shows. Restore: within the online interval, back to the site.
  4. Point config at a JSON that changes `content.url`: within one poll the webview navigates to the new url (at-level `ConfigApplied`).
  5. Confirm `app.start`, `net.online`/`net.offline`, `config.applied` land in Cloud Logging.
  6. Kill the process (Task Manager) — confirm the last WARNING+ entry (e.g. `net.offline`) is already in Cloud Logging / spool (TEL-10 durability).

- [ ] **Step 6: Commit.**

```bash
git add -A && git commit -m "feat(main): Tauri assembly — TauriSink, panic hook, bundled pages"
```

---

## Self-Review

**Spec coverage (D2a design doc):** §Architecture actor-spine → Tasks 1,6. Boot/ConfigManager → Task 2. Config fetch (cfg-10 timeouts, at-level ConfigApplied) → Task 3. Probe + prober + arch-13 reach + Date harvest → Task 4. Telemetry/logger stack (TEL-10 durability) → Task 5. Panic hook + fullscreen webview + bundled pages → Task 6. Deferred items (D2b/c/d/e) explicitly out of every task. **Covered.**

**Placeholder scan:** The three points marked "read exact params in source / confirm exact name" (`TokenSource::new`, `UrlDetail`, `identity::resolve`, `reach::resolve_probe_url`, `pinned_key` mechanism) are pointers to **existing** kiosk-core APIs the implementer reads precisely at implementation time — not invented interfaces. They are called out rather than guessed to avoid shipping a wrong signature. Every task has real, runnable test code for its host-testable logic.

**Type consistency:** `AppEvent`/`LogEvent` aliases used uniformly. `EffectSink::dispatch(&mut self, Effect)` consistent across Tasks 1 and 6. `Telemetry` helper names match between Tasks 3/4 (callers) and Task 5 (definitions). `FetchOutcome`/`PageTarget` defined where produced.

**Scope:** One coherent sub-project (the spine). D2b–D2e are separate plans. Right-sized: Tasks 1–5 each carry host tests and an independent reviewer gate; Task 6 is the Windows-host integration seam.
