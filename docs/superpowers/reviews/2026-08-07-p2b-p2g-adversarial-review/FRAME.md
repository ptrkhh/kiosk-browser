# FRAME — Adversarial design review of P2-B … P2-G

Set by the Moderator from source materials. **All arguments are judged against this frame.**
Frame disputes are debated first; a role that disputes the frame must say so explicitly in
its turn block before arguing anything else.

## 0. What is under review

Six draft design specs, all marked "draft, 2026-08-06 (awaiting review)":

| Spec | File |
|---|---|
| P2-B | `docs/superpowers/specs/2026-08-06-p2b-linux-hardening-egress-design.md` |
| P2-C | `docs/superpowers/specs/2026-08-06-p2c-linux-launcher-supervision-design.md` |
| P2-D | `docs/superpowers/specs/2026-08-06-p2d-linux-native-input-design.md` |
| P2-E | `docs/superpowers/specs/2026-08-06-p2e-offline-video-soak-design.md` |
| P2-F | `docs/superpowers/specs/2026-08-06-p2f-ci-functional-gate-design.md` |
| P2-G | `docs/superpowers/specs/2026-08-06-p2g-linux-packaging-image-design.md` |

**P2-A** (`…-p2a-linux-bringup-design.md`, rev 3) is **already reviewed and is NOT under
review.** It is a reference and a source of binding precedent: where a B–G decision cites
A, A's text is authority, not a proposal. Objections of the form "A is wrong" are **out of
frame** and will be struck; the only admissible A-related objection is "B–G contradicts A"
or "B–G's re-derivation of an invariant A explicitly handed forward is missing/incorrect"
(A records two such hand-forwards: the `FrameLoadInterruptedByPolicyChange` filter
re-derivation, and the WebKitGTK feature-floor re-derivation).

## 1. Sources of record, in evidence-tier order

1. **Parent spec of record** — `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md`
   (rev 2). Cited by every P2 spec. Requirement IDs (`SEC-*`, `arch-*`, `RT-*`, `PF-*`,
   `cfg-*`, `M*`, `H*`, `TEL-*`, `OD-*`) come from here.
2. **P2-A rev 3** (reviewed) and the P1 sub-specs in the same directory (D2a/D2b/D2c, E1,
   E2, F1, F2, G) — prior work, binding as precedent.
3. **The codebase** — `crates/kiosk-{core,main,launcher}/`, `.github/workflows/`,
   `packaging/`, `dist-template/`, `Cargo.toml`, `Cargo.lock`. A spec claim about existing
   code is checkable and MUST be checked before it is argued.
4. **Vendored dependency sources** — present under `~/.cargo/registry/src/*/` after
   `cargo fetch`: `wry-0.55.1`, `tauri-2.11.5`, `webkit2gtk-2.0.2`, `webkit2gtk-sys-2.0.2`,
   `gdk-0.18.2`, `gtk-0.18.2`, `tao-0.35.3`, `glib-0.18.*`, `gio-0.18.*`. Every
   `file.rs:NNN` citation in a spec is mechanically verifiable against these. **Verify
   before arguing.**
5. External sources (upstream docs, bug trackers, distro package facts) — weakest
   documentary tier; usable but loses to 1–4 on direct conflict.
6. Stated assumption — admissible only when declared as an assumption with its risk.

**A verified counterexample defeats any general claim it contradicts, regardless of tier.**

## 2. Goals (what these six specs must collectively achieve)

Parent §9 P2 row, verbatim, is the completion contract for phase P2:

> **P2** | Linux + robustness | WebKitGTK parity (incl. pinch-gesture intercept, keep-awake
> at compositor), .deb + systemd + cage docs + §7.2 Linux hardening, idle reset (native),
> **memory cap restart + health-sampled RSS**, cross-platform webview-hang detection (JS
> ping), config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log
> level, restart_app

Plus the phased/deferred obligations the parent explicitly assigns to P2 elsewhere:

- **arch-04 / RT-02 / OD-1** — webview-hang liveness via cross-platform JS round-trip ping
  "lands in **P2** (WebKitGTK/Android, where no native unresponsive signal exists)" (§3.1).
- **PF-04 / wry #544** — pinch zoom is not suppressible via `zoom-level`; "P2 intercepts the
  GTK zoom gesture in the platform layer / upstreams a wry hook; validate on touch
  hardware" (§7 zoom-lock row, §11).
- **PF-07 / M8 / H5** — Wayland display blanking is compositor-owned; "PRIMARY is
  configuring cage/wlroots not to blank"; `systemd-inhibit` is suspend-only (§7 keep-awake
  row, §11).
- **PF-05 / RT-05** — offline-video loop soak on the pinned Debian 12 image; ≥72 h hardware
  soak is a pre-release gate (§10, §11).
- **SEC-09** — credential owner-only mode, enforced, both crates (§4, §8).
- **SEC-10** — egress containment: **every** resource request checked against
  `content.allowlist` plus injected restrictive CSP; residual gaps documented (§7).
- **§7.2 Linux** — cage locked session, VT-switch/zap disabled, dedicated seat, DPMS/
  screensaver off, sleep/suspend targets masked, as a *deployment gate*.
- **§10 CI** — "Linux compile check (P0 → **functional at P2**)"; soak/endurance scheduled,
  not per-PR; RT-13 end-to-end watchdog test.
- **RT-16** — the document-start injection engine ships in P1; **P2 exposes the operator
  `inject_css` / `inject_js` knobs on top of it** (§7 preamble).

**Coverage is a first-class frame criterion.** A P2 row item that no spec in A–G owns is a
HIGH defect against whichever spec is its natural owner, or a HIGH integration defect if no
owner is identifiable. "Deferred to P3/P4" is only admissible if the parent itself defers it.

## 3. Constraints (binding, not negotiable inside this debate)

- **C1 — No reimplementation of decision logic.** `kiosk-core` stays platform-free; every
  Linux sub-project ports *observation/enforcement edges only* and reuses the existing pure
  decision functions (parent §4 layering rule; every P2 spec restates it).
- **C2 — Shipped API first.** Tauri/wry APIs before raw `webkit2gtk`; raw sys-FFI only where
  no safe binding exists, contained. (A's doctrine, inherited by B–G explicitly.)
- **C3 — Honest parity.** Divergence from Windows is permitted but must be stated in both
  directions (stricter *and* looser) with justification. Silent divergence is a defect.
  Parity with what Windows *actually enforces*, not with what P1 descoped.
- **C4 — Best-effort doctrine.** Hardening/telemetry failures degrade (`config.warn` /
  event) and never block boot; telemetry is observation, never a dependency.
- **C5 — Fail-closed on security gates.** SEC-09/SEC-10 gates fail closed; nav/permission
  defaults are deny. A change that converts a fail-closed gate to fail-open is HIGH.
- **C6 — No new dependencies without justification** (ponytail doctrine, applied throughout
  P1/P2-A: `zbus` rejected, local externs preferred, "a few lines over a new crate").
- **C7 — Platform floor.** Debian 12 / Ubuntu 22.04, x86_64; WebKitGTK 4.1 via
  `webkit2gtk-4.1`; target hardware explicitly TBD (P2-G designs to the floor).
- **C8 — Windows must stay green.** Every change is `cfg`-gated or host-tested-pure unless
  the spec explicitly declares a cross-platform change and justifies it (P2-E declares two).
- **C9 — Merge gates are real.** Each spec's declared gate (smoke scenarios, CI job, RT-13)
  is part of the change; a gate that cannot actually run in the stated environment is a
  feasibility defect.

## 4. Success criteria (what "adopted" means for a change)

A design decision is adoptable when all of:

1. **Correct** — it does not break an existing invariant, requirement, or state-machine
   property; cited mechanisms exist and behave as claimed.
2. **Feasible** — buildable against the pinned dependency versions on the platform floor,
   by the declared gate, without unpinned magic.
3. **Consistent** — with the parent spec, with P2-A, with its sibling P2 specs, and
   internally.
4. **Evidenced** — every load-bearing claim is either verified against tier 1–4, or
   declared as an assumption with a named pinning mechanism (a smoke scenario, a plan-time
   check) and a documented residual risk.
5. **Owned** — the requirement it discharges is named, and anything it defers has a named
   owner (a later sub-project, a `ponytail:` record, or a hardware-checklist row).

## 5. Explicit quality criteria for subjective points

When a point is not mechanically decidable, judge it in this order and say which rung you
are on:

- **Q1 — Requirement traceability.** Does it discharge a named requirement, or is it
  invention? Invention loses to traceability.
- **Q2 — Least mechanism** (ponytail). Fewest moving parts that meets the requirement;
  stdlib/shipped-API/existing-pattern before new code; new dependency last. A simpler
  design that meets the requirement beats a richer one that also meets it.
- **Q3 — Observability of failure.** A failure mode that is silent in the field is worse
  than one that is loud, at equal cost. "Silent" is a defect class in this project (the
  parent names silent black video, silent egress blocks, retry-laundering).
- **Q4 — Blast radius.** Prefer changes whose failure is bounded to one control/one
  platform. Cross-platform changes carry a higher bar (C8).
- **Q5 — Reviewability.** Prefer a decision a plan-time implementer can execute without
  re-deriving the design. "Resolve at plan time" is legitimate for *values and shims*, not
  for *whether the mechanism works at all* — the latter must be pinned by a gate.

**Preference-based argument is struck.** "I would have done X" is not an objection.
"X violates Q2 because the same requirement is already discharged by <existing thing at
file:line>" is an objection.

## 6. Severity

- **HIGH** — violates a named requirement (parent/A/P1 precedent), or breaks correctness or
  feasibility. Includes: uncovered P2-row item, fail-open on a security gate, a gate that
  cannot run, contradiction with P2-A, cross-platform regression risk on Windows.
- **MED** — degrades quality or adds unmitigated risk: undocumented divergence, an
  unpinned load-bearing assumption, a silent failure mode, an unowned deferral.
- **LOW** — polish: wording, citation drift, ordering, redundant prose. **Fast-tracked by
  the Moderator without debate**, and may be bundled. The Critic may veto a fast-track with
  cause (one line naming the cause).

## 7. Protocol reminders binding on every role

- Write **only** inside your own turn block. Never anticipate, paraphrase, or speak for
  another role. Do not write "the Critic will say…" or "as the Writer conceded…" unless
  quoting a turn block that already exists in the ledger.
- **Verify before arguing.** If a claim is mechanically checkable — a `file.rs:NNN`
  citation, a symbol's existence, a feature gate, a `cargo` fact, a config key, a CI job —
  check it and cite the result. An unchecked checkable claim is struck on sight.
- Every objection carries: what breaks, when, why it matters, severity, evidence tier.
- Silence on an objection is a concession.
- Argue from the ledger, not from memory. Defeated points may not repeat without new
  evidence.
- A clean pass ("no material objections") is a legitimate Critic outcome. Struck or
  frivolous objections count against the Critic.
- Burden of proof sits with whoever is asserting: the Writer per change, the Critic per
  objection.

## 8. Ledger location

`scratchpad/debate/ledger.md`, maintained by the Moderator only.
