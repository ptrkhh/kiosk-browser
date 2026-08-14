# P2-D — Linux Native Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux/Wayland. Tasks 1–2 are pure/host-tested. Tasks 3–6 compile on Linux and are proven by the Task 7 smoke (scenarios 16–17).

**Goal:** `idle_reset_seconds` fires `IdleExpired` (completing the app-path `ClearProfile` → `ProfileCleared` chain P2-A could only reach via a harness binary), and both exit paths — N corner taps and the technician keyboard chord — open the pin pad on Linux. PF-04's interactive-pinch intercept lands on the same widget.

**Architecture:** Two independent legs of GTK **widget-signal observation** — pointer/touch on the `webkit2gtk::WebView`, keys on the `gtk::ApplicationWindow`. The split is required by GTK3's asymmetry, not stylistic. Every `Propagation`-returning handler is built by one `observe()` wrapper that supplies `Proceed`, so **no handler body can return `Stop`** — enforced by `rustc`, not convention. We are never on the dispatch path, so a defect in our code costs at most our own feature and never the webview's input.

**Tech Stack:** Rust 2021, gtk 0.18 (one new direct dependency; `gdk`/`glib` arrive as `gtk::gdk`/`gtk::glib`), webkit2gtk 2.0.2.

**Spec:** `docs/superpowers/specs/2026-08-06-p2d-linux-native-input-design.md` (rev 2)

**Depends on:** P2-A, P2-B, P2-C. **No new module and no `main.rs` diff.**

## Global Constraints

- **The always-`Proceed` rule is D's entire safety argument.** Every signal handler is wrapped by `observe()`. Any inline `connect_*` returning anything but `Proceed` is a reviewable smell, **not** a designed carve-out. PF-04's intercept is an `EventController`, whose closure returns `()` — it is not on the `-> glib::Propagation` surface at all, so it is not an exception.
- **`gdk::Event::set_handler` / `gdk_event_handler_set` is rejected on evidence and must not be reintroduced.** GTK itself installs `gdk_event_handler_set(gtk_main_do_event, …)` in its init path (objdump-verified), so `set_handler` **replaces** GTK's dispatch: one function pointer, no chaining API, a `gdk_event_copy` per event, and `assert_initialized_main_thread!()` can panic on install. Any defect in our handler takes **all input to the webview** with it — the black, unclickable, un-exitable pane parent §3.5:319-320 forbids.
- **Do not touch `is_technician_chord`.** Adding `&& !mods.win` at the shared root is withdrawn — the function's own doc forbids exactly that edit and `technician_chord_is_matched_but_never_swallowed` (`shortcuts.rs:395-402`) pins it. The replacement is a `should_swallow` **guard at the Linux call site**, reproducing Vector A's ordering verbatim: zero diff to shared reviewed code, zero D2c test delta.
- **`idle_secs_from_ticks` must stay un-`cfg`ed.** It has no attribute today, its test is unconditional, and the ubuntu `cargo test --workspace` job compiles both. **Zero diff is the correct diff** — adding `#[cfg(windows)]` would break Linux CI.
- **No `catch_unwind`.** It would convert a loud abort into a silently dead feature.
- **Dependencies** (target-gated; reconcile by union if B or C wrote the `webkit2gtk` line first — **there is no ordering edge**):
  ```toml
  [target.'cfg(target_os = "linux")'.dependencies]
  gtk = "0.18"
  webkit2gtk = { version = "2.0.2", features = ["v2_32"] }
  ```
  `gtk 0.18.2` and `gdk 0.18.2` are already in `Cargo.lock` and already built for wry/tauri, and the dependency is **forced by Tauri's own public signature** — `gtk_window()` returns `gtk::ApplicationWindow`, a type we cannot name without the crate.

## Change IDs referenced by sibling specs

P2-B and P2-G cite D's changes by ID; this is where each lands, so an edge followed from either arrives at a task rather than at prose.

| ID | What | Task |
|---|---|---|
| **D1** | leg 1 — pointer/touch observation on the webview widget (also the object PF-04's controller attaches to) | 4 |
| **D3** | the one sentence D contributes to **P2-G's G10**: the technician chord is the in-session escape under the locked cage session; the image intentionally leaves no VT/getty route. **Leg 3 is withdrawn** | 7 Step 4 |
| **D5 / D11** | `GDK_TOUCH_CANCEL` emission and the N-finger over-count, both recorded as ponytails and gated on real touch hardware at **P2-G H4a** | 4 |
| **D10** | the shared `webkit2gtk` dependency declaration — needs nothing above P2-B's B10 floor; union of features, first writer wins, **no ordering edge** | 2 Step 3 |
| **D13** | the PF-04 pinch intercept and its recorded `scale_delta()` deadband, gated at **P2-G H10** | 6 |

## File Structure

| File | Responsibility |
|---|---|
| `crates/kiosk-main/src/gesture.rs` | `#[cfg(not(windows))]` body replaces the stub at `:194`; `TAP_WINDOW_MS` loses its `#[cfg(windows)]`; the shared `observe()` helper; the PF-04 `GestureZoom` intercept |
| `crates/kiosk-main/src/shortcuts.rs` | `#[cfg(not(windows))]` body replaces the stub at `:113`; the keyval→VK map |
| `crates/kiosk-main/src/idle.rs` | Linux `run` arm + the module-local `LAST_INPUT_MS` clock and `note_activity()` |
| `crates/kiosk-main/Cargo.toml` | the `gtk` dependency |
| `packaging/smoke/run-smoke.sh` | scenarios 16–17 |

---

### Task 1: The module-local idle clock

**Files:**
- Modify: `crates/kiosk-main/src/idle.rs` — add `LAST_INPUT_MS`, `note_activity`, and the Linux `run` arm replacing the `#[cfg(not(windows))]` stub

**Interfaces:**
- Produces: `pub fn note_activity()`, `#[cfg(not(windows))] fn idle_secs() -> u64`, and `#[cfg(not(windows))] pub async fn run(threshold: u64, tx: mpsc::Sender<AppEvent>, cancel: CancellationToken)` — **the same signature as the Windows arm**
- Consumes: `should_fire` (unchanged, already host-tested)

> **`idle::run`'s signature is unchanged and `main.rs:917` is untouched** — a single, non-`cfg`-gated `tokio::spawn`. There is no handle to plumb, so the spawn-before-install ordering is a non-issue.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(not(windows))]
mod linux_clock {
    use super::*;

    /// The `0` sentinel mirrors the Windows convention deliberately: idle.rs:78-79 falls
    /// back to "not idle" (0) if the Win32 call fails, "rather than risking a false
    /// idle-fire off garbage data". If BOTH leg installs degrade, nothing ever calls
    /// note_activity(), the clock stays at 0, and nothing fires — loud (both install
    /// errors log) and in the direction Windows chose.
    #[test]
    fn no_observation_source_reads_as_not_idle() {
        reset_clock_for_test();
        assert_eq!(idle_secs(), 0);
    }

    /// `.max(1)` keeps a real first stamp from colliding with the sentinel, at <=1 ms of
    /// skew against a 1 s poll.
    #[test]
    fn a_stamp_at_epoch_zero_still_beats_the_sentinel() {
        reset_clock_for_test();
        note_activity();
        assert_ne!(LAST_INPUT_MS.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    fn a_fresh_stamp_reads_as_zero_idle_seconds() {
        note_activity();
        assert_eq!(idle_secs(), 0);
    }
}
```

Add `#[cfg(test)] fn reset_clock_for_test()` storing `0` into `LAST_INPUT_MS`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main idle`
Expected: FAIL — `note_activity`/`LAST_INPUT_MS` do not exist.

- [ ] **Step 3: Implement**

```rust
/// Linux idle clock. The GTK main thread stores, a tokio worker loads; `Relaxed` is
/// correct for a lone monotonic timestamp with nothing ordered by it.
///
/// ponytail: `.max(1)` costs <=1 ms of skew against a 1 s poll and keeps a real first
/// stamp from being mistaken for "no source yet".
#[cfg(not(windows))]
static LAST_INPUT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(not(windows))]
pub fn note_activity() {
    LAST_INPUT_MS.store(now_ms().max(1), std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(windows))]
fn idle_secs() -> u64 {
    match LAST_INPUT_MS.load(std::sync::atomic::Ordering::Relaxed) {
        0 => 0, // no source ⇒ "not idle" — mirrors idle.rs:88-89
        t => now_ms().saturating_sub(t) / 1000,
    }
}
```

The Linux `run` keeps the **identical** 1 s poll, `should_fire` latch and cancel-awareness as the Windows arm — copy its loop body verbatim and swap only `idle_secs`. SEC-09's never-cancelled property (`idle.rs:16-24`) carries over unchanged.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main idle`
Expected: PASS, including the pre-existing unconditional `idle_secs_is_wrap_safe_across_the_32bit_tick_boundary`.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/idle.rs
git commit -m "feat(linux): module-local idle clock with the not-idle sentinel"
```

---

### Task 2: The keyval→VK map

**Files:**
- Modify: `crates/kiosk-main/src/shortcuts.rs` — add the pure mapping fns
- Modify: `crates/kiosk-main/Cargo.toml` — the `gtk` dependency

**Interfaces:**
- Produces: `#[cfg(not(windows))] fn vk_from_keyval(keyval: gtk::gdk::keys::Key) -> Option<u32>` and `#[cfg(not(windows))] fn mods_from_state(state: gtk::gdk::ModifierType) -> Modifiers`
- Consumes: `VK_K` (`shortcuts.rs:58`), `Modifiers`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(not(windows))]
mod keyval_map {
    use super::*;
    use gtk::gdk::keys::constants as k;
    use gtk::gdk::ModifierType;

    /// Both cases are matched because Shift is held in the chord.
    #[test]
    fn both_cases_of_k_map_to_vk_k() {
        assert_eq!(vk_from_keyval(k::K), Some(VK_K));
        assert_eq!(vk_from_keyval(k::k), Some(VK_K));
    }

    #[test]
    fn every_other_key_maps_to_none() {
        assert_eq!(vk_from_keyval(k::a), None);
        assert_eq!(vk_from_keyval(k::F5), None);
        assert_eq!(vk_from_keyval(k::Escape), None);
    }

    /// Declared assumption: Alt = MOD1_MASK. ModifierType has no ALT_MASK; Mod1 is an XKB
    /// convention, pinned by smoke 17. Residual: an exotic layout mapping Alt off Mod1
    /// kills leg 2 only — leg 1 still exits the device, so §3.5 holds.
    #[test]
    fn the_chord_modifiers_map_from_gdk_state() {
        let state = ModifierType::CONTROL_MASK | ModifierType::MOD1_MASK | ModifierType::SHIFT_MASK;
        let m = mods_from_state(state);
        assert!(m.ctrl && m.alt && m.shift && !m.win);
        assert!(is_technician_chord(VK_K, m));
    }

    #[test]
    fn super_and_mod4_both_read_as_win() {
        assert!(mods_from_state(ModifierType::MOD4_MASK).win);
        assert!(mods_from_state(ModifierType::SUPER_MASK).win);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-main keyval_map`
Expected: FAIL — the functions and the `gtk` dependency do not exist.

- [ ] **Step 3: Implement**

```rust
#[cfg(not(windows))]
fn vk_from_keyval(keyval: gtk::gdk::keys::Key) -> Option<u32> {
    use gtk::gdk::keys::constants as k;
    match keyval {
        k::K | k::k => Some(VK_K),
        _ => None,
    }
}

#[cfg(not(windows))]
fn mods_from_state(m: gtk::gdk::ModifierType) -> Modifiers {
    use gtk::gdk::ModifierType as M;
    Modifiers {
        ctrl: m.contains(M::CONTROL_MASK),
        alt: m.contains(M::MOD1_MASK), // no ALT_MASK exists; Mod1 is the XKB convention
        shift: m.contains(M::SHIFT_MASK),
        win: m.intersects(M::MOD4_MASK | M::SUPER_MASK),
    }
}
```

`Key` is a `pub struct Key(u32)` with `Deref<Target = u32>` and derived `PartialEq`/`Eq` (`gdk-0.18.2/src/keys.rs:8-16`), so the two-arm match compiles.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-main keyval_map`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-main/src/shortcuts.rs crates/kiosk-main/Cargo.toml
git commit -m "feat(linux): keyval to VK chord mapping"
```

---

### Task 3: `observe()` — the compiler-enforced `Proceed` rule

**Files:**
- Modify: `crates/kiosk-main/src/gesture.rs` — add the wrapper (used by both legs)

**Interfaces:**
- Produces: `#[cfg(not(windows))] pub(crate) fn observe<W, E>(f: impl Fn(&E) + 'static) -> impl Fn(&W, &E) -> gtk::glib::Propagation`

- [ ] **Step 1: Implement**

```rust
/// Every `Propagation`-returning handler in this sub-project is built here, so no handler
/// *body* can return `Stop`. Five call sites, one function, checked by rustc on every
/// build. The rule this makes un-violatable is D's entire safety argument: we are never on
/// the dispatch path, so a defect in our code costs at most our own feature and never the
/// webview's input.
///
/// There is no legitimate exception. PF-04's intercept is an `EventController`, not a
/// `Propagation` handler, so it is not a carve-out.
#[cfg(not(windows))]
pub(crate) fn observe<W: 'static, E: 'static>(
    f: impl Fn(&E) + 'static,
) -> impl Fn(&W, &E) -> gtk::glib::Propagation + 'static {
    move |_, e| {
        f(e);
        gtk::glib::Propagation::Proceed
    }
}
```

> `connect_*` requires `F: … + 'static`, and an RPIT over unbounded `W, E` is not known to be `'static` — hence the explicit `W: 'static, E: 'static` bounds and the `+ 'static` on the return type. This is the plan-time shim the spec anticipated.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p kiosk-main`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/kiosk-main/src/gesture.rs
git commit -m "feat(linux): observe() wrapper enforcing always-Proceed at compile time"
```

---

### Task 4: `gesture.rs` Linux body — leg 1

**Files:**
- Modify: `crates/kiosk-main/src/gesture.rs:181-182` (`TAP_WINDOW_MS` loses `#[cfg(windows)]`), `:194` (replace the stub)

**Interfaces:**
- Consumes: `observe()` (Task 3), `idle::note_activity` (Task 1), `TapCounter`, `in_region`, `open_pin_pad`, `EffectiveGesture` — **all unchanged, no D2c test delta**
- Produces: the installed leg-1 handlers. Already called at `main.rs:1106`: **zero `main.rs` diff.**

**cfg-12 handling is the safety fix of this revision.** The Windows body opens with an early return when `effective_gesture` yields `None` (`gesture.rs:293-296`). Mirroring that on Linux would install *no pointer or touch handler at all*, so on a cfg-12-unconfigured keyboardless kiosk nothing would ever stamp the idle clock, `should_fire` would fire, and `(Online, IdleExpired) if idle_clear → ClearProfile{full:true}` would **wipe a live session while the user is tapping** — silently. So: handlers install **unconditionally**, `note_activity()` runs **before** the gate, `TapCounter` is `gesture.map(…)`, and the tap branch early-returns on `None`.

This is not a new convention — `shortcuts.rs` already carries the `Option` all the way to `open_pin_pad`'s own `None` guard. `gesture.rs:293-296` is the outlier.

- [ ] **Step 1: Delete the `#[cfg(windows)]` on `TAP_WINDOW_MS`**

One-line deletion, one shared 3000 ms constant, no second constant and no silent parity divergence.

- [ ] **Step 2: Implement the body**

```rust
#[cfg(not(windows))]
pub fn install(
    window: &tauri::WebviewWindow,
    app: tauri::AppHandle,
    gesture: Option<EffectiveGesture>,
) {
    // Observation installs UNCONDITIONALLY: these handlers feed BOTH the exit gesture and
    // idle reset. cfg-12 disables the *gesture*, not *observation*.
    let tap = gesture.map(|g| std::rc::Rc::new(std::cell::RefCell::new(
        (TapCounter::new(g.taps, TAP_WINDOW_MS), g),
    )));
    let result = window.with_webview(move |platform_webview| {
        use gtk::prelude::*;
        let wv = platform_webview.inner();
        wv.add_events(
            gtk::gdk::EventMask::BUTTON_PRESS_MASK
                | gtk::gdk::EventMask::TOUCH_MASK
                | gtk::gdk::EventMask::POINTER_MOTION_MASK
                | gtk::gdk::EventMask::SCROLL_MASK,
        );
        // ... four observe()-wrapped connects: button-press-event, touch-event,
        // motion-notify-event, scroll-event.
    });
    if let Err(e) = result {
        eprintln!("gesture: with_webview failed, tap capture will never fire: {e}");
    }
}
```

Per event, in this order:

```rust
idle::note_activity();                       // BEFORE the cfg-12 gate — this is the fix
let Some(tap) = &tap else { return };        // cfg-12: observed, but no tap logic
if ev.is_pointer_emulated() { return }       // BUTTON handler only
let (x, y) = ev.position();
let (w, h) = (wv.allocated_width() as f64, wv.allocated_height() as f64);
if x >= 0.0 && y >= 0.0 && x < w && y < h && in_region(x, y, w, h, g.region) { … }
// the RefCell borrow produces `fired: bool` and is DROPPED before open_pin_pad
```

**The pointer-emulation guard sits on the button handler, never the touch handler.** GTK3 emulates a button press only from a touch sequence the widget left *unhandled*, so guarding the touch side would fail in the worse direction — touch skipped, no button follows, tap counts zero, leg 1 silently dead on all touch hardware. On the button side all three rows read 1 regardless of what WebKit does:

| Input | touch handler | button handler | taps counted |
|---|---|---|---|
| Real mouse click | — | not emulated → counts | 1 |
| Touch, WebKit consumes it | `TouchBegin` counts | (no button event) | 1 |
| Touch, GTK emulates a button | `TouchBegin` counts | emulated → skipped | 1 |

`TouchUpdate`/`TouchEnd`/`TouchCancel` are **activity only**.

**Bounds:** `in_region` performs no bounds check and the Windows caller supplies its own at `gesture.rs:254` — replicate it here deliberately.

Add the coordinate-frame ponytail verbatim:

```rust
// ponytail: assumes the webview's GdkWindow is coextensive with its allocation (one
// fullscreen child). If a second child ever lands in default_vbox, use
// WidgetExt::translate_coordinates (widget.rs:1863).
```

And the tap-count ceiling:

```rust
// ponytail: counting TouchBegin unconditionally means an N-finger corner tap counts N,
// where Windows counts one WM_LBUTTONDOWN. Safe for the lock (open_pin_pad only navigates
// to pinpad.html; PIN verification is elsewhere), not free for availability. The recorded
// upgrade — gating on EventTouch::is_emulating_pointer() — is deliberately NOT built: it
// would trade a verified-safe over-count for a field that cannot be verified set under
// Wayland touch. P2-G H4a gates it on real hardware.
```

- [ ] **Step 3: Verify**

Run: `cargo test -p kiosk-main gesture && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS — the existing D2c tests for `TapCounter`/`in_region`/`open_pin_pad` are unchanged and must all still pass.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/gesture.rs
git commit -m "feat(linux): pointer/touch observation with unconditional install (cfg-12 fix)"
```

---

### Task 5: `shortcuts.rs` Linux body — leg 2

**Files:**
- Modify: `crates/kiosk-main/src/shortcuts.rs:113` — replace the stub

**Interfaces:**
- Consumes: `observe()` (Task 3), `vk_from_keyval`/`mods_from_state` (Task 2), `should_swallow`, `is_technician_chord`, `open_pin_pad` — **all unchanged**
- Produces: the installed leg-2 handlers. Already called at `main.rs:1105`.

- [ ] **Step 1: Implement**

Key handlers on `gtk_window()` (`WebviewWindow::gtk_window()`), `observe()`-wrapped, stamping `idle::note_activity()` first:

```rust
idle::note_activity();
let Some(vk) = vk_from_keyval(ev.keyval()) else { return };
let mods = mods_from_state(ev.state());
if should_swallow(vk, mods) { return; }                 // guard, not swallow
if is_technician_chord(vk, mods) { open_pin_pad(&app, gesture.as_ref()); }
```

Connect both `key-press-event` and `key-release-event` (release is activity only). Window-level key handlers returning `Proceed` are tao's *entire* Linux keyboard path (`event_loop.rs:865,872`), live in every Tauri Linux app with a focused WebKitWebView — so this rides an in-tree precedent. **`add_events` is not needed on the window** (tao already set those masks); it **is** needed on the webview (Task 4).

**Scope of the `should_swallow` guard, stated exactly:** its only behavioural effect is the `mods.win` case. `is_technician_chord` tests `vk == VK_K`, and walking `should_swallow`'s table with `0x4B` matches no arm — the only reachable `true` for `VK_K` is `if mods.win { return true }`. Returning `Proceed` swallows nothing, so this does **not** close the F5/F11 residue and no such claim is made.

- [ ] **Step 2: Record the swallowing divergence**

In the module doc: `should_swallow` is deliberately **not** ported as a swallow mechanism — the parent's own `:693` says the in-app hook "is NOT a security boundary; OS-level Assigned Access / Shell Launcher is the covering boundary". *Stricter on Windows:* Ctrl+P, F5, F11 and the Menu/Apps key are swallowed in-app. *Looser on Linux:* they are not; the covering mechanisms are WebKitGTK settings (P2-B), the cage session, and §7.2/P2-G for VT switching. **F5 and F11 are covered by nothing and stay an accepted looser divergence.**

- [ ] **Step 3: Verify**

Run: `cargo test -p kiosk-main shortcuts && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: PASS — `technician_chord_is_matched_but_never_swallowed` and every other D2c test unchanged and green.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/shortcuts.rs
git commit -m "feat(linux): key observation and the technician chord on the GTK window"
```

---

### Task 6: PF-04 — the interactive-pinch intercept

**Files:**
- Modify: `crates/kiosk-main/src/gesture.rs` — inside the same `with_webview` closure as leg 1

**Interfaces:**
- Consumes: the webview widget already held by Task 4

- [ ] **Step 1: Implement**

```rust
// PF-04, the interactive-pinch half. Parent :685 — "interactive pinch is GTK-owned and
// needs a gesture-controller intercept in the platform layer ... validated on touch
// hardware in P2". P2-B owns base zoom (set_zoom_level); neither alone closes PF-04.
let zoom = gtk::GestureZoom::new(&webview);                        // gesture_zoom.rs:24
zoom.set_propagation_phase(gtk::PropagationPhase::Capture);        // event_controller.rs:70
zoom.connect_scale_changed(|g, _| {
    g.set_state(gtk::EventSequenceState::Claimed);                 // gesture.rs:209
});
std::mem::forget(zoom);   // the controller lives for the process, like the window
```

**What it suppresses:** two-finger scale sequences on the webview and `GDK_TOUCHPAD_PINCH`. **Not** single-finger pan (what `touch-action: pan-x pan-y` governs) and **not** two-finger touchpad scrolling, which arrives as `GDK_SCROLL`, a different event class.

- [ ] **Step 2: Record the residual and the fallbacks**

```rust
// Recorded residual: GestureZoom in gtk 0.18.2 exposes no recognition threshold —
// scale_delta() is the only knob — so claiming on the first scale-changed claims
// essentially any two-finger touchscreen sequence, including a two-finger pan whose
// fingers drift apart. P2-G H10's second clause ("two-finger pan/scroll still works") is
// the test that surfaces it, and a scale_delta() deadband is the fix if it fires.
// Recorded, not built.
//
// Fallbacks in the parent's own order if the capture-phase claim loses to WebKit:
// (1) a wry patch upstream (wry #544); (2) the parent's belt-and-suspenders
// `touch-action: pan-x pan-y` CSS, which is explicitly page-overridable and would ride
// RT-16's injection engine (ledger item I1) — so not a substitute.
```

- [ ] **Step 3: Verify**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: clean. The gate is **P2-G H10** on touch hardware — whether a capture-phase claim beats WebKitWebViewBase's own touch handling is a does-the-mechanism-work question with no GTK3/WebKitGTK sources to settle it here.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/gesture.rs
git commit -m "feat(linux): PF-04 capture-phase pinch intercept via GestureZoom"
```

---

### Task 7: Smoke scenarios 16–17

**Files:**
- Modify: `packaging/smoke/run-smoke.sh`
- Create: `packaging/smoke/fixtures/idle-short.json` (signed, short `idle_reset_seconds`), `packaging/smoke/fixtures/input-echo.html`

**Runner, settled:** cage 0.1.4 (the C7 floor) creates **no** `wlr_virtual_pointer_manager_v1` and **no** `wlr_virtual_keyboard_manager_v1` — both appear only from 0.1.5 — and XTEST reaches Xwayland clients only, not a native Wayland webview. **The CI driver on the floor is cage's own Xwayland:** run `kiosk-main` inside cage with `GDK_BACKEND=x11` so the webview is an Xwayland client, and drive it with `xdotool`. Scenarios 16–17 otherwise run under weston headless.

*Declared divergence:* this exercises GTK's **X11** GDK backend, not the Wayland one. That is faithful for what 16–17 assert — D's mechanism is GTK *widget signals*, not a Wayland protocol — and it is **not** a substitute for the Wayland input path, which stays hardware-gated at **P2-G H4a**.

- [ ] **Step 1: Scenario 16 — idle → clear (blocking)**

Needs **no input injection** — idleness is the absence of events. Short-threshold fixture → `IdleExpired` observed → profile clear runs → `ProfileCleared` → session cookie gone, kiosk back on home. **Also assert the latch:** no second fire while idle persists.

This is the first end-to-end app-path run of the clear-gate chain on Linux. P2-A's harness-binary scenario 6 stays as the completion unit check; scenario 16 supersedes it as the app-path proof.

- [ ] **Step 2: Scenario 17 — gesture + chord + activity-reset (blocking)**

Taps in the configured corner → pin pad opens. Technician chord (Ctrl+Alt+Shift+K via `xdotool`) → pin pad opens — this is also what pins the **Alt = `MOD1_MASK`** assumption. Synthetic motion resets the idle countdown (asserted via the latch not firing early). **And the page still receives a click and a keystroke after install** — the behavioural backstop for the `Proceed` rule, asserted through `input-echo.html`.

> The `Proceed` rule's **gate** is `rustc` via `observe()`, not scenario 17. Scenario 17's assertion is the backstop — which is what makes the safety invariant independent of whether 17 runs.

- [ ] **Step 3: Re-run P2-C's scenario 14 with D's chord**

C's technician-exit scenario gains its app-path driver from D's chord; re-run it under the same `GDK_BACKEND=x11` route, with the same declared divergence and the same hardware fallback (P2-G H2 for the systemd half, H4a for the touch half).

- [ ] **Step 4: Add one sentence to P2-G's G10**

G's register already reserves the slot: *the technician chord is the in-session escape under the locked cage session; the image intentionally leaves no VT/getty route.* **Leg 3 is withdrawn** — rev 1's "the image must retain exactly one administrative route to `systemctl stop`" was D's own invention and contradicted P2-G's default recipe; parent §3.5's "and/or" means legs 1+2 discharge the rule on their own.

- [ ] **Step 5: Run and commit**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: 16–17 PASS; 14's app-path half PASS or, if the Xwayland driver fails, recorded on the deferred hardware list against H4a — **recorded, not silently dropped**.

```bash
git add packaging/smoke
git commit -m "test(linux): smoke 16-17 — idle clear, gesture, chord, activity reset"
```

---

## Self-Review

**Spec coverage:** leg 1 → T4; leg 2 → T5; `observe()` → T3; idle clock → T1; keyval map → T2; PF-04 → T6; smoke 16–17 → T7; dependencies → T2 Step 3; the G10 sentence → T7 Step 4.

**Open decisions, each with a landing place:** `GDK_TOUCH_CANCEL` emission on Wayland → **P2-G H4a** records whether the panel emits it at all (residual until then: a cancelled touch counts as one tap toward N, bounded by `TAP_WINDOW_MS`, and it can only make the exit easier, never the lock weaker); `observe()`'s `'static` bounds → resolved in T3; H10's outcome and the `scale_delta()` deadband → T6 Step 2.

**Explicitly not D's:** the Linux touch keyboard and RT-16's `inject_css`/`inject_js` — D opens no file in `inject.rs`. (The keyboard half has since been built as **P2-B's B13**; RT-16 is deferred out of P2.) Webview-hang detection is **P2-C's C17**, which is also what covers a wedged GTK main loop; D asserts no covering control of its own.
