# P1-E1 — Watchdog Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Fully host-testable** — pure kiosk-core, no process/pipe/clock. `cargo test -p kiosk-core` runs on any host (the controller can verify these directly).

**Goal:** The pure core of the launcher watchdog — the `ipc` heartbeat frame protocol and the `watchdog` supervise state machine — as host-testable kiosk-core modules, so every §3.1 rule is pinned by adversarial unit tests before the Windows launcher shell (P1-E2) wires it up.

**Architecture:** `ipc` = newline-JSON `Frame`s (`Ready`/`Ping`). `watchdog` = a pure Mealy machine `Watchdog::on(&mut self, Event) -> Vec<Action>` with time injected via events (`Tick{now}`, `Spawned{at}`, `ChildExited{at}`). No `std::process`, no pipes, no `Instant::now`.

**Tech Stack:** Rust, serde/serde_json (already kiosk-core deps), thiserror.

**Design spec:** `docs/superpowers/specs/2026-07-28-p1e1-watchdog-core-design.md`. Rule authority: parent spec §3.1.

## Global Constraints

- **Pure kiosk-core.** No `std::process`, named pipes/sockets, or `Instant::now`/`SystemTime::now` inside the logic — time is an event field. Spawning, pipes, the real clock, and executing `Action`s are P1-E2 (Windows).
- **Total + panic-free.** Every `(state, event)` has a defined outcome (unhandled = no-op). Counters are `saturating_*`. `ipc::decode` returns `Err` on garbage, never panics (the pipe carries untrusted-ish bytes).
- **Defaults from §3.1** (verbatim): `startup_grace_s`=90, `healthy_run_s`=120, `channel_grace_s`=30; `PING_INTERVAL_S`=5; miss threshold = 3 intervals (15 s); backoff 1→2→4→…→60 s; crash-loop = **>5 restarts in a sliding 10-min (600 s) window**; safe-mode escalation N=3 consecutive `--safe` fails within `healthy_run_s`; **exit code 86** = intentional, never restart.
- **Reuse `LogEvent::Watchdog*`** (P1-B: `WatchdogArm/Restart/Hang/ChannelReset/SafeMode/SafeModeFailed` already exist). `Action::Log(WatchdogEvent)` carries the fields; do NOT add taxonomy entries.

---

### Task 1: `ipc` heartbeat frame protocol

**Files:**
- Create: `crates/kiosk-core/src/ipc.rs`; modify `lib.rs` (`pub mod ipc;`)
- Test: `crates/kiosk-core/src/ipc.rs`

**Interfaces:**
- Produces: `enum Frame { Ready, Ping }`; `fn encode(&Frame) -> String`; `fn decode(&str) -> Result<Frame, IpcError>`; `const PING_INTERVAL_S: u64 = 5`.

- [ ] **Step 1: failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn roundtrips_every_frame() {
        for f in [Frame::Ready, Frame::Ping] {
            assert_eq!(decode(encode(&f).trim()).unwrap(), f);
        }
    }
    #[test] fn encode_is_one_newline_terminated_line() {
        let s = encode(&Frame::Ping);
        assert!(s.ends_with('\n'));
        assert_eq!(s.matches('\n').count(), 1);
    }
    #[test] fn garbage_is_err_not_panic() {
        assert!(decode("not json").is_err());
        assert!(decode("").is_err());
        assert!(decode("{\"type\":\"unknown\"}").is_err(), "forward-compat: unknown frame ignored, not a crash");
    }
}
```

Run `cargo test -p kiosk-core ipc::` → FAIL.

- [ ] **Step 2: implement.**

```rust
use serde::{Deserialize, Serialize};

pub const PING_INTERVAL_S: u64 = 5;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Ready,   // main → launcher: webview up + first nav committed (arch-03)
    Ping,    // main → launcher: liveness, every PING_INTERVAL_S
}

#[derive(Debug, thiserror::Error)]
#[error("bad heartbeat frame: {0}")]
pub struct IpcError(String);

/// One '\n'-terminated JSON line.
pub fn encode(frame: &Frame) -> String {
    let mut s = serde_json::to_string(frame).expect("Frame is always serializable");
    s.push('\n');
    s
}

/// One line → Frame. Malformed / unknown-type → Err (never panics; the launcher must
/// survive garbage or a newer main's P2 frame on the pipe).
pub fn decode(line: &str) -> Result<Frame, IpcError> {
    serde_json::from_str(line.trim()).map_err(|e| IpcError(e.to_string()))
}
```

Run → PASS.

- [ ] **Step 3: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): ipc heartbeat frame protocol (Ready/Ping, newline-JSON)"
```

---

### Task 2: `watchdog` FSM — types, READY arming, heartbeat/miss/disambiguation, exit-86

**Files:**
- Create: `crates/kiosk-core/src/watchdog.rs`; modify `lib.rs` (`pub mod watchdog;`)
- Test: `crates/kiosk-core/src/watchdog.rs`

**Interfaces:**
- Produces: `WatchdogConfig`, `Event`, `Action`, `WatchdogEvent`, `Watchdog::new(cfg) -> (Watchdog, Vec<Action>)`, `Watchdog::on(&mut self, Event) -> Vec<Action>`. (Rules 4–8 land in Tasks 3–4; declare the state fields they need now so the machine is additive.)

- [ ] **Step 1: types.** In `watchdog.rs`:

```rust
use crate::logging::event::Event as LogEvent;

pub struct WatchdogConfig { pub startup_grace_s: u64, pub healthy_run_s: u64, pub channel_grace_s: u64 }

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Spawned { at: u64 },
    Ready,
    Heartbeat { at: u64 },
    ChildExited { code: i32, at: u64 },
    Tick { now: u64 },
    ChannelFault { at: u64 },
    ChannelReconnected,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    SpawnMain,
    SpawnSafe,
    DrainOrphanedSpool,
    Log(WatchdogEvent),
    ExitLauncher { code: i32 },
}

/// The structured payload for a `watchdog.*` telemetry entry. `event()` maps to the
/// existing P1-B `LogEvent`; the shell (E2) turns `fields` into the jsonPayload.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchdogEvent {
    Arm { time_to_ready_s: u64 },
    Restart { code: i32, backoff_s: u64, cause: &'static str },
    Hang,
    ChannelReset,
    SafeMode,
    SafeModeFailed,
}
impl WatchdogEvent {
    pub fn log_event(&self) -> LogEvent {
        match self {
            WatchdogEvent::Arm { .. } => LogEvent::WatchdogArm,
            WatchdogEvent::Restart { .. } => LogEvent::WatchdogRestart,
            WatchdogEvent::Hang => LogEvent::WatchdogHang,
            WatchdogEvent::ChannelReset => LogEvent::WatchdogChannelReset,
            WatchdogEvent::SafeMode => LogEvent::WatchdogSafeMode,
            WatchdogEvent::SafeModeFailed => LogEvent::WatchdogSafeModeFailed,
        }
    }
}

const MISS_LIMIT_S: u64 = 15;          // 3 × PING_INTERVAL_S
```

- [ ] **Step 2: state struct + `new`.**

```rust
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    AwaitingSpawn,                       // new() asked for a spawn; waiting for Spawned
    Spawning { grace_until: u64 },       // spawned, waiting READY
    Armed,                               // enforcing heartbeat
    BackingOff { until: u64 },           // waiting to (re)spawn
}

pub struct Watchdog {
    cfg: WatchdogConfig,
    phase: Phase,
    safe: bool,                          // running --safe
    spawned_at: u64,
    last_heartbeat: u64,
    backoff_s: u64,                      // current backoff (Task 3)
    restarts: Vec<u64>,                  // restart timestamps, sliding window (Task 4)
    safe_fails: u32,                     // consecutive --safe fails within healthy_run_s (Task 4)
    channel_grace_until: Option<u64>,    // set on ChannelFault
}

impl Watchdog {
    pub fn new(cfg: WatchdogConfig) -> (Watchdog, Vec<Action>) {
        let w = Watchdog { cfg, phase: Phase::AwaitingSpawn, safe: false, spawned_at: 0,
                           last_heartbeat: 0, backoff_s: 1, restarts: Vec::new(),
                           safe_fails: 0, channel_grace_until: None };
        (w, vec![Action::SpawnMain])
    }
    pub fn on(&mut self, event: Event) -> Vec<Action> { /* Steps 4+, Tasks 3–4 */ }
}
```

- [ ] **Step 3: failing tests for arming, miss/disambiguation, exit-86.**

```rust
fn cfg() -> WatchdogConfig { WatchdogConfig { startup_grace_s: 90, healthy_run_s: 120, channel_grace_s: 30 } }

#[test]
fn ready_within_grace_arms_and_logs_time_to_ready() {
    let (mut w, boot) = Watchdog::new(cfg());
    assert_eq!(boot, vec![Action::SpawnMain]);
    w.on(Event::Spawned { at: 100 });
    let fx = w.on(Event::Ready);           // READY at… the FSM learns "now" from the next Tick; Ready carries no time
    // arm is logged; time_to_ready measured from spawned_at to the arming tick:
    assert!(fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::Arm { .. }))));
}

#[test]
fn miss_with_child_exited_restarts_with_real_code() {
    let (mut w, _) = Watchdog::new(cfg());
    w.on(Event::Spawned { at: 0 });
    w.on(Event::Ready);
    // child crashed at t=10; the FSM hears the real exit first
    let fx = w.on(Event::ChildExited { code: 1, at: 10 });
    assert!(fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::Restart { code: 1, cause: "exit", .. }))));
}

#[test]
fn armed_missed_heartbeats_with_healthy_channel_is_a_hang() {
    let (mut w, _) = Watchdog::new(cfg());
    w.on(Event::Spawned { at: 0 });
    w.on(Event::Ready);
    w.on(Event::Heartbeat { at: 0 });
    let fx = w.on(Event::Tick { now: 16 });   // 16 s since last heartbeat, child never exited, channel healthy
    assert!(fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::Hang))));
    assert!(fx.contains(&Action::SpawnMain) || fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::Restart { cause: "hang", .. }))));
}

#[test]
fn channel_fault_reconnect_does_not_restart() {
    let (mut w, _) = Watchdog::new(cfg());
    w.on(Event::Spawned { at: 0 });  w.on(Event::Ready);  w.on(Event::Heartbeat { at: 0 });
    w.on(Event::ChannelFault { at: 5 });
    let fx = w.on(Event::ChannelReconnected);
    assert!(fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::ChannelReset))));
    assert!(!fx.contains(&Action::SpawnMain), "a reconnected channel must NOT restart");
}

#[test]
fn exit_86_stops_the_launcher_and_never_restarts() {
    let (mut w, _) = Watchdog::new(cfg());
    w.on(Event::Spawned { at: 0 });  w.on(Event::Ready);
    let fx = w.on(Event::ChildExited { code: 86, at: 30 });
    assert_eq!(fx, vec![Action::ExitLauncher { code: 86 }], "code 86 is a technician exit, not a crash");
}
```

Run → FAIL.

- [ ] **Step 4: implement `on` for rules 1–3 + 9.** Arm on `Ready` (log `Arm{time_to_ready = now - spawned_at}` — track "now" from the latest `Tick`/event `at`; grace-expiry check on `Tick`). On `Tick` while `Armed`: if `now - last_heartbeat >= MISS_LIMIT_S` → disambiguate: a prior `ChildExited` short-circuits (handled in its own arm → restart, Task 3); a live channel fault with an expired `channel_grace_until` → restart, else if healthy channel → `Hang` + restart (Task 3 does the actual backoff/respawn; here emit the `Hang` log + hand to the restart path). `ChildExited{86}` → `ExitLauncher{86}`. `ChildExited{other}` → restart path (Task 3). `ChannelFault{at}` → set `channel_grace_until = at + channel_grace_s`. `ChannelReconnected` → clear it + `Log(ChannelReset)`. `Heartbeat{at}` → `last_heartbeat = at`. Grace expiry in `Spawning` with no `Ready` → a failed start (Task 3 restart path, cause `"no_ready"`).

Run `cargo test -p kiosk-core watchdog::` → the Step-3 tests PASS (restart-emitting ones may depend on Task 3 — if so, assert only the `Log` action here and complete the SpawnMain assertions in Task 3; keep each test's assertions to what this task implements).

- [ ] **Step 5: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): watchdog FSM — types, READY arming, miss disambiguation, exit-86"
```

---

### Task 3: restart, backoff, health-reset (rules 4–6)

**Files:** Modify `crates/kiosk-core/src/watchdog.rs`

- [ ] **Step 1: failing tests.**

```rust
#[test]
fn backoff_doubles_from_1_to_the_60s_ceiling() {
    let (mut w, _) = Watchdog::new(cfg());
    let mut seen = vec![];
    let mut t = 0;
    for _ in 0..10 {
        w.on(Event::Spawned { at: t });  w.on(Event::Ready);
        if let Some(b) = restart_backoff(&mut w, t + 1) { seen.push(b); }   // helper: crash at t+1, capture Restart.backoff_s
        t += 200;
    }
    // 1,2,4,8,16,32,60,60,60,60 — doubles then holds at 60
    assert_eq!(&seen[..7], &[1,2,4,8,16,32,60]);
    assert!(seen[7..].iter().all(|&b| b == 60), "ceiling holds at 60");
}

#[test]
fn a_healthy_run_resets_backoff() {
    let (mut w, _) = Watchdog::new(cfg());
    w.on(Event::Spawned { at: 0 }); w.on(Event::Ready);
    let b1 = restart_backoff(&mut w, 1).unwrap();   // crashed fast → backoff grows
    // next instance runs healthy_run_s+ before crashing → backoff must reset to 1
    w.on(Event::Spawned { at: 100 }); w.on(Event::Ready);
    w.on(Event::Tick { now: 100 + 121 });           // ran > healthy_run_s (120)
    let b2 = restart_backoff(&mut w, 100 + 300).unwrap();
    assert!(b1 > 0 && b2 == 1, "a run past healthy_run_s clears backoff (was {b1}, now {b2})");
}
```

(`restart_backoff` is a test helper: feed a `ChildExited{code:1, at}`, find the `Restart` action, return its `backoff_s`; `None` if no restart.)

Run → FAIL.

- [ ] **Step 2: implement restart + backoff + health reset.** On a restart trigger (non-86 exit, confirmed hang, or grace-expiry no-READY): record `at` into `restarts`; emit `DrainOrphanedSpool` + `Log(Restart{code, backoff_s, cause})` + transition to `BackingOff{until: at + backoff_s}`; then double `backoff_s` (`(backoff_s*2).min(60)`) for next time. On `Tick` in `BackingOff` with `now >= until` → `SpawnMain` (or `SpawnSafe` if `safe`) + `Spawning{grace}`. **Health reset:** on any event carrying `now`, if `Armed` and `now - spawned_at >= healthy_run_s` → `backoff_s = 1` and clear the crash-loop window (Task 4) — once per instance (guard so it fires at the boundary, not every tick).

Run `cargo test -p kiosk-core watchdog::` → PASS (Task-2 restart assertions now complete too).

- [ ] **Step 3: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): watchdog restart + 1..60s backoff + healthy_run_s reset"
```

---

### Task 4: crash-loop sliding window → safe mode + escalation (rules 7–8)

**Files:** Modify `crates/kiosk-core/src/watchdog.rs`

- [ ] **Step 1: failing tests.**

```rust
const WINDOW_S: u64 = 600;

#[test]
fn more_than_5_restarts_in_10min_enters_safe_mode() {
    let (mut w, _) = Watchdog::new(cfg());
    // 6 fast crashes within 600 s → the 6th tips into safe mode
    let mut entered_safe = false;
    for i in 0..6 {
        w.on(Event::Spawned { at: i * 10 }); w.on(Event::Ready);
        let fx = w.on(Event::ChildExited { code: 1, at: i * 10 + 1 });
        if fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::SafeMode))) { entered_safe = true; }
    }
    assert!(entered_safe, ">5 restarts in 10 min must enter safe mode");
    // and the next spawn is --safe
    let fx = w.on(Event::Tick { now: 10_000 });
    assert!(fx.contains(&Action::SpawnSafe) || matches!(w_phase_spawns_safe(&mut w), true));
}

#[test]
fn five_restarts_in_10min_does_not_enter_safe() {
    let (mut w, _) = Watchdog::new(cfg());
    let mut safe = false;
    for i in 0..5 {
        w.on(Event::Spawned { at: i * 10 }); w.on(Event::Ready);
        let fx = w.on(Event::ChildExited { code: 1, at: i * 10 + 1 });
        if fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::SafeMode))) { safe = true; }
    }
    assert!(!safe, "exactly 5 in 10 min stays in normal mode (boundary: > 5, not >= 5)");
}

#[test]
fn three_consecutive_safe_fails_escalate_to_safe_mode_failed() {
    let (mut w, _) = Watchdog::new(cfg());
    force_into_safe(&mut w);                     // helper: drive >5 crashes
    let mut escalated = false;
    for i in 0..3 {                              // 3 --safe starts each failing within healthy_run_s
        w.on(Event::Spawned { at: 10_000 + i * 10 }); w.on(Event::Ready);
        let fx = w.on(Event::ChildExited { code: 1, at: 10_000 + i * 10 + 5 });
        if fx.iter().any(|a| matches!(a, Action::Log(WatchdogEvent::SafeModeFailed))) { escalated = true; }
    }
    assert!(escalated, "N=3 safe-fails within healthy_run_s → safe_mode_failed CRITICAL");
}
```

Run → FAIL.

- [ ] **Step 2: implement.** Maintain `restarts` as timestamps; on each restart prune entries older than `now - WINDOW_S`; if `restarts.len() > 5` and not already `safe` → set `safe = true`, `safe_fails = 0`, clear the window, emit `Log(SafeMode)` and make the next spawn `SpawnSafe`. In safe mode: a `--safe` instance that exits within `healthy_run_s` → `safe_fails += 1`; at `safe_fails >= 3` → `Log(SafeModeFailed)` + hold at the 60 s backoff ceiling (stop fast-looping). A `--safe` (or a normal retry every 10 min) that survives `healthy_run_s` → `safe = false`, reset counters (exits safe). Prune + boundary use `>` (not `>=`) for the 5-restart rule.

Run `cargo test -p kiosk-core watchdog::` → PASS. Run the FULL kiosk-core suite (`cargo test -p kiosk-core`) → green.

- [ ] **Step 3: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): watchdog crash-loop window → safe mode + escalation"
```

---

## Self-Review

**Spec coverage (E1 design / §3.1):** heartbeat frames → T1; READY arming (rule 1) + miss/disambiguation (2,3) + exit-86 (9) → T2; restart + backoff + health-reset (4,5,6) → T3; crash-loop→safe + escalation (7,8) → T4. Every §3.1 rule maps to a task + a test. **Covered.** Deferred (E2/P2): the pipe, spawning, real clock, orphaned-spool execution, RT-13 integration, JS-ping liveness frames.

**Placeholder scan:** test helpers (`restart_backoff`, `force_into_safe`, `w_phase_spawns_safe`) are named and their behavior described — the implementer writes them as ≤5-line test utilities. The FSM `on` body in T2 Step 4 / T3 / T4 is specified by its rules + the adversarial tests that pin it (TDD: tests are the behavioral spec); types are given concretely. No invented external APIs — `LogEvent::Watchdog*` are existing P1-B variants.

**Type consistency:** `Event`/`Action`/`WatchdogEvent`/`WatchdogConfig`/`Watchdog` defined T2, extended T3/T4 additively (state fields declared up-front in T2 Step 2). `Frame`/`encode`/`decode` T1. Boundary constants (`MISS_LIMIT_S`, `WINDOW_S`, backoff ceiling 60, `>5`) consistent across tasks.

**Scope:** One host-testable sub-project (watchdog core). Four tasks, each an adversarial-tested reviewer gate. The intricate rules (backoff boundary, `>5`-not-`>=5` crash-loop, safe escalation, exit-86) each have a mutation-minded test. E2 (Windows shell + client + RT-13) is a separate plan.
