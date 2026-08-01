# P1-F1 — Supervision + Lifecycle Hardening (Design)

> Sub-project of P1-F (the deployable Windows MVP finish line). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.1, §3.5, §5.2 (cfg-15),
> §7.2. Builds on P1-E (launcher + FSM) and P1-D (app state machine, which already has
> `AppState::Safe`). Sibling **P1-F2** is the WiX MSI + autostart + §7.2 lockdown docs.

**Status:** approved 2026-08-02 (design). The nightly-reload schedule computation is
host-testable (kiosk-core); the Job Object, single-instance mutex, `--safe` render, and the
timer wiring are Windows-host.

## Goal

Close the one field failure the watchdog design didn't cover, and finish the runtime lifecycle:
(1) a killed launcher must take kiosk-main down with it and never double-supervise; (2) `--safe`
must actually render the safe page so `watchdog.safe_mode` means something; (3) `nightly_reload`
must reload the site on a DST-safe daily schedule.

## Components

### 1. Launcher: Job Object + single-instance mutex (kiosk-launcher, Windows)

**The field failure (E2 carry-forward):** killing `kiosk-launcher` today orphans `kiosk-main`
(it keeps running unsupervised), and relaunching the launcher spawns a *second* main.

- **Job Object.** At launcher start, `CreateJobObjectW` + `SetInformationJobObjectW` with
  `JOBOBJECT_EXTENDED_LIMIT_INFORMATION { BasicLimitInformation.LimitFlags =
  JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE }`. Every spawned child (`spawn_main`) is
  `AssignProcessToJobObject`'d to it. When the launcher process dies for any reason — crash,
  taskkill, logoff — the job handle closes and the kernel terminates all children. The launcher
  holds the job handle for its whole life. (Assign must happen before the child runs
  unsupervised; spawn the child suspended-or-immediately then assign — confirm the race-free
  order against the `windows` crate: create → assign → resume, or spawn + assign-immediately if
  the tiny window is acceptable.)
- **Single-instance mutex.** At launcher start, `CreateMutexW(name = "Global\\kiosk-launcher")`;
  if `GetLastError() == ERROR_ALREADY_EXISTS`, another launcher owns supervision → **log and
  exit** (do not spawn a second main). Held for the launcher's life.

Both are `#[cfg(windows)]`; the non-Windows path is a no-op stub (Linux uses process groups /
systemd — P2).

### 2. `--safe` render (kiosk-main, Windows/all)

`kiosk-main --safe` currently has no flag and no effect (a no-op — so `watchdog.safe_mode` today
only means "escalation attempted"). Wire it:

- Add `--safe` to the CLI. When set, kiosk-main boots straight into the P1-D1 `AppState::Safe`
  path: navigate the webview to a bundled `safe.html` showing the **device id + last error**, and
  do **not** start the config fetch, the prober, or remote navigation (§3.1: "bundled error page
  showing device ID + last error, no remote load"). The heartbeat client still runs (so the
  launcher sees a live, arming `--safe` child — this is what lets safe-mode escalation count a
  `--safe` that fails vs survives).
- "Last error" source: the most recent `crash-panic.txt` (P1-D2e) if present, else a generic
  message. `safe.html` is a bundled app-origin page; D2d polishes it, F1 ships a functional one.

### 3. Nightly reload (kiosk-core schedule + kiosk-main timer)

`content`/`maintenance.nightly_reload` = a local wall-clock `"HH:MM"` (null = off); fires **once
per calendar day**, **DST-safe**, in `maintenance.timezone` (IANA name, null = system local) —
cfg-15. **`restart_app` is P2** (spec §9) — F1 does NOT implement the full-restart timer.

- **kiosk-core `maintenance` module (pure, host-tested):**
  `fn next_fire(hhmm: &str, tz: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>>` —
  the next UTC instant at which local wall-clock `HH:MM` occurs strictly after `now`, in the given
  IANA timezone (or system local when `None`). Returns `None` on an unparseable `HH:MM`. DST-safe
  via `chrono-tz` (new dep): compute the next local date's `HH:MM`, resolve to UTC through the
  zone (handling the spring-forward gap / fall-back ambiguity by taking the earliest valid
  instant). Time is injected (`now`), so it is fully host-testable.
- **kiosk-main maintenance timer (Windows/all):** a task that loops: `let fire =
  next_fire(nightly_reload, timezone, now_utc())`; sleep until `fire`; **reload the site** (send a
  navigate-home / reload to the driver — reuse the existing `Effect::Navigate(home)` path, a fresh
  load that resets any accumulated page state); recompute the next `fire`. Cancel-aware. Off when
  `nightly_reload` is `None`. A single fire per calendar day is guaranteed by `next_fire` always
  returning a strictly-future instant.

## Data flow

- **Launcher lifecycle:** launcher start → mutex check (exit if a peer exists) → create Job →
  each `spawn_main` assigns the child to the Job. Launcher death → Job closes → children killed.
- **Safe:** watchdog crash-loop → `SpawnSafe` → launcher spawns `kiosk-main --safe` → main renders
  `safe.html` + heartbeats → launcher arms; a `--safe` that survives `healthy_run_s` exits safe
  mode (E1 rule 8), one that fails within it escalates.
- **Nightly reload:** timer computes `next_fire` → sleeps → reloads the site at ~`HH:MM` local →
  recomputes tomorrow's.

## Error handling

- Job/mutex creation failure → log a WARNING and continue **unsupervised-but-running** (never
  block boot on a supervision-hardening failure); the launcher still works, just without the
  kill-on-close guarantee — surfaced in telemetry so a misconfigured host is visible.
- `next_fire` on a bad `HH:MM`/timezone → `None` (reload disabled) + a `config.warn`; never panic.
- `--safe` with no readable last-error → generic message; safe mode must never fail to render.

## Testing

- **Host-testable (kiosk-core `maintenance`):** `next_fire` — a time before/after today's `HH:MM`
  rolls to the right day; an IANA zone differs from UTC by the right offset; a **DST spring-forward**
  date where `HH:MM` falls in the skipped hour resolves to a valid instant; a bad `HH:MM` → `None`;
  the returned instant is always strictly after `now`. Adversarial around the DST boundary.
- **Windows-host:** kill the launcher (Task Manager) → kiosk-main dies within ~1 s (Job Object);
  launch a second launcher while one runs → it logs + exits, no second main (mutex);
  `kiosk-main --safe` shows `safe.html` with the device id, no remote load, launcher arms;
  set `nightly_reload` a minute out → the site reloads at that wall-clock minute.

## Scope / defer

F1 = these three. Deferred: **`restart_app`** full-restart timer (P2, spec §9);
**`max_webview_mem_mb`** memory-cap restart (P2); the **touch keyboard** (PF-02) — a separate item;
the **WiX MSI + autostart Scheduled Task + §7.2 OS-lockdown runbook** = **P1-F2**. Linux
process-group supervision + systemd = P2; Android = P3.
