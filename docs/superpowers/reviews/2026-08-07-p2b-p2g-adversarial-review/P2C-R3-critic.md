# P2-C — CRITIC, Round 3 (closing)

Six items, six accepted. Nothing escalated. Probes at `scratchpad/crit/{deg.rs,pfd.rs}`.

## Disposition

| Item | Writer's move | My check | Status |
|---|---|---|---|
| **NEW-1** | `pidfd: Option<OwnedFd>`; open failure ⇒ WARN + `("pidfd", …)` breadcrumb + continue; `Err` reserved for waiter-thread-creation failure | Reproduced the **degraded** path (`pidfd: None`, gated `kill(pid)`): `events=1` in all four orderings, `zombie=false` throughout, and the `exited` gate **skipped the kill entirely** on both already-reaped cases — 200/200 skips in the stress run, zero kill-by-pid ever issued post-reap. Exactly-one-exit-event is confirmed **independent of the pidfd**: the property comes from the waiter owning the `Child`; I ran the whole table with `fd = -1` and it is unchanged | **ACCEPTED** |
| **NEW-2** | Option (i): rationale rewritten to "a run without systemd, still root"; non-root manual runs declared unsupported and loudly degrading. Option (ii) declined | Both decline grounds verified: `XDG_RUNTIME_DIR` is **unset** here, and `runuser -u nobody -- mkdir -p /var/lib/kiosk-probe` → **Permission denied**. His second ground is the stronger one and it is correct — a per-user socket with an unwritable spool, breadcrumb dir and lock is a *silently* half-working dev run, which is worse than a loud one. **Judgment: acceptable**, and for a reason he did not claim — C14 makes RT-13 the per-PR Linux harness that builds `LauncherSink` directly with tempdirs, so the workflow "debug launcher supervision on Linux without root" is served by the test path, not by `cargo run`. Nothing real is removed | **ACCEPTED** |
| **NEW-3** | Smoke 13 records `cage -v` | Correct; that is the flag that exists and exits 0 | **ACCEPTED** |
| `job.rs:221-223` | `#[cfg(not(windows))] assign` takes `&ChildHandle`; lands in the same edit as C12's `Job::create()` rewrite | Right, and bundling it with the C12 edit to the same impl block is the lazy correct call | **ACCEPTED** |
| C7 `-1` sentinel | → `-2`, commented as the impossible-status sentinel | Trap removed; `-2` collides with nothing (`sink.rs:434-437` owns `-1`, C7's live range is 0…255 ∪ 129…192) | **ACCEPTED** |
| raw `syscall(2)` | Promoted from my note to a stated spec requirement in C5's body | Correct placement — it is a correctness requirement on the platform floor (glibc 2.35 on Ubuntu 22.04), not a plan-time choice. `kill(2)`'s exemption is right; it has been in libc since forever | **ACCEPTED** |

**On NEW-1's cost statement.** "Pre-P2-C status quo" is slightly generous — today's `spawn.rs:63-67`
is unreachable on Linux because `spawn_main` is a stub — but the substantive claim holds and is in
fact conservative: today's shape is an **ungated** `child.kill()` by pid, and the R3 degraded arm is
the same thing **gated** on `exited`. So the degraded mode is strictly better than the only Unix kill
path the tree has ever had, and the residual is a two-instruction window between the atomic load and
the syscall, correctly recorded as a `ponytail:` ceiling with pidfd named as the upgrade. **Not a new
exposure.**

## Residual risks — accepted as documented, each with a named carrier

| Risk | Carrier |
|---|---|
| Wedged cage: unit stays `active`, orphan survives, launcher not restarted (Windows' job object would still fire) | P2-G — image validation + hardware-checklist row |
| Degraded-mode kill loses reuse-immunity (two-instruction window) | `ponytail:` ceiling in C6; upgrade = pidfd when the sandbox permits |
| Non-root manual run unsupported | Stated in C2; degrades loudly (`pipe.rs:384` breadcrumb, C13 WARN) |
| cage behaviour on the floor's 0.1.4 (verified only on 0.1.5) | P2-G image validation; smoke 13 records the version it proved |
| Orphan-kill has no gate inside P2-C | P2-G, named |

**One integration-round note, not an objection.** In-session smoke 13-15 run outside systemd (systemd
is not PID 1 on the harness box), so C12's `INVOCATION_ID` guard will correctly fire and every smoke
run writes a `("job", …)` degraded breadcrumb. I checked that this breaks nothing: neither P2-A nor
P2-B asserts on `startup-degraded.txt` (grepped both specs). C15 should simply expect the breadcrumb
rather than read it as a failure — one sentence.

## Consistency confirmation

**I confirm, independently and explicitly.**

- **Every objection is dispositioned.** Ten raised (7 in R1, 3 in R2), ten accepted by the Writer,
  ten verified by me. Zero standing.
- **No open HIGH.** The two HIGHs of this thread — OB-1's waiter/reaper race and NEW-1's
  black-screen routing — are both closed by construction, and both closures are probed, not asserted
  (`ESRCH` 200/200 for the pidfd mode; gate-skip 200/200 and `events == 1` for the degraded mode).
- **Internally consistent.** The dependency graph closes with no dangling edge: C2 → C16 + C11
  (`RuntimeDirectoryMode=0700` carries the loud-failure property); C5 → C6, C7 and the `job.rs` edit
  in C12; C13 → C16; C12 → C11 (`KillMode`); C16 → a stated hard co-landing constraint with P2-A.
  No claim now rests on a mechanism that another change removed — which was the R2 failure mode
  (C16 invalidating C2's dev-run rationale) and is fixed.
- **Cross-spec obligations are owned, not orphaned.** A is kiosk-main only (verified: one `launcher`
  mention in the whole spec, `p2a:349`); P2-G carries the orphan-kill gate, the wedged-cage risk, the
  cage floor version and smoke 14's systemd half; the P2-A co-landing constraint is named in both
  directions.
- **Values verified against tier 1 where they exist:** `/var/lib/kiosk/` (parent §4 `:409`), all three
  unit directives (parent §3.1 `:171-175`), exit 86 (arch-05).

C's declared assumptions are now, unusually, mostly *measurements*: cage's exit propagation and
abnormal-death exit, `INVOCATION_ID` traversal, `SUN_LEN`, `SO_PEERCRED`, `File::try_lock`, and both
waiter modes were all run rather than argued. The one thing I would still call an assumption — that
cage 0.1.4 behaves as 0.1.5 does — has a gate and a named fallback shape.

**P2-C is converged from my side. No open objection, no open HIGH, no reservation.**
