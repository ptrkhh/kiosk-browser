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

## Change register

Populated at the start of each thread from the Writer's opening turn (one row per change,
dependencies declared upfront).

| ID | Spec | Change | Deps | Verdict | Round |
|---|---|---|---|---|---|
| _(pending)_ | | | | | |

## Objection register

| ID | Change | Objection (one line) | Sev | Evidence tier | Disposition |
|---|---|---|---|---|---|
| _(pending)_ | | | | | |

## Moderator rulings

_(none yet)_

## Struck arguments

_(none yet)_
