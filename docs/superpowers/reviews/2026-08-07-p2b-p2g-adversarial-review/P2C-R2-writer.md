# P2-C — WRITER, Round 2

Probes re-run in-session; sources at `scratchpad/probe/{ob1.rs,waitid.rs,misc.rs,u/kiosk.service}`.
Seven objections, seven positions. **REVISE ×7, REBUT ×0.** OB-2 carries one sub-point as a named
documented risk inside the revision, flagged as such rather than as a separate position.

I re-derived the two places where I do **not** take the Critic's suggested mechanism (OB-2(a),
OB-4) and say why in-line; the objection is accepted in both cases, only the fix differs.

---

## OB-1 — **REVISE**

**Reproduced first.** `probe/ob1.rs`, case A — sink reaps, then the waiter's `waitid`:

```
A/R1 reaper-first: sink=Some(7) | waiter rc=-1 errno=10 si_code=0 si_status=0
                   => C7 would emit ChildExited{128}
```

`errno 10` is `ECHILD`. Both failure modes the Critic named are real: the event is lost, and C7's
total mapping fabricates `ChildExited{128}` from an untouched buffer. His FSM consequence is also
correct and I checked it: `watchdog.rs:194-199` handles `ChildExited` with **no phase guard**, so the
post-kill event re-enters during `Phase::BackingOff` and runs `restart(code, at, "exit")` a second
time, pushing a second timestamp into the rule-7 window (`watchdog.rs:149-156`). A Windows hang
restart therefore counts as two; losing the Linux event would make safe-mode escalation
race-dependent. So "no event on `ECHILD`" alone is **not** an admissible fix — it makes the contract
read *at most one*, and arch-12's window is built on *exactly one*. I reject that branch of the
Moderator's option set explicitly.

**Fix by construction, not by guard.** The root cause is that on Unix a child's exit status is a
**single-consumer resource** — there is no analogue of Windows' duplicated process handle, which is
exactly what `spawn.rs:89-95` is working around. Any design with two readers has this race. So there
is one reader, and it is the one that reports:

> **The waiter thread owns the `Child` and is the sole reaper, sole status consumer and sole
> reporter (`child.wait()`, stdlib). The sink holds a `pidfd` — a reuse-immune handle it uses to
> kill and to observe death, and which cannot consume a status.**

That is the Windows ownership algebra restored: two independent kernel-backed handles, one status
consumer. `waitid`, `WNOWAIT` and the hand-rolled `siginfo` all disappear (see OB-5).

```rust
#[cfg(windows)] pub type ChildHandle = std::process::Child;
#[cfg(unix)]    pub struct ChildHandle { pidfd: OwnedFd, exited: Arc<AtomicBool>, pid: u32 }
// impl ChildHandle { pub fn id(&self) -> u32 }  — Child::id() already satisfies this on Windows
```

- `spawn_main` (unix): spawn → `pidfd_open(pid)` → move the `Child` into the waiter thread → return
  `ChildHandle`. `pidfd_open` failure takes the **existing** `Err` arm verbatim (`spawn.rs:139-151`
  `kill_orphan` + `Err`, caller supplies the one synthetic `ChildExited{-1}`), which is the precise
  analogue of the handle-duplication failure that arm was written for.
- Waiter: `child.wait()` → `ExitStatus` → C7 mapping → `exited.store(true)` → send one
  `ChildExited`. No error branch exists, because there is no competitor.
- `kill_and_wait` (unix, C6): `pidfd_send_signal(pidfd, SIGKILL)` then bounded poll on `exited`.

**Probed, `probe/ob1.rs` case B** — four orderings including the kill/exit race:

| case | `kill_and_wait` | events | code | zombie |
|---|---|---|---|---|
| live child, killed | killed | **1** | 137 | no |
| `exit 86`, then kill | **ESRCH** (already dead+reaped) | **1** | 86 | no |
| SIGKILL self, then kill | **ESRCH** | **1** | 137 | no |
| kill races exit | killed | **1** | 137 | no |

`ESRCH` on an already-reaped `pidfd` is the property that matters: it is the *construction* proof
that the sink's kill can never land on a recycled PID — the objection I raised against every
kill-by-pid design in R1 and could not answer for my own.

**What `spawn.rs:100-109`'s contract now reads.** Unchanged in substance, stronger in derivation:
*"one spawn attempt, one exit event"* — on `Err` the caller supplies the synthetic
`ChildExited{-1}` and no supervised child exists; on `Ok` the waiter thread is the only party that
can observe the status and it sends exactly one event. The R1 sentence "the caller's `Child` is
unaffected" is replaced by "the caller never holds the `Child`; it holds a pidfd, which no reap can
invalidate".

**Cross-platform change, declared per frame C8.** `LauncherSink.child`'s type becomes the alias.
Windows behaviour diff is **zero lines**: on Windows `ChildHandle` *is* `std::process::Child`, so
`job.rs:199 assign(&self, child: &Child)`, `sink.rs:378-381`, `:406-407`, `:421-423` compile
unchanged. This is C's one declared cross-platform change (P2-E declares two).

**How it is tested — a race needs a test that can fail.** `probe/ob1.rs` case B4 ("kill races exit":
`kill_and_wait` issued with zero delay against `sh -c 'exit 7'`) is the failing shape: under R1 it is
the ECHILD window. It becomes a host test in `spawn.rs`, asserting `events == 1` over N=200
iterations of spawn-and-immediately-kill. Under R1 that test fails intermittently; under this design
it cannot fail without the ownership rule being broken. RT-13 does not cover it — `rt13.rs:145-152`
passes `job: None` and never exercises the kill/exit race — so the host test is the gate, not RT-13.

**Dependency movement.** C5 now depends on a declared `sink.rs` seam; C6 and C7 re-source (below);
OB-5 dissolves.

---

## OB-2 — **REVISE** (with one sub-point carried as a documented risk)

**(a) The code stops lying — accepted, different mechanism.** He is right that C12 as written changes
no code, so `job.rs:217-225` keeps returning `Ok` and `main.rs:189-199`'s `Err` arm — the
`("job", …)` breadcrumb replayed at `main.rs:222-226` — can never fire on Linux. A launcher outside a
unit reports armed supervision it does not have. That is the Q3 class.

Fix: `#[cfg(unix)] Job::create()` returns `Err` when the process is not inside a systemd service, so
the **existing** WARNING-and-continue path fires with its existing message and existing breadcrumb.
Zero new plumbing.

I take `std::env::var_os("INVOCATION_ID").is_none() → Err` rather than his `/proc/self/cgroup` read.
Checked on this box: `INVOCATION_ID` is unset, and `/proc/self/cgroup` is a **legacy hybrid**
listing (`9:name=systemd:/`, `0::/`, plus eight v1 controllers) — parsing that correctly across v1 /
v2 / hybrid is more code than the thing it guards. `INVOCATION_ID` is set by systemd for every
service since v232 and inherited by the whole unit tree, so it reads exactly "this process is inside
a service", and it is one env lookup. `ponytail:` ceiling recorded: it is env-settable, so the only
misreport it permits is a false *negative* warning on a box where someone exported it by hand.

Message and breadcrumb reason stay `"job"`; only the text changes to name the cgroup rather than the
Job Object.

**(b) The cooperative-chain divergence — accepted, and carried as a documented risk.** I checked
`job.rs:12-16`: the kill-on-close guarantee is explicitly *"precisely because it needs no cooperation
from the dying process."* My chain needs two cooperating links — cage must notice its child died and
exit, and systemd must then run a stop job. `spawn.rs:31-37` names the realistic kiosk failure (a
process wedged in an uninterruptible kernel wait behind a hung GPU/display driver) and cage is the
process holding the DRM device. If cage wedges: unit stays `active`, no stop job, the orphan
survives **and** the dead launcher is never restarted. On Windows the job object still fires.

I have no in-scope fix. `WatchdogSec` needs `Type=notify` and cage does not `sd_notify`;
`RuntimeMaxSec` would restart a healthy kiosk on a timer. **Residual risk:** a wedged compositor is
an unrecoverable-without-a-site-visit state on Linux where Windows would at least not orphan.
**Carrier:** P2-G — image validation plus a hardware-checklist row, alongside the systemd half of
smoke 14 that C10 already sends there. C12's divergence statement is amended to name **both**
looser-direction cases (non-systemd dev run; wedged cage), not just the first.

**(c) "Closes the U3 gap" is withdrawn as a summary line.** He is right that no P2-C gate observes
orphan-kill: RT-13 passes `job: None` deliberately (`rt13.rs:145-152`), so it never covered this on
Windows either, and smoke 13-15 assert restart / exit-86 / no-zombie. Replacement wording: C12
**reassigns enforcement** to the unit cgroup, **adds detection** per (a), and **defers the gate** to
P2-G with a named owner. Frame C9 is satisfied (the gate has an owner and can run where it lands);
the R1 claim to have closed the gap in this spec is not.

**Not disputed and re-verified:** `KillMode=control-group` really is the default, so making it
explicit costs nothing; `PR_SET_PDEATHSIG` stays rejected for the two reasons he re-confirmed
(`rt13.rs:163-166`, `job.rs:131-134`).

---

## OB-3 — **REVISE**

**Checked, and the fix is smaller than a dependency row — it is C's own.** `crates/kiosk-launcher/src/main.rs:48-53`
is verbatim as he quotes: `ProgramData` else `PathBuf::from(".")`, joined with `"kiosk"`. On Linux
that is `./kiosk`, CWD-relative. `kiosk-main/src/main.rs:436-441` is the identical function.

But the dependency edge on A does **not** exist, because A does not own the launcher's copy. Checked:
P2-A's Scope/In is `crates/kiosk-main/src/{nav,recovery,clear}.rs` … "Linux `resolve_data_dir`
(`/var/lib/kiosk/`) and `machine_id`", and `p2a:113` sits inside a kiosk-main paragraph. A mentions
the launcher exactly **once** in the whole spec — `p2a:349`, deferring "launcher/heartbeat/systemd
(P2-C)". So the launcher's `resolve_data_dir` is C's, by exactly the ownership logic C9 already
applied to the launcher's `credential_acl.rs`. Declaring a prerequisite on A would have been wrong.

**And it is worse than a socket-path prerequisite.** The launcher's own doc says why
(`kiosk-launcher/src/main.rs:44-47`): *"The same rule as kiosk-main's `resolve_data_dir`: the
launcher's `spool/launcher` partition and the `spool/main` partition it drains have to land in the
same place."* If A lands `/var/lib/kiosk` in kiosk-main and C leaves the launcher at `./kiosk`, the
launcher drains an empty `./kiosk/spool/main` and **TEL-10 dies silently on Linux** — the exact
silent-loss class `sink.rs:365-376` exists to prevent. That makes this a correctness item C must
carry, not a citation fix.

**Revision — new change C16.** `#[cfg(unix)] resolve_data_dir() -> PathBuf::from("/var/lib/kiosk")`
in `crates/kiosk-launcher/src/main.rs`, matching A's kiosk-main value exactly, with the doc rewritten
in the same edit. Recorded as a **hard co-landing constraint with P2-A**: the two functions must
agree, so whichever merges second must match the first, and if A's value changes, C's follows. Path
length: `/var/lib/kiosk/hb-<pid>.sock` = 27 bytes against the probed 107-byte ceiling.

C2's `runtime_dir()` fallback is then `/var/lib/kiosk`, absolute, CWD-independent — which is also
what disposes of OB-4.

---

## OB-4 — **REVISE**

Accepted: `runtime_dir()` branches on whether `/run/kiosk` exists, and `RuntimeDirectory=` creates
and destroys it with the unit, so hand-run-then-unit takes two different lock inodes and both
acquire. `File::try_lock` is per-inode; a path that moves is not a token.

**Fix: the lock does not use `runtime_dir()` at all.** `<data_dir>/launcher.lock`, where `data_dir`
is C16's absolute `/var/lib/kiosk`. One fixed inode in both orderings, and the branch that caused the
defect is simply not on this path.

I do not take his `/tmp/.kiosk-launcher.lock`: it introduces a fourth directory the launcher touches,
world-writable, with no `RuntimeDirectoryMode` protection, purely to solve a problem C16 already
removes. Fewer moving parts wins (Q2). `fs::create_dir_all(data_dir)` precedes the lock so the
"directory absent ⇒ silent loss of backstop" path he identified does not occur on a first dev run.

The socket keeps `runtime_dir()`'s `/run` preference (tmpfs, unit-scoped wipe); only the lock is
pinned to the data dir. The two mechanisms want different things and now say so.

---

## OB-5 — **REVISE, by deletion**

He is right on both counts and I checked both: `siginfo_t` is 128 bytes by `__SI_MAX_SIZE` on every
Linux ABI, so an assertion on a struct padded to 128 constrains the filler and never the offsets that
move (`si_status` at 24 on x86_64 vs 20 where there is no `__pad0`), and `waitid` performs no length
check. The analogy to `job.rs:105-111` is misapplied: that assertion is *justified in its own comment*
by `SetInformationJobObject` independently validating `cbJobObjectInformationLength`. Presenting an
inert check as the pin is exactly the thing a plan-time implementer copies forward.

**Disposed of by OB-1's redesign: the struct is gone.** The waiter reads
`ExitStatus::code()` / `ExitStatusExt::signal()` from `child.wait()` — stdlib, no layout, no
assertion, no `waitid`. Remaining local FFI in this spec is (i) C3's `ucred`, three `u32`s with no
arch-varying padding on any Linux ABI and probed returning the correct PID, for which **no size
assertion is claimed** — the probe is the pin; and (ii) a variadic `syscall` extern for
`pidfd_open`/`pidfd_send_signal`, which passes only scalars and has no struct at all.

The false mitigation sentence is struck rather than reworded.

---

## OB-6 — **REVISE**

Confirmed in-session: `apt-cache policy cage` → candidate `0.1.5+20240127-2build1` (noble/universe);
`cage` is not installed but resolves, so C15's assertion is executable here — I take his
feasibility finding as banked, not as something I need to re-argue.

**Decision on the floor: the spec pins no cage version constant.** Frame C7's floor is Debian 12 *and*
Ubuntu 22.04, and the dev/CI environment is neither — so a version constant in C10 would be wrong
wherever it was read. Revision:

- C10's "empirical pin (cage 0.1.4)" is downgraded from a recorded measurement to a **declared
  assumption about cage's behaviour**, with no version attached.
- Smoke 13 asserts the property and **emits `cage --version` as part of its output**, so every run
  records the version it actually proved rather than the spec asserting one.
- The floor-version assertion (Debian 12, cage 0.1.4) is routed to **P2-G image validation**, where a
  Debian 12 image with that package actually exists — the same destination C10 already uses for the
  systemd half of the contract.

Residual risk if 0.1.4 and 0.1.5 differ in propagation: caught at P2-G image validation, before an
image ships, with the fallback shape C10 already names (a two-line `exec` wrapper whose status
systemd sees directly).

---

## OB-7 — **REVISE, reinstating unconditional unlink**

He is right and the withdrawal is reversed. I re-derived the reachability rather than taking it:

The name is `/run/kiosk/hb-<our-own-pid>.sock`. For the corpse-probe's "connect succeeds ⇒ live peer"
branch to fire, some other process must be **listening** on a path named after a PID this process
currently holds — impossible for a peer launcher (we hold the PID), and impossible for a squatter,
because C11's `RuntimeDirectoryMode=0700` on a root-owned directory means no other principal can
create the file at all. C13 excludes the surviving-predecessor case independently. The branch is
unreachable by construction in the deployed shape, and irrelevant in the dev fallback.

So the property `FILE_FLAG_FIRST_PIPE_INSTANCE` buys on Windows for one flag bit is bought on Linux
by `RuntimeDirectoryMode=0700` — a directive C11 already carries for a different reason — not by a
connect-probe, a retry arm and a second failure mode. Q2: the simpler design that meets the
requirement wins.

**Reinstated:** unconditional `let _ = fs::remove_file(&path);` before `bind`, with the comment
stating the reason it is safe here (the name is ours by construction; the directory is 0700
root-owned) and naming `pipe.rs:100-104` as the Windows counterpart whose loud-failure property is
preserved by permissions rather than by a probe. Bind failure for any other cause still takes
`pipe.rs:370-388`'s breadcrumb path unchanged.

Net: one line of code where R1 had a branch, a syscall path and a retry arm.

---

## Updated register — post-round state

| ID | Change | Post-round state | Dependencies moved? |
|---|---|---|---|
| C1 | UDS listener transport | **Clean pass, unchanged** | no |
| C2 | Socket naming / derivation / seam / `SUN_LEN` | **Revised (OB-7, OB-3):** corpse-probe **reinstated as unconditional unlink**; `runtime_dir()` fallback is now C16's absolute path | **yes** — now depends on **C16**; loud-failure property now depends on C11's `RuntimeDirectoryMode=0700` |
| C3 | `SO_PEERCRED` via local `ucred` extern | **Clean pass, unchanged.** No size assertion claimed (OB-5); probe is the pin | no |
| C4 | `pipe.rs` Unix `serve` | **Clean pass, unchanged** | no |
| C5 | Linux `spawn_main` + waiter | **Redesigned (OB-1):** waiter owns the `Child` and is sole reaper/reporter; sink holds a `pidfd`. `waitid`/`WNOWAIT`/`siginfo` **withdrawn** | **yes** — declares one cross-platform change (`LauncherSink.child` → `ChildHandle` alias, zero Windows behaviour diff); no longer depends on C7's siginfo decode |
| C6 | `kill_and_wait` Unix body | **Re-sourced (OB-1):** `pidfd_send_signal(SIGKILL)` + bounded poll on the waiter's `exited` flag; no `try_wait`, no reap. Bound and its `ChannelFault`-race rationale unchanged | **yes** — depends on C5's `pidfd` |
| C7 | `128 + signo`, never-86 | **Re-sourced (OB-1):** from `ExitStatus::code()` / `ExitStatusExt::signal()`, not siginfo. Totality closed: `code()` else `signal()+128` else `-1`. Adopting the Critic's note: ambiguity with a literal `exit 137` is unreachable — kiosk-main exits only 0 (`cli.rs:31`), 86 (`pinpad.rs:156`), 101 on panic — and that sentence is added | **yes** — source changed; encoding unchanged |
| C8 | `heartbeat.rs` Linux client | **Clean pass, unchanged** | no |
| C9 | Launcher `credential_acl.rs` + A's C12 hand-forward | **Clean pass, unchanged** | no |
| C10 | `cage -- kiosk-launcher` shape | **Revised (OB-6):** no cage version constant; behaviour is a declared assumption, version recorded by the gate, floor assertion → P2-G | **yes** — version pin moves to P2-G |
| C11 | systemd unit shape | **Clean pass, unchanged** — and `RuntimeDirectoryMode=0700` now also carries C2's loud-failure property (OB-7) | load-bearing for C2 and C12, newly stated |
| C12 | Orphan-kill parity | **Revised (OB-2):** gains a code deliverable — `#[cfg(unix)] Job::create()` → `Err` when `INVOCATION_ID` is unset, firing the existing WARN + `("job", …)` breadcrumb. Divergence statement extended to the **wedged-cage** case. Summary line downgraded from "closes" to "reassigns + detects + defers the gate" | **yes** — gate ownership explicit to P2-G; documented residual risk (wedged cage) carried by P2-G |
| C13 | Single-instance parity | **Revised (OB-4):** lock is `<data_dir>/launcher.lock`, not `runtime_dir()`; `create_dir_all` first | **yes** — now depends on **C16**, no longer on C2's `runtime_dir()` |
| C14 | RT-13 cross-platform | **Clean pass, unchanged.** Confirmed unaffected by C5's redesign: `rt13.rs` builds `LauncherSink` directly (`:138-152`) and never calls `spawn_main` or `acquire_single_instance` | no |
| C15 | Smoke 13-15 | **Revised (OB-6):** smoke 13 emits `cage --version` with its assertion | **yes** — floor-version half → P2-G |
| **C16** | **NEW** `#[cfg(unix)] resolve_data_dir() -> /var/lib/kiosk` in the launcher crate | **Added (OB-3).** Not a P2-A dependency — A's scope is kiosk-main only (`p2a:29`, `p2a:349`), same ownership split as C9's `credential_acl.rs`. Discharges TEL-10 drain co-location (`kiosk-launcher/src/main.rs:44-47`) | **new edge:** hard co-landing constraint with P2-A's kiosk-main value; C2 and C13 both depend on it |

**Withdrawn this round:** `waitid`/`WNOWAIT`/local `siginfo` (C5, C6, C7 re-sourced); the
`size_of::<siginfo>() == 128` mitigation sentence and the `job.rs:105-111` analogy; the cage 0.1.4
version constant; the connect-probe bind-collision branch; the claim that C12 "closes" the U3 gap.

**Reinstated this round:** unconditional unlink-before-bind (C2), on Q2, with its safety re-derived
from C11's `RuntimeDirectoryMode=0700` rather than asserted.
