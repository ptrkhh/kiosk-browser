# P2-D — CRITIC, Round 2

No frame dispute. All eight R1 objections are **ACCEPTED**. Three of the Writer's
rationale-corrections are right and I concede them explicitly below — **OB-3's is a
straight error on my part** and I withdraw the sentence.

Two **new** objections, both introduced *by the replacements* and both one line of
placement to fix. Neither reopens a settled point.

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | D takes PF-04: `GestureZoom` + `Capture` phase + `set_state(Claimed)` on the webview; gated by new P2-G **H10**; wry #544 patch and `touch-action` CSS as recorded fallbacks | All eight bindings verified exact; H10 does not collide (G's R1 already took H9); his correction that PF-04 is **not** an exception to the `Proceed` rule is right and I concede it; over-suppression checked and does not bite, one residual recorded | **ACCEPTED** |
| OB-2 | `0` sentinel = "not idle", mirroring `idle.rs:88-89`; `.max(1)` against collision; both install sites arm the clock; `max(loop_start_ms, …)` deleted | Sentinel and collision guard verified sound; the mirror of the Windows convention is exact. But "both install sites arm it" collides with `gesture::install`'s cfg-12 early return → **NB-1** | **ACCEPTED** (+ NB-1) |
| OB-3 | `&& !mods.win` withdrawn; `should_swallow` guard at the Linux call site, verbatim `shortcuts.rs:184-190` ordering | Shared code now untouched (C1/C8 clean); guard traced and correct. **His correction of my rationale is right — I withdraw my "every swallow-listed combination" sentence outright** | **ACCEPTED** |
| OB-4 | (a) C3 looser-liveness divergence declared verbatim, plus a refinement; (b) leg 3 withdrawn as his own invention on the parent's "and/or"; one-sentence chord note into G10's reserved slot | Both directions now present → C3 satisfied. His `open_pin_pad` refinement is verified and narrows my objection — conceded. "and/or" and G10's reserved owner slot both confirmed at source | **ACCEPTED** |
| OB-5 | `EventTouch::is_emulating_pointer()` guard in the touch handler; button handler counts unconditionally | Discriminator verified. But the guard is on the **wrong side** and can zero the tap leg entirely under the same unverifiable → **NB-2** | **ACCEPTED** (+ NB-2) |
| OB-6 | `observe()` wrapper supplies `Proceed`; smoke-17 assertion demoted from gate to backstop | Accepted, with one honest qualifier on what "un-violatable" covers, and one compile-detail note. The OB-1 finding genuinely does remove the last would-be exception | **ACCEPTED** |
| OB-7 | `input_watch` withdrawn; legs become the two existing stubs' real Linux bodies; zero `main.rs` diff; `scheme_guard` precedent withdrawn as inapplicable | Verified at source; net deletion; nothing to add | **ACCEPTED** |
| OB-8 | `position()` frame declared an assumption; `ponytail:` marker naming `translate_coordinates` as the upgrade path | `allocation()` `widget.rs:500` and `translate_coordinates` `widget.rs:1863` both verified exact | **ACCEPTED** |

---

## OB-1 — verification of the PF-04 replacement

**Every binding checked at source, all exact.**
`gtk-0.18.2/src/auto/gesture_zoom.rs:15` `pub struct GestureZoom(…) @extends Gesture, EventController`;
`:24` `pub fn new(widget: &impl IsA<Widget>) -> GestureZoom` (and `:26-28` `from_glib_full`, so we own the ref);
`:46` `pub fn connect_scale_changed<F: Fn(&Self, f64) + 'static>`;
`auto/gesture.rs:198` `set_sequence_state(&gdk::EventSequence, EventSequenceState) -> bool`, `:209` `set_state(EventSequenceState) -> bool`;
`auto/enums.rs:2362` `Claimed`, `:2364` `Denied`; `:6642` `PropagationPhase::Capture`;
`auto/event_controller.rs:70` `set_propagation_phase`. No new dependency — all inside D10's single `gtk` crate, as claimed.

**His correction of my line is right, and I concede it.** OB-1 said PF-04 "becomes the single
named, reviewed exception" to D1's `Proceed` rule. It is not an exception:
`connect_scale_changed`'s closure returns `()` (`gesture_zoom.rs:46-49`) and `set_state`
returns `bool` (`auto/gesture.rs:209`) — neither is on the `-> glib::Propagation` surface at
all. So `observe` (OB-6) covers 100% of that surface with no carve-out. That is a real
strengthening of both remedies and I had it wrong.

**Over-suppression — checked, does not bite in the target deployment.** `GestureZoom` consumes
only 2-finger touch sequences and `GDK_TOUCHPAD_PINCH`; it never sees single-finger panning,
which is what the parent's `touch-action: pan-x pan-y` (`:685`) governs, and two-finger
touchpad scrolling arrives as `GDK_SCROLL` (`enums.rs:1599`), a different event class than
`TouchpadPinch` (`:1621`). So the Writer's conclusion holds even though "recognises only the
2-finger scale gesture" is doing some work he does not show.

**One residual worth a line in the spec, not an objection.** `GestureZoom` in gtk 0.18.2
exposes **no recognition threshold** — `scale_delta()` (`gesture_zoom.rs:39-42`) is the only
knob — so claiming on the first `scale-changed` claims essentially any 2-finger touchscreen
sequence, including a 2-finger pan whose fingers drift apart. H10's second clause
("two-finger pan/scroll still works") is exactly the test that surfaces this, and the fix if
it fires is a `scale_delta()` deadband before claiming. Recorded, not designed — which is the
right call under Q2.

**H10 is a real gate, not a deferral.** Frame §4.5 admits "a hardware-checklist row" as a
named owner, and the parent names this very gate: *"validated on touch hardware in P2"*
(`:685`). Numbering is free — G's R1 block already added H9 (`P2G-R1-writer.md:449`), so H10
is next and does not collide. And it asks G for a checklist row, not a change to G's recipe —
the distinction the Writer draws against his own withdrawn leg-3 constraint is correct.

The independent coverage verifier agrees the item was unowned before this
(`verify-COVERAGE.md:34,93,146-153`: *"Zero hits for `pinch` in any of the seven specs"*), so
this closes a real HIGH.

---

## OB-2 — sentinel verified; the arming path is where it leaks (see NB-1)

Sentinel logic is sound. `LAST_INPUT_MS: AtomicU64 = 0` as "no observation source" mirrors
`idle.rs:88-89`'s `else { 0 }` exactly, with `idle.rs:78-79`'s stated reason
(*"rather than risking a false idle-fire off garbage data"*). Collision is impossible:
`now_ms()` is elapsed-from-a-monotonic-base, so the only reachable colliding value is `0` at
the very first stamp, and `.max(1)` removes it — at a cost of ≤1 ms of skew, which is
irrelevant against a 1 s poll. Deleting `max(loop_start_ms, …)` is right; the sentinel
subsumes the boot window, and I verified the ordering it relies on (`main.rs:917` spawn,
`main.rs:1105-1106` installs).

The claim I cannot accept as written is *"Either leg succeeding arms the clock"* — see NB-1.

---

## OB-3 — I withdraw my rationale; the Writer is right

**Concession, unreserved.** My R1 sentence — the guard *"restores parity for **every**
swallow-listed combination on Linux — Ctrl+P, F5, F11, Alt+F4/Tab/Esc, Menu"* — is wrong, and
I traced it rather than take his word. Two independent reasons:

1. The guard's action is `return Propagation::Proceed`. That **swallows nothing**. Ctrl+P/F5/F11
   and the rest reach WebKit exactly as they did before the guard existed. D8's declared looser
   divergence is untouched, as he says.
2. Those keys could never have been affected anyway: `is_technician_chord` tests `vk == VK_K`
   (`shortcuts.rs:99-101`), so the guard can only change an outcome when `should_swallow` **and**
   `is_technician_chord` are both true. Walking `should_swallow`'s table (`:66-87`) with
   `vk = 0x4B`: the `match vk` block has arms for `VK_W|VK_N|VK_T|VK_P` (0x57/0x4E/0x54/0x50),
   `VK_F5|VK_F11`, `VK_APPS`, `VK_F4`, `VK_TAB`, `VK_ESCAPE` — `0x4B` matches none, falling to
   `_ => false`. The **only** reachable `true` for `VK_K` is the `if mods.win { return true }`
   arm at `:71-73`. So the guard's entire behavioural delta is the Super case — precisely what
   `&& !mods.win` addressed.

The remedy's advantage is therefore purely structural: zero diff to shared reviewed code, zero
D2c test delta, verbatim Windows ordering (C1, Q2, C8/Q4). That was the objection's real
ground; the coverage claim was surplus and wrong. Withdrawn.

**The remedy itself verifies clean.** `is_technician_chord` untouched → C1/C8 satisfied, no
existing test moves. The chord still passes the guard:
`should_swallow(0x4B, {ctrl:true, alt:true, shift:true, win:false})` → `mods.win` false, `0x4B`
matches no `match` arm → `false` → falls through to the chord check. The ordering reproduces
`shortcuts.rs:184-190` faithfully (swallow decision first, chord second, chord never swallowed).

---

## OB-4 — both halves accepted, and one of my premises corrected

**(a) C3 both-directions test: passes.** D3 already carried the *stricter* direction from R1
(Linux has no #13919 hook-starvation class). R2 adds the *looser* one verbatim. Both present,
so C3 is satisfied rather than merely acknowledged.

**His refinement is correct and I concede it.** I read `open_pin_pad` at source
(`gesture.rs:153-173`): it ends in `window.navigate(parsed)` at `:167`, which needs the UI
thread on **both** platforms. So a wedged UI thread blocks the pad on Windows too, and the
divergence is in *observation liveness*, not in *exit capability* — narrower than my objection
implied. I also credit the refusal to assert a covering watchdog he has not verified; frame
§1.6 wants exactly that.

**(b) "and/or" — confirmed, and the withdrawal is right.** Parent `:318-320`, re-read:
*"the exit gesture falls back to a reserved `AcceleratorKeyPressed` technician chord **and/or**
the §7.2 OS-lockdown escape, so a locked device is never unexitable."* Either alone discharges
it, so legs 1+2 suffice and leg 3 was an invention. The replacement placement is real, not
notional: `P2G-R1-writer.md:24` — G10's owner column literally reads *"D (chord note lands
here)"*, so G had already reserved the slot; and G's own recipe confirms why the constraint
could not have landed (`:346-347` `NAutoVTs=0`/`ReserveVT=0` + `systemctl mask getty@.service`;
`:416-417` *"SSH keyed-only if present, absent by default"*). A one-sentence note into a
reserved slot is not a change request. Correctly resolved.

---

## OB-5 — guard accepted in principle, but it is on the wrong handler (see NB-2)

`EventTouch::is_emulating_pointer()` verified at `gdk-0.18.2/src/event_touch.rs:32-33`, and
preferring the typed accessor over `Event::is_pointer_emulated()` is right — no downcast, direct
field read. The double-count diagnosis is fully conceded on his side and the direction of the
fix is correct. The *side* it is applied to is not; NB-2.

---

## OB-6 — accepted, with one qualifier and one compile note

**Qualifier on "un-violatable".** `observe` makes it impossible for a handler **body** to
produce `Stop` — it never sees the return value. It does not prevent a future edit adding a
sixth `connect_*` inline. So the compile-time guarantee is "every handler routed through
`observe` proceeds", plus a one-line convention that all `Propagation`-returning connects route
through it. That is still strictly better than a gate that may not run, and — because of the
OB-1 finding that PF-04 is not a `Propagation` handler — there is now **no legitimate
exception**, so any inline `connect_*` returning anything but `Proceed` is a reviewable smell
rather than a designed carve-out. Worth one sentence in D12 stating the rule that way. Not an
objection.

**Compile note, plan-time.** `connect_*` requires `F: … + 'static` (`widget.rs:2017`, `:3035`).
An RPIT `-> impl Fn(&W, &E) -> Propagation` over unbounded `W, E` is not known to be `'static`;
it will need `W: 'static, E: 'static` or `+ 'static` on the return type. A shim, which frame Q5
explicitly permits at plan time. Recording it so it is not rediscovered.

---

## OB-7 / OB-8 — accepted, nothing to add

OB-7: verified `main.rs:1105-1106`, and the stubs at `gesture.rs:193-199` /
`shortcuts.rs:112-118` take exactly `(&tauri::WebviewWindow, tauri::AppHandle,
Option<EffectiveGesture>)`. Withdrawing the `scheme_guard` precedent as inapplicable is the
right read of it. Net deletion of one module and one `main.rs` diff.

OB-8: `allocation()` at `widget.rs:500` (returns `Allocation`, not `gdk::Rectangle` — the R2
text's parenthetical is loose, `Allocation` is gtk's own alias; immaterial) and
`translate_coordinates` at `widget.rs:1863`, both verified. A declared assumption with a named
upgrade path is the correct disposition; engineering for a second vbox child that does not
exist would have been the defect.

---

## New objections

Both are introduced **by the R2 replacements**, neither existed in R1, and each is one line of
placement.

### NB-1 — cfg-12's early return starves the idle clock on a touch-only kiosk (vs OB-2 + OB-7 replacements, MED)

**What breaks.** OB-2 rests on *"Both Linux install sites … call `note_activity()` once on
successful install. Either leg succeeding arms the clock."* OB-7 puts leg 1 — the
pointer/touch/motion/scroll handlers, i.e. **the entire non-keyboard activity source** — inside
`gesture.rs`'s `#[cfg(not(windows))] install`. The Windows body of that same function opens
with a cfg-12 early return (`gesture.rs:292-296`):

```rust
let Some(gesture) = gesture else {
    eprintln!("gesture: exit gesture not configured (cfg-12); tap capture disabled");
    return;
};
```

If the Linux body mirrors that shape — and D7-rewritten says the stub "becomes the real Linux
body", i.e. the counterpart arm — then whenever `effective_gesture` returns `None`
(`gesture.rs:107-145`; a **supported, documented** state, not a failure) **no pointer or touch
handler is installed at all**.

**When.** A kiosk deployed without `input.exit_gesture` / `[exit_gesture]` — cfg-12's
by-design configuration — on touch-only hardware (no keyboard, which is the §7.2 cage target;
the OSK is deferred to P2-G).

**Why it matters.** Leg 2 arms the clock once at install and then never stamps again (no
keyboard). `idle_secs` grows monotonically from boot, `should_fire` (`idle.rs:32-34`) fires,
and the FSM runs `(Online, IdleExpired) if idle_clear → Effect::ClearProfile { full: true }`
(`kiosk-core/src/app/state.rs:296-304`) — wiping a live session while the user is actively
tapping. That is the exact failure OB-2 closed, re-entering through the **configuration** door
instead of the **failure** door, and it is silent (Q3): the only log line is the benign
cfg-12 notice.

**Fix — placement, one line.** Install the observation handlers unconditionally; gate only
`TapCounter` construction and the `open_pin_pad` call on `Some(gesture)`. cfg-12 disables the
*gesture*, not *observation*, and `open_pin_pad` already carries its own `None` guard
(`gesture.rs:158-161`), so the outer early return is a Windows-side optimisation (it skips
installing a hook that would do nothing), not a cfg-12 requirement. On Linux the same handlers
also feed idle, so the optimisation stops being free.

**Falsifiable.** If D specifies the Linux body installs the handlers *before* the `Option`
check, or that leg 2 stamps on something a keyboardless kiosk actually produces, this
evaporates and I withdraw it.

**Evidence tier.** 3 — `crates/kiosk-main/src/gesture.rs:107-145,158-161,292-296`,
`idle.rs:32-34`, `kiosk-core/src/app/state.rs:296-304`. All read at source.

### NB-2 — OB-5's guard is on the wrong handler and can zero the tap leg (vs OB-5 replacement, MED)

**What breaks.** The replacement guards the **touch** handler
(`if ev.is_emulating_pointer() { return Proceed; }`) and lets the **button** handler count
unconditionally. Its truth table's middle row — *"Touch, pointer-emulated | skipped | counts
(emulated) | 1"* — assumes the emulated button press actually arrives. That assumption is the
**same unverifiable** the guard was introduced to sidestep: GTK3 only emulates a button from a
touch sequence the widget left *unhandled*. If WebKitWebViewBase consumes the touch, no button
follows, the touch was skipped, and the tap counts **zero**.

**When.** Any touchscreen. A single-finger tap is normally the pointer-emulating sequence, so
`emulating_pointer` is set on essentially every corner tap — meaning this is not an edge case,
it is the common path.

**Why it matters.** The R1 defect made the exit gesture fire at ⌈N/2⌉ taps (looser). This
replacement can make leg 1 **fire at no number of taps at all** — silently dead tap capture on
all touch hardware. That is worse than what it replaces: parent §3.5:314-318 names unreliable
native tap capture as the risk the chord exists to backstop, and on a keyboardless kiosk the
chord is not reachable. Discovery is deferred to H4 on hardware.

**Fix — same one line, other handler, correct under both branches.** Guard the **button**
handler and count touch unconditionally:

```rust
// button handler
if ev.is_pointer_emulated() { return; }   // this press was synthesised from a touch we already counted
```

| Input | touch handler | button handler | taps |
|---|---|---|---|
| Real mouse click | — | not emulated → counts | 1 |
| Touch, WebKit consumes it | counts | (no button event) | 1 |
| Touch, GTK emulates a button | counts | emulated → skipped | 1 |

All three rows are 1 **whether or not** WebKit consumes the touch — the unverifiable stops
being load-bearing instead of merely changing which side it breaks.

**Available with no downcast, contrary to the R2 rationale for choosing the other accessor.**
`gdk::Event::is_pointer_emulated()` is at `event.rs:302-304`, and `event_wrapper!` gives every
typed event `impl Deref<Target = gdk::Event>` (`event.rs:522-528`), with
`pub struct EventButton(crate::Event)` at `event_button.rs:6`. So `ev.is_pointer_emulated()`
is a plain method call on the `&gdk::EventButton` the signal already hands us — same cost, same
ergonomics, strictly better failure behaviour.

**Evidence tier.** 4 — `gdk-0.18.2/src/event.rs:302-304,522-528`, `event_button.rs:6`,
`event_touch.rs:32-33`. All read at source.

---

## Nothing else is open

Every other R2 replacement verifies clean against the pinned sources. The round is a net
removal — one module, one `main.rs` diff, one shared-code edit, one invented cross-spec
obligation gone; one runtime gate replaced by a compile-time one; one HIGH coverage gap
(PF-04) closed with a named implementing spec **and** a named hardware gate.
