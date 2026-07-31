# P1-E2 — Windows Launcher Shell + Heartbeat Client + RT-13 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** T1 (the loop + `ActionSink`) and the pure drain/mapping helpers are host-testable (`cargo test -p kiosk-launcher`). T2–T6 (named pipe, process spawn, kiosk-main client, RT-13) are Windows-host build + smoke. Controller-side link is blocked (aarch64/Linux) — rely on implementer evidence + reviewer code-read, as prior Windows plans did.

**Goal:** Drive the pure E1 `watchdog` FSM as a running supervisor — `kiosk-launcher` spawns/watches `kiosk-main` over a named-pipe heartbeat, restarts per the FSM, drains a dead main's orphaned spool, logs `watchdog.*`, exits on code 86 — plus the `kiosk-main` heartbeat client and the RT-13 headless integration test.

**Architecture:** Three source threads (pipe-reader, timer, child-waiter) feed one `mpsc::Receiver<watchdog::Event>`; a single loop owns the `Watchdog` and dispatches each `Action` to an **`ActionSink`** (real = spawn/drain/log/exit; fake = host-test seam). No tokio in the launcher. Reimplements NO supervise logic — E1's FSM decides everything.

**Tech Stack:** Rust, `std::process`, Windows named pipes (`windows` crate or `std`), `kiosk-core` (`watchdog`, `ipc`, `logging`).

**Design spec:** `docs/superpowers/specs/2026-07-31-p1e2-launcher-shell-design.md`. Rule authority: parent spec §3.1.

## Global Constraints

- **Reimplement no supervise logic.** All restart/backoff/safe/exit-86 decisions come from `kiosk_core::watchdog::Watchdog::on`. The launcher only maps I/O → `Event` and executes `Action`s.
- **Time is unix seconds**, read once at the edge (`SystemTime::now`) and put into `Event.at`/`Tick.now`. The FSM never reads a clock.
- **Total/panic-free at the edges.** A garbage pipe line → `ipc::decode` `Err` → dropped (E1 guarantees no panic). A spawn failure → a synthetic `ChildExited{non-zero}` so backoff/crash-loop still governs — never busy-loop.
- **Spool partitions (arch-01):** launcher logs to `<data>/spool/launcher`; on `DrainOrphanedSpool` it renames `<data>/spool/main` → `<data>/spool.orphaned` and drains THAT (one writer per partition, no cross-process lock).
- **Telemetry never takes down the supervisor** — the launcher's `Logger` is `try`-based like main's.
- **Windows/P1 only.** Guard pipe/spawn platform bits `#[cfg(windows)]` with a non-Windows stub. Linux socket + systemd exit-86 exemption = P2.
- **`PING_INTERVAL_S`=5, miss=15s** are E1 constants — the client pings every 5 s; do not re-declare them.

## Interfaces this plan drives (E1 + P1-B, merged)

```rust
// kiosk_core::watchdog
struct WatchdogConfig { startup_grace_s: u64, healthy_run_s: u64, channel_grace_s: u64 }
enum Event { Spawned{at:u64}, Ready, Heartbeat{at:u64}, ChildExited{code:i32, at:u64},
             Tick{now:u64}, ChannelFault{at:u64}, ChannelReconnected }
enum Action { SpawnMain, SpawnSafe, DrainOrphanedSpool, Log(WatchdogEvent), ExitLauncher{code:i32} }
enum WatchdogEvent { Arm{time_to_ready_s:u64}, Restart{code:i32,backoff_s:u64,cause:&'static str},
                     Hang, ChannelReset, SafeMode, SafeModeFailed }  // .log_event() -> LogEvent
Watchdog::new(WatchdogConfig) -> (Watchdog, Vec<Action>)            // initial Vec = [SpawnMain]
Watchdog::on(&mut self, Event) -> Vec<Action>
// kiosk_core::ipc
enum Frame { Ready, Ping }   fn encode(&Frame)->String   fn decode(&str)->Result<Frame,IpcError>
const PING_INTERVAL_S: u64 = 5;
// kiosk_core::logging (P1-B) — the launcher builds its OWN stack (spool/launcher):
Logger::new(EntryContext, Spool, GclClient, RateLimiter, TrustedClock)  Logger::log(LogEvent, Map)  flush/tick
Spool::open(&Path, SpoolConfig) -> Result<Spool, SpoolError>   drain_batch(max) -> Vec<LogEntry>   dir()
GclClient::new(TokenSource, Arc<dyn Transport>, TrustedClock)   .write(&[LogEntry])
ServiceAccount::from_json(&str)  TokenSource::new(...)  ReqwestTransport::new(Duration)  RateLimiter::new(clock)
// kiosk_core::config::bootstrap::BootstrapConfig — startup_grace_s/healthy_run_s/channel_grace_s/
//   project_id/credential/site/region (read from kiosk.ini). BootstrapConfig::parse(&str).
// kiosk-main/src/nav.rs (Windows): emits AppEvent::NavigationCommitted (nav.rs:217) — the READY trigger.
```

---

### Task 1: `kiosk-launcher` deps + the `ActionSink` loop (host-tested core)

**Files:**
- Modify: `crates/kiosk-launcher/Cargo.toml`
- Create: `crates/kiosk-launcher/src/loop_.rs` (the event loop + `ActionSink` trait)
- Modify: `crates/kiosk-launcher/src/main.rs` (`mod loop_;`)
- Test: `crates/kiosk-launcher/src/loop_.rs`

**Interfaces:**
- Produces: `trait ActionSink { fn dispatch(&mut self, a: Action) -> ControlFlow<i32>; }` (return `Break(code)` on `ExitLauncher` to stop the loop); `fn run(rx: mpsc::Receiver<Event>, wd: Watchdog, initial: Vec<Action>, sink: &mut dyn ActionSink) -> i32`.
- Consumes: `kiosk_core::watchdog::{Watchdog, Event, Action}`.

- [ ] **Step 1: deps.** In `crates/kiosk-launcher/Cargo.toml` add `serde_json = "1"` (for the Logger fields) — reqwest/etc. come transitively via kiosk-core. (Windows pipe/process deps added in T2/T3.)

- [ ] **Step 2: failing test.** Create `loop_.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::watchdog::{Action, Event, Watchdog, WatchdogConfig};
    use std::sync::mpsc;

    #[derive(Default)]
    struct RecordingSink { actions: Vec<Action> }
    impl ActionSink for RecordingSink {
        fn dispatch(&mut self, a: Action) -> std::ops::ControlFlow<i32> {
            let stop = matches!(a, Action::ExitLauncher { .. });
            let code = if let Action::ExitLauncher { code } = a { code } else { 0 };
            self.actions.push(a);
            if stop { std::ops::ControlFlow::Break(code) } else { std::ops::ControlFlow::Continue(()) }
        }
    }
    fn cfg() -> WatchdogConfig { WatchdogConfig { startup_grace_s: 90, healthy_run_s: 120, channel_grace_s: 30 } }

    #[test]
    fn dispatches_the_initial_spawn_then_exits_on_code_86() {
        let (wd, initial) = Watchdog::new(cfg());
        let (tx, rx) = mpsc::channel();
        tx.send(Event::Spawned { at: 0 }).unwrap();
        tx.send(Event::Ready).unwrap();
        tx.send(Event::ChildExited { code: 86, at: 30 }).unwrap();
        drop(tx);
        let mut sink = RecordingSink::default();
        let code = run(rx, wd, initial, &mut sink);
        assert_eq!(code, 86, "exit-86 stops the loop with code 86");
        assert!(sink.actions.iter().any(|a| matches!(a, Action::SpawnMain)), "initial spawn dispatched");
        assert!(matches!(sink.actions.last(), Some(Action::ExitLauncher { code: 86 })));
    }
}
```

Run `cargo test -p kiosk-launcher loop_::` → FAIL.

- [ ] **Step 3: implement.**

```rust
use std::ops::ControlFlow;
use std::sync::mpsc::Receiver;
use kiosk_core::watchdog::{Action, Event, Watchdog};

/// Executes the FSM's Actions. Real impl (Task 4) spawns/drains/logs; tests record.
/// Returns `Break(code)` when it handled `ExitLauncher`, to stop the loop.
pub trait ActionSink {
    fn dispatch(&mut self, action: Action) -> ControlFlow<i32>;
}

/// Drain events into the FSM; dispatch each Action. Returns the process exit code.
pub fn run(rx: Receiver<Event>, mut wd: Watchdog, initial: Vec<Action>,
           sink: &mut dyn ActionSink) -> i32 {
    for a in initial {
        if let ControlFlow::Break(code) = sink.dispatch(a) { return code; }
    }
    while let Ok(ev) = rx.recv() {
        for a in wd.on(ev) {
            if let ControlFlow::Break(code) = sink.dispatch(a) { return code; }
        }
    }
    0
}
```

Run → PASS. fmt/clippy clean, commit: `feat(launcher): ActionSink event loop driving the watchdog FSM`.

---

### Task 2: process spawn + child-waiter + timer (Windows)

**Files:** Create `crates/kiosk-launcher/src/spawn.rs`, `crates/kiosk-launcher/src/timer.rs`

**Interfaces:**
- Produces: `fn spawn_main(exe: &Path, config_dir: &Path, safe: bool, tx: mpsc::Sender<Event>) -> io::Result<Child>` (spawns, and a detached waiter thread that sends `Event::ChildExited{code, at: now}` on exit); `fn spawn_timer(tx: mpsc::Sender<Event>, cancel: Arc<AtomicBool>)` (a thread sending `Event::Tick{now}` every 1 s).

- [ ] **Step 1: `spawn_main`.** `std::process::Command::new(exe)` with args `["--config", config_dir]` + `"--safe"` when `safe`; `.spawn()`. Immediately push `Event::Spawned{at: now()}` on `tx`, then `std::thread::spawn` a waiter that `child.wait()`s and sends `Event::ChildExited{code: status.code().unwrap_or(-1), at: now()}`. Return the `Child` (the loop keeps exactly one live handle; a spawn `Err` → the caller feeds a synthetic `ChildExited{-1}` so backoff governs — do NOT panic). `now()` = `SystemTime::now().duration_since(UNIX_EPOCH).as_secs()`.

- [ ] **Step 2: `spawn_timer`.** A thread: loop { sleep(1s); if cancel → break; tx.send(Tick{now()}) }. Cancel-aware for clean shutdown.

- [ ] **Step 3: Windows smoke** (in the report): the launcher spawns a real kiosk-main; killing it produces a `ChildExited` event (observable via a debug log); the timer emits Ticks.

- [ ] **Step 4:** fmt/clippy, commit: `feat(launcher): process spawn/child-waiter + 1s tick timer`.

---

### Task 3: named-pipe heartbeat server (Windows)

**Files:** Create `crates/kiosk-launcher/src/pipe.rs`

**Interfaces:**
- Produces: `fn serve(pipe_name: &str, tx: mpsc::Sender<Event>, cancel: Arc<AtomicBool>)` — a thread that creates the named-pipe server, accepts the main connection, reads `'\n'`-delimited lines → `ipc::decode` → `Event::{Ready, Heartbeat{at: now}}`; on EOF/reset with a still-live child → `Event::ChannelFault{at: now}`, then re-accept → `Event::ChannelReconnected`.
- Produces: `const PIPE_NAME: &str` base (e.g. `\\.\pipe\kiosk-heartbeat`; append a per-boot suffix to avoid a stale-instance clash).

- [ ] **Step 1: host-test the pure line→Event mapping.** Factor `fn frame_to_event(line: &str, now: u64) -> Option<Event>` (`Frame::Ready → Event::Ready`; `Frame::Ping → Event::Heartbeat{at: now}`; `decode` Err → `None`). Test: a ready line, a ping line, garbage → None. Run `cargo test -p kiosk-launcher pipe::` → PASS (host).

- [ ] **Step 2: the Windows pipe server.** Create the named pipe (`windows` crate `CreateNamedPipeW` with `PIPE_ACCESS_INBOUND`/message-or-byte mode, or a maintained named-pipe crate), `ConnectNamedPipe`, read into a line buffer, split on `'\n'`, feed each through `frame_to_event`. On a read error / broken pipe while the child is alive → send `ChannelFault{now}`, `DisconnectNamedPipe`, re-`ConnectNamedPipe`; the first frame after a reconnect → send `ChannelReconnected` before the frame's own event. `#[cfg(not(windows))]` stub logs + returns.

- [ ] **Step 3: Windows smoke:** the client (Task 5) connects; `Ready`/`Ping` frames arrive as events; killing the client mid-run yields `ChannelFault` then (on reconnect) `ChannelReconnected`.

- [ ] **Step 4:** fmt/clippy, commit: `feat(launcher): named-pipe heartbeat server -> watchdog events`.

---

### Task 4: `LauncherSink` (spawn/drain/log/exit) + launcher Logger + main.rs assembly (Windows)

**Files:** Create `crates/kiosk-launcher/src/sink.rs`; modify `crates/kiosk-launcher/src/main.rs`

**Interfaces:**
- Produces: `struct LauncherSink { … }` impl `ActionSink`; `fn drain_orphan(data_dir: &Path, client: &mut GclClient) -> io::Result<usize>` (pure-ish: rename + drain, host-testable with a temp dir + a fake transport).
- Consumes: Task-1 `ActionSink`/`run`, Task-2 `spawn_main`/`spawn_timer`, Task-3 `serve`, the E1 FSM, the P1-B Logger stack.

- [ ] **Step 1: host-test `drain_orphan`.** In a temp dir, create `spool/main` with a couple of spooled entries; call `drain_orphan(temp, &mut fake_client)`; assert `spool/main` no longer exists, `spool.orphaned` was created and drained (the fake `GclClient`/transport recorded the entries), returns the count. A missing `spool/main` → `Ok(0)`, no error. (Build a fake `Transport` in the launcher's test module, mirroring kiosk-main's.)

- [ ] **Step 2: implement `LauncherSink::dispatch`.**
  - `SpawnMain`/`SpawnSafe` → `spawn::spawn_main(exe, config_dir, safe, tx.clone())`, store the returned `Child` (drop/replace the prior handle). Spawn `Err` → send a synthetic `ChildExited{-1, now}` on `tx` (governed by backoff), no panic.
  - `DrainOrphanedSpool` → `drain_orphan(data_dir, &mut self.client)`; log/ignore errors.
  - `Log(e)` → `self.logger.log(e.log_event(), fields_for(&e))` where `fields_for` shapes the enumerated jsonPayload (`cause`/`code`/`backoff_s`/`time_to_ready_s` per variant). `Continue`.
  - `ExitLauncher{code}` → `self.logger.flush()`, `ControlFlow::Break(code)`.

- [ ] **Step 3: build the launcher Logger stack** (spool/launcher partition). Same primitives as kiosk-main `telemetry::build` but in this crate: `ServiceAccount::from_json(read(credential))`, `TokenSource::new(...)`, `ReqwestTransport::new(10s)`, `GclClient::new`, `Spool::open(data_dir.join("spool/launcher"), SpoolConfig::default-per-spec)`, `RateLimiter::new(clock)`, `EntryContext{... , node_id: device_id}`, `Logger::new`. Read exact ctor params from `crates/kiosk-core/src/logging/*` at impl time. Share ONE `TrustedClock` with the drain client.

- [ ] **Step 4: `main.rs` assembly.** Read `kiosk.ini` (path next to the exe, or `--config`); `BootstrapConfig::parse` → `WatchdogConfig` + Logger context; resolve the kiosk-main exe path (same dir); create the `mpsc<Event>`, the `cancel` flag; `spawn::spawn_timer` + `pipe::serve` threads (both hold `tx.clone()`); `Watchdog::new(cfg)`; build `LauncherSink`; `let code = loop_::run(rx, wd, initial, &mut sink); std::process::exit(code);`.

- [ ] **Step 5: Windows smoke:** launcher boots main, main shows the site; kill main → `watchdog.restart` in Cloud Logging + main relaunched + main's orphaned spool drained (its pre-death entries appear).

- [ ] **Step 6:** fmt/clippy, commit: `feat(launcher): LauncherSink (spawn/drain/log/exit) + Logger + assembly`.

---

### Task 5: kiosk-main heartbeat client (Windows)

**Files:** Create `crates/kiosk-main/src/heartbeat.rs`; modify `crates/kiosk-main/src/main.rs` (spawn it + wire READY), `crates/kiosk-main/src/nav.rs` (pulse ready on first commit)

**Interfaces:**
- Produces: `fn run(pipe_name: &str, ready: Arc<Notify>, cancel: CancellationToken)` — connect the pipe, `Ping` every `PING_INTERVAL_S`, send `Ready` once after `ready` is notified, reconnect on drop.

- [ ] **Step 1: wire the READY signal.** Add an `Arc<tokio::sync::Notify>` (or `Arc<AtomicBool>`) `ready` shared into `nav::install`. In the `NavigationCommitted` path (nav.rs:217), on the FIRST commit only (a `std::sync::Once`/`AtomicBool` latch), `ready.notify_one()`. This is arch-03: webview initialized + first nav committed.

- [ ] **Step 2: the client.** `heartbeat::run` (a tokio task — main already runs tokio): connect to `pipe_name` (client end of the Windows named pipe; `tokio::net::windows::named_pipe::ClientOptions` OR a std client thread — pick one, confirm availability). Once connected: spawn/await the `ready` notify → write `ipc::encode(&Frame::Ready)`; then a `tokio::time::interval(PING_INTERVAL_S)` writing `encode(&Frame::Ping)` each tick. On a write error (pipe gone) → reconnect with a backoff **< MISS window** (e.g. 2 s) so the launcher sees `ChannelReconnected`, not a restart. If the pipe never connects (dev, no launcher) → log once and return (main must run standalone).

- [ ] **Step 3: spawn in `main.rs`** with the launcher's `pipe_name`, the shared `ready`, and the app `cancel`.

- [ ] **Step 4: Windows smoke:** under the launcher, main sends `Ready` once the site commits (launcher logs `watchdog.arm`) and `Ping`s keep it alive (no spurious restart over minutes).

- [ ] **Step 5:** fmt/clippy, commit: `feat(main): heartbeat client — Ready on first commit, Ping every 5s`.

---

### Task 6: RT-13 headless integration test + mock-main (Windows)

**Files:** Create `crates/kiosk-launcher/tests/rt13.rs` + a mock-main test binary (`crates/kiosk-launcher/tests/bin/mock_main.rs` or a `[[bin]]`/example)

**Interfaces:** Consumes the real launcher (`loop_`/`pipe`/`spawn`/`sink`) + a scriptable mock-main that speaks `ipc` frames.

- [ ] **Step 1: mock-main.** A tiny binary: connect the launcher pipe; per an env/arg script, either (a) send `Ready` then `Ping` forever (healthy); (b) send `Ready` then go silent (hang); (c) exit with a given code after `Ready` (crash / exit-86). No webview — just the real `ipc` protocol over the real pipe.

- [ ] **Step 2: RT-13 test** (spec §10, RT-13) — run the REAL launcher (real `Watchdog` + pipe + spawn) pointed at the mock-main, and assert observable outcomes via the launcher's log/state:
  - healthy → no restart over N seconds; `watchdog.arm` logged.
  - silent-after-ready (hang) → restart within the miss window.
  - crash exit code 7 → restart, `watchdog.restart{code:7}`.
  - exit 86 → launcher exits (no restart).
  (Assert against the launcher's spool/launcher entries or a test-injected `ActionSink` spy, whichever the harness makes observable — the launcher may accept an `ActionSink` for the test build.)

- [ ] **Step 3:** run `cargo test -p kiosk-launcher --test rt13` on Windows; record results. fmt/clippy, commit: `test(launcher): RT-13 headless integration (launcher vs mock-main)`.

---

## Self-Review

**Spec coverage (E2 design / §3.1):** ActionSink loop → T1; spawn/wait + timer → T2; pipe server + frame→event → T3; LauncherSink (spawn/drain/log/exit) + orphan-drain + launcher Logger + assembly → T4; kiosk-main heartbeat client + READY-on-first-commit → T5; RT-13 → T6. exit-86 loop-break → T1; the OS-layer exit-86 exemption + Scheduled-Task doc is P1-F (noted). **Covered.**

**Placeholder scan:** Windows pipe/process/Logger-ctor calls carry "confirm against the crate/version at impl time" pointers to REAL APIs (std::process, the `windows` crate, kiosk-core logging) — resolved by the implementer as prior Windows plans did. T1 + the pure helpers (`frame_to_event`, `drain_orphan`) carry runnable host tests; Windows tasks carry smoke checklists; RT-13 is the end-to-end proof.

**Type consistency:** `ActionSink`/`run` T1 consumed by T4/T6; `Event`/`Action`/`WatchdogEvent` are the merged E1 types; `frame_to_event` T3, `drain_orphan` T4, `heartbeat::run` T5. `PIPE_NAME`/`pipe_name` shared T3↔T5. One `TrustedClock` shared (drain client + Logger).

**Scope:** One sub-project (the Windows launcher shell + client + RT-13). Six tasks; T1 + the pure helpers host-tested, T2–T6 Windows with smoke/RT-13. Signed MSI + OS exit-86 exemption = P1-F; Linux socket = P2.
