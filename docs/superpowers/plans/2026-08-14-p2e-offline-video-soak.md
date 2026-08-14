# P2-E — Offline Video Proof, Memory Cap and Soak Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux for tasks 1–8; **Windows runner** for the 18-W1/18-W2 soak bodies (task 9). Tasks 1–7 are host-tested.

**Goal:** The offline video loops for hours on WebKitGTK with no silent failure mode: a decode failure degrades visibly *and observably*, a loop boundary that stalls is detected rather than mistaken for health (PF-05 / Debian #1062012), memory is bounded by a cap whose level is **measured rather than assumed**, and the parent-approved contingency is designed up front.

**Architecture:** A `media.error` IPC bridge lights up an existing dead telemetry variant; loop-boundary monitoring counts **engine activity** (`timeupdate`) rather than sampling the clock; webview RSS comes from the `System` object `health.rs` already holds across ticks, summed over the process's descendant subtree; the memory cap drives `std::process::exit(80)`, which the launcher's FSM restarts exactly as it does any non-86 exit.

**Tech Stack:** Rust 2021, Tauri commands + ACL, `sysinfo` 0.32.1, GStreamer (harness only).

**Spec:** `docs/superpowers/specs/2026-08-06-p2e-offline-video-soak-design.md` (rev 2)

**Depends on:** P2-A, P2-B, P2-C, P2-D. **E1 depends on P2-A only** — the draft's P2-B edge is deleted.

## Global Constraints

- **E lands in two stages.** **Stage 1 (Tasks 1–9):** E1–E4, E6–E10 and all scenario bodies — **sampler only, no enforcement**. **Stage 2 (Task 10):** E5's enforcement half plus 18-W1 into P2-F's F7 matrix, as **one commit after P2-F lands**, gated on 18-W2's recorded floor.
- **Two deliberate cross-platform changes, and only two** (everything else in P2 is Linux-gated): E1's `media.error` bridge, and E4/E5's webview RSS + memory cap. E4/E5 is **the only behaviour change any P2 spec makes on Windows**.
- **Net taxonomy delta is zero.** `Event::MediaError` is a dead variant that already has its `TAXONOMY` row. The pinned `assert_eq!(TAXONOMY.len(), 23, "spec §6 defines 23 events")` (`event.rs:179`) **does not move**.
- **There is no new config key.** `maintenance.max_webview_mem_mb` ships today, fully declared, validated, documented and tested. **The invented `memory_max_mb` is withdrawn** — it would have been shadowed by `Maintenance`'s `#[serde(flatten)] unknown`, so an operator's real key would keep landing on the inert one.
- **Exit code 80 — never 86.** It sits below 86, outside `128 + signal` (129–192) and outside any negative sentinel, so it survives P2-C's C7 encoding. No self-restart may read as a technician exit.
- **The authoritative 18-W1/18-W2 parameter table lives in the spec (§E8) and nowhere else.** P2-F's F7 job **references them by ID and must not restate them** — restating is what produced the drift the review found. **F owns the job, the runner, scheduling and artifacts; E owns the body, the parameters and the feature.**
- **`WEBKIT_DISABLE_COMPOSITING_MODE=1` is NOT set for scenario 18.** P2-A permits it in the smoke environment only; 18 is a soak and disabling compositing plausibly moves the video off the accelerated path.

## File Structure

| File | Responsibility |
|---|---|
| `crates/kiosk-main/src/media.rs` | **new** — the one `media_error` command with boundary validation |
| `crates/kiosk-main/build.rs`, `capabilities/default.json`, `src/main.rs:989-990` | the four ACL/registration sites E1 must edit together |
| `crates/kiosk-main/src/telemetry.rs` | new `Telemetry::media_error` method |
| `crates/kiosk-core/src/logging/ratelimit.rs:52-63` | one `caps()` row |
| `crates/kiosk-main/bundled/offline.html` | the `fallback()` invoke; the activity-counting monitor; the double-buffered loop |
| `crates/kiosk-core/src/metrics.rs` | `webview_rss_mb` on `HealthSample`, the subtree sum, the PID-recycle guard |
| `crates/kiosk-core/src/…` | `MEM_CAP_N`, `struct MemCap` |
| `crates/kiosk-core/src/config/validate.rs` | **deletions** — two `UNIMPLEMENTED` rows and their arms |
| `packaging/soak/` | scenario 18 / 18-W1 / 18-W2 bodies and artifact format |

---

### Task 1: The `media.error` bridge and its ACL (E1)

**Files:**
- Create: `crates/kiosk-main/src/media.rs`
- Modify: `crates/kiosk-main/src/telemetry.rs` (new `media_error` method), `build.rs`, `capabilities/default.json`, `src/main.rs:989-990`, `src/main.rs` (`mod media;`)
- Modify: `crates/kiosk-main/bundled/offline.html` — `fallback()` gains the invoke

**Interfaces:**
- Produces: `#[tauri::command] fn media_error(kind: String, at: f64, ms_since_wrap: Option<f64>, telem: State<Telemetry>)`; `Telemetry::media_error(kind: &'static str, at: Option<f64>, ms_since_wrap: Option<f64>)`
- Consumes: `Event::MediaError` (already in the taxonomy, no emitter today)

> **The ACL is part of the change, not an implication.** `tauri-macros-2.6.3/src/command/handler.rs:35` calls `filter_unused_commands`, which `retain`s away any command not in the allowed set; a dropped command falls through to `_ => { return false; }` and the invoke **rejects at runtime after a green build**. Without the ACL edits, E1's own try/catch-and-degrade-anyway design converts an ACL-stripped command into a **silent** dead telemetry path — and makes E8's headline criterion (zero `media.error`) vacuously true. A criterion that cannot fail is not a gate.

- [ ] **Step 1: Write the failing tests**

```rust
/// `kind` is validated AT THE BOUNDARY against a closed set. No free-form string crosses
/// IPC: the page's fallback(why) interpolates engine text from the play() .catch, and that
/// text is not a stable label. Precedent: nav_blocked's `reason`, documented as "a stable,
/// greppable label".
#[test]
fn only_the_enumerated_kinds_are_accepted() {
    for k in ["error", "stalled", "emptied", "play_rejected", "no_progress", "stall"] {
        assert!(normalize_kind(k).is_some(), "{k}");
    }
    assert_eq!(normalize_kind("MediaError: pipeline failed to link avdec_h264"), None);
    assert_eq!(normalize_kind(""), None);
}

/// Numeric hygiene at the same boundary: a non-finite or negative value is recorded as
/// null rather than logged.
#[test]
fn non_finite_or_negative_numbers_become_null() {
    assert_eq!(sanitize_number(Some(f64::NAN)), None);
    assert_eq!(sanitize_number(Some(f64::INFINITY)), None);
    assert_eq!(sanitize_number(Some(-1.0)), None);
    assert_eq!(sanitize_number(Some(12.5)), Some(12.5));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main media`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement the command and the telemetry method**

`normalize_kind` maps to a `&'static str`; anything else is **dropped, not logged**. `Telemetry::media_error` emits `Event::MediaError`, which sits in the `Severity::Warning` arm, so WARNING ⇒ `is_high()` ⇒ write-through `fsync` in `Spool::append` — durability is correct unassisted.

- [ ] **Step 4: Edit all four registration sites in the same commit**

```
build.rs                     → .commands(&["verify_pin", "media_error"])
capabilities/default.json    → + "allow-media-error"
main.rs:990                  → generate_handler![pinpad::verify_pin, media::media_error]
main.rs:989                  → .manage(telem.clone())   beside the existing .manage(pinpad_state)
```

`Telemetry` is `#[derive(Clone)]` and documented as a cheap `Send` handle.

- [ ] **Step 5: Wire the page**

`offline.html`'s `fallback()` gains one `window.__TAURI_INTERNALS__.invoke(...)` guarded by `try/catch`, **before** the existing `console.error`, and **the degrade path runs regardless** — telemetry is observation, never a dependency. Delete the comment that records the missing bridge in the same change.

- [ ] **Step 6: Verify the ACL end to end**

Run: `cargo test -p kiosk-main && cargo build -p kiosk-main`
Then boot the smoke fixture with the mp4 **deliberately absent** (the `kioskasset` handler 404s when the file is missing) and confirm **one** `media.error` reaches the spool. A green build proves nothing here — the runtime invoke is the check.

- [ ] **Step 7: Commit**

```bash
git add crates/kiosk-main/src/media.rs crates/kiosk-main/src/telemetry.rs \
        crates/kiosk-main/build.rs crates/kiosk-main/capabilities/default.json \
        crates/kiosk-main/src/main.rs crates/kiosk-main/bundled/offline.html
git commit -m "feat: media.error IPC bridge with its ACL registration"
```

---

### Task 2: Rate cap for `media.error` (E2)

**Files:**
- Modify: `crates/kiosk-core/src/logging/ratelimit.rs:52-63`

**Interfaces:**
- Consumes: `Event::MediaError`

> The draft's claim that the event was already "rate-capped by the standard Logger bucket" was **false**: `caps()` contains exactly `NavBlocked`, `NavError`, `WebviewCrash` and `FocusLost`, its own doc says "Every event not listed here is uncapped", and `admit()` returns `Allow` for an unbucketed event.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn media_error_is_capped_at_six_per_minute() {
    let mut rl = RateLimiter::new();
    let admitted = (0..10).filter(|_| matches!(rl.admit(Event::MediaError, 0), Admit::Allow)).count();
    assert_eq!(admitted, 6);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kiosk-core ratelimit`
Expected: FAIL — all 10 are admitted.

- [ ] **Step 3: Add the row**

```rust
// Within one page load the `degraded` latch in fallback() bounds emission to one, so the
// page is not the driver — repeated page loads are (webview.crash → navigate-home recovery,
// nightly reload, safe-mode cycling), which is WebviewCrash's own 6/min bucket.
(Event::MediaError, 6, 6),
```

Not a parent amendment: parent §6:626 names three defaults and says "defaults:", an open list; `FocusLost` is a fourth cap already added in-code with a written rationale. The cap cannot hide the soak's signal — `admit` can only convert an event to `Suppress`, never manufacture one, E8's criterion is **zero**, and `take_summaries` surfaces the suppressed count.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p kiosk-core ratelimit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-core/src/logging/ratelimit.rs
git commit -m "feat(core): rate-cap media.error at 6/min"
```

---

### Task 3: Loop-boundary self-monitoring (E3)

**Files:**
- Modify: `crates/kiosk-main/bundled/offline.html`

**Interfaces:**
- Consumes: `fallback(kind, at, ms_since_wrap)` (Task 1)
- Produces: `ms_since_wrap` as a **raw number** — which is what restores E6's mechanical trigger

> **The `currentTime`-sampling predicate is rejected.** Asserting `v.currentTime !== last || v.currentTime === 0` is blind to the exact PF-05 failure — the element wraps, `currentTime` becomes 0, decode does not resume, and two consecutive 0.0 samples read as *healthy*. That is the shipped watchdog's own **failure** predicate used as a **success** predicate in the same file. It also aliases on a clip whose duration divides the sample interval, and a `readyState >= 2` guard fails open on a stall that drops back to `HAVE_METADATA`.

- [ ] **Step 1: Replace the monitor**

```js
var ticks = 0, lastT = 0, wrapAt = 0, misses = 0;
v.addEventListener("timeupdate", function () {
  if (v.currentTime < lastT) wrapAt = Date.now();   // a loop wrap; we never seek
  lastT = v.currentTime; ticks++;
});
setInterval(function () {
  if (degraded || v.paused) { misses = 0; return; }
  if (ticks > 0) { ticks = 0; misses = 0; return; }
  if (++misses >= 2) fallback("stall", v.currentTime, Date.now() - wrapAt);
}, 5000);
```

Each property is load-bearing: a healthy loop wrap emits `timeupdate` (≈4–66 Hz, tens to hundreds per 5 s window) and a hung decode emits none, so "looped" and "stalled" are distinguished by **engine activity**, not by comparing floats across a wrap. Stuck at 0.0 is the **primary** detection path, not the blind spot. No float comparison survives, so no interval/duration aliasing remains. No `readyState` guard — absence of `timeupdate` while `!paused && !degraded` is a stall at any readyState. The `degraded` check comes **first**, so the degrade path (which hides the element without pausing it) cannot re-trip the monitor.

- [ ] **Step 2: Delete the `12000` literal from the page**

The threshold lives **once**, in E6's activation rule (Task 5). A threshold in the page would sit 2 s from the monitor's own ≥10 000 ms minimum detection latency on a `setInterval` the page cannot control, and any overrun would silently flip a genuine loop-boundary stall to "not at a boundary".

- [ ] **Step 3: Verify all four arch-09 signals route through `fallback()`**

`error`, `stalled`, `emptied`, the `play()` rejection, plus the startup watchdog and this monitor — all six paths reach `fallback()` and therefore E1. `requestVideoFrameCallback` is **not** assumed to exist on WebKitGTK and is not used; the design needs only `timeupdate`.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/bundled/offline.html
git commit -m "feat: activity-counting loop-boundary monitor for the offline video"
```

---

### Task 4: Webview RSS from the `System` already in hand (E4)

**Files:**
- Modify: `crates/kiosk-core/src/metrics.rs:6-12` (`HealthSample`), `:20-44` (`sample`), `:47-56` (`to_fields`)
- Modify: the startup path — one `sysinfo::set_open_files_limit(0)` call before the first process refresh

**Interfaces:**
- Produces: `HealthSample::webview_rss_mb`, and a pure `fn webview_subtree_rss_mb(procs: &HashMap<Pid, Process>, self_pid: Pid) -> u64`
- Consumes: the `&mut System` `health.rs` already hands to `metrics::sample`

**No new dependency, no new `windows` feature, no `unsafe`, no `#[cfg]`** — with one declared side effect (Step 4). `health.rs` is **untouched**: it owns only the tick.

> **What the number means, declared:** the **arithmetic sum of `Process::memory()` over every descendant of the kiosk-main pid, excluding kiosk-main itself** — total resident on Linux, working set on Windows — **with shared pages counted once per helper**. It is a *footprint proxy*, not a unique-set size; `sysinfo` 0.32 exposes no PSS or private-bytes alternative. The justification for summing is **traceability to parent §6:671's literal "webview RSS"** and nothing else.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_sum_covers_the_whole_descendant_subtree_and_excludes_self() {
    // WebKitGTK runs content in WebKitWebProcess; WebView2 runs a browser process plus
    // renderers. Both are descendants, so a parent-pointer walk is engine-agnostic: no name
    // matching, no per-platform branch, no knowledge of how many helpers an engine spawns.
    let procs = fake_tree(&[
        (1, None, 100),      // unrelated root
        (10, Some(1), 500),  // kiosk-main (self) — EXCLUDED
        (11, Some(10), 200), // web process
        (12, Some(11), 300), // grandchild (network/GPU helper)
        (20, Some(1), 900),  // unrelated sibling
    ]);
    assert_eq!(webview_subtree_rss_mb(&procs, pid(10)), 500);
}

/// Windows never rewrites InheritedFromUniqueProcessId when a parent exits, and PIDs are
/// recycled, so a naive walk can graft an unrelated tree onto the kiosk's — an inflation
/// vector. Reject any candidate child whose start_time() precedes its claimed parent's.
#[test]
fn a_child_older_than_its_claimed_parent_is_rejected() {
    let procs = fake_tree_with_times(&[
        (10, None, 500, 1000),      // self, started at t=1000
        (11, Some(10), 400, 900),   // claims self as parent but predates it — recycled PID
    ]);
    assert_eq!(webview_subtree_rss_mb(&procs, pid(10)), 0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-core metrics`
Expected: FAIL — `webview_subtree_rss_mb` does not exist.

- [ ] **Step 3: Implement**

`sample` gains `sys.refresh_processes(ProcessesToUpdate::All, true)`; the pure helper walks `Process::parent()` from `get_current_pid()`, applying the start-time guard. `to_fields` gains the matching key. Cost: one process enumeration per `health_sample_s` (default 60, range `[10, 3600]`).

Both platform-specific mechanisms (`/proc/self/status` `VmRSS`, Windows `GetProcessMemoryInfo`) are **withdrawn** — redundant against a dependency already instantiated here, and the latter would additionally need `Win32_System_ProcessStatus`, which is not among the declared `windows` features, plus an `unsafe` block, for a number `sysinfo` already returns.

- [ ] **Step 4: Add the FD-retention mitigation**

```rust
// sysinfo's Linux process API retains one /proc/<pid>/stat handle per tracked process
// across refreshes. Permanent per-process FD retention is exactly the class of resource
// growth scenario 18 exists to detect, and it would sit in the baseline. With the budget at
// 0, FileCounter::new returns None and _get_stat_data opens, reads and DROPS the handle.
// Functionality preserved, retention gone. No-op on Windows by design.
sysinfo::set_open_files_limit(0);
```

Call it **once at startup, before the first process refresh**. Record the accepted risk: `remaining_files()`'s `OnceLock` initialiser runs `getrlimit`/`setrlimit(RLIMIT_NOFILE, hard)`, so any use of sysinfo's Linux process API raises the soft FD limit to the hard limit, once. The effect is a *raise*, so a `LimitNOFILE=` in P2-C's unit buys very little — declared as a **non-blocking** ask on P2-C/P2-G.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kiosk-core metrics`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-core/src/metrics.rs crates/kiosk-main/src/main.rs
git commit -m "feat(core): sample webview subtree RSS with a PID-recycle guard"
```

> **E4 ships first and unconditionally.** Every fleet gets `webview_rss_mb` in `health.sample` from this build, with **no enforcement attached**. E5's enforcement is Task 10, after the floor gate.

---

### Task 5: The double-buffered no-seek loop (E6)

**Files:**
- Modify: `crates/kiosk-main/bundled/offline.html`

**Interfaces:**
- Consumes: E3's `ms_since_wrap` (Task 3)

Two stacked `<video>` elements, both preloaded; on `ended` the hidden one — already at 0 and paused-ready — plays and swaps to front; the other resets in the background.

- [ ] **Step 1: Remove `loop` from both elements**

The shipped element carries `loop`, and per the HTML media element spec a looping element seeks to the earliest position and **does not fire `ended`** — so E6's primary trigger could never arm. Worse: while `loop` is present the engine performs precisely the seek-to-0 that #1062012 names, so the double buffer would change nothing. **The loop is driven entirely by the swap.**

- [ ] **Step 2: Reset the background element with `load()`, never `currentTime = 0`**

`currentTime = 0` *is* the seek path #1062012 names; using it would relocate the bug into the background and surface it one loop later with two frozen elements. `load()` re-runs the resource selection algorithm — a fresh fetch and decode pipeline, not a seek — which is affordable against a local `kioskasset` custom-scheme read with a full clip duration of budget.

- [ ] **Step 3: Gate the swap on readiness**

The incoming element must reach `canplaythrough` before it is swapped to front; if it has not by `duration − 0.25 s` of the visible element, the page degrades per arch-09 rather than showing a not-ready element. **Budget:** one clip duration. If `load()` cannot complete within it on target hardware, the fallback is the native-GL path (parent §3.4's second fallback), **not** `currentTime = 0`.

- [ ] **Step 4: Record the activation rule — the only place the threshold lives**

```
Activation rule (mechanical, not judgment): any media.error{kind:"stall"} during scenario 18
whose ms_since_wrap is < 12000 ⇒ this contingency activates. The native-GL path stays out
unless double-buffering also fails on hardware; it forfeits the one-HTML-path property and
needs its own design round.
```

Put this in `packaging/soak/README.md` next to scenario 18, not in the page.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/bundled/offline.html packaging/soak
git commit -m "feat: double-buffered no-seek video loop (PF-05 contingency)"
```

---

### Task 6: `maintenance.restart_app` (E9)

**Files:**
- Modify: `crates/kiosk-main/src/main.rs:1188` (a second `tokio::spawn(maintenance::run(...))`)
- Modify: `crates/kiosk-core/src/config/validate.rs:20` and `:184-185` — **deletions**

**Interfaces:**
- Consumes: `maintenance::run` (already generic — a `next_fire`-driven "HH:MM" loop, not nightly-reload-specific), E5's exit path

The mechanism is **E5's exit path fired by a clock instead of by a threshold**. `hhmm: None` returns immediately (feature off) and an unparseable value calls `warn_once` exactly once, which the caller turns into `config.warn{field}`.

- [ ] **Step 1: Write the failing test**

```rust
/// The config work is a DELETION: the key ships and validates today; only the
/// "feature unavailable in this build" warning has to go.
#[test]
fn restart_app_no_longer_warns_as_unimplemented() {
    let cfg = config_with(r#"{"maintenance":{"restart_app":"03:30"}}"#);
    let warnings = validate(&cfg);
    assert!(!warnings.iter().any(|w| w.field == "maintenance.restart_app"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kiosk-core validate`
Expected: FAIL — the `UNIMPLEMENTED` row still fires.

- [ ] **Step 3: Implement**

Remove `validate.rs:20`'s row and its `:184-185` arm; update the RT-08 warn-path tests. Add the second `tokio::spawn(maintenance::run(...))` at the same site as the nightly-reload timer, with `restart_app` as the time, the same timezone, a `warn_once` naming `maintenance.restart_app`, and a callback taking the clean exit. **It sits inside the same `if safe {} else {}` split as the nightly timer** — a safe-mode run must not restart itself on a clock.

No new dependency, no cron library, no new module.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-core validate && cargo test -p kiosk-main maintenance`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-core/src/config/validate.rs crates/kiosk-main/src/main.rs
git commit -m "feat: maintenance.restart_app on the existing HH:MM timer"
```

---

### Task 7: Remote log level (E10)

**Files:**
- Modify: `crates/kiosk-main/src/telemetry.rs` — parse `logging.level` into a `Severity` once per config apply, drop entries below it

**Interfaces:**
- Consumes: `logging.level` (parses, defaults and validates today, with **zero consumers**), `Severity` (already ordered)

> Unlike `max_webview_mem_mb` and `restart_app`, `logging.level` was **never** added to `UNIMPLEMENTED`, so an operator setting it has been silently ignored with no RT-08 warning since P1. **There is no row to delete; the fix is the consumer itself.**

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn entries_below_the_configured_level_are_dropped() {
    let telem = telemetry_with_level("warning");
    telem.health_sample(/* INFO */);
    telem.nav_error("boom" /* WARNING */);
    assert_eq!(spooled_events(&telem), vec!["nav.error"]);
}

#[test]
fn the_default_level_admits_info() {
    let telem = telemetry_with_level("info");
    telem.health_sample();
    assert_eq!(spooled_events(&telem).len(), 1);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main telemetry`
Expected: FAIL — the level has no consumer.

- [ ] **Step 3: Implement**

Roughly five lines, platform-free, no new taxonomy, no schema change: a `>=` severity drop applied **before the spool**.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main telemetry`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/telemetry.rs
git commit -m "feat: apply logging.level as a severity drop before the spool"
```

---

### Task 8: The GStreamer harness environment and the deliberate miss (E7)

**Files:**
- Modify: `packaging/smoke/README.md`, `packaging/soak/README.md`
- Create: `packaging/soak/fixtures/no-libav.sh`

**Interfaces:**
- Consumes: E1's bridge (Task 1)

- [ ] **Step 1: Install the four packages parent §3.4:285-288 names**

`gstreamer1.0-plugins-{base,good,bad}` and `gstreamer1.0-libav` in the smoke and soak environments. P2-G's design carries the identical four for the `.deb` — three-way consistent.

- [ ] **Step 2: Add the deliberate-miss run**

With `gstreamer1.0-libav` removed: **exactly one `media.error` of any enumerated kind reaches the spool, and the page degrades to the black splash.** The specific `kind` is **recorded in the run artifact, not asserted**.

> Pinning to one `kind` is rejected: removing `libav` removes `avdec_h264` while `qtdemux` (-good) and `h264parse` (-bad) remain, so the pipeline demuxes and parses and then fails to link a decoder — a bus error WebKitGTK surfaces as an `error` event, routed by the shipped listener as `kind:"error"`. But parent §3.4:287-288 also says a missing element yields "a silent black video", which is what `no_progress` assumes. Neither branch is verifiable without GStreamer and WebKitGTK, so assert the **outcome** the parent requires and record the label.

This is E's only per-PR contribution (~3 s as a variant fixture; **P2-F decides inclusion**). E owns the assertion; P2-G owns the `.deb` declaration, P2-F owns the harness.

- [ ] **Step 3: Commit**

```bash
git add packaging/soak packaging/smoke/README.md
git commit -m "test: GStreamer harness environment plus the missing-libav degrade run"
```

---

### Task 9: Soak bodies — scenarios 18, 18-W1, 18-W2 (E8)

**Files:**
- Create: `packaging/soak/scenario-18.sh` (Linux), `packaging/soak/scenario-18-w1.md`, `packaging/soak/scenario-18-w2.md` (Windows bodies + fixtures)

**Interfaces:**
- Produces: the scenario bodies and parameters P2-F's F7 references **by ID**

- [ ] **Step 1: Scenario 18 — the Debian offline-video soak**

Fixture: config-down boot → offline video (P2-A scenario 3's entry path), on the `debian:12` nightly container.

**Positive precondition, before the soak clock starts:** load the offline page with `kiosk-offline.mp4` deliberately **absent** and assert **one `media.error` appears on the spool**. Only then does the soak begin, with the criterion inverted to zero. A criterion that cannot fail is replaced by one with a proven-live producer.

**Pass:** zero `media.error` in the spool; process alive; **zero launcher restarts of any kind, including a memory-cap exit** — `offline.html` has no leak source, so a cap trip here is a real leak and **fails** the soak (leave the cap at its default, do **not** disable it); `webview_rss_mb` delta over the window under a bound declared from the first-run baseline and then pinned; loop count consistent with wall-clock.

**Durations:** in-session ~2 h minimum during execution; scheduled CI multi-hour, **duration set by P2-F within the hosted-runner cap** — E pins no CI wall clock and E's pass criteria are duration-agnostic; hardware ≥72 h is P2-G H5.

**Artifacts:** the t=0 `webview_rss_mb` baseline as the **first line**; on failure, the full spool and the compositor log.

- [ ] **Step 2: Write the 18-W1 and 18-W2 bodies from the spec's parameter table verbatim**

Copy the authoritative table from spec §E8 into `packaging/soak/scenario-18-w1.md` / `-w2.md`. **Do not paraphrase and do not let P2-F restate it** — restating is what produced the drift the review found.

Key parameters: 18-W1 uses `max_webview_mem_mb: 256`, `health_sample_s: 10` (dwell = 50 s), `kiosk.healthy_run_s: 30`, `nightly_reload` unset. 18-W2 uses `max_webview_mem_mb: 0` (off), defaults for the cadences, and `nightly_reload` a few minutes ahead.

- [ ] **Step 3: State 18-W2's four fixture preconditions, each of which silently voids the assertion**

1. **The device must be in `Online`** when the timer fires — every other state is a no-op.
2. **The leaking page must be `content.url`.** The reload navigates to `self.home`, not "the current URL", so a fixture leaking on some other page would swap to a lighter one and go green while proving nothing. This is a false-pass risk, so it gets a **second assertion** — post-reload URL equals the leaking page — not merely a precondition.
3. **`content.clear_data_on_reset` off** — otherwise `ClearProfile{full:true}` also frees memory and the drop cannot be attributed to the reload. **Its default is `true`, so the fixture must set it `false`.**
4. **No `--safe`** — a safe-mode run has no reload timer at all.

- [ ] **Step 4: Record 18-W2's steady-state floor as a first-class artifact number**

This number is E5's merge gate (Task 10). Record it in the run artifact with the fixture and date.

- [ ] **Step 5: Commit**

```bash
git add packaging/soak
git commit -m "test: soak scenario 18 plus the 18-W1/18-W2 Windows bodies"
```

---

### Task 10 (STAGE 2 — after P2-F lands): E5 enforcement

**Files:**
- Modify: `crates/kiosk-core/src/…` — `MEM_CAP_N`, `struct MemCap`
- Modify: `crates/kiosk-main/src/health.rs` call path — `std::process::exit(80)` on trip
- Modify: `crates/kiosk-core/src/config/validate.rs:19` and `:181-183` — **deletions**
- Modify: P2-F's F7 matrix — add 18-W1

**Interfaces:**
- Produces: `pub const MEM_CAP_N: u32 = 5`; `pub struct MemCap { over: u32 }` with `fn observe(&mut self, rss_mb: u64, cap_mb: u64) -> bool`

> **THE GATE:** read 18-W2's recorded steady-state Windows `webview_rss_mb` floor. **If that floor is ≥ 750 MB (half of 1500), E5's enforcement DOES NOT SHIP** — raise a defect against parent §5.2's default instead. Margin rationale: distinguishing a leak from a working set needs at least 2× headroom between healthy steady state and the cap; below that the cap fires on normal variance rather than on a leak.

- [ ] **Step 1: Check the gate**

Read the 18-W2 artifact. Record the number and the decision in the commit message. If ≥ 750 MB, **stop here** and file the defect.

- [ ] **Step 2: Write the failing tests**

```rust
#[test]
fn a_zero_cap_never_trips() {
    let mut c = MemCap::default();
    for _ in 0..100 { assert!(!c.observe(9999, 0)); }
}

#[test]
fn the_latch_trips_once_after_n_consecutive_over_samples_then_resets() {
    let mut c = MemCap::default();
    for _ in 0..(MEM_CAP_N - 1) { assert!(!c.observe(300, 256)); }
    assert!(c.observe(300, 256));
    assert!(!c.observe(300, 256), "trips once, then resets");
}

#[test]
fn a_sample_at_or_below_the_cap_resets_the_run() {
    let mut c = MemCap::default();
    for _ in 0..(MEM_CAP_N - 1) { c.observe(300, 256); }
    assert!(!c.observe(256, 256), "strictly over, so equal does not count");
    for _ in 0..(MEM_CAP_N - 1) { assert!(!c.observe(300, 256)); }
}
```

Plus the cross-process interlock, pinned **in `kiosk-launcher`** where both values are observable:

```rust
#[test]
fn mem_cap_dwell_exceeds_the_launchers_healthy_run_window() {
    let dwell = MEM_CAP_N as u64 * RemoteConfig::default().logging.health_sample_s;
    assert!(dwell > watchdog_config(None).healthy_run_s,
        "a memory-cap exit must land after the crash-loop window has been cleared");
}
```

> A host test in `kiosk-main` could only compare against a hardcoded 120 and would keep passing after any launcher change — **that test is withdrawn**. `kiosk-launcher` owns both real values. No hardcoded copy of either number; the test fails if `d_health_sample`, `MEM_CAP_N` or the launcher's default moves.

- [ ] **Step 3: Implement**

`cap_mb == 0` ⇒ always `false`; `MEM_CAP_N` consecutive samples **strictly over** ⇒ `true` once, then reset. `kiosk-main` calls `std::process::exit(80)` on `true`. There is one `N` and one dwell formula: **dwell = `MEM_CAP_N × health_sample_s`**.

Delete `validate.rs:19`'s `UNIMPLEMENTED` row and its `:181-183` arm; update the RT-08 warn-path tests. Schema and range validation are unchanged.

- [ ] **Step 4: Record durability honestly**

```
The restart's cause is durable in watchdog.restart{code: 80} — ERROR, fsynced, written by
the SURVIVING launcher, with code, backoff_s and cause as first-class fields. The
webview_rss_mb series is delivered on the normal telemetry path, best-effort like every INFO
event; the sample immediately preceding the exit may be lost.
```

A `health.memory_cap` event is **rejected** (it would cost a variant, two forced match arms, a `TAXONOMY` row, a `23 → 24` bump of a deliberately pinned test, and a parent §6 amendment — to record something a surviving process already records durably at ERROR). An exit-ordering handshake is **also rejected**; reusing `telemetry::spool_boot_config_error`'s direct-`Spool::open` pattern would be a **bug** here — a second concurrent `Spool` handle appending to the same segments is corruption, not durability.

- [ ] **Step 5: Write the release note**

> From P2, a webview tree exceeding `max_webview_mem_mb` for `MEM_CAP_N` consecutive health samples restarts the app. **After upgrading to the E4 build, read your fleet's `health.sample.webview_rss_mb` p99 over one week and set `maintenance.max_webview_mem_mb` to roughly 2× that value within `[256, 8192]`, or to `0` to disable; until then the shipped default of 1500 applies.**

Carry 18-W2's recorded floor in the note ("your Windows engine baseline is ~X MB; your content adds on top") so the rule is actionable on day one rather than after a week of telemetry.

Record the accepted residual: 18-W2's fixture is a test page, not fleet content, so its number is a **floor**. The gate catches the disqualifying case and cannot certify the general one. A Windows fleet whose real site drives the summed tree between that floor and 1500 gets a clean, well-logged, permanent restart cycle every 300 s it did not have in P1 — **carried by the operator**, informed rather than surprised, via the sampler, the release-note rule and the `0` lever.

- [ ] **Step 6: Run everything and commit as ONE commit with 18-W1**

Run: `cargo test --workspace`
Expected: PASS.

```bash
git add crates/kiosk-core crates/kiosk-main .github/workflows packaging/soak
git commit -m "feat: memory-cap restart at exit 80, gated on the 18-W2 floor"
```

---

## Self-Review

**Spec coverage:** E1 → T1; E2 → T2; E3 → T3; E4 → T4; E5 → T10; E6 → T5; E7 → T8; E8 → T9; E9 → T6; E10 → T7.

**Staging:** Tasks 1–9 are stage 1 and can land before P2-F. Task 10 is stage 2, lands **after** P2-F, and is **conditional on 18-W2's floor**.

**Open at plan time (values only):** `requestVideoFrameCallback` is not assumed to exist on WebKitGTK and is not used; if a plan-time check finds it present, nothing here changes.

**Declared residuals:** the memory-cap *level* has no derivation and no measurement (`grep -n "1500"` across every spec returns exactly one line — parent §5.2:538); `health_sample_s` and `healthy_run_s` are both operator-settable and `healthy_run_s` has **no** range validation at all, documented in the operator note and deliberately not defended in code; a stall class visible only with compositing enabled would be invisible in CI, bounded by P2-G H5 on real hardware.
