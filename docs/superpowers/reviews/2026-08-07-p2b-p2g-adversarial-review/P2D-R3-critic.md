# P2-D — CRITIC, Round 3 (closing)

## Disposition

**NB-1 — ACCEPTED, and his strengthening is correct.** Verified at source: `shortcuts.rs` has
**no** early return on `gesture` anywhere — the only `else`-return in the file is
`let Some(args) = args else` at `:173` (an unrelated COM arg). The `Option` is carried through
`install` (`:104-110`), `install_accelerator_handler` (`:157-160`) and `windows_impl::install`
(`:258-263`) to `open_pin_pad(&app, gesture.as_ref())`, relying on that function's own `None`
guard. So `gesture.rs:293-296` is the outlier and NB-1's prescribed shape is the sibling
module's existing convention — rung 2, not a new one. **He is also right about the guard's
line numbers and I was wrong:** `open_pin_pad`'s `None` guard is `gesture.rs:154-157`; my R2
cited `:158-161`. Correcting my own citation.

**cfg-12 with handlers installed unconditionally — confirmed, nothing exits, nothing unlocks.**
Traced all three legs: (1) `gesture == None` ⇒ no `TapCounter` ⇒ tap branch returns before any
outward call; (2) leg 2 reaches `open_pin_pad(&app, None)` which self-guards at `:154-157` and
returns — byte-identical to Vector A at `:190`; (3) the installed handlers make exactly one
outward call on that path, `idle::note_activity()`. No pad opens either way, so the module
doc's cfg-12 rule (`gesture.rs:17-23`) holds and the C3 divergence he declares
("not installing" vs "installing and no-opping") is behaviourally invisible. `note_activity()`
before the gate is the whole fix and it is placed correctly. OB-2 survives intact: both
installs degrading still leaves `LAST_INPUT_MS` at the `0` sentinel and nothing fires.

*Citation drift, LOW, no action needed:* he cites `:186` for the `open_pin_pad` call (actual
`:190`; `:186` is the `else if is_technician_chord` line) and `:261` as "the LL-hook path"
(`install_ll_hook()` takes no `gesture`; the carry is `:263`). The substantive claim is true.

**NB-2 — ACCEPTED verbatim.** Guard moved to the button handler, R2's typed-accessor rationale
withdrawn. Re-verified: `Event::is_pointer_emulated()` `event.rs:302-304`, `event_wrapper!`'s
`impl Deref<Target = Event>` `event.rs:522-528`, `pub struct EventButton(crate::Event)`
`event_button.rs:6`. All three truth-table rows are 1 regardless of whether WebKit consumes the
touch — the unverifiable is no longer load-bearing. Nothing further.

## Residual accepted as documented risk

**Multi-finger over-count — the safe-direction claim holds, with one wording caveat.**
Confirmed for the **lock**: `open_pin_pad` (`gesture.rs:153-173`) only calls
`window.navigate(bundled_url("pinpad.html"))` at `:167-169`; it verifies nothing, and PIN
verification is P1-D2c Task 5's. So an over-counted tap cannot weaken the lock — his claim is
correct as stated, and I confirm it.

It is **not free**, though, and the declaration should say so rather than read as costless.
The over-count is real and reachable unintentionally: `TapCounter::tap` pushes one timestamp
per `TouchBegin`, so a 5-finger corner press twice reaches the bootstrap default of 7
(`gesture.rs:181-182` doc), where Windows' one `WM_LBUTTONDOWN` per press
(`gesture.rs:244-245`) would need 7 deliberate taps. The consequence is availability, not
security: the kiosk navigates away to `pinpad.html`, and grep over
`crates/kiosk-main/bundled/pinpad.html` finds no `cancel`/`back` affordance, so an unintended
opening plausibly strands the session until a correct PIN. That is Task 5's scope and is
identical in kind on Windows — D only raises its frequency — so it is **not** a live objection.
One word in D5's declaration: *safe for the lock, not free for availability*. H4 already
exercises corner-tap on real touch hardware and the `is_emulating_pointer()` deadband is
recorded as the one-line upgrade if it fires. Declining to build it now is right: it would
re-import the unverifiable NB-2 just removed.

## Consistency confirmation

**I confirm P2-D independently.** Checked as a whole, not as a diff:

- **Every objection dispositioned.** R1 OB-1…OB-8 all ACCEPTED; R2 NB-1/NB-2 both adopted, with
  the placement I asked for. Nothing carried unanswered; nothing re-argued from a defeated
  position.
- **No open HIGH.** The round's only HIGH was OB-1 (PF-04 uncovered — independently corroborated
  by `verify-COVERAGE.md:34,93,146-153`). It is closed with a named implementing spec (D), a
  named gate (P2-G **H10**, numbering free after G's R1 took H9), and recorded fallbacks in the
  parent's own order (wry #544 patch, `touch-action` CSS).
- **Internally consistent.** The cross-references that moved this round all close:
  D1↔D7 (handlers live in the two existing stubs, zero `main.rs` diff), D4↔D7
  (`note_activity()` precedes the cfg-12 gate, which is what makes "either leg arms the clock"
  true), D5↔NB-2 (dedup on the button side), D6↔D8 (the `should_swallow` guard does **not**
  close the F5/F11 residue and D8 says so), D3↔parent `:318-320` ("and/or"), D11↔D4 (degrade
  semantics defined by the sentinel, now also covering the cfg-12 path), D12↔OB-6 (`rustc` via
  `observe` is the gate, smoke 17 the backstop), D10 unchanged and duplicate-free.
- **Every deferral owned with a named gate.** `GDK_TOUCH_CANCEL` → H4; cage-headless virtual
  input → smoke 17 → H4; multi-finger over-count → H4 + recorded deadband; PF-04 → H10 +
  fallbacks; F5/F11 → declared C3 divergence in D8; `observe`'s `'static` bounds → plan-time
  shim, which frame Q5 permits for shims.

No refusal, no cause to withhold. Two LOW wording notes above (the availability caveat; the
two drifted citations) are fast-trackable and I do not veto bundling them.

**P2-D is converged.**
