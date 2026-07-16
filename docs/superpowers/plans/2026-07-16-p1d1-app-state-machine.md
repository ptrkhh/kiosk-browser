# P1-D1 — `kiosk-core`: App State Machine

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the app state machine — the pure decision core that drives the kiosk's visible state (`Boot → ConfigLoad → Online ⇄ Offline → ErrorPage`, plus idle-reset gating) from events, emitting effects that the `kiosk-main` webview layer (a later plan) executes.

**Architecture:** A pure Mealy machine in `crates/kiosk-core/src/app/state.rs`: `(state, event) → (state, Vec<Effect>)`. No Tauri, no webview, no timer, no HTTP — exactly like the connectivity prober (P1-C), which put the pure FSM in `kiosk-core` and left the impure driving to `kiosk-main`. The state machine receives events (config applied, prober flipped, navigation committed/failed, countdown expired, idle expired, profile-clear completed) and returns effects (navigate, show video, show error page, show splash, clear profile). The timer, the actual navigation, and the profile clear all live in `kiosk-main` (P1-D2); this plan is the brain, fully host-testable.

**Why pure/host-testable matters here:** this FSM is the arbiter of what a locked, unattended device shows 24/7. A wrong transition means a kiosk stuck on the offline video while the site is up, or flashing an error page in a loop. A pure `(state, event) → (state, effects)` function can be exhaustively tabled and tested in WSL with no Windows, no webview, no flakiness — which is the only way to trust it. P1-D2 (the Tauri app) is a **separate plan** that needs the Windows host and a drive-the-real-app verification model; do not pull its webview/hardening/exit-gesture work into this one.

**Tech Stack:** Rust stable; `serde` (already present). No new dependency.

## Global Constraints

- Spec of record: `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` — **§3.3** (the state-machine diagram + prober/error rules), **§3.4** (offline video), **§3.5** (idle reset gating), **§4** (layering). **On conflict, the spec wins over this plan.**
- **⚠️ This plan's sample code is a STARTING POINT, NOT GOSPEL.** Across P1-A/B/C, *eight+* genuine defects were bugs in the plan's own sample code and fixtures — a `debug_assert` that vanished in release, a clock parser that accepted `EST` as −5h, two test fixtures concealing live secret leaks, a compose seam that reopened a closed hole. **If the code here looks wrong, it may well be wrong. Say so and stop.** Reviewers: a defect is a defect even when the plan mandated it.
- **Layering rule (spec §4): `kiosk-core` must NEVER depend on Tauri or any per-OS API.** This FSM takes events and returns effects; it does not act.
- **This FSM is in RAM by design and must NOT be persisted.** The recurring failure mode this project keeps producing — *state that should survive a crash but doesn't* (found 6× in P1-A/B) — does **not** apply here: a fresh process re-boots through `Boot → ConfigLoad` from scratch, re-reads config from the store (P1-A), and re-probes (P1-C). There is no prior visible-state to restore. Inventing persistence here would be wrong. (The anti-rollback revision floor and the telemetry spool are the things that persist; they already do.)
- Rust stable, edition 2021. Lint gates before every commit: `cargo fmt --check`, `cargo clippy -p kiosk-core --all-targets -- -D warnings`, `cargo test -p kiosk-core`.
- **NEVER run `cargo test --workspace`** — it rebuilds Tauri and takes >8 minutes; agents have hung on it and lost work. **Run cargo commands SERIALLY** — concurrent runs deadlock on the build-directory lock. Use `cargo test -p kiosk-core app::`.
- **Commit early once GREEN.** Agents in this project have been cut off by session limits / API errors mid-task and lost uncommitted work. Get to a committed green state, then polish.
- **Never report a test or lint result you have not actually seen.** Verify committed files from **`git ls-tree`**, not the working tree — a `*.pem` gitignore rule once silently excluded a fixture the code depended on, so a green local run proved nothing.
- **Honesty about tests:** twice in this project a test whose name claimed a property it never checked was *concealing a live bug*. For each transition rule, **verify the test FAILS against a deliberately broken transition table** (e.g. one that ignores `fallback`, or flips on the wrong event). Report which you verified this way.
- Commit: conventional prefix + trailer `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. Repo-local git identity is configured — just `git commit`; do NOT pass `-c user.email=...`. Stage explicit paths; never `git add -A`. Nothing under `.superpowers/` or `FROM_GH/`.

## What exists already (consume, do not reimplement)

- `kiosk_core::config::schema::{Content, Fallback}` — `content.url: Option<String>`, `content.fallback: Fallback` (`Video | ErrorPage`), `content.error_max_retries: u32` (default 5), `content.idle_reset_seconds: u64` (0 = off), `content.clear_data_on_reset: bool`.
- `kiosk_core::config::ConfigManager` — `home_url()` gives the effective navigation URL (`content.url` templated, or the bootstrap url). The FSM receives the URL to navigate to; it does not compute it.
- `kiosk_core::net::prober::Link` — `Online | Offline`. The FSM receives a `Link` flip as an event.
- `kiosk_core::logging::event::Event` — `NetOnline`, `NetOffline`, `NavError`, `ConfigApplied`, `ConfigError` exist. **Emitting them is `kiosk-main`'s job (P1-D2); this FSM only decides.**

## The state machine (spec §3.3 + §3.5), stated precisely

**States:**
- `Boot` — initial, before any config.
- `ConfigLoad` — attempting to load config (cached last-good or bootstrap).
- `Online { url }` — showing the content site.
- `Offline` — showing the looping offline video.
- `ErrorPage { attempts, ... }` — showing the bundled error page with a retry countdown.
- `Clearing { next: Box<AppState> }` — profile clear in flight after an idle reset; re-display is gated until it completes (spec §3.5: "the state machine gates re-display until the clear completes").
- `Safe` — entered only out-of-band via `kiosk-main --safe`; NOT reachable by any normal transition. Model it as a state the FSM can be constructed into, but no `Event` transitions *into* it.

**Events:**
- `ConfigApplied { url: String }` — config loaded/applied, navigate here.
- `ConfigUnavailable` — no cached config and no network (spec §3.3: "no cached config + no network → Offline(video), keep trying").
- `LinkChanged(Link)` — the prober flipped (from P1-C; only fires on a flip).
- `NavigationCommitted` — the webview successfully loaded the current target.
- `NavigationFailed` — the current site failed to load (DNS/TLS/HTTP error) while the link is believed up.
- `CountdownExpired` — the ErrorPage retry countdown elapsed; the caller has re-attempted navigation and reports the result via the *next* `NavigationCommitted`/`NavigationFailed`. (See the ErrorPage note below — decide and document how retry result is delivered.)
- `IdleExpired` — the native idle timer fired (spec §3.5).
- `ProfileCleared` — the async profile clear finished (spec §3.5 "some clears are asynchronous").
- `Reconnected` — network returned; triggers a config refetch (spec §3.3 "Reconnect triggers immediate config refetch, then navigation back to the site"). Model the effect; the refetch itself is `kiosk-main`'s.

**Effects** (what `kiosk-main` executes):
- `Navigate(String)` — navigate the webview to this URL.
- `ShowVideo` — display the offline video loop.
- `ShowErrorPage { retry_after_seconds: u64 }` — display the bundled error page armed to retry after N seconds.
- `ShowSplash` — the bundled boot splash.
- `ClearProfile { full: bool }` — clear cookies/storage (and autofill/login stores if `full`).
- `RefetchConfig` — kick a config refetch (on reconnect).

**The transition rules — every one is load-bearing (spec §3.3):**

1. `Boot` + `ConfigApplied{url}` → `Online{url}`, effect `Navigate(url)`.
2. `Boot`/`ConfigLoad` + `ConfigUnavailable` → `Offline`, effect `ShowVideo`. (Keep probing; a later `LinkChanged(Online)` does NOT by itself leave Offline — see rule 8.)
3. `Online` + `LinkChanged(Offline)` → `Offline`, effect `ShowVideo`.
4. `Offline` + `LinkChanged(Online)` → **re-navigate home**: `Online{url}`, effect `Navigate(url)`. (The FSM must carry the last-known home url so it can re-navigate on reconnect — decide how; see the "carrying the url" note.)
5. `Online` + `NavigationFailed` + `fallback == Video` → `Offline`, effect `ShowVideo`.
6. `Online` + `NavigationFailed` + `fallback == ErrorPage` → `ErrorPage{attempts: 1}`, effect `ShowErrorPage{retry_after_seconds}`.
7. `ErrorPage` behavior (spec §3.3 sub-diagram):
   - countdown expiry, retry OK (`NavigationCommitted`) → `Online{url}`.
   - countdown expiry, retry fails (`NavigationFailed`) → `ErrorPage{attempts+1}`, re-arm `ShowErrorPage`.
   - `attempts >= error_max_retries` → `Offline`, effect `ShowVideo`.
   - `LinkChanged(Offline)` while in `ErrorPage` → `Offline` immediately, effect `ShowVideo` (spec: "a prober-offline flip transitions to Offline(video) immediately").
8. **Damping is the prober's job, not the FSM's.** `LinkChanged` only ever fires on an actual flip (P1-C guarantees `record()` returns `Some` only on a flip). So the FSM treats every `LinkChanged` as a real, debounced transition — it must NOT add its own hysteresis.
9. Idle reset (spec §3.5): any *visible* state + `IdleExpired` (when `idle_reset_seconds > 0`) →
   - if `clear_data_on_reset`: → `Clearing{ next: Online{home} }`, effects `[ClearProfile{full:true}, ...]`; on `ProfileCleared` → `Online{home}`, effect `Navigate(home)`. Re-display is gated: while `Clearing`, no content is shown (show splash or hold the current frame — decide and document).
   - if not `clear_data_on_reset`: → `Online{home}` directly, effect `Navigate(home)` (return home without clearing).
   - `idle_reset_seconds == 0` means the caller never sends `IdleExpired`; the FSM needs no special case, but a defensive `IdleExpired` in a state where idle-reset is off should be a no-op, not a panic.
10. Reconnect (spec §3.3): `Offline` + `Reconnected` → effect `RefetchConfig` (state stays `Offline` until the refetch yields a `ConfigApplied`, then rule 1-style navigate). Decide whether `Reconnected` and `LinkChanged(Online)` are the same event or distinct — the spec describes reconnect as "immediate config refetch, then navigation" — argue your modeling.

**Decide and document (do not guess):**
- **Carrying the url.** Rules 3/4/9 require re-navigating home after Offline/idle. The FSM must remember the last home url. Options: store it on the `Offline`/`ErrorPage` variants, or hold a `home: Option<String>` on the machine struct updated on every `ConfigApplied`. Pick one, argue it. A machine that reaches `Offline` and then can't re-navigate on reconnect is a stuck kiosk.
- **ErrorPage retry delivery.** The spec's sub-diagram couples "countdown expiry" with "retry OK/fails". Model how the retry result reaches the FSM: does `CountdownExpired` emit a `Navigate` effect (retry) and then the result arrives as `NavigationCommitted`/`NavigationFailed`? Or does `CountdownExpired` carry the result? The former is cleaner (the FSM emits the retry navigate, the webview reports back) — but decide and make the tests match the model.

---

### Task 1: State/event/effect types + the core connectivity transitions (rules 1–5, 8)

**Files:**
- Create: `crates/kiosk-core/src/app/mod.rs`
- Create: `crates/kiosk-core/src/app/state.rs`
- Modify: `crates/kiosk-core/src/lib.rs` (add `pub mod app;`)

**Interfaces produced:**
- `app::state::{AppState, Event, Effect, Machine}`.
- `Machine::new(cfg: MachineConfig) -> Machine` where `MachineConfig` carries `fallback: Fallback`, `error_max_retries: u32`, `idle_clear: bool`, `error_retry_seconds: u64` (the countdown length — from `network`/a sensible default; decide the source and document).
- `Machine::state(&self) -> &AppState`.
- `Machine::on(&mut self, event: Event) -> Vec<Effect>` — the transition function. Returns the effects to execute; mutates the state.

- [ ] **Step 1: Write the transition tests** for rules 1–5 and 8: Boot+ConfigApplied→Online+Navigate; Boot+ConfigUnavailable→Offline+ShowVideo; Online+LinkChanged(Offline)→Offline+ShowVideo; Offline+LinkChanged(Online)→Online+Navigate(home); Online+NavigationFailed with `fallback:Video`→Offline; and a test that `LinkChanged` is treated as already-debounced (two `LinkChanged(Offline)` in a row don't double-fire an effect from a state already Offline — a `LinkChanged` to the state you're already in is a no-op).
- [ ] **Step 2: RED** — `cargo test -p kiosk-core app::state`.
- [ ] **Step 3: Implement** the types and the rule 1–5/8 transitions. **Commit here once green** (before adding the ErrorPage/idle sub-machines) so a session cutoff can't lose it.
- [ ] **Step 4: GREEN + attack.** Verify a broken transition table (e.g. one that ignores `fallback` and always goes to ErrorPage) makes the fallback:Video test RED. Report it.
- [ ] **Step 5: fmt, clippy (serially), commit.**

```
feat(core): app state machine — core connectivity transitions
```

---

### Task 2: The ErrorPage sub-machine (rules 6, 7)

**Files:**
- Modify: `crates/kiosk-core/src/app/state.rs`

- [ ] TDD the ErrorPage rules: Online+NavigationFailed with `fallback:ErrorPage`→ErrorPage{1}+ShowErrorPage; countdown-expiry retry OK→Online; retry fail→ErrorPage{attempts+1} re-armed; `attempts >= error_max_retries`→Offline+ShowVideo; `LinkChanged(Offline)` in ErrorPage→Offline immediately.
- [ ] **The boundary test that matters:** with `error_max_retries = 5`, exactly 5 failed attempts stay on ErrorPage and the 5th→6th falls to Offline (or however the spec's `attempts >= error_max_retries` reads — pin the exact boundary; the off-by-one here is a kiosk stuck retrying forever vs. giving up one try early). Verify it fails against a `>` vs `>=` mutation.
- [ ] RED → implement → GREEN → verify the boundary mutation → fmt/clippy → commit.

```
feat(core): app state machine — ErrorPage retry sub-machine
```

---

### Task 3: Idle-reset gating + reconnect (rules 9, 10)

**Files:**
- Modify: `crates/kiosk-core/src/app/state.rs`

- [ ] TDD the idle-reset rules: IdleExpired with `clear_data_on_reset:true`→Clearing + ClearProfile{full:true}, re-display gated; ProfileCleared→Online{home}+Navigate; IdleExpired with `clear_data_on_reset:false`→Online{home}+Navigate directly (no clear); IdleExpired when idle-reset is off is a no-op (no panic); Offline+Reconnected→RefetchConfig.
- [ ] **The gating test that matters (spec §3.5):** while in `Clearing`, an event that would normally show content does NOT re-display until `ProfileCleared` arrives — i.e. the clear genuinely gates re-display. Verify this fails against an implementation that navigates home before the clear completes (which would show the *new* session over un-cleared data — the exact privacy leak §3.5 exists to prevent).
- [ ] RED → implement → GREEN → verify the gating mutation → fmt/clippy → commit.

```
feat(core): app state machine — idle-reset gating and reconnect
```

---

## Plan self-review (run at write time)

**Spec coverage.** §3.3 core transitions (Boot/ConfigLoad/Online/Offline, prober flips, reconnect) → T1+T3. §3.3 ErrorPage sub-diagram + `error_max_retries` → T2. §3.5 idle reset + async-clear gating → T3. Damping-is-the-prober's-job (no double hysteresis) → T1.

**Deliberately out of scope — this is P1-D2 (the Tauri app, a SEPARATE plan needing the Windows host):**
- The actual webview, the navigation intercept wiring to `nav::decide` (P1-C), and every §7 webview-hardening control.
- The native idle timer, the exit gesture / tap capture (proven in the P0 gate), the PIN pad, and the profile-clear platform calls — this FSM only emits `ClearProfile`/`IdleExpired` semantics.
- Offline video rendering (`offline.html`), the boot splash, the error page HTML.
- The HTTP config fetch, the prober timer, the dedicated logger thread, the panic hook, `health.sample` content, and the `display.monitor` topology check.
- `nightly_reload` / `restart_app` timers — a maintenance timer that emits a navigate/reload; it does not change these FSM states, so it lives with the P1-D2 driver.
- **P1-D2 MUST use `nav::decide`** (P1-C), constrain `content.url` to http(s) in `validate.rs`, and reject wildcard-host allowlist patterns — carried forward from the P1-C review, recorded in the ledger.

**Known risks flagged, not hidden:**
- **Two modeling decisions (carrying the url; ErrorPage retry delivery) are genuine judgment calls**, flagged above for the implementer to decide and the reviewer to scrutinize — not buried.
- The `Clearing` gating is the one privacy-relevant transition; T3 pins it with a mutation test.

## Follow-on plans (in order)

1. **P1-D2 — the `kiosk-main` Tauri app** (needs the Windows host; drive-the-real-app verification): wire this FSM to a real webview, the §7 hardening set, `nav::decide`, offline video, the native idle timer + exit gesture + PIN pad, the HTTP fetch + prober timer, the logger thread, the panic hook, `health.sample`, the `display.monitor` check.
2. **P1-E — `kiosk-launcher` watchdog:** heartbeat, READY arming, liveness disambiguation, backoff, safe mode, orphaned-spool drain.
3. **P1-F — packaging:** WiX MSI (Authenticode-signed, credential ACL), §7.2 Windows OS-lockdown docs.
