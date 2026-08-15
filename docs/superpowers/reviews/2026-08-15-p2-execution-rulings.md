# P2 execution — controller rulings ledger

Decisions taken during subagent-driven execution of the seven P2 plans, on the
executor's own authority, without stopping to ask. Each records what was decided,
why, and what it costs if wrong.

This file is **tracked in git on purpose**. The SDD working ledger lives at
`.superpowers/sdd/<plan>/progress.md`, which is git-ignored and therefore dies
with the ephemeral container. The rulings must outlive it — they are decisions
made on the maintainer's behalf and are the only record of them.

Per-task completion lines, deferred minors and review packages stay in the
working ledger; only rulings are promoted here.

Branch: `claude/p2-specs-review-p0vyy9`.

---

## Cross-plan constraints carried from the execution brief

| Id | Constraint | State |
|---|---|---|
| X1 | P2-C's C16 launcher `resolve_data_dir` must equal P2-A's `/var/lib/kiosk`. Mismatch silently kills the TEL-10 spool drain. | A landed the literal at `main.rs:451` as a bare, copy-pasteable line. C matches. |
| X2 | P2-C's C5 `spawn_main` must pass `--config`, or every P2-G device boots into safe mode. | Open — P2-C. |
| X3 | P2-E task 10 is gated: read 18-W2's recorded Windows floor first. If ≥ 750 MB, do **not** ship E5 enforcement; file a defect against parent §5.2's default of 1500. | Open — P2-E stage 2. |
| X4 | P2-F's F7 matrix ships as `[18-W2]` only. 18-W1 is added by E5's enforcement commit, not before. | Open — P2-F. |
| X5 | The `webkit2gtk` dependency line is shared by B, C and D. First writer wins; others reconcile by union. | A wrote it (Task 4). See ruling R8 — it is `v2_20`, not the plans' `v2_16`. |

---

## Rulings

### R1 — Plan line-number citations are hints, not addresses
*P2-A pre-flight.* The plans cite `main.rs:45-52`, `:433-441`, `:1014-1049` and
similar. These drift as soon as the first task edits the file. Implementers are
instructed to locate by symbol name and treat cited ranges as hints, reporting
any discrepancy.
**Cost if wrong:** an implementer edits the wrong region — caught by the task
review's diff read.

### R2 — P2-A Tasks 6 and 7 ship without unit tests
*P2-A pre-flight.* Both specify "Test: smoke scenario N" and no unit tests. Their
bodies are signal wiring, not decision logic (the plans forbid re-deriving
decisions). Accepted as plan-mandated. Any **pure helper** either task introduces
still gets a unit test.
**Cost if wrong:** a wiring defect reaches the smoke gate instead of a unit test.
Acceptable because the smoke gate is blocking for A.

### R3 — The Linux clippy gate is "no new findings", not "clean"
*Raised by P2-A Task 1.* The Global Constraint says clippy `-D warnings` passes on
both platforms. At P2-A's base commit, Linux already fails with **56 pre-existing
`dead_code` errors** — Windows-only bodies and the very stubs these plans fill in.
Per-task gate on Linux is therefore **no new findings versus the task's base
commit**, proven by an A/B run captured against a verified-clean tree at the
committed head. A *decrease* is expected as stubs get filled. Full `-D warnings`
clean on Linux is a **plan completion gate**, checked after the last task.
Windows clippy stays absolute per task.
**Cost if wrong:** a genuine new lint hides inside the 56-error noise floor.
Mitigated by requiring the A/B proof every task plus a clean full run at plan end.

### R4 — The rustfmt gate is "no new drift"; no blanket reformat
*Raised by P2-A Task 2.* Verified against a clean worktree of base `4f60d0f`:
`cargo fmt --check` already reports **9 diffs across 4 files**
(`kiosk-launcher/src/credential_acl.rs`, `kiosk-main/src/credential_acl.rs`,
`kiosk-main/src/gesture.rs`, `kiosk-main/src/main.rs`). Per-task gate is no new
drift. A blanket `cargo fmt` was deliberately **not** run: it is unrequested
scope, and it would rewrite three files immediately before the tasks that edit
them (A-T3, A-T5, P2-D), adding conflict risk for zero functional gain.
**Cost if wrong:** those four files stay unformatted through P2 and need a
cleanup pass later. No behavioral risk.

This ruling immediately earned itself: A-T2's brief mandated an `assert_eq!` that
rustfmt rejects. Without the "no new drift" gate it would have vanished into the
pre-existing noise.

### R5 — `APP_SAFE_URL`'s hardcoded Windows spelling is not a gap
*P2-A Task 1 review.* `boot.rs:202` is
`APP_SAFE_URL = "http://tauri.localhost/safe.html"`, flagged as a possibly dead
URL on Linux safe boot. Traced end to end: `safe_boot()` is reached only from
`boot::load`'s two `RenderSafe` arms; `main.rs:661`
`let safe = args.safe || config_error.is_some()` is therefore always true when it
runs; so `home_url` resolves to `safe_url` = `bundled_url("safe.html")`, which
Task 1 made platform-correct. `APP_SAFE_URL` never drives navigation on any path.
Its two residual readers (`boot.rs:115` `first_event`, informational; `fetch.rs:45`,
inert because the safe config's `config_url` is `https://invalid/`) receive the
Windows spelling on Linux — and Task 1's **no-`cfg`** `(scheme, host)` classifier
is exactly what keeps that harmless. This is what spec A:91 means by "keeps
working with zero signature churn".
**Cost if wrong:** Linux safe mode paints nothing — caught empirically by Task 10
scenario 7.

### R6 — P2-A Task 3 was allowed to modify `boot.rs`, outside its brief's file list
*P2-A Task 3 review.* Replacing the fail-open credential stub with a real check
broke three pre-existing `boot.rs` tests that pinned the old behavior. Accepted
after a per-test verdict: `missing_credential_load_returns_render_safe` was
**strengthened** (exact `reason`/`message` equality replacing a weaker `contains`
helper, matching the Windows counterpart); the other two had their **fixtures**
chmod'd to `0600` with assertions byte-identical. No assertion loosened, no
coverage dropped, no test deleted or ignored. The edit was forced, not creep:
`boot.rs:165` runs the gate before the read at `:173`, so any real Unix check
necessarily reroutes those fixtures. The defect is the brief's file list, which
specified a behavior change without tracing its call sites' test coverage.
**Cost if wrong:** a security test passes for the wrong reason — mitigated by the
per-test verdict and by scenario 7.

### R7 — SEC-09 remains fail-open in the launcher on Linux; P2-C owns it
*P2-A Task 3 review.* `crates/kiosk-launcher/src/credential_acl.rs:100-104` is
still `Ok(true)`, consumed by `sink.rs:88`. Confirmed P2-C Task 8 names that exact
file and line range and specifies the same four tests, so it is owned, not
orphaned. Not a gap in A.
**Cost if wrong:** SEC-09 stays fail-open in the launcher process on Linux —
caught when P2-C Task 8 lands, and it is on that task's critical path.

### R8 — The shared `webkit2gtk` feature level is `v2_20`, not the plans' `v2_16`
*P2-A Task 4.* The Global Constraint's literal dependency line specifies
`features = ["v2_16"]`. Verified against the vendored crate:
`connect_web_process_terminated` is `#[cfg(feature = "v2_20")]`
(webkit2gtk-2.0.2 `src/auto/web_view.rs:2853`) — under `v2_16` the signal the task
exists to install **does not compile**. The plan contradicts itself: its own
constraint text says these signals are "stable since 2.20". Features are strictly
additive (`v2_20 → v2_18 → v2_16`), so `WebsiteDataManager::clear` (`v2_16`, needed
by A-T7) remains available, and `v2_20` does not reach `v2_40`, so that
prohibition is intact. Installed runtime is WebKitGTK 2.52.3.
**This is the X5 line P2-B, P2-C and P2-D reconcile against: union features onto
`v2_20`, never downgrade to `v2_16`.**
**Cost if wrong:** a later plan expects a narrower API surface than it gets —
harmless direction, since additivity removes nothing.

### R9 — Linux bodies keep `#[cfg(not(windows))]` gating despite the macOS mismatch
*P2-A Task 4 review.* The Linux bodies are gated `#[cfg(not(windows))]` while the
`webkit2gtk` dependency is scoped `cfg(target_os = "linux")`, so they would fail to
compile on macOS where the stubs they replace did compile. Kept the plans' gating:
this kiosk targets Windows and Linux only, it is the pre-existing convention the
plans mandate, and changing it would fork from the plans across every Linux body
in A/B/C/D for a target that does not exist. P2-B/C/D inherit this pattern.
**Cost if wrong:** a macOS build fails loudly at compile time, not silently at
runtime. Mechanical to re-gate if macOS ever ships.

### R10 — Hostless external schemes are an unowned gap; make it visible, do not close it
*P2-A Task 5 review.* `mailto:`, `tel:`, `sms:` short-circuit at `is_remote_origin`
(`Url::host_str()` is `None` → not remote → `should_block` returns `None`), so the
nav guard never judges them — while Task 5's new doc claimed it did. Checked
downstream: **P2-B Task 7 (plan line 626) is scheduled to update the same stub to
say "external schemes ride the nav guard (P2-A)"**, restating the false claim as
settled fact, after which no plan owns the control and the source asserts one
already does.

Ruled: fix the **text** in A, do **not** implement hostless-scheme enforcement.
No plan asks for it; practical impact on this target is low (WebKitGTK does not
auto-launch external handlers, so an unjudged `mailto:` fails to load rather than
escaping the kiosk, and it does not feed the FSM since `feeds_fsm` delegates to
the same classifier); and adding an unrequested security control mid-plan is scope
creep. The requirement is that the gap be *visible*. `scheme_guard.rs:58-71` now
reads "PARTIAL no-op on Linux" and labels it "KNOWN GAP, unenforced anywhere on
Linux"; `install()` remains a literal empty no-op.
**Carry into P2-B Task 7: it must not restate the false claim.**
**Cost if wrong:** hostless schemes stay unenforced on Linux with no telemetry —
visible in the source; Windows parity unaffected.

---

## Standing observation: the plans' literal code does not compile

Five of the first five P2-A tasks hit a defect in the plan's own code blocks:

| Task | Defect in the plan's literal code |
|---|---|
| A-T1 | Predicted two tests would fail RED; only one did — the described vulnerability class never existed in the old code. |
| A-T2 | Mandated an `assert_eq!` that fails `cargo fmt --check`. |
| A-T3 | Four defects: a by-reference arg on a non-`Copy` `io::Result`, a `super::super::` path resolving to the crate root, a missing `#[cfg(test)]` breaking `-D warnings`, and `assert_eq!(x, true)`. |
| A-T4 | A dependency feature level (`v2_16`) that could not compile the signal the task installs. |
| A-T5 | An `E0382` — the specified `.clone()` moved `telem` out from under the later shutdown closure. |

The plans' **reasoning, values, names, signatures and signal-to-event mappings**
have been reliable every time. Their **code is a draft**. Every dispatch says so
explicitly and requires deviations to be documented rather than silently smoothed
over. Expect roughly one extra fix round per task from this, and expect it to
persist into the harder plans.

---

## Carried into the smoke gate (P2-A Tasks 9–10)

- **Scenario 5 must drive a real `window.open()`, not a simulation.**
  `w.navigate(url)` from inside the GTK `create` callback runs `handle_user_message`
  **inline** (tauri-runtime-wry `lib.rs:239-248`), which borrows
  `WindowsStore(RefCell<…>)`. If WebKit emits `create` while tauri already holds
  that borrow, the callback panics on a double-borrow instead of returning `Deny`.
  Unprovable from a diff; scenario 5 is what settles it.
