# P2-D — WRITER, Round 3

No frame dispute. R1's eight objections are closed as ACCEPTED; the three rationale
concessions (OB-1's non-exception, OB-3's withdrawn coverage sentence, OB-4a's
`open_pin_pad` refinement) are banked and not re-argued.

**Both new objections: REVISE.** Both are real, both are one line of placement, and in each
case the Critic's placement is better than mine. NB-1 turns out to be even stronger than
argued — the fix is not a new shape, it is the shape the *sibling module already uses*.

---

## NB-1 — cfg-12's early return starves the idle clock — **REVISE (adopt)**

Conceded. Traced at source and the chain holds exactly as stated.

**Verified.** `gesture.rs:293-296` is the early return:

```rust
let Some(gesture) = gesture else {
    eprintln!("gesture: exit gesture not configured (cfg-12); tap capture disabled");
    return;
};
```

`effective_gesture` returning `None` is a supported documented state (module doc `:17-23`),
not a failure. If the Linux body mirrors that arm — which is what R2's D7 rewrite implied —
then on a cfg-12-unconfigured, keyboardless kiosk no pointer or touch handler installs, leg 2
arms the clock once at install and nothing stamps it again, `should_fire` (`idle.rs:32-34`)
fires, and `(Online, IdleExpired) if idle_clear → Effect::ClearProfile { full: true }`
(`kiosk-core/src/app/state.rs:296-304`) wipes a live session. My OB-2 fix closed the failure
door and left the configuration door open. Q3-silent: the only log line is the benign cfg-12
notice.

**One finding that strengthens the objection beyond how it was argued.** I grepped
`shortcuts.rs` for the same gate: there is **no** early return on `gesture` anywhere in the
file — `Option<EffectiveGesture>` is carried through `install` (`:104-110`),
`install_accelerator_handler` (`:159`) and the LL-hook path (`:261`) all the way to
`open_pin_pad(&app, gesture.as_ref())` at `:186`, relying on `open_pin_pad`'s own `None`
guard at `:154-157`. So **`shortcuts.rs` already implements exactly the shape NB-1
prescribes**, and `gesture.rs:293-296` is the outlier. Adopting it is rung 2 of the ladder
(reuse the pattern already in the tree), not a new convention. The Windows early return is
what NB-1 calls it: an optimisation that skips installing a hook which would do nothing —
free on Windows, not free on Linux where the same handlers feed idle.

**Code shape — `gesture.rs`, `#[cfg(not(windows))]` body:**

```rust
// Observation installs UNCONDITIONALLY: these handlers feed BOTH the exit gesture and
// idle reset (idle.rs). cfg-12 disables the *gesture*, not *observation*. Same shape
// shortcuts.rs already uses (Option carried to open_pin_pad's own guard, :154-157);
// the Windows early return at :293-296 is an optimisation that stops being free here.
let tap = gesture.map(|g| RefCell::new((TapCounter::new(g.taps, TAP_WINDOW_MS), g)));

// inside each pointer/touch handler, via observe() (OB-6):
idle::note_activity();
let Some(tap) = &tap else { return };        // cfg-12: observed, but no tap logic
// … bounds check, in_region, counter.tap(), drop borrow, open_pin_pad …
```

`note_activity()` is called **before** the cfg-12 gate — that is the whole fix. Leg 2's body
keeps `shortcuts.rs`'s existing unconditional shape.

**cfg-12 semantics with handlers installed unconditionally — confirmed, nothing exits,
nothing unlocks:**

1. `gesture == None` ⇒ no `TapCounter` ⇒ the tap branch never runs ⇒ leg 1 never reaches
   `open_pin_pad`.
2. Leg 2 calls `open_pin_pad(&app, gesture.as_ref())` with `None`, which self-guards at
   `:154-157` and returns — byte-identical to Windows Vector A's behaviour at `:186`.
3. The installed handlers are pure observation: `observe()` supplies `Proceed` (OB-6), and
   the only outward call on the cfg-12 path is `idle::note_activity()`.

No exit path and no unlock path exists when unconfigured. The module doc's rule (`:17-23`)
holds. **Declared C3 divergence, one line:** the doc phrases cfg-12 as *"the tap-capture hook
no-ops"*; on Windows that is implemented as *not installing*, on Linux as *installing and
no-opping the tap logic*, because the same handler has a second, cfg-12-independent consumer.
Observable behaviour is identical — no pad opens either way.

**OB-2 still intact under the change.** If both installs degrade (D11), `note_activity()` is
never reached, `LAST_INPUT_MS` stays at the `0` sentinel, `idle_secs()` returns 0, nothing
fires. The failure door stays closed while the configuration door now closes too.

---

## NB-2 — the emulation guard is on the wrong handler — **REVISE (adopt)**

Conceded, including the defeat of my stated reason for choosing the other accessor.

**Verified.** `gdk::Event::is_pointer_emulated()` at `gdk-0.18.2/src/event.rs:302-304`
(wrapping `gdk_event_get_pointer_emulated`); `event_wrapper!` emits
`impl ::std::ops::Deref for $name { type Target = crate::event::Event; }` at
`event.rs:522-528`; `pub struct EventButton(crate::Event)` at `event_button.rs:6` with
`event_wrapper!(EventButton, GdkEventButton)` at `:8`. So `ev.is_pointer_emulated()` is a
plain method call on the `&gdk::EventButton` the signal already hands us — no downcast. My R2
rationale ("no downcast, no deref question") was wrong on the facts; both accessors cost one
call, and only one of them is robust.

**The substantive point is the one that decides it.** My table's middle row assumed the
emulated button press arrives. GTK3 emulates a button only from a touch sequence the widget
left unhandled, so that row re-imports the very unverifiable the guard was meant to sidestep,
and fails in the worse direction: touch skipped, no button follows, tap counts **zero** —
leg 1 silently dead on all touch hardware, discovered only at H4. That is worse than the R1
double-count it replaced.

**Adopted, verbatim:**

```rust
// button handler
if ev.is_pointer_emulated() { return; }   // synthesised from a touch we already counted
// touch handler: count TouchBegin unconditionally (Update/End/Cancel are activity-only)
```

| Input | touch handler | button handler | taps |
|---|---|---|---|
| Real mouse click | — | not emulated → counts | 1 |
| Touch, WebKit consumes it | TouchBegin counts | (no button event) | 1 |
| Touch, GTK emulates a button | TouchBegin counts | emulated → skipped | 1 |

All three rows are 1 regardless of what WebKit does. The unverifiable stops being
load-bearing rather than changing which side it breaks.

**Self-reported residual, since I am the one adding it.** Counting `TouchBegin`
unconditionally means an N-finger tap in the corner counts N, where Windows' `mouse_hook`
counts one `WM_LBUTTONDOWN` (`gesture.rs:244-245`). Declared C3 looser divergence, in the
safe direction — it can only make the pad *open* sooner, never weaken the lock, since
`open_pin_pad` only navigates and PIN verification is Task 5's. Same bounding argument
already accepted for `GDK_TOUCH_CANCEL`. **Recorded upgrade path, not built:** if H4 shows it
in practice, gate the touch count on `EventTouch::is_emulating_pointer()`
(`event_touch.rs:32`), which GDK sets on exactly one sequence per gesture. `ponytail:` comment
naming that ceiling. Not built now — it would trade a verified-safe over-count for a
dependence on a field I cannot verify is set under Wayland touch, which is the mistake NB-2
just corrected.

---

## Final register — post-Round-3

| ID | Final state | Changed this round |
|---|---|---|
| D1 | GTK widget-signal observation: keys on `gtk::ApplicationWindow`, pointer/touch/motion/scroll on `webkit2gtk::WebView`; all handlers via `observe()` → `Proceed`. | Button handler gains `is_pointer_emulated()` guard (NB-2). |
| D2 | evdev rejected; `ext-idle-notify-v1` parked; `gdk_event_handler_set` rejected with objdump evidence. | — |
| D3 | Legs 1+2 only; C3 liveness divergence declared; one-sentence chord note into G10's reserved slot. | — |
| D4 | `0` sentinel = "not idle", mirroring `idle.rs:88-89`; `.max(1)`; no signature change, no `main.rs:917` diff. | `note_activity()` now called **before** the cfg-12 gate (NB-1) — this is what makes "either leg arms the clock" true. |
| D5 | `position()`/`allocation()` geometry with `inside_window` bounds parity; `TAP_WINDOW_MS` un-cfg'd; borrow dropped before `open_pin_pad`; `position()` frame a declared assumption. | Tap dedup moved to the button side; multi-finger over-count declared with a recorded deadband upgrade (NB-2). |
| D6 | Two-arm keyval match; `should_swallow` **guard** at the Linux call site, verbatim `shortcuts.rs:184-190`; shared code untouched. | — |
| D7 | Two existing stubs become real Linux bodies; no new module; zero `main.rs` diff. | `gesture.rs`'s Linux body **does not** mirror the Windows cfg-12 early return; it follows `shortcuts.rs`'s existing carry-the-`Option` shape (NB-1). |
| D8 | `should_swallow` not ported; both-directions divergence stated; F5/F11 residue explicitly open. | — |
| D9 | Clear-gate chain completes app-path. | — |
| D10 | One direct target-gated dep: `gtk = "0.18"` (`gdk`/`glib` via re-export). | — |
| D11 | `Result`-based degrade per C4; no `catch_unwind` (Q3); degrade semantics defined by D4's sentinel. | Now also covers the cfg-12 path: observation survives an unconfigured gesture (NB-1). |
| D12 | Smoke 16–17; `Proceed` gated by `rustc` via `observe`, smoke assertion demoted to backstop; plan-time `'static` shim recorded. | — |
| D13 | PF-04: `GestureZoom` + `Capture` + `set_state(Claimed)` on the webview; gated by new P2-G **H10**; fallbacks recorded (wry #544 patch, `touch-action` CSS); `scale_delta()` deadband recorded as the fix if H10's second clause fires. | — |

**Open items carried to plan time:** `GDK_TOUCH_CANCEL` emission; cage-headless virtual
input (smoke 17 → P2-G H4); `observe`'s `'static` bounds; PF-04's H10 outcome. All owned.
