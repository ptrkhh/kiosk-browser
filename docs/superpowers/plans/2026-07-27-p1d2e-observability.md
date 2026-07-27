# P1-D2e — App-Layer Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** T1 (kiosk-core metrics) + T5a (the pure `is_remote_origin` change) are host-testable (`cargo test -p kiosk-core` / `-p kiosk-main` on the x64 cross-toolchain). T2–T4, T5b touch Tauri/WebView2/native — Windows-host build + smoke.

**Goal:** Close the app-layer observability gaps after D2a–D2c: a periodic `health.sample`, the `display.monitor` out-of-range fallback, a crash panic file for the launcher, and two D2c-smoke follow-ups (ipc.localhost egress false-block; silent hardening degrade → telemetry).

**Architecture:** `sysinfo` sampling lives in a pure-ish kiosk-core `metrics` module (host-tested). The health timer, display enumeration, panic-file, and hardening-degrade telemetry are kiosk-main, reusing the D2a `Telemetry` handle and D2b `hardening.rs`.

**Tech Stack:** Rust, `sysinfo` (new, kiosk-core), Tauri 2.11.x, `kiosk-core` (`Logger`, `LogEvent`).

**Design spec:** `docs/superpowers/specs/2026-07-27-p1d2e-observability-design.md`.

## Global Constraints

- **health.sample is BASIC (P1).** Sample CPU %, mem used/total, disk-free (data-dir volume), uptime, `spool.dropped_expired`. Webview-process **RSS** and `max_webview_mem_mb` enforcement are **P2** (roadmap §9) — do NOT implement them.
- **Telemetry `try_send`, never panics** (D2a `Telemetry`). New helpers follow the existing pattern in `telemetry.rs`.
- **Layering:** `sysinfo` is cross-platform (sanctioned in kiosk-core by spec §4 `metrics/`); no per-OS API in kiosk-core. Display enumeration, the panic file, and COM degrade telemetry are kiosk-main.
- **Reuse existing events** (`LogEvent::{HealthSample, ConfigWarn}` already exist, P1-B). Do not add taxonomy entries.
- **`ipc.localhost` is app-origin.** The fix goes in the single shared classifier `nav_policy::is_remote_origin` so the nav guard and egress filter agree by construction.
- **Panic-file write is best-effort** — no allocation-heavy or re-entrant work in the hook; a failure must never turn one panic into two.

## Interfaces this plan uses (merged)

```rust
// kiosk-core::logging::Logger
pub fn dropped_expired(&self) -> u64                       // spool.dropped_expired (mod.rs:666)
// kiosk-core::logging::event::Event (LogEvent): HealthSample (INFO), ConfigWarn (WARNING) exist.
// kiosk-main/src/telemetry.rs — Telemetry (Clone, try_send): net_online/app_start/config_applied/
//   config_error/nav_blocked/nav_error/focus_lost/webview_crash/panic(msg). ADD: health, config_warn.
// kiosk-main/src/nav_policy.rs:233
pub fn is_remote_origin(url: &str) -> bool {   // host != tauri.localhost && != kioskasset.localhost
    // T5a adds: && != "ipc.localhost"
}
// kiosk-main/src/main.rs
fn resolve_data_dir() -> PathBuf                          // %ProgramData%\kiosk
fn install_panic_hook(telem: telemetry::Telemetry)        // T4 adds a data_dir param
// kiosk-main/src/hardening.rs — apply(...) with the Settings4/5-missing eprintln branch (~L119)
// config: RemoteConfig.logging.health_sample_s: u64 (default 60), .display.monitor: u32
```

---

### Task 1: kiosk-core `metrics` module (host-tested)

**Files:**
- Modify: `crates/kiosk-core/Cargo.toml` (add `sysinfo = "0.32"`)
- Create: `crates/kiosk-core/src/metrics.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (`pub mod metrics;`)
- Test: `crates/kiosk-core/src/metrics.rs`

**Interfaces:**
- Produces: `struct HealthSample { cpu_percent: f32, mem_used_mb: u64, mem_total_mb: u64, disk_free_mb: u64, uptime_secs: u64 }`; `fn sample(sys: &mut sysinfo::System, disks: &mut sysinfo::Disks, data_dir: &Path, started: Instant) -> HealthSample`; `fn to_fields(s: &HealthSample, dropped_expired: u64) -> serde_json::Map<String, Value>`.

- [ ] **Step 1: dep + module.** `sysinfo = "0.32"` under `[dependencies]`; `pub mod metrics;` in `lib.rs`.

- [ ] **Step 2: failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn sample_reports_plausible_memory_and_disk() {
        let mut sys = sysinfo::System::new();
        let mut disks = sysinfo::Disks::new_with_refreshed_list();
        let s = sample(&mut sys, &mut disks, std::path::Path::new("."), Instant::now());
        assert!(s.mem_total_mb > 0, "total memory must be readable");
        assert!(s.mem_used_mb <= s.mem_total_mb, "used <= total");
        // disk_free_mb may be 0 on an exotic mount, but the field must be present (no panic).
    }

    #[test]
    fn to_fields_has_the_enumerated_keys_plus_dropped_expired() {
        let s = HealthSample { cpu_percent: 1.0, mem_used_mb: 100, mem_total_mb: 200,
                               disk_free_mb: 50, uptime_secs: 10 };
        let f = to_fields(&s, 7);
        for k in ["cpu_percent","mem_used_mb","mem_total_mb","disk_free_mb","uptime_secs",
                  "spool_dropped_expired"] {
            assert!(f.contains_key(k), "missing {k}");
        }
        assert_eq!(f["spool_dropped_expired"], serde_json::json!(7));
    }
}
```

Run `cargo test -p kiosk-core metrics::` → FAIL.

- [ ] **Step 3: implement.**

```rust
use serde_json::{Map, Value};
use std::path::Path;
use std::time::Instant;
use sysinfo::{Disks, System};

pub struct HealthSample {
    pub cpu_percent: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_free_mb: u64,
    pub uptime_secs: u64,
}

const MB: u64 = 1_048_576;

/// Sample host health. `sys`/`disks` are held across ticks by the caller so CPU % is a real
/// delta between refreshes (the first sample right after boot reads ~0 — acceptable for a
/// 60 s heartbeat). ponytail: no persistent averaging; the raw instantaneous reading is
/// enough signal for fleet dashboards.
pub fn sample(sys: &mut System, disks: &mut Disks, data_dir: &Path, started: Instant) -> HealthSample {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    disks.refresh();
    // Free space on the disk whose mount point is the longest prefix of data_dir.
    let disk_free = disks.list().iter()
        .filter(|d| data_dir.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(0);
    HealthSample {
        cpu_percent: sys.global_cpu_usage(),
        mem_used_mb: sys.used_memory() / MB,
        mem_total_mb: sys.total_memory() / MB,
        disk_free_mb: disk_free / MB,
        uptime_secs: started.elapsed().as_secs(),
    }
}

/// The enumerated `health.sample` jsonPayload (spec §6 — no free-form content).
pub fn to_fields(s: &HealthSample, dropped_expired: u64) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("cpu_percent".into(), Value::from(s.cpu_percent));
    m.insert("mem_used_mb".into(), Value::from(s.mem_used_mb));
    m.insert("mem_total_mb".into(), Value::from(s.mem_total_mb));
    m.insert("disk_free_mb".into(), Value::from(s.disk_free_mb));
    m.insert("uptime_secs".into(), Value::from(s.uptime_secs));
    m.insert("spool_dropped_expired".into(), Value::from(dropped_expired));
    m
}
```

(Confirm the exact `sysinfo` 0.32 method names — `global_cpu_usage`, `used_memory` [bytes], `Disks::list`/`refresh` — against the crate; adjust if the minor version differs.)

Run `cargo test -p kiosk-core metrics::` → PASS.

- [ ] **Step 4: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): metrics module — sysinfo health sample (basic; RSS is P2)"
```

---

### Task 2: health-sample timer (kiosk-main)

**Files:**
- Create: `crates/kiosk-main/src/health.rs`; modify `main.rs` (spawn), `telemetry.rs` (add `health`)

**Interfaces:**
- Produces: `async fn run(sys: System, disks: Disks, data_dir: PathBuf, started: Instant, period_s: u64, dropped: impl Fn() -> u64, telem: Telemetry, cancel: CancellationToken)`; `Telemetry::health(&self, fields: Map<String,Value>)`.

- [ ] **Step 1: add `Telemetry::health`.** In `telemetry.rs`, following the existing helpers:

```rust
pub fn health(&self, fields: serde_json::Map<String, serde_json::Value>) {
    self.emit(LogEvent::HealthSample, fields);
}
```

- [ ] **Step 2: the timer task.** In `health.rs`:

```rust
use std::time::{Duration, Instant};
use sysinfo::{Disks, System};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use crate::telemetry::Telemetry;

/// Emit a `health.sample` every `period_s` (spec §6; [10,3600], default 60). `dropped`
/// reads `Logger::dropped_expired()` at sample time. Cancel-aware.
pub async fn run(mut sys: System, mut disks: Disks, data_dir: std::path::PathBuf,
                 started: Instant, period_s: u64,
                 dropped: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
                 telem: Telemetry, cancel: CancellationToken) {
    let mut tick = interval(Duration::from_secs(period_s.clamp(10, 3600)));
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tick.tick() => {
                let s = kiosk_core::metrics::sample(&mut sys, &mut disks, &data_dir, started);
                telem.health(kiosk_core::metrics::to_fields(&s, dropped()));
            }
        }
    }
}
```

(`dropped` is a closure over the logger handle — the logger owns `&mut Logger` on its thread, so expose `dropped_expired()` through a small `Arc`-shared atomic the logger updates, OR read it via an existing shared accessor. Confirm how the D2a logger surfaces counters; if none, the simplest is the logger task periodically publishing `dropped_expired()` into an `Arc<AtomicU64>` the health task reads. Do NOT reach into `&mut Logger` from another thread.)

- [ ] **Step 3: spawn in `main.rs`** with `logging.health_sample_s`, a fresh `System::new()`/`Disks::new_with_refreshed_list()`, `resolve_data_dir()`, the process-start `Instant`, and the shared `cancel`.

- [ ] **Step 4: Windows smoke.** Set `health_sample_s:15`; confirm a `health.sample{cpu_percent,mem_used_mb,disk_free_mb,uptime_secs,spool_dropped_expired}` lands in Cloud Logging every ~15 s.

- [ ] **Step 5: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): periodic health.sample telemetry timer"
```

---

### Task 3: display.monitor out-of-range → primary + config.warn (kiosk-main)

**Files:**
- Modify: `crates/kiosk-main/src/main.rs` (post-window-build), `telemetry.rs` (add `config_warn`)

**Interfaces:**
- Produces: `Telemetry::config_warn(&self, field: &str, reason: &str)`; a startup check using `window.available_monitors()` / `window.primary_monitor()`.

- [ ] **Step 1: add `Telemetry::config_warn`.**

```rust
pub fn config_warn(&self, field: &str, reason: &str) {
    let mut f = serde_json::Map::new();
    f.insert("field".into(), serde_json::Value::from(field));
    f.insert("reason".into(), serde_json::Value::from(reason));
    self.emit(LogEvent::ConfigWarn, f);
}
```

- [ ] **Step 2: the display check.** After the window is built in `main.rs`, read `display.monitor` (u32) from the booted config. `let monitors = window.available_monitors()?;` If `monitor as usize >= monitors.len()` → position the window on `window.primary_monitor()?` and `telem.config_warn("display.monitor", "index beyond available displays; using primary")` (spec §5.2). Else set the window position/monitor to `monitors[monitor as usize]`. (Confirm the Tauri 2 API for placing a window on a specific monitor — `set_position` with the monitor's `position()`, or the monitor-targeting builder option.)

- [ ] **Step 3: Windows smoke.** On a single-monitor box set `display.monitor:5` → the window opens on primary and `config.warn{field:"display.monitor"}` appears in Cloud Logging; `monitor:0` → no warning.

- [ ] **Step 4: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): display.monitor out-of-range falls back to primary + config.warn"
```

---

### Task 4: crash panic file (kiosk-main)

**Files:**
- Modify: `crates/kiosk-main/src/main.rs` (`install_panic_hook`)

**Interfaces:**
- Consumes: `resolve_data_dir()`; `install_panic_hook(telem, data_dir: PathBuf)`.

- [ ] **Step 1: write the panic file in the hook.** Add a `data_dir: PathBuf` param to `install_panic_hook` (pass `data_dir.clone()` at the call site). In the hook closure, after `telem.panic(...)`:

```rust
// Best-effort durable breadcrumb for the launcher (P1-E) to attach to watchdog.restart.
// No allocation-heavy / re-entrant work — a panic in the hook must not cascade.
let path = data_dir.join("crash-panic.txt");
if let Ok(mut f) = std::fs::File::create(&path) {
    use std::io::Write;
    let _ = writeln!(f, "{info}");     // message + location (Display of PanicHookInfo)
    let _ = f.sync_all();              // fsync the file
}
```

The `crash.panic` CRITICAL event itself is already durable via `telem.panic` → the logger's TEL-10 write-through fsync (D2a/P1-B) — no extra spool fsync needed here; the file is the new artifact.

- [ ] **Step 2: Windows smoke.** Trigger a panic (a dev-only `--panic-test` flag or a forced unwrap in a scratch build). Confirm `<data_dir>\crash-panic.txt` exists with the panic message + location after the process dies, and `crash.panic` reached Cloud Logging (or the spool, drained next boot).

- [ ] **Step 3: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): write crash-panic.txt in the panic hook for the launcher"
```

---

### Task 5: D2c-smoke follow-ups — ipc.localhost egress + hardening degrade telemetry

**Files:**
- Modify: `crates/kiosk-main/src/nav_policy.rs` (`is_remote_origin` + test), `crates/kiosk-main/src/hardening.rs` (degrade → `config_warn`; pass `Telemetry`)

**Interfaces:**
- Consumes: `Telemetry::config_warn` (Task 3).

- [ ] **Step 1 (host-testable): fix `is_remote_origin`.** In `nav_policy.rs:238`, add `ipc.localhost` to the app-origin set:

```rust
Some(host) => host != "tauri.localhost" && host != "kioskasset.localhost"
           && host != "ipc.localhost",
```

Add a test beside the existing app-origin test (~L294):

```rust
#[test]
fn ipc_origin_is_app_origin_not_remote_egress() {
    // Tauri's own IPC custom-protocol origin (Windows) must not be classed as remote
    // content — the D2c smoke saw it false-reported as nav.blocked{egress}.
    assert!(!is_remote_origin("http://ipc.localhost/"));
    assert!(!is_remote_origin("http://ipc.localhost/anything"));
}
```

Run `cargo test -p kiosk-main nav_policy::` → PASS (and the existing `resource_allowed` reuse means the egress filter now admits it too — verify a `resource_allowed("http://ipc.localhost/…")` assertion if `resource_allowed` doesn't already route through `is_remote_origin`).

- [ ] **Step 2 (Windows): hardening degrade → telemetry.** Thread the `Telemetry` handle into `hardening::apply(...)`. At the `Settings4`/`Settings5`-missing branch (~L119) that currently only `eprintln`s, also emit `telem.config_warn("hardening.autofill", "Settings4 unavailable; autofill/password-save stay on")` and `telem.config_warn("hardening.pinch_zoom", "Settings5 unavailable; pinch zoom stays on")` respectively — so a stale-runtime device is visible in the fleet, not silently under-hardened. Keep the `eprintln` for local dev.

- [ ] **Step 3: Windows smoke.** On the stale-runtime dev host (the one from the D2c smoke) confirm the two `config.warn{field:"hardening.*"}` events appear at boot; on an evergreen-runtime host confirm they do NOT (Settings4/5 present → hardened, no warning).

- [ ] **Step 4: fmt, clippy, commit.**

```bash
git add -A && git commit -m "fix(main): ipc.localhost is app-origin (egress); surface hardening degrade to telemetry"
```

---

## Self-Review

**Spec coverage (design doc):** basic health.sample → T1+T2; display.monitor fallback → T3; panic file → T4; ipc.localhost egress fix → T5 Step 1; hardening degrade telemetry → T5 Step 2. RSS/mem-cap deferred to P2 (constraint stated). **Covered.**

**Placeholder scan:** the sysinfo/Tauri method names carry "confirm against the crate/version" notes (resolved at impl time as prior plans did) — not invented APIs. T1 + T5a have runnable test code; the Windows tasks carry smoke checklists. The one soft spot — how the health task reads `dropped_expired()` without touching `&mut Logger` cross-thread — is called out explicitly in T2 Step 2 with the `Arc<AtomicU64>` resolution, not hand-waved.

**Type consistency:** `HealthSample`/`sample`/`to_fields` defined T1, used T2. `Telemetry::health` T2, `Telemetry::config_warn` T3 (reused T5). `is_remote_origin` T5 matches the merged signature. `LogEvent::{HealthSample,ConfigWarn}` are existing P1-B variants.

**Scope:** One sub-project (observability). Five small tasks; T1 + T5a host-tested, T2–T4 + T5b Windows with smoke. Each an independent reviewer gate.
