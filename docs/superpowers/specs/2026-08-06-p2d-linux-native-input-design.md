# P2-D — Linux Native Input: Idle Reset + Exit Gesture (Design)

> Fourth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.5 (idle reset &
> exit gesture), §5.2 (gesture config), §7 (zoom-lock row / PF-04). **Builds on P1-D2c** (the
> pure decision layer — `idle::should_fire`, `gesture::{in_region, TapCounter,
> effective_gesture, open_pin_pad}`, `shortcuts::{should_swallow, is_technician_chord}` — all
> host-tested and reused **verbatim, unedited**) and on P2-A/B/C. Linux replaces only the
> input *observation* source; every decision stays where D2c put it.

**Status:** rev 2, 2026-08-07 — adversarial design review; see
`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/`.

## Goal

`idle_reset_seconds` fires `IdleExpired` (completing the app-path `ClearProfile` →
`ProfileCleared` chain that P2-A could only reach via a harness binary), and both exit paths
— N corner taps and the technician keyboard chord — open the pin pad on Linux. PF-04's
interactive-pinch intercept lands on the same widget. The "never unexitable" rule
(parent §3.5:318-320, restated at `gesture.rs:12-15`) and cfg-12's no-fail-open semantics
(`gesture.rs:17-23`) carry over unchanged.

## Scope

**In:** Linux (`#[cfg(not(windows))]`) bodies for the three existing stubs —
`gesture.rs:194`, `shortcuts.rs:113`, `idle.rs` — the keyval→VK chord mapping, the PF-04
pinch intercept, and smoke scenarios 16–17. **No new module and no `main.rs` diff.**

**Out:** VT/console lockdown, seat permissions, the hardware checklist → P2-G. Offline video
→ P2-E. Webview-hang detection (arch-04 / RT-02 / OD-1) → **P2-C, change C17** (a JS
round-trip on the heartbeat), which is also what covers a wedged GTK main loop; see
§Divergence. **The Linux touch keyboard (parent §7 Linux row) is explicitly not D's** — it
and RT-16's `inject_css`/`inject_js` are one unowned row, carried as review ledger item
**I1**; D opens no file in `inject.rs`.

## Architecture — GTK widget signals, two independent legs

Windows scatters observation across **three** vectors, not two: `WH_MOUSE_LL` on a dedicated
thread (`gesture.rs:204-327`), `WH_KEYBOARD_LL` on another (`shortcuts.rs:208-256`), and the
chord — which rides neither, but `AcceleratorKeyPressed` (`shortcuts.rs:184-190`, the sole
`is_technician_chord` call site; the LL hook calls only `should_swallow`) — plus a system poll
(`GetLastInputInfo`, `idle.rs:85`, loop at `:95-110`). On Linux the kiosk is one fullscreen GTK
window in a compositor with no other client, and every user input event the kiosk cares about is
already delivered to two GTK objects Tauri hands us. Observe them; do not get between GDK and
GTK.

| Leg | Object | Signals | Feeds |
|---|---|---|---|
| 1 | `webkit2gtk::WebView` (via `with_webview` → `PlatformWebview::inner()`, `tauri-2.11.5/src/webview/mod.rs:173`) | `button-press-event` (`gtk-0.18.2/src/auto/widget.rs:2015`), `touch-event` (`:3899`), `motion-notify-event` (`:3224`), `scroll-event` (`:3549`), after `add_events(BUTTON_PRESS_MASK\|TOUCH_MASK\|POINTER_MOTION_MASK\|SCROLL_MASK)` | tap capture + activity |
| 2 | `gtk::ApplicationWindow` (`WebviewWindow::gtk_window()`, `tauri-2.11.5/src/webview/webview_window.rs:1861`) | `key-press-event` (`widget.rs:3035`), `key-release-event` (`:3068`) | technician chord + activity |

`webkit2gtk::WebView` is `@extends gtk::Container, gtk::Widget`
(`webkit2gtk-2.0.2/src/auto/web_view.rs:58`), and `WidgetExt`'s blanket impl
(`widget.rs:4840`) reaches it through `IsA<gtk::Widget>`, so the signals above are available
on both objects with no `webkit2gtk` import in the handler code.

**The split is required, not stylistic.** GTK3 delivers pointer events to the widget owning
the `GdkWindow` and propagates up only if unhandled: tao connects
`button-press`/`touch`/`motion`/`scroll` on the *window* (`tao-0.35.3/src/platform_impl/
linux/event_loop.rs:496,521,556,785`, masks at `:471-478`) and those are starved when the
webview consumes — which is precisely why wry attaches its own to the webview
(`wry-0.55.1/src/webkitgtk/synthetic_mouse_events.rs:10,15`, called unconditionally from
`mod.rs:463`). Keys are the other way round: `event_loop.rs:865,872` — window-level
`connect_key_press_event`/`connect_key_release_event` returning `Propagation::Proceed` — is
tao's *entire* Linux keyboard path, live in every Tauri Linux app with a focused
WebKitWebView. Two in-tree precedents, one per event family, matching GTK3's actual asymmetry.
`add_events` is therefore needed on the webview and **not** on the window (tao already set
those masks).

**Ordering.** gtk-rs connects with plain `g_signal_connect_data` and no `G_CONNECT_AFTER`
(`widget.rs:3054-3062`), so user handlers precede the class closure on these `RUN_LAST`
signals — confirmed behaviourally by wry returning `Propagation::Stop` at
`synthetic_mouse_events.rs:44` to keep WebKit from handling buttons 8/9, which can only work
if the user handler runs first.

**`observe()` — the `Proceed` rule is enforced by the compiler, not by convention.** Every
`Propagation`-returning handler is built by one wrapper that supplies the return value, so no
handler *body* can return `Stop`:

```rust
fn observe<W, E>(f: impl Fn(&E) + 'static) -> impl Fn(&W, &E) -> glib::Propagation {
    move |_, e| { f(e); glib::Propagation::Proceed }
}
```

Five call sites, one function, checked by `rustc` on every build. The rule this makes
un-violatable is D's entire safety argument: **we are never on the dispatch path, so a defect
in our code costs at most our own feature and never the webview's input.** There is **no
legitimate exception** — PF-04's intercept is an `EventController`, not a `Propagation`
handler (below) — so any inline `connect_*` returning anything but `Proceed` is a reviewable
smell rather than a designed carve-out. `observe` does not stop a future edit from adding a
sixth handler inline; that is what the convention plus review covers, and it is still strictly
better than a scenario that may not run. Plan-time shim: `connect_*` requires `F: … + 'static`
(`widget.rs:2017`, `:3035`), and an RPIT over unbounded `W, E` is not known to be `'static` —
it will need `W: 'static, E: 'static` or `+ 'static` on the return type.

### Rejected, recorded

**`gdk::Event::set_handler` / `gdk_event_handler_set` — rejected on evidence.** This was rev
1's central mechanism and it is withdrawn. The slot is not free: **GTK itself owns it.**

```
$ objdump -R /usr/lib/x86_64-linux-gnu/libgtk-3.so.0 | grep event_handler_set
00000000007c4788 R_X86_64_JUMP_SLOT  gdk_event_handler_set@Base
# exactly one call site in the whole library, in GTK's init path
# (do_post_parse_initialization, gtk/gtkmain.c):
1fdbcd: lea 0x591c(%rip),%rdi   # 2034f0 <gtk_main_do_event@@Base>
1fdbd4: xor %edx,%edx           # destroy = NULL
1fdbd6: xor %esi,%esi           # data    = NULL
1fdbd8: call 91df0 <gdk_event_handler_set@plt>
```

Reproduced independently by both review roles (environment: Ubuntu 24.04,
libgtk-3.so.0.2409.32 — above the C7 floor, which does not matter, because the finding is
what *kills* the design, not what supports a replacement). Consequences, each verified:

- `gdk_event_handler_set` stores **one** function pointer with **no chaining API**. "Chain to
  GTK's handler" therefore *is* calling `gtk::main_do_event` yourself — i.e. the withdrawn
  design. There is no narrower fix.
- The forwarded event is **not the same event**: `gdk-0.18.2/src/event.rs:65`
  `from_glib_none(event)` → `glib-0.18.5/src/boxed.rs:482-485` → `gdk_event_copy`. GTK would
  dispatch a copy, and whether GTK3's `GdkEventPrivate` state survives that for
  `gtk_main_do_event`'s purposes is a GTK3 C-internals question with no GTK3 sources
  available — genuinely unverifiable, sitting on the only path by which the product receives
  input. Plus an alloc/free per `MotionNotify` on the main thread.
- The install **can** fail: `event.rs:58` `assert_initialized_main_thread!()` panics
  (`rt.rs:16-25`), and `gtk::main_do_event` re-asserts per event. Rev 1's error model rested
  on "it cannot fail", which is false.
- **The disqualifier is blast radius.** Because `set_handler` *replaces* dispatch, any defect
  in our handler — a panic across the `extern "C"` trampoline (`event.rs:59`, UB/abort in
  Rust 2021), a `RefCell` reentry, a missed forward — takes **all input to the webview** with
  it. The kiosk becomes a black, unclickable, un-exitable pane: exactly the state parent
  §3.5:319-320 forbids, produced by our own code. FRAME Q4 and §3.5 point the same way.

Cost of the replacement: five `connect_*` calls instead of one `set_handler`. Bought: zero
`unsafe`, zero per-event copy, zero process-global slot, zero reentrancy analysis, zero
GTK-dispatch re-implementation, one new dependency instead of two, one unverifiable retired
rather than pinned.

**evdev — rejection upheld.** Privilege, hotplug, and touchscreen→window transforms:
re-deriving coordinates GTK hands us for free in the exact window-relative form `in_region`
already takes. On Windows the hook must convert screen→window itself (`gesture.rs:249-253`);
Linux skips that step, and gets the widget's own `w`/`h` from the same object.

**`ext-idle-notify-v1` — rejection upheld, parked as the fallback.** It solves idle only; the
gesture and chord still need the GTK path, so it can replace §`idle.rs`'s clock and nothing
else. It costs `wayland-client` as a first-of-its-kind direct dependency (`grep '^name =
"wayland' Cargo.lock` → zero rows), which is why it stays parked rather than pre-emptively
adopted. Taken only if the per-window clock fails smoke 16/17 in the field.

## Components

### `gesture.rs` — Linux body (leg 1)

The existing stub at `gesture.rs:194` becomes the real Linux body. It takes exactly the
arguments leg 1 needs — `(&tauri::WebviewWindow, tauri::AppHandle, Option<EffectiveGesture>)`
— and is already called at `main.rs:1106` inside the setup closure. **Zero `main.rs` diff, no
new module.**

Inside `with_webview`, `add_events(…)` then the four `observe()`-wrapped handlers. Per event:

```rust
// Observation installs UNCONDITIONALLY: these handlers feed BOTH the exit gesture and
// idle reset. cfg-12 disables the *gesture*, not *observation*.
let tap = gesture.map(|g| RefCell::new((TapCounter::new(g.taps, TAP_WINDOW_MS), g)));

// in each handler, via observe():
idle::note_activity();                       // BEFORE the cfg-12 gate — this is the fix
let Some(tap) = &tap else { return };        // cfg-12: observed, but no tap logic
if ev.is_pointer_emulated() { return }       // button handler only (see below)
let (x, y) = ev.position();
let (w, h) = (wv.allocated_width() as f64, wv.allocated_height() as f64);
if x >= 0.0 && y >= 0.0 && x < w && y < h && in_region(x, y, w, h, g.region) { … }
// borrow produces `fired: bool` and is DROPPED before open_pin_pad
```

**cfg-12 handling is the safety fix of this revision.** The Windows body opens with an early
return when `effective_gesture` yields `None` (`gesture.rs:293-296`) — a supported, documented
configuration state (`gesture.rs:17-23`), not a failure. Mirroring that arm on Linux would
install *no pointer or touch handler at all*, so on a cfg-12-unconfigured keyboardless kiosk
nothing would ever stamp the idle clock, `should_fire` (`idle.rs:32-34`) would fire, and
`(Online, IdleExpired) if idle_clear → Effect::ClearProfile { full: true }`
(`kiosk-core/src/app/state.rs:296-304`) would wipe a live session while the user is tapping —
silently, the only log line being the benign cfg-12 notice. So: **handlers install
unconditionally, `note_activity()` runs before the gate, `TapCounter` is `gesture.map(…)`, and
the tap branch early-returns on `None`.**

This is not a new convention. `shortcuts.rs` has **no** early return on `gesture` anywhere: the
`Option` is carried through `install` (`:104-110`), `install_accelerator_handler` (`:157-160`)
and `windows_impl::install` (`:258-263`) all the way to `open_pin_pad(&app, gesture.as_ref())`,
relying on that function's own `None` guard at `gesture.rs:154-157`. **`gesture.rs:293-296` is
the outlier**; the sibling module already implements the prescribed shape. The Windows early
return is an optimisation — it skips installing a hook that would do nothing — which is free on
Windows and stops being free on Linux, where the same handlers have a second, cfg-12-independent
consumer.

cfg-12 semantics with handlers installed unconditionally, traced: (1) `gesture == None` ⇒ no
`TapCounter` ⇒ the tap branch returns before any outward call; (2) leg 2 reaches
`open_pin_pad(&app, None)`, which self-guards at `:154-157` and returns — byte-identical to
Windows Vector A at `shortcuts.rs:190`; (3) the only outward call on that path is
`idle::note_activity()`. **Nothing exits and nothing unlocks when unconfigured.**

**Tap counting — the pointer-emulation guard sits on the button handler.** GTK3 emulates a
button press only from a touch sequence the widget left *unhandled*, so guarding the touch
side would re-import that unverifiable and fail in the worse direction (touch skipped, no
button follows, tap counts zero — leg 1 silently dead on all touch hardware). Guarding the
button side is correct under both branches:

| Input | touch handler | button handler | taps counted |
|---|---|---|---|
| Real mouse click | — | not emulated → counts | 1 |
| Touch, WebKit consumes it | `TouchBegin` counts | (no button event) | 1 |
| Touch, GTK emulates a button | `TouchBegin` counts | emulated → skipped | 1 |

All three rows read 1 **regardless of what WebKit does** — the unverifiable stops being
load-bearing instead of merely changing which side it breaks. `gdk::Event::is_pointer_emulated()`
is at `gdk-0.18.2/src/event.rs:302-304`, and `event_wrapper!`'s `impl Deref<Target = Event>`
(`event.rs:522-528`) with `pub struct EventButton(crate::Event)` (`event_button.rs:6`) makes it
a plain method call on the `&gdk::EventButton` the signal hands us — no downcast. `TouchUpdate`
/`TouchEnd`/`TouchCancel` are activity only.

**Bounds and constants.** `in_region` performs **no** bounds check (`gesture.rs:34-43`;
`TopLeft` is true for negative coordinates) and the Windows caller supplies its own at
`gesture.rs:254` — replicated here deliberately. `TAP_WINDOW_MS` **loses its `#[cfg(windows)]`**
(`gesture.rs:181-182`): one-line deletion, one shared 3000 ms constant, no second constant and
no silent parity divergence.

**Coordinate frame — declared assumption.** `EventButton::position()` (`event_button.rs:19`)
and `EventTouch::position()` (`event_touch.rs:21`) return the raw `event->x/y`, relative to the
`GdkWindow` the event was delivered to; `allocated_width`/`allocated_height` (`widget.rs:494`,
`:473`) are the widget's allocation. They coincide under Tauri's single-fullscreen-child
`default_vbox` layout (`webview_window.rs:1874`), which is the only layout this kiosk ships.
That is an assumption about layout, not an identity, and the bounds check would silently pass a
shifted frame. `// ponytail: assumes the webview's GdkWindow is coextensive with its allocation
(one fullscreen child). If a second child ever lands in default_vbox, use
WidgetExt::translate_coordinates (widget.rs:1863).`

### `shortcuts.rs` — Linux body (leg 2)

The stub at `shortcuts.rs:113` becomes the real Linux body, installed from the existing call
site at `main.rs:1105`. Key handlers on `gtk_window()`, `observe()`-wrapped, stamping
`idle::note_activity()` first:

```rust
use gtk::gdk::keys::constants as k;
let vk = match ev.keyval() { k::K | k::k => VK_K, _ => return };
let m = ev.state();
let mods = Modifiers {
    ctrl:  m.contains(CONTROL_MASK),
    alt:   m.contains(MOD1_MASK),
    shift: m.contains(SHIFT_MASK),
    win:   m.intersects(MOD4_MASK | SUPER_MASK),
};
if should_swallow(vk, mods) { return; }                 // guard, not swallow
if is_technician_chord(vk, mods) { open_pin_pad(&app, gesture.as_ref()); }
```

**`is_technician_chord` is not touched.** Rev 1 proposed adding `&& !mods.win` at the shared
root; that is **withdrawn**. The function's own doc comment forbids exactly that edit —
*"Deliberately checked INDEPENDENTLY of [`should_swallow`] (never folded into that table) … the
two decisions must never be layered on the same key"* (`shortcuts.rs:88-101`), pinned by
`technician_chord_is_matched_but_never_swallowed` (`:395-402`). The replacement is a
`should_swallow` **guard** at the Linux call site, reproducing Vector A's ordering verbatim
(`shortcuts.rs:184-190`): zero diff to shared reviewed code, zero D2c test delta, zero
cross-platform blast radius.

Scope of the guard, stated exactly: its **only** behavioural effect is the `mods.win` case.
`is_technician_chord` tests `vk == VK_K`, and walking `should_swallow`'s table
(`shortcuts.rs:66-87`) with `0x4B` matches no `match` arm — the only reachable `true` for `VK_K`
is `if mods.win { return true }` at `:71-73`. Returning `Proceed` swallows nothing, so it does
**not** close the F5/F11 residue below and no such claim is made.

**Keyval set — settled, one key.** `shortcuts.rs:58` `const VK_K: u32 = 0x4B;` is the sole chord
constant; `gdk::keys::constants::{K, k}` are at `gdk-0.18.2/src/keys.rs:886,952`, `Key` is a
`pub struct Key(u32)` with `Deref<Target = u32>` and derived `PartialEq/Eq` (`:8-16`), so the
two-arm match compiles. Both cases are matched because Shift is held.

**Declared assumption: Alt = `MOD1_MASK`.** `ModifierType` has no `ALT_MASK`
(`gdk-0.18.2/src/auto/flags.rs:563-620`); Mod1 is an XKB convention. Pinned by smoke 17.
Residual: an exotic layout mapping Alt off Mod1 kills leg 2 only — leg 1 still exits the device,
so §3.5 holds.

**`should_swallow` is deliberately not ported as a swallow mechanism.** The justification is the
parent's own: `:693` says the in-app hook *"is NOT a security boundary; OS-level Assigned Access
/ Shell Launcher is the covering boundary (§7.2, §12/OD-5)"*, and OD-5 (`:927`) repeats it. It is
defence-in-depth on the **lockdown** side and was never a leg of the **exit** chain, so not
porting it removes nothing from §3.5. Divergence, both directions (C3): *stricter on Windows* —
Ctrl+P, F5, F11 and the Menu/Apps key are swallowed in-app; *looser on Linux* — they are not,
and the covering mechanisms are WebKitGTK settings (P2-B: `set_enable_developer_extras`,
`connect_context_menu` for the Menu key), the cage session for shell chords, and §7.2/P2-G for
VT switching. **F5 (reload) and F11 (fullscreen under an already-fullscreen cage surface) are
covered by nothing and stay an accepted looser divergence**, stated rather than hidden.

### `idle.rs` — Linux body

Module-local clock, no plumbing, no signature change:

```rust
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);   // 0 = no observation source yet

pub fn note_activity() { LAST_INPUT_MS.store(now_ms().max(1), Relaxed); }

fn idle_secs() -> u64 {
    match LAST_INPUT_MS.load(Relaxed) {
        0 => 0,                              // no source ⇒ "not idle" — mirrors idle.rs:88-89
        t => now_ms().saturating_sub(t) / 1000,
    }
}
```

`run`'s Linux arm keeps the identical 1 s poll and `should_fire` latch. The GTK main thread
stores, a tokio worker loads; `Relaxed` is correct for a lone monotonic timestamp with nothing
ordered by it.

**The `0` sentinel mirrors the Windows convention deliberately.** `idle.rs:78-79` — *"Falls back
to 'not idle' (0) if the Win32 call fails, rather than risking a false idle-fire off garbage
data"* — implemented as the `else { 0 }` at `idle.rs:88-89`. Rev 1's `max(loop_start_ms, …)`
covered the boot window but silently inverted that choice for a permanently degraded install:
`idle_secs` would grow monotonically, `should_fire` would fire once, and the FSM would wipe a
live session. With the sentinel, if both leg installs degrade nothing ever calls
`note_activity()`, the clock stays at `0`, and nothing fires — loud (both install errors log)
and in the direction Windows chose. `.max(1)` keeps a real first stamp from colliding with the
sentinel, at ≤1 ms of skew against a 1 s poll; `ponytail:` comment on the sentinel naming the
ceiling. The sentinel also subsumes the boot window: `idle::run` spawns at `main.rs:917`,
installs happen at `main.rs:1105-1106`.

- **`idle::run`'s signature is unchanged and `main.rs:917` is untouched** — a single,
  non-`cfg`-gated `tokio::spawn`. There is no handle to plumb, so the spawn-before-install
  ordering is a non-issue.
- **`idle_secs_from_ticks` (`idle.rs:44`) is not `#[cfg(windows)]`, and must stay that way.**
  Rev 1 claimed it already carried the attribute and would keep it; both halves were false.
  It has no `cfg`, its test `idle_secs_is_wrap_safe_across_the_32bit_tick_boundary`
  (`idle.rs:129-139`) is unconditional, and the ubuntu `cargo test --workspace` job
  (`.github/workflows/ci.yml:24`) compiles both. **Zero diff is the correct diff** — adding the
  attribute would break Linux CI (C8/C9).
- SEC-09's never-cancelled property (`idle.rs:16-24`) carries over unchanged — same `cancel`
  token, same wiring.

### PF-04 — interactive-pinch intercept

Parent `:685`: *"WebKitGTK fixed `zoom-level` — note this fixes only base zoom, **interactive
pinch is GTK-owned and needs a gesture-controller intercept in the platform layer** / a wry
patch, validated on touch hardware in P2 (wry #544, PF-04)"*; `:894` repeats it. That half was
owned by no spec in A–G. **D owns it**, on the parent's own words and because D1 already holds
the widget:

```rust
let zoom = gtk::GestureZoom::new(&webview);                        // gesture_zoom.rs:24
zoom.set_propagation_phase(gtk::PropagationPhase::Capture);        // event_controller.rs:70
zoom.connect_scale_changed(|g, _| { g.set_state(EventSequenceState::Claimed); }); // :46 / gesture.rs:209
std::mem::forget(zoom);   // the controller lives for the process, like the window
```

Bindings all verified in `gtk-0.18.2`: `GestureZoom` `@extends Gesture, EventController`
(`auto/gesture_zoom.rs:15`), `new(&impl IsA<Widget>)` (`:24`), `connect_scale_changed` (`:46`),
`GestureExt::set_state(EventSequenceState) -> bool` (`auto/gesture.rs:209`),
`EventSequenceState::Claimed` (`auto/enums.rs:2362`), `PropagationPhase::Capture` (`:6642`),
`EventControllerExt::set_propagation_phase` (`auto/event_controller.rs:70`). No new dependency.

**This is not an exception to the always-`Proceed` rule.** `GestureZoom` is an
`EventController`; `connect_scale_changed`'s closure returns `()` and `set_state` returns
`bool` — neither is on the `-> glib::Propagation` surface at all. It suppresses by claiming a
sequence, a different mechanism entirely, so `observe()` still covers 100% of that surface with
no carve-out.

**What it suppresses:** two-finger scale sequences on the webview and `GDK_TOUCHPAD_PINCH`. Not
single-finger pan (which is what the parent's `touch-action: pan-x pan-y` intent governs) and
not two-finger touchpad scrolling, which arrives as `GDK_SCROLL` (`enums.rs:1599`), a different
event class from `TouchpadPinch` (`:1621`).

**Gate.** Whether a capture-phase claim beats WebKitWebViewBase's own touch handling is a
does-the-mechanism-work question and there are no GTK3/WebKitGTK sources to settle it here. The
parent names the gate — *"validated on touch hardware in P2"* — and P2-G owns the checklist:
**P2-G row H10, "pinch-zoom does not scale the page on touch hardware; two-finger pan/scroll
still works"** (implementing spec D, gating spec G; the row exists in G's register).

**Recorded residual.** `GestureZoom` in gtk 0.18.2 exposes no recognition threshold —
`scale_delta()` (`gesture_zoom.rs:39-42`) is the only knob — so claiming on the first
`scale-changed` claims essentially any two-finger touchscreen sequence, including a two-finger
pan whose fingers drift apart. H10's second clause is the test that surfaces it and a
`scale_delta()` deadband is the fix if it fires. Recorded, not built.

**Fallbacks, in the parent's own order, if the claim loses to WebKit:** (1) a wry patch upstream
(wry #544, `:685`); (2) the parent's belt-and-suspenders `touch-action: pan-x pan-y` CSS
(`:685`, explicitly page-overridable, so not a substitute), which would ride RT-16's injection
engine — i.e. ledger item I1. Recorded so an H10 failure has a next step rather than reopening
the design.

**PF-04 splits cleanly:** P2-B owns base zoom (`set_zoom_level`), D owns the interactive-pinch
intercept. Neither alone closes PF-04; together they do.

### The clear-gate chain completes

With `IdleExpired` live, `Online + idle_clear` → `Effect::ClearProfile { full: true }`
(`kiosk-core/src/app/state.rs:296-304`) → `clear::clear` (A's body) → `ProfileCleared` runs
end-to-end through the app for the first time on Linux. A's harness-binary scenario (A smoke 6,
`p2a:303-305`, which names P2-D as its successor) stays as the completion unit check; D's smoke
16 supersedes it as the app-path proof.

## Dependencies

`[target.'cfg(target_os = "linux")'.dependencies]`:

- **`gtk = "0.18"` — one new direct dependency.** `gdk` and `glib` arrive as `gtk::gdk` /
  `gtk::glib` (`gtk-0.18.2/src/lib.rs:18,21`), so this is strictly one crate, not two or three.
  Justified in P2-A's own template: no new `Cargo.lock` entries (`gdk 0.18.2` at
  `Cargo.lock:1101-1102`, `gtk 0.18.2` at `:1345-1346`), no new compile units (both already
  built for wry/tauri/webkit2gtk), version unification guaranteed by the lock so our types
  cannot diverge from Tauri's, and **the dependency is forced by Tauri's own public signature**
  — `gtk_window()` returns `gtk::ApplicationWindow`, a type we cannot name without the crate.
  No second GTK binding over one C library. Both ubuntu CI jobs already install
  `libgtk-3-dev` (`.github/workflows/ci.yml:16-18,49-52`).
- **`webkit2gtk = { version = "2.0.2" }`, target-gated, no features beyond P2-B's `["v2_32"]`.**
  D's mechanism is stated on `webkit2gtk::WebView` and neither wry nor Tauri re-exports the
  crate (`wry-0.55.1/src/lib.rs:363-415`), so D declares it. This is a **`Cargo.toml`
  declaration, not an artifact one spec hands another**: the same line is written by P2-B (B10)
  and used by P2-C (C17), Cargo unions features across a single declaration, and D needs nothing
  above B10's floor — `WidgetExt` and the signal connects are ungated. **Whichever spec lands
  first writes the line; the others reconcile by union. There is no B→D ordering edge.**

## Divergence from Windows (C3), both directions

- **Stricter / better:** Windows' two hook vectors carry the Tauri #13919 caveat
  (`shortcuts.rs:18`, `gesture.rs:10-11`: Windows silently starving low-level hooks while the
  webview holds focus). There is no GDK analogue — leg 1 rides an in-widget signal on the
  focused webview and leg 2 rides the same window-level path that is tao's entire Linux keyboard
  input. Both exit paths are kept anyway, for parity with the two-vector design.
- **Looser — liveness:** both Linux legs are dispatched by the one GTK main loop. A wedged main
  iteration disables observation on both at once, where Windows' legs run on dedicated OS
  threads with their own message pumps (`gesture.rs:286-322`, `shortcuts.rs:239-252`). P2-A's
  GTK-main-thread rule (`p2a:76-77`) forbids moving them, so this is a documentation defect, not
  a design one. Narrowing, verified: `open_pin_pad` ends in `window.navigate` (`gesture.rs:167`),
  which needs the UI thread on **both** platforms — so the divergence is in *observation
  liveness*, not in *exit capability*. **The wedged-loop case itself is covered by P2-C's C17**
  (a `run_javascript` round-trip on the heartbeat under a 3 s cap; a wedged GTK loop withholds
  pings, the existing 3-missed rule restarts main). D asserts no covering control of its own.
- **Looser — idle scope:** Windows measures *system-wide* last input (`GetLastInputInfo`); Linux
  measures *our-window* last input. Input to a second Wayland client is invisible to us. The
  review's INT-5 finding narrows this: an XTEST/Xwayland OSK would produce no GDK events in our
  process, while a `zwp_virtual_keyboard_v1` client injects at the seat and *would* — but has no
  way to display itself under cage on either 0.1.4 or 0.1.5 (`zwlr_layer_shell_v1` absent on
  both). Pinned by smoke 17's activity-reset assertion. Residual: idle reset can fire while a
  technician is at the pad but not typing.
- **Looser — tap counting:** counting `TouchBegin` unconditionally means an N-finger corner tap
  counts N, where Windows' `mouse_hook` counts one `WM_LBUTTONDOWN` per tap
  (`gesture.rs:244-245`). **Safe for the lock, not free for availability.** Safe: `open_pin_pad`
  (`gesture.rs:153-173`) only navigates to `pinpad.html`; it verifies nothing, and PIN
  verification is P1-D2c Task 5's — an over-count cannot weaken the lock. Not free: a two-finger
  press five times reaches the bootstrap default of 7 taps where Windows needs 7 deliberate taps,
  `pinpad.html` carries no cancel/back affordance, and an unintended opening plausibly strands
  the session until a correct PIN is entered. Identical in kind on Windows; D only raises its
  frequency. **P2-G H4a** gates it on real touch hardware. **Recorded upgrade, deliberately not
  built:** gate the touch count on `EventTouch::is_emulating_pointer()` (`event_touch.rs:32`),
  which GDK sets on exactly one sequence per gesture — declined now because it would trade a
  verified-safe over-count for a dependence on a field that cannot be verified set under Wayland
  touch, which is the mistake the button-side placement above just corrected. `ponytail:` comment
  naming that ceiling.
- **Looser — swallowing:** F5 and F11 are not swallowed on Linux; see `shortcuts.rs` above.
- **cfg-12 shape, behaviourally identical:** the module doc phrases cfg-12 as *"the tap-capture
  hook no-ops"*. Windows implements that as **not installing**; Linux as **installing and
  no-opping the tap logic**, because the same handler has a second, cfg-12-independent consumer.
  No pad opens either way, so the observable behaviour is the same.

**Leg 3 is withdrawn.** Rev 1 stated a constraint on P2-G — "the hardened image must retain
exactly one administrative route to `systemctl stop` the kiosk unit" — as a third fallback leg.
That was **D's own invention**: parent §3.5:318-320 reads *"a reserved `AcceleratorKeyPressed`
technician chord **and/or** the §7.2 OS-lockdown escape, so a locked device is never
unexitable"*. "and/or" means legs 1+2 discharge the rule on their own, and the constraint
contradicted P2-G's default recipe (`NAutoVTs=0`/`ReserveVT=0`, `systemctl mask getty@.service`,
SSH absent by default). D contributes **one sentence** to G10, where G's register already
reserved the slot: *the technician chord is the in-session escape under the locked cage session;
the image intentionally leaves no VT/getty route.*

## Error handling

`gtk_window()` and `with_webview` both return `Result`. Each leg installs independently:
separate `Result`s, separate log lines, separate telemetry events, at two call sites that
already exist. If one fails to install, that is logged loudly and the other still exits the
device.

Degradation follows C4 and the repo's existing `eprintln!` + telemetry shape
(`shortcuts.rs:200-206`, `gesture.rs:315-320`): log and continue, never block boot. Both installs
degrading leaves `LAST_INPUT_MS` at the `0` sentinel, so nothing fires — the failure door and the
cfg-12 configuration door are both closed.

Panic containment is structural, not a rule to remember: no `unwrap`/index in the classifiers,
and the `RefCell` borrow is scoped to produce a `fired: bool` and dropped before `open_pin_pad`,
so reentry through a nested GTK main loop cannot hit a live borrow (mirroring `mouse_hook`'s own
documented lock discipline, `gesture.rs:270-273`). **No `catch_unwind`** — it would convert a
loud abort into a silently dead feature, which FRAME Q3 rates worse.

Rev 1's sentence *"cfg-12's no-fail-open semantics: nothing exits, nothing unlocks"* as a
consolation for a failed install is **deleted, not softened**: it asserted the forbidden state
and cited cfg-12 (`gesture.rs:17-23`) for a rule that lives at `gesture.rs:12-15`.

## Smoke additions

16. **idle → clear (blocking; needs no input injection — idleness is the absence of events):**
    short-threshold fixture → `IdleExpired` observed → profile clear runs → `ProfileCleared` →
    session cookie gone, kiosk back on home. Also asserts the latch: no second fire while idle
    persists.
17. **gesture + chord + activity-reset (blocking):** taps in the configured corner → pin pad
    opens; technician chord → pin pad opens; synthetic motion resets the idle countdown
    (asserted via the latch not firing early); **and the page still receives a click and a
    keystroke after install** — the behavioural backstop for the `Proceed` rule.

**Runner, settled (INT-3).** Rev 1 left "whether cage-headless exposes the wlr virtual-input
protocols" as an open plan-time question. It is **closed negatively for the C7 floor**: cage
0.1.4-4 (Debian 12) creates no `wlr_virtual_pointer_manager_v1` and no
`wlr_virtual_keyboard_manager_v1` (full `*_create(` list from `cage.c`; both appear only from
0.1.5). XTEST reaches Xwayland clients only, not a native Wayland webview. **The CI driver on the
floor is cage's own Xwayland:** run `kiosk-main` inside cage with `GDK_BACKEND=x11` so the webview
is an Xwayland client, and drive it with `xdotool` (`wlr_xwayland_create` is present on 0.1.4).
P2-F's nightly `debian:12` container carries `cage`, `xwayland` and `xdotool` for this; scenarios
16–17 otherwise run under weston headless. **Declared divergence (C3, a stricter statement of what
is proved):** the run exercises GTK's X11 GDK backend, not the Wayland one. That is faithful for
what 16–17 assert — D's mechanism is GTK *widget signals*, not a Wayland protocol — and it is
**not** a substitute for the Wayland input path, which stays hardware-gated at **P2-G H4a**.
Fallback if even that fails: 17 moves to the deferred hardware list against H4a — recorded, not
silently dropped.

The `Proceed` rule's **gate** is `rustc` via `observe()`, not scenario 17; 17's assertion is the
backstop. That is what makes the safety invariant independent of whether 17 runs.

## Testing

- **Host tests:** the keyval→VK map (both directions, all chord keys) and the activity-set
  classification, as pure functions. `TapCounter`/`in_region`/`should_fire`/`is_technician_chord`
  /`should_swallow`/`open_pin_pad` are already pinned by D2c's tests and are **unchanged** — no
  D2c test delta from this sub-project. `idle_secs_from_ticks`'s unconditional test keeps running
  on Linux CI.
- **Compile-time:** `observe()` is the gate for the always-`Proceed` invariant, checked on every
  build including the Windows path.
- **Smoke:** 16–17 above; C's scenario 14 (technician exit 86) gains its app-path driver from D's
  chord and is re-run under the same `GDK_BACKEND=x11` route, with the same declared divergence
  and the same hardware fallback (P2-G H2 for the systemd half, H4a for the touch half).

## Open decisions to resolve at plan time

- `GDK_TOUCH_CANCEL` emission on Wayland — a cancelled touch should not count as a tap. Owner:
  **P2-G H4a**, which records whether the panel emits it at all. Residual until then: a cancelled
  touch counts as one tap toward N, bounded by `TAP_WINDOW_MS`, and it can only make the exit
  easier, never the lock weaker.
- `observe()`'s `'static` bounds (the RPIT shim above). A shim, which FRAME Q5 permits at plan
  time.
- H10's outcome, and whether the `scale_delta()` deadband is needed.

## Scope / defer

VT/console lockdown, seat permissions, the hardware checklist (H4a, H10) → P2-G. Offline-video
soak → P2-E. Webview-hang detection → P2-C (C17). The Linux touch keyboard and RT-16's
`inject_css`/`inject_js` → review ledger item **I1**, which has no owner inside A–G; D opens no
file in `inject.rs` and claims no part of that row. The `ext-idle-notify-v1` fallback stays parked
unless the per-window clock fails its smoke.
