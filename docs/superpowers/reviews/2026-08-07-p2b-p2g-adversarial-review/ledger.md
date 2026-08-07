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

## Moderator rulings

_(none required yet — no deadlock, no round-cap exhaustion, nothing struck)_

## Struck arguments

_(none — no role has argued from an unchecked checkable claim)_
