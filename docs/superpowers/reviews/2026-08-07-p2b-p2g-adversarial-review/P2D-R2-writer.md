# P2-D — WRITER, Round 2

No frame dispute. Clean passes on D1-core, D2, D4-mechanics, D5-geometry, D6-mapping, D8,
D9, D10, D11, D12-ownership and the R1 disposition table are banked and not re-argued.

**Result: 8 REVISE, 0 REBUT.** Three of the eight carry a verified correction to the
Critic's *rationale* while taking the Critic's *remedy* (OB-3, OB-4a, OB-6); the remedies
stand regardless, so they are logged as REVISE, not partial rebuttals.

Two objections turn out to make the design smaller, and I say so plainly: **OB-7 deletes
the new module and the `main.rs` diff**, and **OB-3 deletes an edit to shared reviewed
code**. Both are net removals. OB-1 is the only one that adds design.

---

## OB-1 — PF-04 pinch intercept — **REVISE (D takes ownership)**

D owns it. D13 is withdrawn as a "coverage flag" and replaced by a specified change.

**Why D and not elsewhere — verified at source.** Parent `:685` verbatim: *"WebKitGTK fixed
`zoom-level` — note this fixes only base zoom, interactive pinch is GTK-owned and needs a
**gesture-controller intercept in the platform layer** / a wry patch, validated on touch
hardware in P2 (wry #544, PF-04)"*. Parent `:894` repeats *"P2 intercepts the GTK zoom
gesture in the platform layer"*. Frame §2 lists "incl. pinch-gesture intercept" in the P2
row. I re-ran the sibling grep: `zoom|pinch|PF-04` over P2-A/B/G returns only B's
`set_zoom_level` rows — the *base-zoom* half. The gesture-controller half is unowned, D1
already holds the widget the parent's own words point at, so D is the owner. Frame §4.5 is
satisfied by naming a spec **and** a gate, both below.

**The change.** In `gesture.rs`'s Linux install (OB-7 puts it there), after the tap wiring:

```rust
let zoom = gtk::GestureZoom::new(&webview);                       // gesture_zoom.rs:24
zoom.set_propagation_phase(gtk::PropagationPhase::Capture);       // event_controller.rs:70
zoom.connect_scale_changed(|g, _| { g.set_state(EventSequenceState::Claimed); }); // :46 / gesture.rs:209
std::mem::forget(zoom);   // controller lives for the process, like the window
```

- **What it suppresses:** two-finger zoom sequences on the webview. Not scroll and not pan —
  `GestureZoom` recognises only the 2-finger scale gesture, so the parent's own
  `touch-action: pan-x pan-y` intent (`:685`) is unaffected.
- **How it is gated:** capture phase, so the claim lands before the widget's own handling.
- **Bindings verified in-session:** `GestureZoom::new(&impl IsA<Widget>)`
  (`gtk-0.18.2/src/auto/gesture_zoom.rs:24`), `@extends Gesture, EventController` (`:15`),
  `connect_scale_changed` (`:46`); `GestureExt::set_state(EventSequenceState) -> bool`
  (`auto/gesture.rs:209`) and `set_sequence_state` (`:198`);
  `EventSequenceState::{Claimed, Denied}` (`auto/enums.rs:2362,2364`);
  `EventControllerExt::set_propagation_phase` (`auto/event_controller.rs:70`) with
  `PropagationPhase::Capture` (`auto/enums.rs:6642`). No new dependency — all inside D10's
  single `gtk` crate.
- **PF-04 split, stated:** B owns base zoom (`set_zoom_level`, `p2b:48`); D owns the
  interactive-pinch intercept. Neither alone closes PF-04; together they do.

**The gate — and I accept frame Q5 in full.** Whether a capture-phase claim beats
WebKitWebViewBase's own touch handling is a *does-the-mechanism-work* question and I have
no GTK3/WebKitGTK sources here to settle it. The parent already names the gate: *"validated
on touch hardware in P2"* (`:685`), and P2-G owns the hardware checklist. So: **a new P2-G
row H10 — "pinch-zoom does not scale the page on touch hardware; two-finger pan/scroll still
works"** — implementing spec **D**, gating spec **G**. That is a named owner plus a named
gate, not a self-deferral. Unlike D3's withdrawn leg-3 constraint (OB-4b), this asks G for a
checklist row, which is exactly what G's checklist is for — not a change to G's recipe.

**Declared fallbacks, in the parent's own order.** If the claim loses to WebKit: (1) the
parent's named alternative, a wry patch upstream (`:685`, wry #544); (2) the parent's
belt-and-suspenders `touch-action: pan-x pan-y` CSS (`:685`, explicitly "page-overridable",
so not a substitute), carried by the RT-16 injection engine. Recorded so H10 failing has a
next step rather than reopening the design.

**Correction to my own R1 text, and to one line of the Critic's.** OB-1 says PF-04 "becomes
the single named, reviewed exception" to D1's `Proceed` rule. It is not an exception at all:
`GestureZoom` is an `EventController`, not a `-> glib::Propagation` signal handler
(`gesture_zoom.rs:15`, `auto/gesture.rs:209` returns `bool`, not `Propagation`). It
suppresses by claiming a sequence, on a different mechanism entirely. **D1's `Proceed` rule
therefore stays absolute, with zero exceptions** — which is what makes OB-6's compile-time
enforcement clean. Verified this myself; it strengthens both objections' remedies.

---

## OB-2 — Spurious `IdleExpired` on a degraded install — **REVISE**

Conceded, and the Critic's chain is exactly right. I re-read the Windows convention at
source: `idle.rs:78-79` — *"Falls back to 'not idle' (0) if the Win32 call fails, **rather
than risking a false idle-fire off garbage data**"* — implemented as the `else { 0 }` at
`idle.rs:88-89`. My R1 `max(loop_start_ms, LAST_INPUT_MS)` covers the *boot* window and
silently inverts that choice for the *permanent-degrade* window. `should_fire`
(`idle.rs:32-34`) then fires once, the FSM takes `(Online, IdleExpired) if idle_clear →
Effect::ClearProfile { full: true }` (`kiosk-core/src/app/state.rs:296-304`), and a live
session is wiped. Re-verified all four citations.

**Replacement — mirror Windows exactly, one sentinel, no plumbing:**

```rust
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);   // 0 = no observation source yet

pub fn note_activity() { LAST_INPUT_MS.store(now_ms().max(1), Relaxed); }

fn idle_secs() -> u64 {
    match LAST_INPUT_MS.load(Relaxed) {
        0 => 0,                              // no source — "not idle", same as idle.rs:88-89
        t => (now_ms().saturating_sub(t)) / 1000,
    }
}
```

- Both Linux install sites (OB-7: `gesture.rs` and `shortcuts.rs`) call `note_activity()`
  **once on successful install**. Either leg succeeding arms the clock; both failing leaves
  it at 0 forever, and the loop reads "not idle" forever — never a spurious fire.
- The `max(loop_start_ms, …)` term is **deleted**. The sentinel subsumes it: `idle::run`
  spawns at `main.rs:917`, installs happen in the setup closure at `main.rs:1105-1106`, so
  the boot window is exactly the `0` case and is now covered by the same branch.
- `.max(1)` keeps a real timestamp from colliding with the sentinel when the monotonic base
  is initialised on the first stamp. `ponytail:` comment on the sentinel naming the ceiling.
- Cost of the failure this now prevents: destroyed user session state, silent (Q3). Cost of
  the failure it now allows: idle reset never fires on a fully-degraded install, which is
  loud (both install errors already log per D11) and is the direction Windows chose
  deliberately. No new C3 divergence — this *removes* one.

---

## OB-3 — `&& !mods.win` at the shared root — **REVISE (take the Critic's guard)**

Conceded on the invariant. I read `shortcuts.rs:95-98` at source: *"Deliberately checked
INDEPENDENTLY of [`should_swallow`] (never folded into that table) … **the two decisions must
never be layered on the same key**."* My edit folds `should_swallow`'s `if mods.win { return
true }` (`:71-73`) into `is_technician_chord` (`:99-101`). That is the layering the function
forbids, in the function that forbids it. C1 and Q2 both point at the Critic's alternative,
which edits nothing shared.

**Adopted, verbatim from Vector A's ordering (`shortcuts.rs:184-190`):**

```rust
if should_swallow(vk, mods) { return Propagation::Proceed; }   // guard, not swallow
if is_technician_chord(vk, mods) { open_pin_pad(&app, gesture.as_ref()); }
Propagation::Proceed
```

`is_technician_chord` is **not touched**. No D2c test delta. `&& !mods.win` withdrawn.

**Verified it does not break the chord:** `should_swallow(0x4B, {ctrl,alt,shift,win:false})`
→ `mods.win` false, and `VK_K` matches no arm of the `match vk` block (`:77-86`) → `false`.
With `win:true` → `true` at `:71-73` → guarded. Exactly the intended behaviour, both ways.

**One correction to the objection's rationale, which does not change the remedy.** OB-3 says
the guard *"restores parity for **every** swallow-listed combination on Linux — Ctrl+P, F5,
F11, Alt+F4/Tab/Esc, Menu"*. It does not: returning `Proceed` swallows nothing, so those keys
still reach WebKit exactly as before. The guard's only behavioural effect is the
`mods.win`+chord case — the same single case my one-liner addressed. Its advantage is
therefore structural (zero shared-code diff, zero test delta, verbatim Windows ordering), not
coverage. D8's rewritten both-directions divergence stands unchanged; the F5/F11 residue is
**not** closed by this, and I am not claiming it is.

---

## OB-4 — "Two independent legs" + leg 3 — **REVISE (both halves)**

### (a) Liveness independence — declare the divergence

Conceded. Verified the Windows side myself: `gesture.rs:286-289` doc — *"a low-level hook's
callbacks are delivered by pumping messages on the installing thread, which must never be
the Tauri/WebView2 UI thread"* — and `gesture.rs:309-322`
`std::thread::Builder::new().name("gesture-mouse-hook")` + `GetMessageW` pump;
`shortcuts.rs:239-252` the same for the LL keyboard hook. Linux legs both ride the GTK main
loop and P2-A's GTK-main-thread rule (A:76-77) forbids moving them, so this is a
documentation defect. **Added to D3, C3 looser direction:**

> *Divergence (looser than Windows): both Linux legs are dispatched by the one GTK main
> loop. A wedged main iteration disables observation on both at once, where Windows'
> dedicated hook threads keep observing.*

**One verified refinement, which narrows the gap rather than denying it.** I read
`open_pin_pad` (`gesture.rs:153-173`): it ends in `window.navigate(parsed)` (`:167`), which
needs the UI thread on both platforms. So under a wedged UI thread Windows' legs keep
*observing* but still cannot *open the pad*. The divergence is in observation liveness, not
in exit capability. I claim no covering control for a wedged GTK main loop — I have not
verified that any existing watchdog detects it, so I am not asserting one.

### (b) Leg 3 — withdrawn

Conceded, and on the strongest ground the Critic offers: the parent's own word. Parent
`:318-320` reads *"a reserved `AcceleratorKeyPressed` technician chord **and/or** the §7.2
OS-lockdown escape"*. "and/or" means legs 1+2 discharge "never unexitable" on their own.
My R1 constraint — *"the hardened image must retain exactly one administrative route"* — was
**my invention**, not a parent requirement, and I verified it contradicts G as written:
`P2G-R1-writer.md` G10 (cage without `-s`, `NAutoVTs=0`/`ReserveVT=0`, `systemctl mask
getty@.service`, "no other TTYs" promoted to a gate step) and G12 (*"SSH keyed-only if
present, absent by default"*). Inventing a cross-spec obligation that its named owner
excludes by default is exactly the unowned deferral frame §4.5 forbids.

**Replacement:** D3 is now legs 1+2, full stop. D contributes **one sentence** to G10, where
G's own register already reserved the slot (`P2G-R1-writer.md:24`, G10's owner column reads
*"D (chord note lands here)"*): *the technician chord is the in-session escape under the
locked cage session; the image intentionally leaves no VT/getty route, per G10.* No change
request to G's recipe, no new G row from this objection. (OB-1's H10 is a checklist row, a
different and legitimate ask.)

---

## OB-5 — Touch double-count — **REVISE**

Conceded. D1 routes both `connect_button_press_event` and `connect_touch_event` into the tap
path and D5's snippet discriminates nothing. On touch hardware under cage — the target
deployment — one physical tap can increment `TapCounter` twice, firing the gesture at ⌈N/2⌉
taps against Windows' N (`mouse_hook` counts one `WM_LBUTTONDOWN` per tap,
`gesture.rs:244-245`). Undeclared C3 divergence in the looser direction on a
security-adjacent control. I did not raise it and should have.

**Fix — one `if`, on a discriminator I verified in-session:**

```rust
// touch handler
if ev.is_emulating_pointer() { return Proceed; }   // GTK will also deliver a button press
```

`EventTouch::is_emulating_pointer() -> bool` at `gdk-0.18.2/src/event_touch.rs:32`, read at
source. The button handler counts unconditionally. Resulting truth table:

| Input | touch handler | button handler | taps counted |
|---|---|---|---|
| Real mouse click | — | counts | 1 |
| Touch, pointer-emulated | skipped | counts (emulated) | 1 |
| Touch, not emulated | counts | — | 1 |

I use `EventTouch::is_emulating_pointer()` rather than `Event::is_pointer_emulated()`
(`event.rs:304`) because it is a direct field read on the typed event the signal already
hands us — no downcast, no deref question. Both exist; one suffices.

I do **not** attempt the falsification OB-5 offers (proving WebKit's `touch-event` class
handler always returns `TRUE` under the C7 floor). That would be tier-5 and unverifiable
here; the one-`if` guard is correct whether or not emulation occurs, so the guard is cheaper
than the proof.

---

## OB-6 — The `Proceed` invariant has no gate known to run — **REVISE**

Conceded on C9. Smoke 17 is conditional by D's own text (`:122-129`) with P2-G H4 as its
fallback owner, and the verifier confirmed neither `cage` nor `wlrctl` nor any wlr
virtual-input tooling is installed. Pinning D1's one product-breaking failure mode to a
scenario that may not run is a feasibility defect.

**Adopted: enforce it in the type, not the scenario.** All observation handlers go through
one helper that supplies the return value, so no handler *can* return `Stop`:

```rust
fn observe<W, E>(f: impl Fn(&E) + 'static) -> impl Fn(&W, &E) -> glib::Propagation {
    move |_, e| { f(e); glib::Propagation::Proceed }
}
```

Five call sites, one function, invariant checked by `rustc` on every build including the
Windows-only CI path. The added smoke-17 assertion stays as a behavioural backstop but is no
longer the gate.

**And it is cleaner than OB-6 assumes**, per the OB-1 finding above: PF-04's intercept is a
`GestureZoom` sequence-claim, not a `Propagation`-returning handler, so it is **not** an
exception carved out of this rule. `observe` covers 100% of the `Propagation` surface with no
escape hatch to review.

---

## OB-7 — Unlisted install site, and a free fit discarded — **REVISE (net deletion)**

Conceded, and this is the objection that makes the design smallest. Verified at source:
`main.rs:1105` `shortcuts::install(&window, app.handle().clone(), exit_gesture_setup.clone())`
and `:1106` `gesture::install(…)`, both inside the setup closure, in that order; the Linux
stubs at `gesture.rs:193-199` and `shortcuts.rs:112-118` take exactly
`(&tauri::WebviewWindow, tauri::AppHandle, Option<EffectiveGesture>)` — precisely D1's
inputs. They map 1:1 onto D3's two legs.

**Replacement:**

- **`input_watch` is withdrawn.** No new module.
- **Zero `main.rs` diff.** The unlisted scope change disappears rather than getting declared.
- Leg 1 (taps, pointer/touch/motion/scroll on the `webkit2gtk::WebView`, plus OB-1's
  `GestureZoom`) becomes `gesture.rs`'s `#[cfg(not(windows))]` body.
- Leg 2 (chord + key activity on the `gtk::ApplicationWindow`) becomes `shortcuts.rs`'s
  `#[cfg(not(windows))]` body.
- Both call `idle::note_activity()` on successful install (OB-2) and on each input event.
- D3's "separate install sites, separate `Result`s, separate log lines" is now structural at
  two call sites that already exist, matching each platform arm's Windows counterpart.

**D7 is rewritten, not just re-cited.** The R1 framing — stubs become *documented
delegations*, citing A's `scheme_guard`-covered-by-nav precedent — is withdrawn. OB-7 is
right that the precedent does not apply: `scheme_guard` is a control genuinely subsumed by
another control, whereas here each stub's own job is simply being done. The stubs become
**real Linux bodies**. That is a smaller and more honest change than a delegation plus a new
module.

---

## OB-8 — `position()`'s reference frame — **REVISE**

Conceded. `EventButton::position()` (`gdk-0.18.2/src/event_button.rs:19`) and
`EventTouch::position()` (`event_touch.rs:21`) return the raw `event->x/y`, relative to the
`GdkWindow` the event was delivered to; `allocated_width/height()` (`widget.rs:494`/`:473`)
are the widget's allocation. My R1 phrase *"so they cannot desync"* asserts an identity that
holds only while the widget owns a `GdkWindow` coextensive with its allocation. Under Tauri
the webview sits inside `default_vbox` (`webview_window.rs:1874`) inside the
`ApplicationWindow`, one fullscreen child, offset 0 — benign today, an assumption not an
identity, and D5's bounds check (correct in itself) would silently pass a shifted frame.

**Replacement wording, plus the marker:**

> `position()` is `GdkWindow`-relative; `allocation()` (`widget.rs:500`, returning
> `gdk::Rectangle`) is the widget frame. They coincide under Tauri's single-fullscreen-child
> vbox layout, which is the only layout this kiosk ships.
> `// ponytail: assumes the webview's GdkWindow is coextensive with its allocation (one
> fullscreen child). If a second child ever lands in default_vbox, use
> WidgetExt::translate_coordinates (widget.rs:1863).`

Declared assumption with a named upgrade path, not an engineered generalisation for a layout
that does not exist. `allocation()` and `translate_coordinates` both verified present.

---

## Updated register — post-Round-2 state

| ID | Post-R2 state | Moved? |
|---|---|---|
| D1 | Mechanism unchanged (clean pass). **`input_watch` module withdrawn** (OB-7); handlers live in the two existing stubs. All handlers wrapped by `observe` (OB-6). Touch guarded by `is_emulating_pointer` (OB-5). | **Deps moved:** no longer depends on a new module or a `main.rs` edit. Now depends on D7's rewritten shape. |
| D2 | Unchanged. Clean pass. | — |
| D3 | **Leg 3 withdrawn** (OB-4b); legs 1+2 only, per parent's "and/or". C3 looser-liveness divergence declared (OB-4a). | **Deps moved:** P2-G dependency **removed** from D3. One-sentence chord note lands in G10's already-reserved slot. |
| D4 | Mechanics clean pass. **`max(loop_start_ms, …)` deleted**, replaced by the `0` sentinel = "not idle", mirroring `idle.rs:88-89` (OB-2). | Now depends on D7's install sites calling `note_activity()`. |
| D5 | Geometry clean pass. `position()` frame **declared as an assumption** with a `ponytail:` marker (OB-8). Tap counting deduplicated (OB-5). | — |
| D6 | Mapping clean pass. **`&& !mods.win` withdrawn**; replaced by a `should_swallow` guard at the Linux call site, verbatim Vector A ordering (OB-3). | **Deps moved:** no longer touches shared reviewed code; D2c test delta **removed**. |
| D7 | **Rewritten.** Stubs become real Linux bodies, not documented delegations. `scheme_guard` precedent withdrawn as inapplicable. Zero `main.rs` diff. | **Deps moved:** D1, D3, D4 now all route through D7's two call sites. |
| D8 | Unchanged. Clean pass; rebuttal confirmed by the Critic. F5/F11 residue explicitly **not** closed by OB-3's guard. | — |
| D9 | Unchanged. Clean pass. | — |
| D10 | Unchanged. Clean pass. OB-1's `GestureZoom` adds no dependency — same `gtk` crate. | — |
| D11 | Shape clean pass. Degrade semantics now defined by D4's sentinel (OB-2). | Now depends on D4. |
| D12 | Smoke 16–17 unchanged. The `Proceed` assertion **demoted from gate to backstop**; the gate is now `rustc` via `observe` (OB-6). | **Deps moved:** D1's safety invariant no longer depends on scenario 17 running. |
| D13 | **Withdrawn as a flag; replaced by an owned change.** D implements the `GestureZoom` capture-phase sequence-claim; gated by new P2-G row **H10** on touch hardware. Fallbacks recorded (wry #544 patch; `touch-action` CSS). | **New dep:** P2-G H10. Pairs with P2-B's `set_zoom_level` for full PF-04. |

**Net effect of the round:** one new module deleted, one `main.rs` diff deleted, one edit to
shared reviewed code deleted, one invented cross-spec obligation deleted, one runtime gate
replaced by a compile-time one — against one added change (PF-04) that the parent required
all along.
