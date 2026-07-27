# P1-D2c — Native Idle-Reset + Exit Gesture + PIN Pad (Design)

> Sub-project of P1-D2 (the `kiosk-main` Tauri app). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.5, §7 input rows,
> §8/SEC-05. Builds on P1-D2a (webview, driver, event channel, `NavPolicy`) and P1-D1
> (the FSM's dormant `IdleExpired`/`ProfileCleared`/`Clearing`/`ClearProfile` path).

**Status:** approved 2026-07-27 (design). Executes on a Windows host (native input + WebView2
profile-clear); the plan is authored on Linux, the kiosk-core pieces are host-testable, the
native pieces are Windows-host + hardware smoke.

## Goal

The last kiosk-behavior piece: native idle-session reset — which flips the P1-D1 `Clearing`
privacy gate from dormant to live — plus the technician exit gesture and the PIN pad
(correct PIN → exit code 86). Windows / P1.

## Layering

- **kiosk-core (pure, host-tested, security-critical → adversarial tests):** PIN verification
  (argon2id against the PHC `pin_hash`) and the lockout/backoff state machine.
- **kiosk-main (native I/O):** the idle timer, tap capture + chord fallback, the
  `ClearProfile` execution, the PIN-pad page + IPC, lockout persistence, and the exit.

Layering rule holds: no per-OS API or I/O in kiosk-core; the security logic that must be
correct-by-test lives where it can be tested on any host.

## Components

### Idle timer (kiosk-main)
Poll the Windows last-input timestamp (`GetLastInputInfo`) ~every 1 s; when idle ≥
`content.idle_reset_seconds` (0 = off — never arm), send `AppEvent::IdleExpired` into the
D2a event channel, then reset the local "already fired" latch so it fires once per idle
episode, not every poll. The FSM (P1-D1) owns everything after: it decides clear-vs-no-clear
and gates re-display. Idle reset is only meaningful while `Online`; the FSM already no-ops
`IdleExpired` from other states, so the timer emits unconditionally and the FSM filters.

### ClearProfile execution (kiosk-main) — makes the privacy gate live
On the `Clearing` gate the FSM emits `Effect::ClearProfile{full:true}`. D2a's `TauriSink`
currently logs-and-no-ops it. D2c wires it: WebView2 `Profile.ClearBrowsingDataAsync` over
the full profile — cookies, web storage, IndexedDB, **and** the autofill/Web-Data and
Login-Data stores (M5) — then, in the async completion callback, send
`AppEvent::ProfileCleared` into the channel. That release is what lets the FSM navigate home
over the freshly-cleared profile (P1-D1 rule 9). Until the callback fires the gate holds and
nothing re-displays — the privacy property proven in P1-D1 now has a real async clear behind
it. A clear failure still sends `ProfileCleared` (a kiosk stuck black on a failed clear is
worse than a best-effort clear) but logs a WARNING.

### Exit gesture (kiosk-main) — tap capture + chord fallback
Two parallel triggers, both opening the PIN pad:
1. **Native tap capture** — count OS pointer/touch taps landing in the configured
   `exit_gesture.region` (top-left/… /center), reached via `with_webview` at the same input
   layer as the P0/D2b keyboard hook. After `taps` genuine taps within a rolling window →
   open the PIN pad. (P0 tap-window caveat: anchor to a **rolling** window, not the first
   tap — the P0 spike's first-tap anchor could miss slow taps.)
2. **Reserved technician chord** — a fixed `AcceleratorKeyPressed` combo (added to the D2b
   handler, NOT swallowed) → open the PIN pad.

Both exist because **P0 never hardware-confirmed pointer capture over a focused WebView2**
(P0-T5 was PARTIAL). Spec §3.5 mandates the fallback so a locked device is never unexitable.
Neither is a security boundary — §7.2 OS lockdown is; these are the *sanctioned exit*, and
the PIN is the gate.

### PIN pad (kiosk-main + bundled)
A bundled app-origin `pinpad.html` (spec §3.6: bundled pages keep full Tauri IPC, unlike
remote origins). The trigger navigates the webview to it (or overlays it). It calls a
`verify_pin` Tauri command; on success the process exits with **code 86** (the launcher/
§7.2 shell treats 86 as an intentional technician exit, not a crash). On failure the pad
shows the lockout state. D2c ships a minimal functional pad; D2d polishes it.

### PIN verify + lockout (kiosk-core, pure, host-tested)
- `verify_pin(pin: &str, phc: &str) -> bool` — argon2id verify against the PHC-string
  `pin_hash` (add the `argon2` crate to kiosk-core). Constant-time compare via the crate.
- `Lockout` — an attempt counter with exponential backoff (e.g. after K failures, block for
  `base * 2^(n-K)` up to a cap), **persisted across restarts** in the data dir (SEC-05: the
  offline-crack risk of a short PIN is mitigated by throttling + a non-fleet-readable
  per-device hash, not argon2 alone). Pure state machine: `Lockout::check(now) ->
  Allowed | BlockedUntil(t)`, `record_failure(now)`, `record_success()`. kiosk-main persists
  it (same "persist next to fsynced state" idiom as the spool) and supplies `now` (monotonic
  where possible; a wall-clock is acceptable here — an attacker who can roll the clock back
  already has data-dir write, a higher-tier compromise). Adversarial host tests: backoff is
  monotonic, survives a restart (reload mid-backoff still blocks), a success resets it, and no
  bypass via counter overflow.

## Data flow

- **Idle → clear → home:** idle ≥ threshold → `IdleExpired` → FSM (rule 9, `clear_data_on_reset`
  true) → `Clearing` + `ClearProfile{full:true}` → D2c clears the profile → `ProfileCleared`
  → FSM navigates home over the clean profile. (`clear_data_on_reset` false → FSM reloads home
  directly, no clear.)
- **Exit:** tap/chord → PIN pad → `verify_pin` (gated by `Lockout`) → success → exit 86;
  failure → `Lockout.record_failure` → pad shows backoff.

## Error handling

- Clear failure → still `ProfileCleared` (don't strand the kiosk) + WARNING telemetry.
- No `pin_hash` configured (neither bootstrap `[exit_gesture]` nor remote) → exit gesture is
  **disabled** (cfg-12), logged; the device is exitable only via §7.2 OS lockdown. Do not
  fall open to a no-PIN exit.
- PIN pad dismissed / wrong PIN → return to content; `Lockout` persists the failure.
- Telemetry (`try_send`, never panics): a `focus`/exit-attempt event is optional — keep D2c
  to the existing taxonomy, no new events unless the FSM/exit needs one.

## Testing

- **Host-testable (kiosk-core):** `verify_pin` (correct/incorrect/malformed PHC), `Lockout`
  (backoff monotonic, restart-survival, success-reset, overflow-safe) — adversarial, the
  security core.
- **Windows-host + hardware smoke:** idle timer fires `IdleExpired` after the threshold;
  `ClearProfile` actually clears (a set cookie/autofill entry is gone after an idle reset);
  the `Clearing` gate holds (no home flash before the clear completes); tap gesture opens the
  pad (**and/or** the chord if tap-capture proves unreliable — record which); correct PIN →
  process exits 86; wrong PINs trigger escalating lockout that survives a restart.

## Scope / defer

D2c = Windows native input only. Deferred: **D2d** polished pinpad/splash/error assets (D2c
ships a minimal functional `pinpad.html`); **D2e** panic-hook richness / `health.sample` /
`display.monitor`. Linux/Android exit-gesture + idle are P2/P3. The launcher's handling of
exit code 86 is P1-E (the watchdog) — D2c just emits it.
