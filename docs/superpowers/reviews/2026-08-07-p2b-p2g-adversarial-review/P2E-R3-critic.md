# P2-E — CRITIC, Round 3 (closing)

Re-verified at HEAD `1decd59`. Five items open at R3, five dispositions. No counters remain.

## Dispositions

| Item | Writer's move | Verified | Status |
|---|---|---|---|
| **OB-2** | `pub const MEM_CAP_N = 5` + launcher-side relation test | Both reach points confirmed; the test observes shipped values and fails if either moves. | **CLOSED — ACCEPTED** |
| **OB-3** | One authoritative table owned by E; F references by ID, must not restate; 18-W1/18-W2 rename; hard blocking E→F edge | Collision cleared; the statement is consumable by a citing spec without ambiguity. | **CLOSED — ACCEPTED** |
| **NEW-1** | Floor gate on 18-W2 (≥750 MB ⇒ defect, not ship); OOM justification withdrawn; `start_time()` guard; E4-before-E5 as spec; residual to the operator | He implemented the remedy I named in R2, plus a threshold, a consequence, an evidenced ordering and an operator rule. | **CLOSED — ACCEPTED-AS-DOCUMENTED-RISK. No HIGH remains.** |
| **NEW-2** | Four preconditions as fixture spec text + post-reload-URL assertion | All four verified against `state.rs` / `main.rs`; the URL assertion is stronger than I asked. | **CLOSED — ACCEPTED** |
| **NEW-3** | `media_error(kind, at, ms_since_wrap: Option<f64>)`; `12000` deleted from the page | `at` restored, threshold now lives once (E6's activation rule). Boundary hygiene is a consistent addition. | **CLOSED — ACCEPTED** |

## Verification detail

**OB-2 — the pin observes the shipped values.** `impl Default for RemoteConfig` is
`serde_json::from_str("{}")` (`schema.rs:303-307`), i.e. the *same* value the existing pin
asserts at `:345` (`health_sample_s == 60`, from `d_health_sample()` at `:44-46`).
`watchdog_config(None).healthy_run_s == 120` (`kiosk-launcher/src/main.rs:110-124`, pinned at
`:285-290`). `kiosk-launcher/Cargo.toml:14` carries `kiosk-core.workspace = true`, and the
launcher's own `#[cfg(test)] mod tests` (`main.rs:256-258`, `use super::*`) has
`watchdog_config` in scope. The test therefore reads both real defaults with **no hardcoded
copy** and fails if `d_health_sample`, `MEM_CAP_N` or the launcher's default moves. Confirmed
exactly as claimed. Keeping 18-W1's `no watchdog.safe_mode` assertion alongside it is correct
— they pin different properties (shipped-default relation vs cross-process end-to-end).

**NEW-2 — all four preconditions check out.** `state.rs:296-304` (`Online` + clear →
`Clearing`), `:306-311` (`Online` no-clear → `go_online(self.home)`), `grep` returns exactly
those two `IdleExpired` transition arms, and `:979-995` pins the no-op for Boot and Offline by
test. `--safe` never spawns the timer (`main.rs:1184-1185`, verbatim). The post-reload-URL
assertion converts my false-pass case from a precondition into a checked property — that is
the right call, since a precondition can be silently violated and an assertion cannot.

**NEW-1 — why I accept, and what it buys.** My R2 verdict named one remedy: a recorded
steady-state Windows number on a run that already exists. He took it and added a threshold, a
consequence, and an ordering. Judged against frame §4.4/§4.5 the residual is now: inherited
from tier 1 (1500 is the parent's number, and E cannot overturn §5.2:538 + §10:872
unilaterally), **declared** in both directions per C3, **pinned** by a mechanical gate that
catches the disqualifying case, **owned** by a named party, and **mitigated** by an ordering
that the evidence actually supports — E4 ships the number first and unconditionally, so the
release-note rule (p99 × 2, or `0`) is actionable rather than aspirational, and the `0` lever
is range-valid and already tested (`validate.rs:107-114`, `:267`). Withdrawing the
misattribution to G's Linux checklist was necessary and is done.

He is right that a harness floor is not fleet content, and he says so in the spec rather than
in the debate — which is the standard §4.4 sets. The gate bounds rather than eliminates; that
is the correct outcome for a risk the parent, not E, created. **No HIGH remains open.**

## Residuals I accept as documented risk

1. **Windows content between the recorded floor and 1500.** Bounded by the floor gate, carried
   by the operator, mitigated by E4-before-E5 + the release-note rule + the `0` lever.
2. **Operator-settable both sides of the interlock** — `health_sample_s` (remote config) and
   `healthy_run_s` (`kiosk.ini`, no range validation: `bootstrap.rs:75-91`'s `number()` applies
   no bounds). Declared since R1, unchanged.
3. **`setrlimit(RLIMIT_NOFILE, hard)`** on first process refresh — unavoidable inside sysinfo,
   effect is a *raise*, `LimitNOFILE` ask on C/G correctly downgraded to non-blocking.
4. **`watchdog.restart{code:80}` carries the fact, not the number** — no requirement names the
   number durable (OB-5, closed R2).

## One LOW for the integration pass — not blocking, no fast-track veto

The new authoritative parameter table names **two config keys that do not exist**, and this
matters more than usual precisely because F is now instructed to consume that table by
reference:

- `input.idle_clear` → the real key is **`content.clear_data_on_reset`** (`schema.rs:118`), and
  its default is **`true`** (`d_true`), so 18-W2 must actively set it `false`. `idle_clear` is
  the FSM's internal `MachineConfig` field name (`main.rs:291`), not a config key.
- `content.home` → the home URL key is **`content.url`** (`schema.rs`, `Content`; asserted at
  `:319`). `home` is the machine's internal field.

Four of six rows are correct (`maintenance.max_webview_mem_mb`, `logging.health_sample_s`,
`kiosk.healthy_run_s`, `maintenance.nightly_reload`). Two one-word fixes.

**Non-blocking integration note (not an objection):** the floor gate's outcome is binary, but
the recorded floor is useful either way — carrying the measured number into the release note
("your Windows engine baseline is ~X MB; your content adds on top") makes the p99 × 2 guidance
actionable on day one instead of after a week of telemetry. One sentence, no new work.

## Termination — my independent confirmation

**Confirmed.** I independently find P2-E internally consistent, with every objection
dispositioned and **no open HIGH**.

Checks I ran for consistency across the final register, not just per-objection: `MEM_CAP_N = 5`
is now the single N, and dwell = `5 × health_sample_s` holds simultaneously at shipped defaults
(300 > 120, pinned by the launcher test) and in 18-W1 (50 > 30, pinned by `no
watchdog.safe_mode`) — no contradiction, and no interaction with `startup_grace_s`. The
`ms_since_wrap` chain is consistent end to end (E3 emits the raw number → E1's signature
carries it → E6 owns the one threshold). E4's rationale is now Q1 traceability alone, which is
consistent with keeping the sum. 18-W2's preconditions match `state.rs`'s actual transitions.

One conditional, stated rather than waived: **E5's enforcement gate depends on 18-W2, which
runs in F7, which E declares "not implementable as written" until re-synced to the two-run
table.** That dependency is declared, blocking and owned — the correct handling — so it is an
integration action, not an open defect against E. It must not be lost: if the F re-sync does
not happen, E5's enforcement half has no gate and NEW-1 reopens.

## Verdict on the memory cap

**Safe to enable on a Windows fleet only under the stated gate and residual** — not
unconditionally at the shipped default. The mechanism, the exit path and breach→restart are
sound and now proven end to end; the *level* is the parent's undherived number compared against
a quantity that over-counts shared pages worst on Windows, and what makes shipping it
acceptable is the 18-W2 floor gate plus E4-before-E5 plus the operator lever — not evidence
that 1500 is right.
