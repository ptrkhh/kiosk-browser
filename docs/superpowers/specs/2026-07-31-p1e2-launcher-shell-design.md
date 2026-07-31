# P1-E2 — Windows Launcher Shell + Heartbeat Client + RT-13 (Design)

> Sub-project of P1-E (the launcher watchdog). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.1. **Builds on
> P1-E1** (the pure `watchdog` supervise FSM + `ipc` heartbeat protocol, merged). This is the
> Windows I/O shell that drives the E1 FSM — it reimplements NO supervise logic.

**Status:** approved 2026-07-31 (design). Executes on a Windows host (named pipes, process
spawn, WebView2). The launcher's event-loop core is host-testable behind an `ActionSink`
trait; the pipe/spawn/timer edges + RT-13 are Windows-host.

## Goal

Turn the pure E1 watchdog FSM into a running supervisor: `kiosk-launcher` spawns and watches
`kiosk-main` over a named-pipe heartbeat, restarts per the FSM's decisions, drains a dead
main's orphaned spool, logs `watchdog.*`, and exits cleanly on a technician exit (code 86).
Plus the `kiosk-main` heartbeat client and the RT-13 headless integration test.

## Architecture — actor loop (mirrors D2a's `EffectSink`)

Three source threads feed one `mpsc::Sender<watchdog::Event>`; a single loop owns the
`Watchdog` and dispatches each returned `Action` to an **`ActionSink`**.

```
 pipe-reader thread  ─ decode(line) → Ready/Ping → Event::{Ready, Heartbeat{at}} ─┐
 timer thread        ─ every ~1 s → Event::Tick{now} ────────────────────────────┤
 child-waiter thread ─ child.wait() → Event::ChildExited{code, at} ──────────────┤
 (pipe accept/EOF)   ─ → Event::{ChannelFault{at}, ChannelReconnected} ──────────┤
                                                                                  ▼
                                                    LOOP (owns Watchdog)
                                                    for ev: for a in wd.on(ev): sink.dispatch(a)
                                                                                  │
                                                    ┌─────────────────────────────┴───┐
                                               ActionSink (trait)                  (fake for tests)
                                               = LauncherSink (real):
                                                 SpawnMain/SpawnSafe → CreateProcess
                                                 DrainOrphanedSpool → rename+drain
                                                 Log(WatchdogEvent) → Logger (spool/launcher)
                                                 ExitLauncher{code} → process exit
```

The `ActionSink` trait is the host-test seam: a recording fake lets the loop + FSM-driving be
tested without pipes or processes (same push-I/O-to-the-edge pattern as E1's design intent and
D2a's `EffectSink`). **No tokio in the launcher** — a supervisor is a small threads+mpsc loop;
an async runtime is overkill (ponytail). Time (`now`/`at`) is unix seconds, read once per event
at the edge and injected into the pure FSM.

## Components

### kiosk-launcher (the shell)

- **`main.rs`** — read `kiosk.ini` → `WatchdogConfig{startup_grace_s, healthy_run_s,
  channel_grace_s}` + the Logger context (project_id/credential/site/region); build the
  `Watchdog`, the `mpsc<Event>`, the `LauncherSink`; execute the initial `SpawnMain` action;
  run the loop until `ExitLauncher`.
- **`pipe.rs`** — the named-pipe **server** (`\\.\pipe\kiosk-heartbeat`, per-boot instance name
  to avoid a stale-instance collision). Accepts one main connection; a reader thread reads
  `'\n'`-delimited lines → `ipc::decode` → `Event::{Ready, Heartbeat{at: now}}`. Pipe EOF/reset
  while the child is alive → `Event::ChannelFault{at: now}`; a re-accept + first frame →
  `Event::ChannelReconnected`. A garbage line (`decode` Err) is dropped (never crashes the
  launcher).
- **`spawn.rs`** — `SpawnMain`/`SpawnSafe` via `std::process::Command` (the same exe path,
  `--safe` for the latter, inheriting `--config`); a child-waiter thread `wait()`s and emits
  `Event::ChildExited{code, at}`. Exposes the child handle so the loop can supervise exactly
  one live child.
- **`sink.rs`** — `ActionSink` trait + `LauncherSink`. `DrainOrphanedSpool` → rename
  `<data>/spool/main` → `<data>/spool.orphaned`, open it as a `Spool`, drain via the shared
  `GclClient` (arch-01/TEL-10 — main's pre-death context is delivered though main is dead).
  `Log(WatchdogEvent)` → `Logger::log(e.log_event(), fields)` into the **launcher's own**
  `spool/launcher` partition (one writer per partition — no cross-process lock). `ExitLauncher`
  → flush + `std::process::exit(code)`.
- **`timer.rs`** — a thread emitting `Event::Tick{now}` every 1 s (cancel-aware for clean exit).

### kiosk-main — the heartbeat client

A dedicated thread (or tokio task — main already runs a tokio runtime): connect to the
launcher's pipe; send `ipc::Frame::Ping` every `PING_INTERVAL_S` (5 s); send `Frame::Ready`
**once**, when the driver first observes a committed navigation (the first
`AppEvent::NavigationCommitted` from the webview — webview initialized + first nav committed,
arch-03). On a pipe drop, reconnect with a backoff **below** the launcher's miss timeout so a
transient drop self-heals into a `ChannelReconnected` (not a restart). If the launcher pipe is
absent entirely (e.g. main run standalone in dev), the client logs once and no-ops — main must
still run without a launcher.

### RT-13 — headless integration test (spec §10)

A **mock-main** test binary that speaks the heartbeat protocol per a script (send `Ready`, then
`Ping`s, then optionally go silent / exit with a code) — the real webview replaced by this stub,
but the real `ipc` frames + real pipe. A test runs the **real launcher** (real `Watchdog` + real
`pipe.rs`/`spawn.rs`) against the mock and asserts the observable outcomes: heartbeat-timeout →
restart; silent-after-ready (hang) → restart; a crash exit code → restart with that code; clean
exit 86 → launcher exits (no restart). This is the end-to-end proof the FSM + I/O compose (RT-13).

## Data flow — a restart

main crashes → child-waiter → `ChildExited{code, now}` → FSM → `[DrainOrphanedSpool,
Log(Restart{code, backoff, cause}), (after backoff) SpawnMain]` → `LauncherSink` renames+drains
main's spool, logs `watchdog.restart`, waits the backoff, spawns a fresh main → pipe re-accepts →
`Ready` re-arms.

## Error handling

- Every pipe read is `decode`-guarded; garbage never crashes the launcher (E1 guarantees `Err`,
  not panic).
- A spawn failure (CreateProcess error) is itself a failed start → feed the FSM a synthetic
  `ChildExited` (non-zero) so backoff/crash-loop still governs; never busy-loop.
- Orphan-drain failure (spool missing/locked) → log and continue; a lost drain is not fatal.
- The launcher's own Logger is `try`-based like main's (telemetry never takes down the
  supervisor).
- **exit-86 exemption at the OS layer:** the autostart integration must not auto-restart the
  launcher on code 86 (the launcher owns crash-restart, not the OS). Windows uses a boot/logon
  Scheduled Task with no restart-on-exit; a technician exit (86) reaches the desktop. Relaunch is
  explicit operator action (reboot / next logon), documented in the runbook (arch-05).

## Testing

- **Host-testable (behind traits, any platform):** the event→`Event` mapping (frame line →
  `Heartbeat`/`Ready`; EOF → `ChannelFault`); the loop driving the FSM against a **fake
  `ActionSink`** with a scripted `Event` sequence → assert the exact `Action` dispatch order
  (spawn → drain → log → respawn); the orphan-drain path's rename+drain logic factored to a pure
  function fed a temp dir.
- **Windows-host:** the real named pipe (server↔client round-trip), real process spawn/exit, the
  kiosk-main heartbeat client, and **RT-13** end-to-end (launcher ↔ mock-main). A manual smoke:
  kill main → launcher restarts it (visible `watchdog.restart` in Cloud Logging); crash-loop main
  → safe mode → `watchdog.safe_mode`; exit 86 via the PIN pad → both processes exit to desktop.

## Scope / defer

Windows only. Deferred: Linux unix-socket transport + systemd `RestartPreventExitStatus=86`
(P2); Android has no launcher (P3); the **signed WiX MSI** that installs the launcher, sets the
Scheduled Task, ACLs the credential, and bootstraps the evergreen WebView2 runtime is **P1-F**
(E2 ships the launcher binary + the exit-86 behavior + a Scheduled-Task setup doc). The
cross-platform webview-round-trip / JS-ping liveness is **P2** (P1 hang handling = the native
`ProcessFailed` reload already in D2c + the launcher's liveness disambiguation from E1).
