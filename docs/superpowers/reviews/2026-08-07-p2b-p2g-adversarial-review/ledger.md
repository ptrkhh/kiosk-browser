# LEDGER — Adversarial design review, P2-B … P2-G

Maintained by the **Moderator** only. Roles argue from this file, not from memory.

Frame: `scratchpad/debate/FRAME.md`. Evidence base: `scratchpad/debate/verify-P2{B..G}.md`
and `verify-COVERAGE.md`.

## Status

| Phase | State |
|---|---|
| 0 — Frame set | DONE |
| 1 — Verification (7 agents) | DONE |
| 2 — Threads B–G (Writer/Critic, ≤4 rounds each) | R1 Writer RUNNING |
| 3 — Integration round | NOT STARTED |
| 4 — Spec revision + commit | NOT STARTED |

## Phase 1 — verification record (admitted evidence, tier 3–4)

| Spec | VERIFIED | FALSE | DRIFT | UNVERIFIABLE | Undeclared assumptions |
|---|---|---|---|---|---|
| P2-B | 47 | 5 | 7 | 9 | 11 |
| P2-C | 26 | 3 | 7 | 2 | 10 |
| P2-D | 31 | 6 | 4 | 4 | 9 |
| P2-E | 17 | 6 | 10 | 5 | — |
| P2-F | 32 | 8 | 4 | 4 | — |
| P2-G | 31 | 7 | 9 | 3 | 13 |
| Coverage | — | 8 UNOWNED | 6 PARTIAL | — | — |

Reports: `verify-P2{B,C,D,E,F,G}.md`, `verify-COVERAGE.md`.

### Standing HIGH candidates carried into Phase 2 (not yet ruled)

Architecture-level, not citation-level:

| # | Spec | Finding |
|---|---|---|
| V1 | B | Allowlist is **URLPattern, not globs** (`nav/allowlist.rs:26-27`); both pure helpers written against wrong pattern language. Also no accessor exposes pattern strings. |
| V2 | B | `connect_resource_load_started` exists ungated (`web_view.rs:2523`) and parent §7 names `resource-load-started` as *the* SEC-10 mechanism; B asserts no request-level API and never rebuts the parent. |
| V3 | C | `spawn.rs` "plain `child.wait()`" not implementable — `spawn.rs:89-95` documents why; `Child` retained at `sink.rs:421`, no unix duplication API. |
| V4 | C | `job.rs:217-225` Job Object is a Unix no-op ⇒ launcher death orphans `kiosk-main`; contradicts C's "kill semantics see exactly what Windows sees". |
| V5 | D | GTK itself installs `gdk_event_handler_set(gtk_main_do_event,…)` (objdump-proven) ⇒ `set_handler` **replaces** GTK dispatch; "the slot is free" false in the load-bearing direction. |
| V6 | D | "`set_handler` cannot fail" false — `assert_initialized_main_thread!()` panics; D's whole error model rests on it. |
| V7 | D | "never unexitable" (parent §3.5) cited to the wrong rule and then inverted; both Linux exit vectors on one handler, no fallback leg. |
| V8 | E | `maintenance.max_webview_mem_mb` already ships (schema + validate + RT-08 `UNIMPLEMENTED` + parent §5.2/§10); E invents `memory_max_mb`. Default is **1500, not 0** ⇒ E's "Windows unchanged" property false. |
| V9 | E | Wrong process measured — parent §6/D2e/`health.rs:3` specify **webview-process** RSS; `sysinfo` already held across ticks ⇒ both proposed mechanisms redundant (Q2). |
| V10 | F | GitHub-hosted job cap is **6 h**; F's 8 h+ soak cannot run on any runner it names (C9). |
| V11 | F | Harness F promotes does not exist as code (zero `weston`/`cage` hits, no shell scripts, no spool helpers, A6 binary absent). |
| V12 | F | Three parent §10 obligations neither covered nor deferred: Authenticode signing gate, Windows-runner leak soak, RT-09 live token exchange. |
| V13 | G | Install dir `/usr/libexec/kiosk/` contradicts parent §4 `/opt/kiosk/`, undeclared; `resolve_config_dir` = binary dir ⇒ config/credential/mp4 land under `/usr`. |
| V14 | G | `StartLimitIntervalSec` in `[Service]` silently discarded (systemd 255 proven) — the start-limit decision depends on it. |
| V15 | G | Parent §7.2 "disable DPMS/screensaver in the cage session" + §7 PRIMARY keep-awake (compositor no-blank) have **no runbook step**; H3 asserts an outcome nothing produces. |
| V16 | ALL | 8 UNOWNED P2 obligations: JS-ping hang detection (arch-04), pinch intercept (PF-04), `inject_css`/`inject_js` (RT-16), remote log level, `restart_app`, WebKitGTK `print` signal (H1), PDF Linux column (M4/OD-8), Windows-runner leak soak (§10). |

## Thread status

| Thread | Changes | Objections (R1 + later) | Rounds used | State |
|---|---|---|---|---|
| P2-B | B1–B12 | 9 + 4 = 13 | 3 | Critic closing turn in flight |
| P2-C | C1–C16 | 7 + 3 = 10 | 3 | **CONVERGED** — both confirmations given |
| P2-D | D1–D13 | 8 + 2 = 10 | 3 | **CONVERGED** — both confirmations given |
| P2-E | E1–E8 | 11 + 3 = 14 | 3 | Critic closing turn in flight |
| P2-F | F1–F16 | 12 + 2 = 14 | 3 | Writer round 3 in flight |
| P2-G | G1–G16 | 12 | 3 | Writer round 3 in flight |

Totals so far: **71 objections raised**, 0 struck, 0 frivolous. No Critic has needed a
fast-track veto. Every thread's Critic accepted its full Round-1 set after the Writer's
revisions; every thread then found new defects *in the replacements* — which is the
protocol earning its cost.

## Converged threads — verdicts

### P2-C — adopted-with-revisions
16 changes (3 added during debate: C12 orphan-kill parity, C13 single-instance parity,
C16 launcher-owned `resolve_data_dir`). Both HIGHs closed by construction and probed:
OB-1's waiter/reaper race (waiter owns the `Child`, sole status consumer; `ESRCH` 200/200)
and NEW-1's `pidfd_open`-denial black screen (`pidfd: Option<OwnedFd>`, WARN + breadcrumb +
continue; gate-skip 200/200 with `events == 1`). 10 spec claims withdrawn, 4 open decisions
closed. Residuals each carry a named carrier (wedged cage → P2-G; degraded-mode reuse
window → `ponytail:` in C6; non-root manual run → unsupported, loudly degrading; cage
0.1.4-vs-0.1.5 → P2-G smoke 13 records the version it proved).

### P2-D — adopted-with-revisions
13 changes. Central mechanism **withdrawn**: `gdk::Event::set_handler` replaces GTK's own
dispatch (objdump-proven, reproduced by both roles), so a handler defect kills all input —
the un-exitable device parent §3.5:319-320 forbids. Replaced by GTK widget-signal
observation (keys on `gtk::ApplicationWindow`, pointer/touch on the webview), a split the
Critic showed is *required* by GTK3's asymmetry. Net deletion: a module, a `main.rs` diff,
an edit to shared reviewed code, an invented cross-spec obligation, a runtime gate.
PF-04 gains an owner (D implements `GestureZoom` capture-phase intercept; G H10 gates it).
Residual: N-finger tap over-counts vs Windows — safe for the lock, not free for
availability; H4 gates, deadband recorded and deliberately not built.

## Cross-spec items accumulating for integration

| # | Item | Raised in | Touches |
|---|---|---|---|
| X1 | M4 / OD-8 PDF default-block — **unowned**, parent never defers it | B (conceded, escalated) | B, parent |
| X2 | B9 `systemd-inhibit` child is inert on both axes under G's runbook; parent §11's "confirm cage honours idle-inhibit" answered **negatively** | G OB-7 | B, G, parent §11 |
| X3 | Linux touch keyboard: squeekboard/onboard impossible under cage (verified both roles); substitute rests on **unowned** `inject_js` | G OB-3 (escalated HIGH) | G, D, parent §7 |
| X4 | E↔F parameter coupling — F copied E's R1 numbers; rule adopted: *cite the sibling's scenario, don't restate its parameters* | F OB-2/9/14, E OB-3 | E, F, G |
| X5 | C16 launcher `resolve_data_dir` needs hard co-landing with P2-A's `/var/lib/kiosk/` | C OB-3 | C, A |
| X6 | Remaining UNOWNED P2-row items from `verify-COVERAGE.md` | Moderator | all |

## HIGH integration items (standalone — NOT resolved inside any one spec)

Recorded here so they are visible to the requirement matrix rather than buried in a
spec's prose. The P2-G Critic made this placement an explicit condition of accepting the
keyboard escalation.

| # | Item | Status | Evidence |
|---|---|---|---|
| **I1** | **Linux touch keyboard + RT-16 `inject_css`/`inject_js` — ONE unowned row, not two.** Both live in `inject.rs` on the shipped P1 engine (`inject.rs:1-19`, wired at `main.rs:1041-1046`). squeekboard/onboard are *impossible* under cage — verified independently by both roles: cage 0.1.4/0.1.5 exposes no layer-shell, no input-method-v2, no virtual-keyboard, no text-input. A **bundled** always-on keyboard (parent §7's actual wording; `pinpad.html` is the precedent) needs no live-reinjection path and does **not** depend on RT-16 landing. | **OPEN — HIGH.** Owner: whoever picks up RT-16; fallback a new `inject.rs`-scoped sub-project. Phase P2. Discoverability: G's H4 + runbook prerequisite. Not P2-G's (packaging, not `kiosk-main` code) and explicitly **Out** in P2-D (`p2d:26`, `:162`). Windows has the identical PF-02 gap (`grep tabtip\|InputPane` → zero), so Linux is not diverging downward. | G OB-3, both roles |
| **I2** | **M4 / OD-8 PDF default-block, Linux column.** Parent §12 records OD-8 as **applied**, not deferred; P2-B's factual claim (unwired on Windows too) is correct but symmetric non-delivery does not discharge a live parent requirement. | **OPEN — HIGH.** No owner. Conceded outright by P2-B's Writer rather than given an invented owner. | B OB-8 |
| **I3** | Remaining UNOWNED P2-row items from `verify-COVERAGE.md` not otherwise resolved: JS-ping webview-hang detection (arch-04/RT-02 — `watchdog.hang` has no Linux producer, arch-15 case (c) unreachable), remote log level, `restart_app`, WebKitGTK `print` signal (H1). | **OPEN.** To be dispositioned in the integration round. | coverage matrix |

## Moderator rulings

### R1 — Parent §4 install path (`/opt/kiosk/` vs `/usr/lib/kiosk/` + `/etc/kiosk/`)

**Ruling: G's layout is adopted; parent §4's Linux install-dir cell is recorded as an
erratum requiring an owner-level amendment to the spec of record.**

Rationale. The parent is tier 1 and a spec under review cannot overrule it by fiat — that
was the correct objection. But the conflict here is not preference against requirement, it
is requirement against a verified external constraint: `/opt` triggers lintian
`dir-or-file-in-opt` at severity **error**, which G's own §Testing gate ("lintian clean")
would fail, and Debian Policy 9.1.1/10.7.2 give the blessed alternative. Both roles
verified this independently. The frame's evidence order settles it — a verified
counterexample defeats a general claim regardless of tier — but the *amendment* is the
owner's call, not the Moderator's, so it is flagged rather than silently absorbed.
Survivable fallback if the owner refuses: ship under `/opt/kiosk/` with a documented
lintian override.

### R2 — Parent §7 Linux touch-keyboard cell (squeekboard/onboard)

**Ruling: erratum, on the same standard as R1; the obligation itself is NOT discharged and
becomes integration item I1.**

**Rationale — AMENDED after the integration round (INT-5).** The verdict stands; the
reasoning it originally rested on was partly wrong and is corrected here, because a ruling
carrying a false sub-claim will be re-derived wrongly by whoever picks it up.

- **Stands, and carries the ruling alone:** cage exposes no **layer-shell** protocol on
  either 0.1.4 or 0.1.5. An on-screen keyboard must display itself as an overlay; without
  layer-shell it cannot. squeekboard is out on both versions.
- **Withdrawn from the evidence base:** `wlr_virtual_keyboard_manager_v1_create` **is**
  present in cage 0.1.5. The original rationale cited its absence; that was true of 0.1.4
  only. Input *injection* is therefore available on 0.1.5 — it is display that is not.
- **Withdrawn entirely:** the derived claim that a separate-process OSK would break P2-D's
  per-process `ActivityClock`. True for an XTEST/onboard client, false for a
  `zwp_virtual_keyboard_v1` client. It must not be reused as an argument.
- Every cage claim in P2-G is now version-stamped, which is what made this correction
  findable at all.

G's contribution (the deployment prerequisite stated unhedged + H4b enumeration per device
class) is the achievable fraction and a Q3 win, and G is right that it is not a discharge.
Left in G alone the requirement would vanish from the matrix, so it is lifted to I1.

### R3 — Fast-tracked mechanical items (frame §6; no Critic veto lodged)

- **P2-B:** corpus-implication test must assert in `H(u)` terms, not `re.is_match(u) ⇒
  allow.allows(u)` — the latter is R2's withdrawn full-URL claim and cannot pass
  (falsified by the banked path divergence at `allowlist.rs:641` and the home-origin
  widening at `:387-397`). Add `ws://`/`wss://` and `http://` rows to the battery corpus,
  which is https-only while the compiler accepts four schemes.
- **P2-E:** the authoritative parameter table names two keys that do not exist —
  `input.idle_clear` → `content.clear_data_on_reset` (default `true`, so the fixture must
  set it `false`), `content.home` → `content.url`. Matters because F now consumes that
  table by reference.
- **P2-C:** smoke 13 uses `cage -v` (`--version` exits 1 and would fail the script under
  `set -e`).

## Struck arguments

**None.** No role argued from an unchecked mechanically-checkable claim, so nothing met the
striking standard. No Critic lodged a fast-track veto. No objection was ruled frivolous.

---

# FINAL LEDGER

## Termination check (frame §"Terminate when")

| Criterion | Met | Evidence |
|---|---|---|
| No open HIGH objections | **Yes** | All 4 integration HIGHs closed by construction; no thread carries an open HIGH |
| Every objection dispositioned | **Yes** | 83 objections: refuted / conceded / fixed-by-revision / accepted-as-documented-risk |
| Tradeoffs documented | **Yes** | See "Assumptions, risks, tradeoffs" below |
| Surviving changes actionable and executable | **Yes** | Merge order derived and committed; every declared edge stated in both directions |
| Writer and Critic independently confirm internal consistency | **Yes, all six** | Each thread's closing turn carries both confirmations, given separately |

**Terminated on the criteria, not on the round cap.** No thread exhausted its 4 rounds; no
Moderator ruling was needed to substitute for a consistency confirmation.

## Per-spec verdicts

| Spec | Verdict | Changes | Objections (sev) | Rounds |
|---|---|---|---|---|
| **P2-B** | **adopted-with-revisions** | B1–B12 | 13 (2 HIGH, 8 MED, 3 LOW) | 3 |
| **P2-C** | **adopted-with-revisions** | C1–C17 (4 added) | 10 (2 HIGH, 4 MED, 4 LOW) | 3 |
| **P2-D** | **adopted-with-revisions** | D1–D13 (2 added) | 10 (1 HIGH, 7 MED, 2 LOW) | 3 |
| **P2-E** | **adopted-with-revisions** | E1–E10 (2 added) | 14 (1 HIGH, 9 MED, 4 LOW) | 3 |
| **P2-F** | **adopted-with-revisions** | F1–F16 | 14 (2 HIGH, 10 MED, 2 LOW) | 4 |
| **P2-G** | **adopted-with-revisions** | G1–G16 | 12 (3 HIGH, 7 MED, 2 LOW) | 3 |
| **Integration** | **adopted-with-revisions** | cross-cutting | 12 (4 HIGH, 5 MED, 3 LOW) | 1 |

No change was withdrawn outright as a *unit*; three central **mechanisms** were withdrawn
and replaced (B's content-filter/`send-request` route, D's global GDK handler, E's invented
config key). Nothing was rejected-by-ruling.

## Mechanisms withdrawn on evidence (the review's highest-value output)

| Spec | Withdrawn | Why | Replaced by |
|---|---|---|---|
| B | `send-request` cancel via `connect_local` | The signal does not exist on `WebKitWebResource` — it is `sent-request`, past tense, void return; the gboolean `send-request` is a `WebKitWebPage` (web-process-extension) signal the crate does not bind. A boot probe would return `None` on every device and SEC-10 would land fail-open. | Content filter reinstated as the enforcement authority; `resource-load-started` observe-only |
| B | "allowlist patterns are globs" | They are URLPattern (`allowlist.rs:26-27`, in words). Two pure helpers were specified against the wrong pattern language. | Compiler at host+scheme+port, soundness by corpus implication test |
| C | `child.wait()` / then `waitid(WNOWAIT)` | `spawn.rs:89-95` documents why the Windows waiter clones a handle; the reaper-first race then fabricates `ChildExited{128}`, breaking exactly-one-exit-event | Waiter owns the `Child` as sole reaper and reporter; sink holds a `pidfd` |
| D | `gdk::event::set_handler` | GTK itself installs `gdk_event_handler_set(gtk_main_do_event, …)` (objdump-verified by both roles) — `set_handler` *replaces* GTK's dispatch, has no chaining API, copies every event, and can panic on install. Any defect kills all input: the un-exitable device parent §3.5 forbids. | GTK widget-signal observation, keys on the window, pointer/touch on the webview |
| E | `memory_max_mb` | `maintenance.max_webview_mem_mb` already ships, default **1500** not 0 | Deletion from the RT-08 `UNIMPLEMENTED` table |
| F | "F writes no product code" | A (reviewed), B, C and E all assign harness automation to F; the loop was circular | F owns the harness, as `crates/kiosk-smoke` |

## Resolved

Every objection in all seven threads. Highlights, by class:

- **Correctness/safety:** the un-exitable-device composition (INT-4) — a wedged GTK loop
  was invisible to the launcher, D had withdrawn its third exit leg, and G's runbook removes
  VT/getty/SSH; closed by giving arch-04's JS-ping an owner (P2-C's new **C17**), verified
  reachable at the 2.32 floor. D's cfg-12 gap (an unconfigured exit gesture let a live
  session be wiped). C's `pidfd_open`-denial black screen. B's verified SEC-10 false-allows.
- **Feasibility (C9):** the 8 h soak against a 6 h hosted-runner cap; scenarios 13–15 with
  no runner (F installed weston, never cage); five deferrals pointing at P2-G rows that did
  not exist; a `.deb` whose unit had no `[Install]` section, making autostart a total no-op.
- **Coverage:** 5 of 8 previously-unowned P2-row obligations now owned — arch-04 JS-ping (C),
  `restart_app` (E9), remote log level (E10), PF-04 pinch intercept (D), and the Windows-runner
  leak soak, Authenticode gate and RT-09 live token exchange (F).
- **Consistency:** the E↔F parameter-copying class, fixed once as **F-CITE** rather than
  three times.

## Unresolved — Moderator-ruled, with rationale

| # | Item | Ruling | Why it is not closed here |
|---|---|---|---|
| **I1** | Linux touch keyboard + RT-16 `inject_css`/`inject_js` — **one** unowned row, not two (both live in `inject.rs` on the shipped P1 engine) | Escalated. Owner: whoever takes RT-16, else a new `inject.rs`-scoped sub-project. Phase P2. | No B–G spec opens `inject.rs`. squeekboard/onboard are impossible under cage (verified). G is packaging, not `kiosk-main` code; P2-D lists it **Out** twice. Windows has the identical PF-02 gap, so Linux is not diverging downward. Needs an owner-level decision. |
| **I2** | M4 / OD-8 PDF default-block, Linux column | Escalated, no owner invented | Undelivered on **both** platforms while parent §12 records OD-8 as *applied*. Symmetric non-delivery does not discharge a live requirement. Fleet-wide decision, not P2-B's. |
| **R1** | Parent §4 install path `/opt/kiosk/` | G's `/usr/lib/kiosk` + `/etc/kiosk` adopted; parent cell recorded as **erratum** | `/opt` is lintian `dir-or-file-in-opt` at severity **error** — G's own gate would fail. The amendment to the spec of record is the owner's call. Fallback: ship under `/opt/kiosk/` with a documented override. |
| **R2** | Parent §7 Linux keyboard cell | **Erratum** (rationale amended per INT-5 above) | Same standard as R1; obligation lifted to I1. |

## Assumptions, risks, tradeoffs

**Assumptions still carried (each with a named pinning mechanism):**
- WebKit content rules apply to the request classes SEC-10 names — pinned by smoke 8(a)–(d),
  including the service-worker variant; a failure is recorded before merge, not in the field.
- `INVOCATION_ID` survives the cage hop — probed in-session; C12's guard depends on it.
- cage propagates child exit codes and exits on abnormal child death — probed against 0.1.5;
  the deployment floor is 0.1.4 and smoke 13 records the version it proved.

**Documented risks, each with a carrier:**
- Memory-cap **level**: 1500 has no derivation and no measurement; the cap measures a
  footprint proxy that over-counts worst on Windows. Enforcement is merge-gated on a recorded
  floor; the residual between harness floor and real fleet content is **the operator's**.
- Wedged *cage* (as opposed to a wedged app): carried by P2-G's new H11 plus a runbook
  power-cycle line. C17 covers the app-side cases.
- Degraded-mode composition: `pidfd_open` denial, unset `INVOCATION_ID`, filter-compile
  failure and absent operator files each degrade loudly and independently.
- Feature floor is a *declared* minimum, not build-enforced — Cargo unifies features and
  `tauri-runtime-wry` already pulls `v2_40`.

**Tradeoffs accepted, with the losing side stated:**
- **B:** off-pattern *paths* on allowlisted hosts are unenforced for subresources. Accepted
  because path scoping is not an exfil control on either platform (`/assets/*` admits
  `?d=SECRET`) — verified, not assumed.
- **D:** an N-finger tap over-counts against Windows' single `WM_LBUTTONDOWN` — safe for the
  lock, **not free for availability**; the deadband is recorded and deliberately not built,
  because building it would reintroduce the unverifiable field the fix removed.
- **C:** orphan-kill parity *reassigns + detects + defers*; it does not close the gap.
- **G:** root-by-default service user (weakest posture, simplest DRM access), with the full
  non-root `seatd` recipe drafted and H1 promoting one on evidence.
- **F:** the smoke gate exercises a binary differing from a shipping one in exactly the
  `KIOSK_CONFIG_PUBKEY_B64` constant — a variation production already has.

## Merge order (committed)

```
A rev3
  ├─ C (⊕ C16 ⊕ C17 ⊕ --config)   ┐  order-independent
  └─ B                            ┘
        └─ D
             └─ E stage 1  (E1–E4, E6–E10, all scenario bodies; no enforcement)
                  └─ G  (⊕ H4a, H4b, H10, H11, the two G15 assertions)
                       └─ F  (F7 matrix = [18-W2])
                            └─ one commit: E5 enforcement + 18-W1 into F7's matrix
```

C16 carries a hard co-landing constraint with P2-A's `/var/lib/kiosk/`: a mismatch silently
kills the TEL-10 spool drain.
