# P2-D — Linux Native Input: Idle Reset + Exit Gesture (Design)

> Fourth sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.5 (idle reset &
> exit gesture), §5.2 (gesture config). **Builds on P1-D2c** (the pure decision layer —
> `idle::should_fire`, `gesture::{in_region, TapCounter, effective_gesture, open_pin_pad}`,
> `shortcuts::is_technician_chord` — all host-tested and reused verbatim) and on P2-A/B/C.
> Linux replaces only the input *observation* source; every decision stays where D2c put it.

**Status:** draft, 2026-08-06 (awaiting review). Approach approved in-session: GDK
event-layer interception (over evdev and Wayland `ext-idle-notify`).

## Goal

`idle_reset_seconds` fires `IdleExpired` (completing the app-path `ClearProfile` →
`ProfileCleared` chain that P2-A could only reach via a harness binary), and both exit
paths — N corner taps and the technician keyboard chord — open the pin pad on Linux. The
"never unexitable" rule (cfg-12, `gesture.rs:17-23`) and no-fail-open semantics carry
over unchanged.

## Scope

**In:** a new `#[cfg(not(windows))]` platform module (`input_watch`) owning the one GDK
handler; Linux bodies/delegations for `idle.rs`, `gesture.rs`, `shortcuts.rs`; the
keyval→VK chord mapping; smoke scenarios 16–17. **Out:** VT-switch/console lockdown
(P2-G image), on-screen keyboard deployment (parent §7 table — P2-G), video (P2-E).

## Architecture — one handler, three consumers

Windows needs two global hooks (`WH_MOUSE_LL` in `gesture.rs:184-291`, `WH_KEYBOARD_LL`
in `shortcuts.rs:103-208`) plus a system poll (`GetLastInputInfo`, `idle.rs:66-75`)
because observation is scattered across OS surfaces. On Linux the kiosk is one
fullscreen GTK window in a compositor with no other client: every user input event
enters our process through GDK. One process-global interceptor serves all three
consumers:

- **Mechanism:** `gdk::event::set_handler` (`gdk-0.18.2/src/event.rs:56-57`, wrapping
  `gdk_event_handler_set`) — sees every `gdk::Event` before dispatch and forwards each
  to `gtk::main_do_event` (`gtk-0.18.2/src/auto/functions.rs:376`). Both are safe
  bindings in crates already in our tree (gtk/gdk 0.18 via tao/wry).
- **The slot is free:** tao installs no GDK handler (checked
  `tao-0.35.3/src/platform_impl/linux/` — zero hits for `event_handler_set`); wry
  neither. First-party conflict risk is nil at the pinned versions; the plan pins it
  with a lockfile-bump note.
- **Handler discipline (load-bearing):** every event is forwarded exactly once,
  unconditionally, before any classification; the handler never panics, never blocks,
  does O(1) work per event, and runs on the GTK main thread (same thread GTK would have
  dispatched on — no ordering change). Observation only — this handler never swallows
  (`should_swallow` stays Windows-only, see below).
- **Reliability note for the record:** this *removes* the Windows caveat class — Tauri
  #13919 (`shortcuts.rs:18`, `gesture.rs:10-11`: Windows silently starving low-level
  hooks while the webview holds focus) has no GDK analogue; delivery here is our own
  process's dispatch. Both exit paths are kept anyway — parity with the two-vector
  design, not because the second vector is still load-bearing.

Rejected, recorded: **evdev** (privilege + hotplug + touchscreen→window transforms —
re-deriving coordinates GDK hands us for free, in the exact window-relative form
`in_region` already takes; on Windows the hook must convert screen→window itself,
`gesture.rs:239-241` — Linux skips that step); **`ext-idle-notify-v1`** (solves idle
only, gesture still needs the GDK path, and it costs a raw wayland-client dependency —
noted as the fallback if the GDK idle path surprises us).

## Components

### `input_watch` (new, `#[cfg(not(windows))]`)

Installed once from setup, after the window exists. Fan-out per event:

1. **Activity → `ActivityClock`** — a shared monotonic-millis cell (`Arc`) stamped on
   every *input* event. The activity set is a pure, host-tested classifier over the GDK
   event type: key press/release, button press/release, motion, scroll, touch
   begin/update/end are activity; expose/configure/focus/crossing and other
   non-input events are not.
2. **Pointer/touch → gesture routing** — `GDK_BUTTON_PRESS` and `GDK_TOUCH_BEGIN`
   (both: Wayland delivers real touch as touch events, not synthesized buttons) feed
   the existing `TapCounter` + `in_region` against `effective_gesture`'s region/config
   (`gesture.rs:107`), window-relative coordinates straight off the event; fire →
   `open_pin_pad` (`gesture.rs:153`), unchanged.
3. **Keys → chord** — a small keyval→VK map for exactly the keys the technician chord
   uses, then the existing `is_technician_chord(vk, mods)` (`shortcuts.rs:99`) with
   modifier state read from the event's GDK modifier field. One chord *definition*,
   two key-code domains, and the map is host-tested against both — the invariant is
   that the same physical chord opens the pad on both platforms.

State lives in the handler closure (main-thread, `RefCell` — same single-thread shape
as the Windows hook thread-affinity). Outbound: `open_pin_pad` (AppHandle),
`ActivityClock` store — nothing else.

### `idle.rs` — Linux body

The Windows shape is a 1 s poll loop over a system-wide last-input source with the
`should_fire` latch (`idle.rs:1-14,32-34`). Linux keeps the identical loop and latch,
swapping the source: idle seconds = now − `ActivityClock` (monotonic, 64-bit — the
32-bit wrap discipline of `idle_secs_from_ticks` is a Windows-tick artifact and stays
`#[cfg(windows)]` with its doc). The stub at `idle.rs:57-64` is replaced; the SEC-09
never-cancelled property (`idle.rs:16-24`) carries over as-is (same `cancel` token
wiring). Signature grows the `ActivityClock` handle on the Linux arm only — the
Windows call site is untouched.

### `gesture.rs` / `shortcuts.rs` — stubs become delegations

Both Linux stubs (`gesture.rs:193`, `shortcuts.rs:112`) become documented
"covered by `input_watch` on Linux" no-ops (the `scheme_guard`-covered-by-nav
precedent from A). `should_swallow` (`shortcuts.rs:66`) is deliberately **not** ported:
under cage there is no desktop shell and no OS chord to swallow — Alt-F4/Win-key
suppression is meaningless where no shell exists, and VT-switch (`Ctrl-Alt-Fn`) is a
kernel/console concern the P2-G image handles (documented divergence: swallowing is a
Windows-shaped countermeasure to a Windows-shaped environment).

### The clear-gate chain completes

With `IdleExpired` live, `Online + idle_clear` → `Effect::ClearProfile` →
`clear::clear` (A's body) → `ProfileCleared` runs end-to-end through the app for the
first time on Linux. A's harness-binary scenario (A smoke 6) stays as the completion
unit check; D's smoke 16 supersedes it as the app-path proof.

## Smoke additions

16. **idle → clear (blocking; needs no input injection — idleness is the absence of
    events):** short-threshold fixture → `IdleExpired` observed → profile clear runs →
    `ProfileCleared` → session cookie gone, kiosk back on home. Also asserts the latch:
    no second fire while idle persists.
17. **gesture + chord + activity-reset (blocking under cage-headless IF virtual input
    is available, else hardware-checklist):** `wlrctl`/virtual-pointer taps in the
    configured corner → pin pad opens; technician chord → pin pad opens; synthetic
    motion resets the idle countdown (asserted via the latch not firing early).
    Whether cage exposes the wlr virtual-input protocols headless is pinned at plan
    time; if it does not, scenario 17 moves to the deferred hardware list with the
    cage items — recorded, not silently dropped.

## Testing

- **Host tests:** activity-set classifier (full event-type table); keyval→VK map (both
  directions, all chord keys); the `input_watch` fan-out decision logic as pure
  functions where extractable. `TapCounter`/`in_region`/`should_fire`/
  `is_technician_chord` are already pinned by D2c's tests — unchanged.
- **Smoke:** 16–17 above; C's scenario 14 (technician exit 86) gains its app-path
  driver from D's chord and is re-run.

## Error handling

The handler forwards first, classifies second — a classification bug can never eat an
event. `open_pin_pad` and telemetry failures degrade as on Windows (log + continue).
If `set_handler` installation itself fails (it cannot — it is a process-global
function-pointer store, not a fallible call — but the closure's install site is still
wrapped) the kiosk runs with gesture/idle dead and the chord's absence is covered by
cfg-12's no-fail-open semantics: nothing exits, nothing unlocks.

## Open decisions to resolve at plan time

- GDK handler + `main_do_event` reentrancy: confirm forwarding from within the handler
  cannot recurse into the handler (GDK docs say the handler replaces the dispatch step;
  verify with the A-harness under weston before building on it).
- The exact keyval set for the chord map (depends on the chord definition constant in
  `shortcuts.rs` — enumerate, don't wildcard).
- Multi-seat/`GDK_TOUCH_CANCEL` semantics for `TapCounter` (a cancelled touch should
  not count as a tap — confirm GDK emits cancel distinctly on Wayland).
- Whether cage-headless exposes `zwlr_virtual_pointer`/`virtual_keyboard` for smoke 17.

## Scope / defer

VT/console lockdown, on-screen keyboard (squeekboard/onboard per parent §7), seat
permissions → P2-G. Video soak → P2-E. The `ext-idle-notify` fallback stays parked
unless the GDK idle path fails its smoke.
