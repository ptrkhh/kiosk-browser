# P1-F1 — Supervision + Lifecycle Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** T1 (kiosk-core `maintenance::next_fire`) is host-testable (`cargo test -p kiosk-core`). T2–T4 (`--safe` render, nightly-reload timer, Job Object + mutex) are Windows-host build + smoke. Controller-side link is blocked (aarch64/Linux) — rely on implementer evidence + reviewer code-read.

**Goal:** Close the E2 field failure (a killed launcher orphaning kiosk-main / double-supervise) and finish the runtime lifecycle: `--safe` renders the safe page; `nightly_reload` reloads the site on a DST-safe daily schedule.

**Architecture:** The nightly-reload schedule is a pure kiosk-core function (`next_fire`, time injected, host-tested). The Job Object + single-instance mutex are Win32 in the launcher; `--safe` + the reload timer are kiosk-main.

**Tech Stack:** Rust, `chrono` + `chrono-tz` (new), the `windows` crate (Job Object / mutex), Tauri.

**Design spec:** `docs/superpowers/specs/2026-08-02-p1f1-supervision-lifecycle-design.md`. Rule authority: parent spec §3.1, §3.5, cfg-15.

## Global Constraints

- **`restart_app` and `max_webview_mem_mb` are P2** (spec §9) — do NOT implement them. F1 does `nightly_reload` only.
- **Never block boot on a supervision-hardening failure.** Job/mutex creation error → log WARNING + continue running (unsupervised-but-alive), surfaced in telemetry. `next_fire` bad input → `None` (reload off) + `config.warn`, never panic. `--safe` must never fail to render (generic message if no last-error).
- **`--safe` still heartbeats** — the client runs so the launcher sees a live arming `--safe` child (E1 safe-mode escalation counts a failed vs a surviving `--safe`).
- **Reuse the merged interfaces:** `AppState::Safe` (P1-D1) already exists; `NavPolicy::home()` gives the reload target; `TauriSink::navigate` performs a navigation; the launcher `spawn_main(exe, config_dir, safe, pipe_name, tx) -> io::Result<Child>` already takes `safe`.
- **Windows bits `#[cfg(windows)]`** with non-Windows no-op stubs (Linux supervision = P2).
- **`nightly_reload` fires once per calendar day** by construction (`next_fire` always returns a strictly-future instant; the timer recomputes after each fire).

## Interfaces this plan uses (merged)

```rust
// kiosk-core (existing): chrono 0.4 (std/clock/serde). config::schema::Maintenance {
//   nightly_reload: Option<String>, restart_app: Option<String> (P2), timezone: Option<String>, ... }
// kiosk-main/src/cli.rs: Args { windowed: bool, config: Option<String> }  (add `safe`)
// kiosk-main/src/nav_policy.rs: NavPolicy::home(&self) -> &str
// kiosk-main/src/main.rs: TauriSink::navigate(&self, url:&str); AppHandle; WINDOW_LABEL; bundled_url(page)
// kiosk-core::app::state::AppState::Safe   (entered only via --safe; no Event transitions in)
// kiosk-launcher/src/spawn.rs: spawn_main(...) -> io::Result<Child>  (#[cfg(windows)])
// kiosk-launcher/src/sink.rs: LauncherSink (dispatch SpawnMain/SpawnSafe -> spawn_main)
```

---

### Task 1: kiosk-core `maintenance::next_fire` (host-tested)

**Files:**
- Modify: `crates/kiosk-core/Cargo.toml` (add `chrono-tz = "0.10"`)
- Create: `crates/kiosk-core/src/maintenance.rs`; modify `lib.rs` (`pub mod maintenance;`)
- Test: `crates/kiosk-core/src/maintenance.rs`

**Interfaces:**
- Produces: `fn next_fire(hhmm: &str, tz: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>>`.

- [ ] **Step 1: dep + module.** `chrono-tz = "0.10"` under `[dependencies]`; `pub mod maintenance;` in `lib.rs` (after `logging`, before `metrics`).

- [ ] **Step 2: failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(s: &str) -> DateTime<Utc> { DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc) }

    #[test]
    fn rolls_to_today_when_hhmm_is_still_ahead() {
        // 03:00 UTC now, fire 04:00 UTC → today 04:00.
        let n = next_fire("04:00", Some("UTC"), utc("2026-07-01T03:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-01T04:00:00Z"));
    }
    #[test]
    fn rolls_to_tomorrow_when_hhmm_already_passed() {
        let n = next_fire("04:00", Some("UTC"), utc("2026-07-01T05:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-02T04:00:00Z"));
    }
    #[test]
    fn applies_the_iana_zone_offset() {
        // 04:00 in Asia/Jakarta (UTC+7) = 21:00 UTC the previous day.
        let n = next_fire("04:00", Some("Asia/Jakarta"), utc("2026-07-01T00:00:00Z")).unwrap();
        assert_eq!(n, utc("2026-07-01T21:00:00Z")); // 2026-07-02 04:00 +07:00
    }
    #[test]
    fn strictly_future_and_dst_spring_forward_resolves() {
        // US/Eastern spring-forward 2026-03-08: 02:30 does not exist locally; must still return
        // a valid strictly-future instant, not panic or None.
        let now = utc("2026-03-08T06:00:00Z"); // 01:00 EST
        let n = next_fire("02:30", Some("America/New_York"), now).unwrap();
        assert!(n > now, "always strictly future");
    }
    #[test]
    fn bad_input_is_none_not_panic() {
        assert!(next_fire("nope", Some("UTC"), utc("2026-07-01T00:00:00Z")).is_none());
        assert!(next_fire("04:00", Some("Not/AZone"), utc("2026-07-01T00:00:00Z")).is_none());
        assert!(next_fire("25:00", Some("UTC"), utc("2026-07-01T00:00:00Z")).is_none());
    }
}
```

Run `cargo test -p kiosk-core maintenance::` → FAIL.

- [ ] **Step 3: implement.**

```rust
use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The next UTC instant at which local wall-clock `hhmm` ("HH:MM") occurs strictly after
/// `now`, in IANA `tz` (or system local when `None`). `None` on unparseable input.
/// DST-safe: resolves the local time through the zone, taking the earliest valid instant
/// (a spring-forward gap rolls to the next day; fall-back ambiguity takes the earlier).
pub fn next_fire(hhmm: &str, tz: Option<&str>, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let (h, m) = hhmm.split_once(':')?;
    let time = NaiveTime::from_hms_opt(h.parse().ok()?, m.parse().ok()?, 0)?;
    match tz {
        Some(name) => next_in_zone(time, name.parse::<Tz>().ok()?, now),
        None => next_in_zone_local(time, now),   // chrono::Local; see note
    }
}

fn next_in_zone<Z: TimeZone>(time: NaiveTime, zone: Z, now: DateTime<Utc>) -> Option<DateTime<Utc>>
where Z::Offset: std::fmt::Debug {
    let local_now = now.with_timezone(&zone);
    // Try today, then up to a few days forward (covers a spring-forward skipped hour).
    for add in 0..4 {
        let date = (local_now.date_naive()) + Duration::days(add);
        let naive = date.and_time(time);
        // earliest valid instant for this local wall-clock in this zone
        if let Some(dt) = zone.from_local_datetime(&naive).earliest() {
            let utc = dt.with_timezone(&Utc);
            if utc > now { return Some(utc); }
        }
    }
    None
}
```

(Provide `next_in_zone_local` using `chrono::Local` — the same logic with `Local` as the zone; note it is env-dependent so it is exercised lightly, the IANA path carries the adversarial tests. Confirm `from_local_datetime(...).earliest()` and the `Tz: FromStr` API against `chrono-tz 0.10`; adjust if the minor version differs.)

Run `cargo test -p kiosk-core maintenance::` → PASS. fmt/clippy clean, commit: `feat(core): maintenance::next_fire — DST-safe daily schedule (cfg-15)`.

---

### Task 2: `--safe` flag + Safe render (kiosk-main, Windows)

**Files:** Modify `crates/kiosk-main/src/cli.rs`, `crates/kiosk-main/src/main.rs`; create `crates/kiosk-main/bundled/safe.html`

- [ ] **Step 1: CLI.** Add `pub safe: bool` to `Args`; parse `"--safe" => args.safe = true`. Update the `parses_all_flags`/defaults tests to include it (assert `safe: false` default; a `--safe` arg sets it).

- [ ] **Step 2: bundled `safe.html`.** A minimal app-origin page: shows "Kiosk safe mode", the **device id**, and the **last error** (a `{{DEVICE_ID}}` / `{{LAST_ERROR}}` substitution the Rust side fills, or an initialization-script injection — simplest: navigate with query params `?device=...&err=...` the page reads, since it's app-origin and trusted). No remote resources. Register in `tauri.conf.json` bundle.

- [ ] **Step 3: Safe boot path.** In `main.rs`, when `args.safe`: do NOT spawn the config fetch / prober / driver-for-remote; instead navigate the webview to `bundled_url("safe.html")` with the device id (from boot) + last error (read `<data_dir>\crash-panic.txt` if present, else "unknown"), and **still spawn the heartbeat client** (so the launcher arms and safe-mode escalation works). The FSM goes to `AppState::Safe` (or is simply not driven — Safe has no Event transitions; the webview just shows safe.html). Keep the window + hardening (§7) active.

- [ ] **Step 4: Windows smoke:** `kiosk-main --safe` shows `safe.html` with the device id + last error, makes no remote request, and (under the launcher) arms the heartbeat.

- [ ] **Step 5:** fmt/clippy, commit: `feat(main): --safe renders the bundled safe page (device id + last error)`.

---

### Task 3: nightly-reload timer (kiosk-main, Windows)

**Files:** Create `crates/kiosk-main/src/maintenance.rs`; modify `crates/kiosk-main/src/main.rs`

**Interfaces:**
- Produces: `async fn run(nightly_reload: Option<String>, timezone: Option<String>, reload: impl Fn(), cancel: CancellationToken)`.

- [ ] **Step 1: the timer.** In kiosk-main `maintenance.rs`:

```rust
use kiosk_core::maintenance::next_fire;
use tokio_util::sync::CancellationToken;
// ponytail: a plain loop over next_fire; no cron lib for a single daily reload.
pub async fn run(hhmm: Option<String>, tz: Option<String>, reload: impl Fn() + Send,
                 cancel: CancellationToken) {
    let Some(hhmm) = hhmm else { return };            // None = off
    loop {
        let now = /* Utc::now() */;
        let Some(fire) = next_fire(&hhmm, tz.as_deref(), now) else { return }; // bad input -> off + (caller logs config.warn)
        let dur = (fire - now).to_std().unwrap_or_default();
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(dur) => reload(),
        }
    }
}
```

- [ ] **Step 2: wire the reload.** In `main.rs`, spawn `maintenance::run` (unless `args.safe`) with `content.maintenance.nightly_reload` + `.timezone`, and a `reload` closure that navigates the webview to `NavPolicy::home()` (a fresh load resetting page state) — reuse the `TauriSink::navigate` / `AppHandle` path. If `next_fire` returns `None` for a non-empty `nightly_reload`, emit a `config.warn{field:"maintenance.nightly_reload"}` once.

- [ ] **Step 3: Windows smoke:** set `nightly_reload` to ~1 minute out (a signed config) → the site reloads at that wall-clock minute; `null` → no reload.

- [ ] **Step 4:** fmt/clippy, commit: `feat(main): nightly-reload timer navigates home on the daily schedule`.

---

### Task 4: Job Object + single-instance mutex (kiosk-launcher, Windows)

**Files:** Create `crates/kiosk-launcher/src/job.rs`; modify `crates/kiosk-launcher/src/main.rs`, `crates/kiosk-launcher/src/sink.rs`, `crates/kiosk-launcher/src/lib.rs`

**Interfaces:**
- Produces: `struct Job` (owns the job handle) with `fn create() -> io::Result<Job>` (kill-on-close) and `fn assign(&self, child: &Child) -> io::Result<()>`; `fn acquire_single_instance() -> Option<OwnedHandle>` (the mutex; `None` if a peer already holds it).

- [ ] **Step 1: Job Object.** `job::create` → `CreateJobObjectW(None, None)`; `SetInformationJobObjectW` with `JOBOBJECT_EXTENDED_LIMIT_INFORMATION { BasicLimitInformation: { LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, ..zeroed }, ..zeroed }`. `job::assign(child)` → `AssignProcessToJobObject(job, child_handle)` (`child.as_handle()` / raw). The `Job` holds the `OwnedHandle` for the launcher's life (drop = handle close = kernel kills children). (Confirm the assign-order race: with `std::process::Command` there is a small window between spawn and assign; document it as acceptable for P1, or spawn suspended + resume after assign if the `windows` crate makes it easy — decide at impl time.)

- [ ] **Step 2: single-instance mutex.** `acquire_single_instance` → `CreateMutexW(None, TRUE, w!("Global\\kiosk-launcher"))`; if `GetLastError() == ERROR_ALREADY_EXISTS` → return `None` (a peer supervises). Return the `OwnedHandle` otherwise (held for life).

- [ ] **Step 3: wire into the launcher.** In `main.rs`: FIRST `acquire_single_instance()` — `None` → log "another kiosk-launcher is running; exiting" + `std::process::exit(0)`. Then `job::create()` (on error → WARNING + continue unsupervised). Thread the `Job` into `LauncherSink` (or the spawn path) so every `SpawnMain`/`SpawnSafe` calls `job.assign(&child)` after `spawn_main` returns (assign error → WARNING, continue). Non-Windows: stubs (`create`/`assign` = Ok no-op; `acquire` = Some).

- [ ] **Step 4: Windows smoke:** launch the launcher (it spawns main); `taskkill /PID <launcher> /F` → kiosk-main dies within ~1 s (Job Object). Start a second launcher while one runs → it logs + exits, no second main.

- [ ] **Step 5:** fmt/clippy, commit: `feat(launcher): Job Object (kill-on-close) + single-instance mutex`.

---

## Self-Review

**Spec coverage (F1 design):** Job Object + mutex → T4; `--safe` render → T2; nightly_reload (schedule + timer) → T1+T3. `restart_app`/mem-cap deferred to P2 (constraint stated); MSI/lockdown = P1-F2; touch keyboard separate. **Covered.**

**Placeholder scan:** T1 carries full runnable host code + adversarial DST tests. T2–T4 name real Win32/Tauri APIs (`CreateJobObjectW`, `AssignProcessToJobObject`, `CreateMutexW`, `TauriSink::navigate`, `AppState::Safe`) with "confirm against the crate/version" pointers resolved at impl time. The `safe.html` substitution mechanism and the `next_in_zone_local` fallback are described, not hand-waved. No invented interfaces.

**Type consistency:** `next_fire` T1 consumed by the T3 timer; `Job`/`acquire_single_instance` T4 wired in `main.rs`; `Args.safe` T2 drives the Safe boot path + gates T3's timer. `NavPolicy::home()`/`TauriSink::navigate`/`AppState::Safe`/`spawn_main` are the merged signatures.

**Scope:** One sub-project (supervision + lifecycle). Four tasks; T1 host-tested, T2–T4 Windows with smoke. MSI + autostart + §7.2 docs = P1-F2 (separate). The Job Object closes the one field failure E2 surfaced.
