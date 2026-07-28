# P1-E1 — Watchdog Core: Heartbeat Protocol + Supervise FSM (Design)

> Sub-project of P1-E (the launcher watchdog). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.1. **P1-E1 is the
> pure, host-testable core**; the Windows launcher shell + kiosk-main heartbeat client + the
> RT-13 integration test are **P1-E2** (a separate spec/plan, executed on Windows).

**Status:** approved 2026-07-28 (design). Fully host-testable (`cargo test -p kiosk-core`) —
no process spawning, no OS pipes, no clock reads inside the logic.

## Goal

The two pieces of the watchdog that must be correct-by-test: (1) the **heartbeat frame
protocol** shared by `kiosk-main` (client) and `kiosk-launcher` (server), and (2) the
**supervise state machine** that decides when to arm, restart, back off, enter safe mode, and
escalate — as a pure `(state, event, now) → (state, Vec<Action>)` function, so every rule in
spec §3.1 is unit-testable without a real process or pipe. Mirrors how P1-D1 made the app FSM
host-testable.

## Layering

Both modules are pure kiosk-core: no `std::process`, no named pipes/sockets, no `Instant::now`
inside the logic (time is injected). P1-E2's launcher shell owns spawning, the pipe, the real
clock, and executing the `Action`s; it feeds the FSM `Event`s. The `ipc` protocol is shared by
both `kiosk-main` and `kiosk-launcher` (spec §4 puts `ipc/` in kiosk-core).

## Component 1 — `ipc` heartbeat protocol (`crates/kiosk-core/src/ipc.rs`)

Newline-delimited JSON frames over the heartbeat channel (simple, debuggable, transport-
agnostic — the P1-E2 named pipe / P2 unix socket just carry bytes).

```rust
#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Ready,           // main → launcher: webview initialized + first navigation committed (arch-03)
    Ping,            // main → launcher: liveness, every 5 s (PING_INTERVAL_S)
    // P2: Echo round-trip frames for JS-ping webview liveness — NOT in P1-E1.
}
pub fn encode(frame: &Frame) -> String;              // one line, '\n'-terminated
pub fn decode(line: &str) -> Result<Frame, IpcError>; // one line → Frame; junk → Err (never panic)
pub const PING_INTERVAL_S: u64 = 5;
```

Host tests: round-trip every variant; a malformed/partial line is an `Err`, not a panic
(the launcher must survive garbage on the pipe); unknown `type` → `Err` (forward-compat:
a newer main sending a P2 frame doesn't crash an older launcher — it's ignored).

## Component 2 — `watchdog` supervise FSM (`crates/kiosk-core/src/watchdog.rs`)

A pure Mealy machine. The launcher shell (P1-E2) translates real events (process exited,
byte on pipe, timer tick) into `Event`s, calls `on`, and executes the returned `Action`s.

```rust
pub struct WatchdogConfig {   // from kiosk.ini (§3.1 defaults)
    pub startup_grace_s: u64,   // 90  — max wait for READY before an unarmed start counts as failed
    pub healthy_run_s: u64,     // 120 — continuous uptime that clears backoff + crash-loop counter
    pub channel_grace_s: u64,   // 30  — reconnect window for a channel fault before restart
}
pub enum Event {
    Spawned { at: u64 },               // launcher spawned main (unix secs)
    Ready,                             // READY frame received
    Heartbeat,                         // Ping frame received
    ChildExited { code: i32, at: u64 },// main process exited
    Tick { now: u64 },                 // periodic clock tick (the shell drives cadence)
    ChannelFault,                      // pipe EOF/reset while child still alive
    ChannelReconnected,                // main re-accepted within channel_grace_s
}
pub enum Action {
    SpawnMain,                         // (re)spawn kiosk-main normally
    SpawnSafe,                         // spawn kiosk-main --safe
    DrainOrphanedSpool,                // rename spool/main → spool.orphaned, drain (on restart)
    Log(WatchdogEvent),                // watchdog.{arm,restart,hang,channel_reset,safe_mode,safe_mode_failed}
    ExitLauncher { code: i32 },        // technician exit 86 → launcher exits too
}
pub struct Watchdog { /* state, last_heartbeat, backoff, restart-window, safe counters, … */ }
impl Watchdog {
    pub fn new(cfg: WatchdogConfig) -> Watchdog;   // initial state emits SpawnMain
    pub fn on(&mut self, event: Event) -> Vec<Action>;
}
```

### Rules (spec §3.1 — the authority; defaults cited, not re-derived)

1. **READY arming (arch-03):** after `Spawned`, heartbeat enforcement is DISARMED. `Ready`
   within `startup_grace_s` → arm, `Log(watchdog.arm{time_to_ready})`. Grace expires with no
   `Ready` → one **failed start** (feeds backoff/crash-loop), NOT an infinite kill-loop.
2. **Heartbeat miss:** while armed, `Heartbeat` refreshes liveness. `Tick` with no heartbeat
   for ≥ 3 intervals (15 s) → a **miss event**, resolved by disambiguation (rule 3) — a bare
   miss never restarts.
3. **Liveness disambiguation (arch-15):** on the 3rd miss — (a) `ChildExited` seen → restart
   immediately with the real code; (b) `ChannelFault` (child alive) → re-accept, wait
   `channel_grace_s` for `ChannelReconnected`, no restart, `Log(watchdog.channel_reset)`; if
   grace expires → restart; (c) child alive + healthy channel + still no heartbeat → genuine
   hang → restart, `Log(watchdog.hang)`.
4. **Restart** (any exit code except 86, or a confirmed hang/failed-start): `DrainOrphanedSpool`
   + `SpawnMain` + `Log(watchdog.restart{code, backoff, cause})`, after the backoff delay.
5. **Backoff (arch-12):** 1 → 2 → 4 → … → 60 s ceiling between restarts.
6. **Health reset:** a main instance running continuously ≥ `healthy_run_s` clears backoff AND
   the restart counter (an occasional crash never ratchets).
7. **Crash-loop → safe (arch-12):** the restart counter is a **sliding 10-minute window**;
   > 5 restarts within any 10-min window → `SpawnSafe` + `Log(watchdog.safe_mode)`; entering
   safe clears the window. In safe mode, retry normal every 10 min; a retry that fails within
   `healthy_run_s` re-enters safe; a retry surviving `healthy_run_s` exits safe + resets.
8. **Safe-mode escalation (arch-14):** N=3 consecutive `--safe` starts that fail within
   `healthy_run_s` → stop fast-looping, back off to the 60 s ceiling, `Log(watchdog.safe_mode_failed)`
   CRITICAL (bounded, visible — not an invisible infinite loop).
9. **Exit 86 (arch-05):** `ChildExited{code:86}` → `ExitLauncher{86}`, NO restart. (Autostart
   OS integration also exempts 86 — that's P1-E2 packaging, not the FSM.)

`WatchdogEvent` carries the structured fields each `watchdog.*` telemetry entry needs (the
existing `LogEvent::Watchdog*` taxonomy from P1-B — reuse, don't add).

## Data flow

Shell tick loop (P1-E2): read pipe → `decode` → `Event::{Ready,Heartbeat}`; `waitpid`-style
child check → `Event::ChildExited`; periodic `Event::Tick{now}`. Each `on` returns `Action`s
the shell executes (spawn, drain, log, exit). The FSM holds all timing/counter state; the
shell only supplies `now` and raw events.

## Error handling

- `decode` never panics on malformed input (the pipe carries untrusted-ish bytes; a garbage
  line is dropped).
- The FSM is total: every `(state, event)` has a defined outcome (unhandled = no-op, never
  panic) — a locked kiosk's watchdog must never crash.
- Counters are overflow-safe (`saturating_*`), like the app FSM's retry counter.

## Testing (host, adversarial — this is the whole point of E1)

- **`ipc`:** frame round-trip; malformed/partial/unknown-type → `Err` not panic.
- **`watchdog` FSM** (inject `now`, assert exact `Action` sequences):
  - READY within grace → `watchdog.arm`; grace expiry with no READY → counts as a failed start.
  - 3-miss with child exited → restart with the real code; with channel fault → `channel_reset`
    + no restart, then restart if grace expires; with healthy channel → `watchdog.hang` + restart.
  - Backoff sequence 1,2,4,…,60 and the ceiling; a `healthy_run_s` survival resets it (mutation:
    off-by-one on the reset boundary).
  - Sliding-window crash-loop: exactly >5 in 10 min → safe; 5-in-10 does NOT; a spread-out crash
    rate (each run ≥ healthy_run_s) never quarantines.
  - Safe-mode escalation: N=3 safe-fails-within-healthy_run_s → `safe_mode_failed` CRITICAL;
    a safe run surviving healthy_run_s exits safe + resets.
  - Exit 86 → `ExitLauncher`, never a restart (mutation: treating 86 as a crash).

## Scope / defer (P1-E2 and beyond)

Out of E1: process spawning, the Windows named-pipe server + kiosk-main client thread, the
real clock/timers, orphaned-spool file rename+drain execution, the RT-13 headless-integration
test, autostart/OS exit-86 exemption — all **P1-E2 (Windows)**. The webview round-trip / JS-ping
liveness frames are **P2**. Linux unix-socket transport **P2**; Android has no launcher (**P3**).
