# P1-D2c — Native Idle-Reset + Exit Gesture + PIN Pad Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST: Windows.** T1 (kiosk-core PIN/lockout) is fully host-testable (`cargo test -p kiosk-core`, runs on the x64 cross-toolchain — controller-side link is blocked, same as prior plans). T2–T5 are WebView2 / native-input / Tauri-IPC — Windows-host build + per-task hardware smoke.

**Goal:** Native idle-session reset (flips the dormant P1-D1 `Clearing` privacy gate live), the technician exit gesture (tap-capture + chord fallback), and the PIN pad (correct PIN → exit code 86).

**Architecture:** Security-critical, pure logic (argon2id PIN verify + persisted lockout/backoff) lives in **kiosk-core**, host-tested adversarially. Native I/O (idle timer, tap capture, `ClearProfile` execution, PIN-pad IPC, the exit) lives in **kiosk-main**, wired through the D2a event channel + `TauriSink` and D2b's `with_webview` COM pattern.

**Tech Stack:** Rust, Tauri 2.11.x, `webview2-com`, `windows` crate, `argon2` (new, kiosk-core), `kiosk-core` (`app::state` FSM, already carries the `IdleExpired`/`ProfileCleared`/`Clearing`/`ClearProfile` path).

**Design spec:** `docs/superpowers/specs/2026-07-27-p1d2c-native-input-design.md`.

## Global Constraints

- **Windows / P1 only.** Guard native code behind `#[cfg(windows)]` + a non-Windows `eprintln!` stub (as `nav.rs`/`shortcuts.rs` do). Linux/Android idle+exit are P2/P3.
- **Security core in kiosk-core, host-tested.** `verify_pin` + `Lockout` must have adversarial unit tests (backoff monotonic, survives restart, success resets, no overflow bypass). Never put PIN logic in the untestable Tauri layer.
- **No fail-open exit.** No `pin_hash` configured (neither remote `input.exit_gesture` nor bootstrap `[exit_gesture]`) → exit gesture is **DISABLED** (cfg-12), logged; the device exits only via §7.2 OS lockdown. A missing/empty hash must never grant a no-PIN exit.
- **Lockout persists** across restarts in the data dir (`resolve_data_dir()`), written with the "persist next to fsynced state" idiom (fsync + atomic rename) — SEC-05.
- **The exit is code 86** (`std::process::exit(86)`) — the sanctioned technician exit the launcher/§7.2 shell distinguishes from a crash (P1-E consumes it; D2c only emits it).
- **`ProfileCleared` is always sent** after a clear attempt, success or failure — a kiosk stranded on a failed clear is worse than a best-effort clear; a failure logs WARNING.
- **`ClearProfile`/`ProfileCleared`/`IdleExpired` already exist** in `kiosk_core::app::state` (P1-D1). Do NOT add FSM variants or change kiosk-core's FSM — D2c only produces `IdleExpired`/`ProfileCleared` events and executes `ClearProfile`.
- **Telemetry `try_send`, never panics.** Reuse existing `LogEvent` taxonomy; add a helper only if genuinely needed (a clear-failure WARNING can reuse an existing event with fields).

## D2a/D2b interfaces this plan builds on (merged)

```rust
// kiosk-main/src/main.rs
struct TauriSink { app: AppHandle, tx: mpsc::Sender<AppEvent>, refetch: Arc<Notify>,
                   telem: telemetry::Telemetry, cancel: CancellationToken }
impl EffectSink for TauriSink { fn dispatch(&mut self, effect: Effect) {...} }
//   Effect::ClearProfile{full} arm is a no-op at main.rs:173 — D2c replaces it.
//   TauriSink::navigate(&self, url:&str)  — marshals to the webview.
fn resolve_data_dir() -> PathBuf                     // %ProgramData%\kiosk
const WINDOW_LABEL: &str = "kiosk";  const APP_ORIGIN: &str = "http://tauri.localhost";
fn bundled_url(page:&str) -> String                  // "http://tauri.localhost/<page>"
// AppEvent (kiosk_core::app::state::Event): IdleExpired, ProfileCleared already exist.
// kiosk-main/src/nav_policy.rs: NavPolicy::home() -> &str (live home).

// kiosk-core::config::schema — the effective exit gesture:
//   Input.exit_gesture: Option<ExitGesture { taps:u8, region:GestureRegion,
//     min_len:Option<u8>, alphanumeric:bool, pin_hash:String }>
//   enum GestureRegion { TopLeft, TopRight, BottomLeft, BottomRight, Center }
// kiosk-core::config::bootstrap — BootstrapExitGesture { pin_hash:String, taps:u8, region... }
// kiosk-core::config::schema::Content.idle_reset_seconds: u64 (0 = off)
```

---

### Task 1: kiosk-core `exit` module — PIN verify + lockout (host-tested security core)

**Files:**
- Modify: `crates/kiosk-core/Cargo.toml` (add `argon2 = "0.5"`)
- Create: `crates/kiosk-core/src/exit.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (`pub mod exit;`)
- Test: `crates/kiosk-core/src/exit.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `fn verify_pin(pin: &str, phc: &str) -> bool`; `struct Lockout { consecutive_failures: u32, blocked_until: Option<i64> }` (serde) with `fn check(&self, now: i64) -> Gate`, `fn record_failure(&mut self, now: i64)`, `fn record_success(&mut self)`; `enum Gate { Allowed, Blocked { until: i64 } }`; consts `FREE_ATTEMPTS`, `BACKOFF_BASE_S`, `BACKOFF_CAP_S`.

- [ ] **Step 1: add dep, module decl.** `argon2 = "0.5"` under `[dependencies]`; `pub mod exit;` in `lib.rs`.

- [ ] **Step 2: failing tests for `verify_pin`.** In `exit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // A real argon2id PHC hash of the PIN "1234" (generate once with the argon2 crate;
    // paste the literal string so the test needs no RNG). Regenerate via a throwaway
    // `Argon2::default().hash_password(b"1234", &SaltString::generate(...))`.
    const PHC_1234: &str = "$argon2id$v=19$m=19456,t=2,p=1$<salt>$<hash>"; // REPLACE with a real hash

    #[test]
    fn correct_pin_verifies() { assert!(verify_pin("1234", PHC_1234)); }
    #[test]
    fn wrong_pin_rejected() { assert!(!verify_pin("9999", PHC_1234)); }
    #[test]
    fn malformed_phc_is_false_not_panic() { assert!(!verify_pin("1234", "not-a-phc")); }
    #[test]
    fn empty_pin_rejected() { assert!(!verify_pin("", PHC_1234)); }
}
```

Run `cargo test -p kiosk-core exit::` → FAIL. (Generate `PHC_1234` first: a 4-line scratch `main` using `argon2::PasswordHasher`, or reuse the `kioskctl` pattern; paste the literal.)

- [ ] **Step 3: implement `verify_pin`.**

```rust
use argon2::{Argon2, PasswordHash, PasswordVerifier};

/// Verify `pin` against a PHC-string argon2id hash. Any parse/verify failure → false
/// (never panics on attacker-influenced input); the crate's verify is constant-time.
pub fn verify_pin(pin: &str, phc: &str) -> bool {
    match PasswordHash::new(phc) {
        Ok(hash) => Argon2::default().verify_password(pin.as_bytes(), &hash).is_ok(),
        Err(_) => false,
    }
}
```

Run → PASS.

- [ ] **Step 4: failing tests for `Lockout`** (deterministic — `now` injected as unix seconds):

```rust
    #[test]
    fn allowed_until_free_attempts_exhausted() {
        let mut l = Lockout::default();
        for _ in 0..FREE_ATTEMPTS { assert!(matches!(l.check(0), Gate::Allowed)); l.record_failure(0); }
        assert!(matches!(l.check(0), Gate::Blocked { .. }), "blocks after FREE_ATTEMPTS failures");
    }
    #[test]
    fn backoff_is_monotonic_and_capped() {
        let mut l = Lockout::default();
        let mut prev = 0i64;
        for n in 0..12 {
            l.record_failure(0);
            if let Gate::Blocked { until } = l.check(0) {
                assert!(until >= prev, "backoff never shrinks (attempt {n})");
                assert!(until <= BACKOFF_CAP_S, "capped");
                prev = until;
            }
        }
    }
    #[test]
    fn block_expires_then_allows() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS { l.record_failure(100); }
        let until = match l.check(100) { Gate::Blocked { until } => until, _ => panic!("blocked") };
        assert!(matches!(l.check(until + 1), Gate::Allowed), "past the block window → allowed");
    }
    #[test]
    fn success_resets_the_counter() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS { l.record_failure(0); }
        l.record_success();
        assert!(matches!(l.check(0), Gate::Allowed), "success clears the lockout");
    }
    #[test]
    fn survives_a_restart_via_serde() {
        let mut l = Lockout::default();
        for _ in 0..=FREE_ATTEMPTS { l.record_failure(500); }
        let json = serde_json::to_string(&l).unwrap();
        let reloaded: Lockout = serde_json::from_str(&json).unwrap();
        assert!(matches!(reloaded.check(500), Gate::Blocked { .. }), "reload mid-backoff still blocks");
    }
```

Run → FAIL.

- [ ] **Step 5: implement `Lockout`.**

```rust
use serde::{Deserialize, Serialize};

pub const FREE_ATTEMPTS: u32 = 3;        // first N failures are free (fat-finger tolerance)
pub const BACKOFF_BASE_S: i64 = 5;       // then 5s, 10s, 20s, ...
pub const BACKOFF_CAP_S: i64 = 3600;     // capped at 1h

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Lockout {
    consecutive_failures: u32,
    blocked_until: Option<i64>,           // unix seconds
}

pub enum Gate { Allowed, Blocked { until: i64 } }

impl Lockout {
    pub fn check(&self, now: i64) -> Gate {
        match self.blocked_until {
            Some(until) if now < until => Gate::Blocked { until },
            _ => Gate::Allowed,
        }
    }
    pub fn record_failure(&mut self, now: i64) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures > FREE_ATTEMPTS {
            let over = self.consecutive_failures - FREE_ATTEMPTS - 1;  // 0,1,2,...
            let shift = over.min(20);                                  // guard the shl
            let wait = BACKOFF_BASE_S.saturating_mul(1i64 << shift).min(BACKOFF_CAP_S);
            self.blocked_until = Some(now.saturating_add(wait));
        }
    }
    pub fn record_success(&mut self) { self.consecutive_failures = 0; self.blocked_until = None; }
}
```

Run `cargo test -p kiosk-core exit::` → PASS.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(core): exit module — argon2id PIN verify + persisted lockout/backoff (SEC-05)"
```

---

### Task 2: `ClearProfile` execution → `ProfileCleared` (Windows — makes the privacy gate live)

**Files:**
- Modify: `crates/kiosk-main/src/main.rs` (the `TauriSink` `Effect::ClearProfile` arm) or a new `crates/kiosk-main/src/clear.rs`

**Interfaces:**
- Consumes: `TauriSink { app, tx, telem }`, WebView2 `ICoreWebView2Profile2::ClearBrowsingDataAsync`.
- Produces: sends `AppEvent::ProfileCleared` on the completion callback.

- [ ] **Step 1: implement the clear.** Replace the `Effect::ClearProfile{full}` no-op (main.rs:173). Via `with_webview` → `CoreWebView2()?.Profile()?` cast to `ICoreWebView2Profile2` → `ClearBrowsingDataAsync(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE, handler)` (the `_ALL_PROFILE` kind covers cookies + DOM storage + autofill/Web-Data + password/Login-Data, M5 — confirm the exact enum in `webview2-com-sys`). In the completion `ClearBrowsingDataCompletedHandler`, `tx.blocking_send`/`try_send(AppEvent::ProfileCleared)`. A cast/call failure → still send `ProfileCleared` + `telem` WARNING (never strand the kiosk).

- [ ] **Step 2: gate a stray double-clear.** `ClearProfile` is only emitted on entry to `Clearing` (P1-D1 rule 9); no de-dup needed in D2c, but confirm the handler sends `ProfileCleared` exactly once per clear (don't subscribe a handler per dispatch that leaks — build the handler inline per call, as D2a/D2b COM handlers do).

- [ ] **Step 3: Windows smoke (record in report).** Set a cookie / autofill entry on the site; force an idle reset with `clear_data_on_reset:true` (drive it via T3, or a temp manual `IdleExpired`): confirm (a) the screen does NOT flash home before the clear completes (the `Clearing` gate holds — the P1-D1 privacy property, now with a real clear), and (b) the cookie/autofill entry is gone after re-navigation.

- [ ] **Step 4: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): execute ClearProfile via WebView2, release the Clearing gate on ProfileCleared"
```

---

### Task 3: Native idle timer → `IdleExpired` (Windows + host-tested latch)

**Files:**
- Create: `crates/kiosk-main/src/idle.rs`; modify `main.rs` (spawn it)

**Interfaces:**
- Produces: `fn should_fire(idle_secs: u64, threshold: u64, already_fired: bool) -> bool` (pure/host-tested); `fn run(threshold: u64, tx: mpsc::Sender<AppEvent>, cancel: CancellationToken)` (Windows).

- [ ] **Step 1: host-test the latch.** `should_fire` fires once when idle crosses the threshold and re-arms only after activity resumes:

```rust
#[test] fn fires_once_when_idle_exceeds_threshold() {
    assert!(should_fire(200, 180, false));           // crossed, not yet fired → fire
    assert!(!should_fire(200, 180, true));           // already fired this episode → no repeat
    assert!(!should_fire(10, 180, false));           // below threshold → no
}
#[test] fn threshold_zero_never_fires() { assert!(!should_fire(9999, 0, false)); } // 0 = off
```

Impl: `threshold != 0 && idle_secs >= threshold && !already_fired`. Run → PASS.

- [ ] **Step 2: Windows idle loop.** `run`: every 1 s, `GetLastInputInfo` → `idle_secs = (GetTickCount64() - dwTime)/1000`; track `already_fired`; when `should_fire` → `tx.try_send(AppEvent::IdleExpired)` and set the latch; when `idle_secs < threshold` clear the latch (activity resumed → re-arm). `cancel`-aware `tokio::select!` on a 1 s interval. Non-Windows stub logs and returns.

- [ ] **Step 3: spawn in `main.rs`** with `content.idle_reset_seconds` (read from the booted config) and the shared `tx` + `cancel`. (The FSM no-ops `IdleExpired` unless `Online`, so emit unconditionally — no state check here.)

- [ ] **Step 4: Windows smoke.** Set `idle_reset_seconds:20`; leave the kiosk untouched → after ~20 s an idle reset happens (home reload, and profile clear if `clear_data_on_reset`); touching input before 20 s does not trigger it.

- [ ] **Step 5: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): native idle timer emitting IdleExpired (GetLastInputInfo)"
```

---

### Task 4: Exit-gesture triggers — tap capture + technician chord (Windows + host-tested geometry)

**Files:**
- Create: `crates/kiosk-main/src/gesture.rs`; modify `main.rs`/`shortcuts.rs` (chord)

**Interfaces:**
- Produces: `fn in_region(x:f64, y:f64, w:f64, h:f64, region: GestureRegion) -> bool` (pure); `struct TapCounter { taps_needed, window_ms, hits: Vec<i64> }` with `fn tap(&mut self, now_ms:i64) -> bool` (true when N taps land within a rolling window) — pure/host-tested. `fn open_pin_pad(app:&AppHandle)`.

- [ ] **Step 1: host-test geometry + rolling window.**

```rust
#[test] fn top_left_quadrant() {
    assert!(in_region(10.0, 10.0, 1000.0, 800.0, GestureRegion::TopLeft));
    assert!(!in_region(900.0, 10.0, 1000.0, 800.0, GestureRegion::TopLeft));
}
#[test] fn rolling_window_not_anchored_to_first_tap() {   // the P0 first-tap-anchor bug
    let mut c = TapCounter::new(3, 1000);
    assert!(!c.tap(0));  assert!(!c.tap(2000)); // first tap fell out of the window
    assert!(!c.tap(2200)); assert!(c.tap(2400), "3 taps within the rolling 1000ms window fire");
}
```

Impl `TapCounter::tap`: push `now_ms`, retain only hits within `window_ms` of `now_ms`, return `hits.len() >= taps_needed` (and clear on fire). `in_region`: quadrant/centre split of `w`×`h`. Run → PASS.

- [ ] **Step 2: native tap capture (Windows).** In `gesture.rs`, via the same input layer as the D2b keyboard hook / P0 spike, observe pointer-down events with window-relative coords → `in_region(x,y,w,h, cfg.region)` → `TapCounter::tap(now_ms)` → on true `open_pin_pad(&app)`. Reads the effective gesture (Step 4). Window size from the Tauri window.

- [ ] **Step 3: technician chord fallback.** Add a reserved combo (e.g. Ctrl+Alt+Shift+K) to the D2b `AcceleratorKeyPressed` handler — matched and **NOT** swallowed by `should_swallow`; instead → `open_pin_pad(&app)`. Comment: this is the fallback for the P0-unconfirmed tap path (spec §3.5), so a locked device is never unexitable.

- [ ] **Step 4: effective-gesture resolution helper** (pure, host-tested): `fn effective_gesture(remote: Option<&ExitGesture>, bootstrap: Option<&BootstrapExitGesture>) -> Option<EffectiveGesture>` — remote wins, else bootstrap, else `None` (gesture disabled, cfg-12). `EffectiveGesture { taps, region, pin_hash, min_len, alphanumeric }`. Test: remote-present → remote; only-bootstrap → bootstrap; neither → None.

- [ ] **Step 5: `open_pin_pad`.** Navigate the webview to `bundled_url("pinpad.html")` (app-origin → full IPC per §3.6). Guard: if `effective_gesture` is `None`, do nothing (disabled).

- [ ] **Step 6: Windows smoke.** `taps` taps in the configured corner opens the pad; taps elsewhere do not; the chord opens the pad (record whether tap-capture worked over the focused webview — the P0-unconfirmed question).

- [ ] **Step 7: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): exit gesture — tap capture + technician chord open the PIN pad"
```

---

### Task 5: PIN pad page + `verify_pin` IPC + exit 86 (Windows + bundled + host-tested decision)

**Files:**
- Create: `crates/kiosk-main/bundled/pinpad.html`, `crates/kiosk-main/src/pinpad.rs` (the Tauri command + lockout persistence); modify `main.rs` (register the command)

**Interfaces:**
- Produces: Tauri command `verify_pin(pin: String) -> PinResult`; `enum PinResult { Ok, Blocked { until: i64 }, Rejected }`; `fn adjudicate(lockout:&mut Lockout, pin:&str, phc:&str, now:i64) -> PinResult` (pure/host-tested).
- Consumes: `kiosk_core::exit::{verify_pin, Lockout, Gate}`, `resolve_data_dir()`, the effective `pin_hash`.

- [ ] **Step 1: host-test the pure adjudication.** In `pinpad.rs`:

```rust
#[test] fn correct_pin_when_allowed_is_ok() {
    let mut l = Lockout::default();
    assert!(matches!(adjudicate(&mut l, "1234", PHC_1234, 0), PinResult::Ok));
}
#[test] fn wrong_pin_records_failure_and_rejects() {
    let mut l = Lockout::default();
    assert!(matches!(adjudicate(&mut l, "0000", PHC_1234, 0), PinResult::Rejected));
}
#[test] fn blocked_pin_does_not_even_check_hash() {
    let mut l = Lockout::default();
    for _ in 0..=kiosk_core::exit::FREE_ATTEMPTS { l.record_failure(0); }
    assert!(matches!(adjudicate(&mut l, "1234", PHC_1234, 0), PinResult::Blocked { .. }),
            "a correct PIN during lockout is still blocked");
}
```

(`PHC_1234` = the same literal hash as Task 1.) `adjudicate`: `match lockout.check(now) { Blocked{until} => Blocked{until}, Allowed => if verify_pin(pin,phc) { record_success; Ok } else { record_failure(now); Rejected } }`. Run → PASS.

- [ ] **Step 2: the Tauri command + persistence.** `verify_pin(pin)`: resolve the effective `pin_hash` (Task-4 helper; `None` → the command is never reachable because the pad never opens, but return `Rejected` defensively); load `Lockout` from `<data_dir>/exit-lockout.json` (default if absent); `now = SystemTime unix secs`; `let r = adjudicate(&mut lockout, &pin, &phc, now)`; persist `lockout` (fsync + atomic rename — reuse the spool's write idiom); on `PinResult::Ok` → `std::process::exit(86)` (after the persist); else return `r` to the page.

- [ ] **Step 3: bundled `pinpad.html`.** Minimal app-origin page: a numeric keypad, calls `window.__TAURI__.core.invoke('verify_pin', {pin})`, shows `Rejected`/`Blocked until …` on the result, exits (window closes) on `Ok` (the process is already exiting). No styling beyond functional — D2d polishes. Register in `tauri.conf.json` bundle + the command in the Tauri builder `invoke_handler`.

- [ ] **Step 4: register the command** in `main.rs` (`tauri::generate_handler![pinpad::verify_pin]`), threading the effective `pin_hash` + `data_dir` into the command's state (Tauri `manage`).

- [ ] **Step 5: Windows smoke.** Open the pad (T4). Correct PIN → process exits with code 86 (`echo %ERRORLEVEL%` = 86). Wrong PIN × (FREE_ATTEMPTS+1) → escalating "blocked until" that **survives killing + relaunching** the process (reload reads `exit-lockout.json`). No `pin_hash` in config → gesture never opens the pad.

- [ ] **Step 6: fmt, clippy, commit.**

```bash
git add -A && git commit -m "feat(main): PIN pad + verify_pin IPC with persisted lockout, exit code 86"
```

---

## Self-Review

**Spec coverage (design doc):** idle timer → IdleExpired → T3; ClearProfile exec + ProfileCleared (gate live) → T2; exit gesture tap+chord → T4; PIN pad + exit 86 → T5; argon2id verify + persisted lockout (SEC-05) → T1; no-pin_hash-disabled (cfg-12) → T4 Step 4 + T5 Step 2; exit-code-86 → T5. **Covered.** Deferred: D2d polished pinpad/assets (T5 ships minimal), D2e, Linux/Android, P1-E's handling of code 86.

**Placeholder scan:** `PHC_1234` is explicitly flagged to be generated + pasted as a real literal (a fixture, not a placeholder-in-code); COM calls name real WebView2 interfaces (`ICoreWebView2Profile2::ClearBrowsingDataAsync`, `_ALL_PROFILE`) with "confirm exact enum in webview2-com-sys" pointers, resolved at impl time as prior plans did. Every host-testable task carries runnable test code.

**Type consistency:** `Lockout`/`Gate`/`verify_pin`/`FREE_ATTEMPTS` defined T1, consumed T5. `PinResult`/`adjudicate` T5. `in_region`/`TapCounter`/`effective_gesture`/`open_pin_pad` T4. `AppEvent::{IdleExpired,ProfileCleared}` are P1-D1 existing. `should_fire` T3. All the pure host-tested seams are named where produced and referenced consistently.

**Scope:** One sub-project (native input). Five tasks: T1 the host-tested security core; T2–T5 native with per-task hardware smoke. Each is an independent reviewer gate.
