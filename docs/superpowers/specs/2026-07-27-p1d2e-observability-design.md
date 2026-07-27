# P1-D2e — App-Layer Observability: health.sample, display check, panic file (Design)

> Sub-project of P1-D2 (the `kiosk-main` Tauri app). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §6, §5.2, §4.
> Builds on P1-B (the `Logger`, `LogEvent::{HealthSample,CrashPanic}` already exist) and
> P1-D2a (the panic hook + `Telemetry` handle) / P1-D2b (`hardening.rs`).

**Status:** approved 2026-07-27 (design). kiosk-core `metrics` is host-testable; the timer,
display enumeration, panic-file, and hardening-degrade telemetry are Windows-host.

## Goal

Close the app-layer observability gaps left after D2a–D2c: a periodic `health.sample`
telemetry heartbeat, the `display.monitor` out-of-range fallback (a P1-A carry-forward the
pure layer cannot do), a crash **panic file** for the launcher (P1-E) to attach, and — folded
in from the D2c smoke — turning a silently-degraded hardening interface into a visible
telemetry WARNING.

## Scope decisions

- **health.sample is BASIC in P1.** Roadmap §9 puts webview-process **RSS** and the
  **memory-cap restart** in P2 ("memory cap restart + health-sampled RSS"). D2e therefore
  samples CPU %, used/total memory, free disk (data-dir volume), process uptime, and
  `spool.dropped_expired` — NOT webview RSS, and it does not enforce `max_webview_mem_mb`.
  Those are P2.
- **Panic file, not panic richness.** D2a already emits `crash.panic` via the panic hook.
  D2e adds the durable **panic file** (spec §6: "writes panic file, fsync spool") the P1-E
  launcher attaches to the next `watchdog.restart`. The watchdog's *consumption* of the file
  is P1-E; D2e only writes it.
- **Hardening-degrade telemetry** (from the D2c smoke): when a WebView2 hardening interface
  is missing (`Settings4`/`Settings5` → `E_NOINTERFACE` on an old runtime, so autofill/pinch
  stay ON — an M5 gap), emit a WARNING to telemetry instead of an `eprintln` only. The root
  fix (evergreen runtime + a version floor) is P1-F; this makes a bad-runtime device visible
  in the field rather than silently under-hardened.

## Components

### kiosk-core `metrics` module (pure-ish, host-tested)
Spec §4 places `metrics/ health sampling (sysinfo)` in kiosk-core; `sysinfo` is a
cross-platform library (not a per-OS API), so it does not break the layering rule and runs
on the Linux test host.

```rust
// crates/kiosk-core/src/metrics.rs
pub struct HealthSample {
    pub cpu_percent: f32,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_free_mb: u64,      // free space on the volume holding `data_dir`
    pub uptime_secs: u64,       // this process's uptime
}
pub fn sample(data_dir: &std::path::Path, started: std::time::Instant) -> HealthSample;
pub fn to_fields(s: &HealthSample, dropped_expired: u64) -> Map<String, Value>;  // health.sample jsonPayload
```

`sample` reads a `sysinfo::System` (refreshed for CPU + memory), the disk with the longest
mount-point prefix of `data_dir` for free space, and `now - started` for uptime. `to_fields`
shapes the enumerated jsonPayload allow-list (spec §6 — no free-form content), adding
`spool.dropped_expired` (supplied by the caller from `Logger::dropped_expired()`).

### kiosk-main: health-sample timer
A `tokio::time::interval(health_sample_s)` task (default 60 s; range [10, 3600]) → `metrics::sample`
→ `Telemetry` emits `LogEvent::HealthSample` with `to_fields(sample, logger.dropped_expired())`.
Cancel-aware. `health_sample_s` from `logging.health_sample_s`.

### kiosk-main: display.monitor check (P1-A carry-forward)
At startup, after building the window: read `display.monitor` (index); enumerate
`window.available_monitors()`. If the index ≥ count → move the window to
`primary_monitor()` and emit `config.warn{field:"display.monitor", …}` (spec §5.2: "index
beyond available displays ⇒ fall back to primary + WARNING"). In-range → position on that
monitor. This is the one piece kiosk-core explicitly could not do (no display enumeration,
layering rule).

### kiosk-main: panic file
Extend the D2a panic hook: before it returns, write a panic file to
`<data_dir>/crash-panic.txt` (panic message + location + a timestamp) and fsync it, then
fsync the spool. The write must be panic-hook-safe (no allocation-heavy or re-entrant work;
a best-effort `File::create`+`write_all`+`sync_all`, ignoring errors). The launcher (P1-E)
reads and attaches it to the next `watchdog.restart`, then deletes it. D2e writes; P1-E
consumes.

### kiosk-main: hardening-degrade telemetry (D2c smoke follow-up)
In `hardening.rs`, where a missing `Settings4`/`Settings5` (or any hardening interface) is
currently `eprintln`'d and skipped, also emit a `config.warn` (or reuse an existing WARNING
event) with the specific control left un-hardened (e.g. `autofill_stays_on`,
`pinch_zoom_stays_on`). `try_send`, never panics. Does not change behaviour — just makes the
silent downgrade visible.

### kiosk-main: egress allow the IPC origin (D2c smoke follow-up)
The D2c smoke found the egress guard (D2b) reporting
`nav.blocked{reason:egress, url:"http://ipc.localhost"}` — Tauri's own IPC custom-protocol
origin on Windows. Nothing user-visible broke (IPC still works), but the app's internal
origin must not be classified as remote/off-list egress. Fix in the single shared classifier
`nav_policy::is_remote_origin` (used by both the nav-guard FSM feed and the egress
`resource_allowed`): add `ipc.localhost` to the app-origin set alongside `tauri.localhost`
and `kioskasset.localhost`. One place, so the nav guard and egress filter agree by
construction; a host-test pins that `http://ipc.localhost/...` is app-origin (not remote).

## Data flow

- **health:** every `health_sample_s` → sample → `HealthSample` INFO entry (batched by the
  Logger like any other event).
- **display:** startup one-shot; out-of-range → primary + WARNING.
- **panic:** panic → hook writes `crash-panic.txt` + fsyncs → process aborts → P1-E attaches.
- **hardening degrade:** startup (in `hardening::apply`) → per missing interface, one WARNING.

## Error handling

- Sampling failure (sysinfo hiccup) → skip that sample (a missing heartbeat is benign);
  never panic the timer.
- Disk/monitor enumeration failure → log + use a safe default (primary monitor; omit
  disk_free if unreadable). `Telemetry` is `try_send`, never blocks/panics.
- The panic-file write is best-effort — a failure there must not turn one panic into two.

## Testing

- **Host-testable (kiosk-core):** `metrics::sample` returns plausible non-zero
  memory/uptime and a `disk_free_mb` for the data-dir volume on the Linux test host;
  `to_fields` includes every enumerated key + `dropped_expired` and no free-form content.
- **Windows-host:** the health timer emits `health.sample` at the configured cadence
  (visible in Cloud Logging); `display.monitor` out-of-range lands on primary + `config.warn`;
  a forced panic leaves a `crash-panic.txt` with the message + fsynced spool; a stale-runtime
  host emits the hardening-degrade WARNING (the D2c smoke's Settings4/5 case).

## Scope / defer

D2e = these four app-layer gaps, Windows/P1. Deferred: webview-process RSS + `max_webview_mem_mb`
enforcement (P2); the launcher's consumption of the panic file (P1-E); the WebView2 evergreen
runtime bootstrap + version floor that *fixes* the hardening degrade (P1-F). Linux/Android
health/display are P2/P3.
