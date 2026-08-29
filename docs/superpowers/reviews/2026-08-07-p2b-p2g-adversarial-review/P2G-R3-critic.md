# P2-G — CRITIC, Round 3 (closing)

No frame dispute. **[C]** = checked by me this turn. Eleven R2-accepted objections stay
closed.

## Disposition

| Item | Writer's move | My response | Status |
|---|---|---|---|
| OB-3 (a) erratum, G1 standard | Named the §7 keyboard cell as an erratum, escalated for ruling, fallback labelled as fallback-if-the-cell-binds | Correct, and now symmetric with G1 | **ACCEPTED** |
| OB-3 (b) `inject_js` route | Withdrawn outright; the "named P2-row deliverable ⇒ owned" conflation conceded | Correct — that was the load-bearing error | **ACCEPTED** |
| OB-3 (c) "bundled" stretch | Conceded | Correct | **ACCEPTED** |
| OB-3 "one row, not two" | Keyboard and RT-16 are the same gap in the same file; keyboard is the easier half | **Verified — reasoning holds**, with one clause to add | **ACCEPTED** |
| OB-3 refusal of the code deliverable | G is packaging; D disclaimed twice; parity gap on Windows; no app-owned surface affected | **Declining is correct.** G is not the right owner | **ACCEPTED** |
| OB-3 scope finding | Zero text inputs in any bundled page; gap confined to deployed sites | **Verified.** Adequate as G's contribution, and he does not claim it as a discharge | **ACCEPTED** |
| AC-1 `test -x` | Added to G15 | Reproduced in R2; correct | **ACCEPTED** |
| AC-2 spool-retention bound | Label bounded, `spool.dropped_expired` cited | Correct | **ACCEPTED** |
| AC-3 `dpkg-gencontrol` | Three tools named + `grep -q '\${shlibs'` FAIL assertion | The extra assertion is better than what I asked for | **ACCEPTED** |
| Consistency item | G15 asserts all three operator files **absent**, example present | Correct | **ACCEPTED** |
| B9 recorded for integration | Inert-both-axes, keep-and-relabel, §11 closed negatively, owner = integration | Matches my R2 statement | **ACCEPTED** |

### 1. "One row, not two" — verified, and it does simplify routing

**[C]** `crates/kiosk-main/src/inject.rs:1-19` says exactly what he cites:
`initialization_script` *"may be called only ONCE per webview … and is set at BUILD time
from the just-booted config … **there is no live-reinjection path, by design**"*, and it
already names the cursor-autohide timer as the "survives every navigation without
re-injection" shape. **[C]** `main.rs:1041-1046` is *"the ONE `initialization_script` call
for this webview"*. **[C]** `nav_policy.rs:169-180` does carry the recorded `ponytail:`
about CSP not being wired, so his caution to the implementer is accurate rather than
decorative.

So the reasoning holds: a **bundled, always-on** keyboard is the same shape as a control
already shipping in `build_injection`, and it does **not** depend on RT-16 landing. That is
material — it means integration routes one file, not a dependency chain.

One clause to add so the implementer is not surprised: gating the keyboard "on a boot-time
config value" is a new schema field in `kiosk-core`, not `inject.rs` alone. And a supporting
correction that runs his way: RT-16's knobs are not *unimplementable* on this engine, only
un-*live-reloadable* — boot-scope `inject_css`/`inject_js` with a documented
next-restart-to-change caveat is exactly what `display.cursor_autohide_seconds` already
does. That makes both halves boot-scope work in the same file and strengthens the one-row
argument.

### 2. Declining the code deliverable — correct, and G is not the owner

**[C]** `p2d:26` lists *"on-screen keyboard deployment (parent §7 table — P2-G)"* under
**Out**, repeated at `p2d:162`. **[C]** `grep -rni "tabtip|InputPane" crates/` → zero hits:
Windows shipped P1 with PF-02 open, so Linux is not diverging downward. A usable OSK is
layout + shift/symbols + focus tracking + viewport shift — a feature, in `kiosk-main`, on
hardware that is explicitly TBD. Building it inside a packaging spec is the scope error I
would object to anywhere else; I am not going to demand it here.

**Is the escalation properly discharged?** Yes, by the frame's own words. Frame §2:
*"a HIGH defect against whichever spec is its natural owner, **or a HIGH integration defect
if no owner is identifiable**."* D disclaimed it twice, G cannot take code scope — so no
owner is identifiable within A–G, and the frame's second clause is the correct
classification. Frame §4.5 is satisfied too: a named owner (whoever picks up RT-16; fallback
a new P2 sub-project scoped to `inject.rs`), a named phase (P2 unless the Moderator defers
RT-16, which G correctly refuses to do unilaterally), and a discoverability row (H4).

**My one condition, since I raised the escalation and the Moderator is right that an
escalation nobody picks up is how requirements get lost:** it must be entered in the
**ledger** as a standalone HIGH integration item — *"Linux touch keyboard + RT-16
`inject_css`/`inject_js`: one gap, `crates/kiosk-main/src/inject.rs`, phase P2 unless the
parent defers RT-16"* — not left as a paragraph inside G's spec text. Recorded in G only, it
is invisible to anyone reading the requirement matrix. With that entry, I accept the
disposition.

### 3. Scope finding — verified, and adequate as *contribution*, not as discharge

**[C]** `grep -n "<input\|<textarea\|contenteditable" crates/kiosk-main/bundled/*.html` →
rc=1, **zero hits** across all five pages. `pinpad.html` is a `<button>` grid. So nothing
P2-G installs is broken today; the gap is a deployed site's own UI surface on touch
hardware.

"Loud at provisioning rather than at first touch" is **not** a discharge of a parent §7 row
and the Writer says so unhedged ("G does not discharge the Linux touch-keyboard obligation
… this remains HIGH and open"). What it *is*: the achievable fraction of "deployment docs" —
documenting a hard constraint an integrator must check before deployment — plus H4 as the
enumeration mechanism. Against Q3 that is the right trade: an unhedged prerequisite sentence
converts a silent field failure into a provisioning-time one. I accept it on that basis and
on that basis only.

## Residuals accepted as documented risk

1. **Linux touch keyboard** — HIGH, open, escalated to integration, bound to RT-16, phase P2.
2. **Parent §4 erratum** (`/opt/kiosk/` → `/usr/lib/kiosk/` + `/etc/kiosk/`) — awaiting a
   Moderator ruling; fallback stated and survivable.
3. **Parent §7 keyboard-cell erratum** — same, with its fallback stated.
4. **Root by default** — C3 divergence declared; H1 promotes or rejects the `seatd` recipe.
5. **`Conflicts:` removal risk** — `apt -y` pulling an idle daemon proposes removing `kiosk`;
   loud, bounded by G12's update discipline.
6. **`StartLimitIntervalSec=0`** — a permanently broken install loops at 30 s forever;
   `Storage=persistent` + a computed ≥7-day journal floor preserve the cause.
7. **Unpinned-until-hardware** — cage on a real DRM seat, and the non-root uid path (H1/H2).
8. **G1's second dependency on C** — the Linux `spawn_main` must carry `--config`;
   fail-closed if it does not.
9. **B9** — inert on a conforming image; keep, relabel, correct B's inverted framing. Owner:
   integration.

## Consistency confirmation

I independently confirm the following, from my own checks across all three rounds:

- **Every objection is dispositioned.** 12 raised, 11 accepted-as-fixed in R2, OB-3 revised
  in R3 with all three of my grounds conceded. Zero countered, zero dangling.
- **No open HIGH *in* P2-G.** OB-1 (no `[Install]`) and OB-2 (gate with no runner) are fixed
  and I reproduced both fixes. OB-3's HIGH is correctly reclassified — under frame §2's own
  "no owner identifiable" clause — from a defect *in* G to a HIGH **integration** defect,
  conditional on the ledger entry above.
- **Internally consistent.** I swept the final G1–G16 register for contradictions: the
  zero-conffile rule now applies uniformly to all three operator files (G5/G6/G9) and G15
  asserts it; first-install-only `chown` (G4) and upgrade-only `chmod` (G5) do not collide
  and are both uid-agnostic on upgrade, so G16's flip survives; `Conflicts:` (G3) and the
  runbook grep (G11) are enforcement and verify of the same rule; `RestartSec=30` (G8) is
  the input to G12's journal-floor arithmetic; H2/H3/H4 (G14) match the G8/G11/OB-3 changes;
  G15's container assertions are all runnable without PID-1 systemd, which I verified
  directly. I found no remaining internal contradiction.
- **Frame §4 adoptability:** correct, feasible, consistent, evidenced, and owned — with the
  keyboard row owned *outside* G by an escalation I accept on the stated condition.

**Verdict: converged.** I have no further objections and no new ones to raise.
