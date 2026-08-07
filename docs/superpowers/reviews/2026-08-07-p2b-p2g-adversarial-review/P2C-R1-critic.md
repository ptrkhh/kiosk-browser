# P2-C — CRITIC, Round 1

No frame dispute.

All probes below were re-run in-session on this box (`rustc 1.94.1`, `systemd 255 (255.4-1ubuntu8.14)`,
systemd not PID 1). Probe sources at `scratchpad/crit/{wid.rs,sun.rs,lk.rs,u/kiosk.service}`.
I reproduced the Writer's three probes before attacking them; **all three reproduce exactly**
(`waitid`/`WNOWAIT` table, `SUN_LEN` 107-OK/108-Err, `File::try_lock` → `WouldBlock`), and I say so
rather than re-litigating them.

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | C5 | `waitid(WNOWAIT)` is **not** ownership-identical to Windows: if the sink reaps first the waiter gets `ECHILD` (`si_code=0`), breaking `spawn.rs:100-109`'s exactly-one-exit-event contract, and C7's total mapping turns the unhandled branch into a fabricated `ChildExited{128}` | **HIGH** | probe (in-session) + tier 3 |
| OB-2 | C12 | Orphan-kill parity is asserted but is unverifiable in every environment P2-C can run in, and the *code* stays silent: `job.rs:217-225` still returns `Ok`, so `main.rs:189-199`'s degraded-supervision warning can never fire on Linux — a dev/CI launcher reports armed supervision it does not have. Looser-direction divergence (cooperative vs in-kernel) unstated | MED | tier 3 |
| OB-3 | C2/C13 | "else the data dir" is unchecked: `main.rs:48-53` resolves the data dir from `ProgramData`, else `PathBuf::from(".")` — on Linux today it is **`./kiosk`, relative to CWD**. C13's backstop lock is then per-CWD, and the undeclared dependency is on P2-A's `/var/lib/kiosk/` (`p2a:113`), which is designed-not-merged | MED | tier 3 |
| OB-4 | C13 | The backstop fails in one of the two orderings it exists for: hand-run first (no `/run/kiosk`) → data-dir lock; unit then starts, `RuntimeDirectory` creates `/run/kiosk`, its launcher locks a *different* file. Two launchers, both "the one" | MED | tier 3 + tier 5 |
| OB-5 | C5 | The `size_of::<siginfo>() == 128` assertion is inert (`__SI_MAX_SIZE` is 128 on every Linux ABI) and constrains no field offset; the analogy to `job.rs:105-111` is misapplied — that assertion is justified there by `SetInformationJobObject`'s own length check | LOW | tier 3 + probe |
| OB-6 | C10/C15 | The cage gate is feasible (good), but not against the recorded build: `apt-cache policy cage` here → **0.1.5**+20240127, while C10 pins the record to cage **0.1.4** (Debian 12 = frame C7's floor). Gate and version-pin live in different environments | LOW | tier 3 (apt) + tier 5 |
| OB-7 | C2 | Concession went too far: the corpse-probe replaces a free property with a branch nothing can reach. `FILE_FLAG_FIRST_PIPE_INSTANCE` costs one bit; the Linux equivalent costs a connect-probe + retry arm guarding a state that needs PID reuse *and* a surviving predecessor — which C13 now excludes. Q2 | LOW | tier 3 |

---

## OB-1 — `waitid(WNOWAIT)` is not ownership-identical to Windows (vs C5, **HIGH**)

**What breaks.** C5's load-bearing claim is: *"This is ownership-identical to Windows: the waiter
observes a handle it alone owns, the caller's `Child` is untouched."* On Windows that is literally
true — `spawn.rs:89-95` hands the waiter an **owned duplicate handle**, so `WaitForSingleObject` +
`GetExitCodeProcess` succeed regardless of what the sink does or when. `waitid(P_PID, …, WNOWAIT)`
gives no such independence: it is a query against a PID that a *different thread* is racing to reap.

**Verified counterexample** (`scratchpad/crit/wid.rs`, re-run in-session). The four rows of the
Writer's table reproduce exactly. I added the fifth case he did not run — the sink reaping first:

```
RACE(reaper-first): sink wait=ExitStatus(unix_wait_status(1792));
                    waiter waitid=-1 errno=ECHILD ("No child processes") si_code=0 si_status=0
```

**When.** Whenever the **sink** initiates the kill while the child is still alive — i.e. the
`hang`, `no_ready` and `channel` restart causes (`watchdog.rs:212-224`, `:126-127` →
`sink.rs:471 kill_child`), `ExitLauncher` (`sink.rs:483-485`), and `spawn()`'s leading `kill_child`
(`sink.rs:377-381`). In all of those, C6's `try_wait()` poll and the waiter's `waitid` are both live
on the same PID. The waiter is usually already queued in `do_wait` and usually wins; "usually" is not
the property `spawn.rs:100-109` states.

**Why it matters.** Two failure modes, both silent:

1. **Zero exit events.** `spawn.rs:100-109` is a named contract — *"one spawn attempt, one exit
   event"* — and arch-12's backoff is built on it. Losing the event is not cosmetic: on Windows the
   post-kill `ChildExited` re-enters `Watchdog::on` during `Phase::BackingOff` and runs
   `restart(code, at, "exit")` a **second** time (`watchdog.rs:186-190`), pushing a second timestamp
   into the rule-7 sliding window (`watchdog.rs:150-158`) and doubling backoff again. So a hang
   restart counts as *two* restarts on Windows and *one or two, nondeterministically*, on Linux. The
   safe-mode escalation threshold (`>5` in `WINDOW_S`) is therefore reached at a race-dependent rate.
   That is an FSM-visible divergence C3 requires be stated and C5 asserts cannot occur.
2. **A fabricated event.** C7 specifies a **total** function: *"`CLD_EXITED (1)` → `si_status`;
   anything else → `128 + si_status`."* On the `ECHILD` return the buffer is untouched
   (`si_code=0, si_status=0`, probed), so a waiter that maps without first checking `waitid`'s return
   value emits `ChildExited{128}` — a spurious restart with cause `exit` and a bogus code, on the
   exact paths (`sink.rs:365-376`) whose diagnostics the code says matter most. C5 names no error
   branch for the waiter at all.

**Evidence.** In-session probe (above); tier 3: `spawn.rs:89-95`, `:100-109`; `sink.rs:377-381`,
`:406-407`, `:421`, `:461-481`, `:483-489`; `watchdog.rs:126-158`, `:186-190`, `:212-224`.

**What is *not* broken, and I concede it.** The sole-reaper rule itself holds. I traced every FSM
path: `ChildExited{86}` → `ExitLauncher` → `kill_child` (`sink.rs:483-485`); every other
`ChildExited` and every `Tick`-driven restart → `Action::DrainOrphanedSpool` **first**
(`watchdog.rs:126-127`) → `kill_child` (`sink.rs:471`); `BackingOff` → `spawn()` → `kill_child`
(`sink.rs:385`). Every path reaps, in the same FSM turn, before any backoff sleep. On the
technician-exit path the zombie is reaped before `process::exit` (`main.rs:257`), and it would not
matter if it were not — `loop_::run` returns and PID 1 reaps. `Child::kill()` on an unreaped zombie
is `Ok` and cannot land on a reused PID (probed, all four rows). **U2 is genuinely discharged.**
Drain ordering is unchanged: `WNOWAIT` fires at child *exit*, exactly when
`WaitForSingleObject`'s wait satisfies, and `drain_orphan` (`sink.rs:271-285`) still runs strictly
after `kill_child`. **TEL-10 ordering: clean pass.**

**The fix is one line** (`if waitid(...) != 0 { return; }`, or a `si_code`-must-be-`{1,2,3}` guard),
which is why this is a spec defect rather than a design dead end — but C5 as written asserts the
property instead of establishing it, and a verified counterexample defeats it (frame §1).

---

## OB-2 — C12 closes the U3 gap in prose, not in anything runnable, and leaves the code lying (vs C12, MED)

**What breaks.** Three separate things, in descending order of how much I care.

**(a) The launcher will report armed supervision it does not have.** C12's deliverable list is
(1) an explicit `KillMode=`, (2) doc-comment rewrites, (3) a P2-G gate. It changes **no code**. So
`job.rs:217-225` stays `create() -> Ok(Job)` / `assign(&self,_) -> Ok(())`, and `main.rs:189-199`'s
`Err` arm — the one that prints *"job object unavailable …; a supervised kiosk-main will survive an
unexpected launcher death"* and writes the `("job", …)` breadcrumb replayed at `main.rs:222-226` —
**can never fire on Linux**. A launcher run outside a unit (a dev box, an `ssh` session, a
technician shell) has no cgroup teardown and no orphan-kill, and says nothing. That is precisely the
Q3 class the parent names, and C12 states the dev-run divergence in the *spec* while leaving the
*device* with no way to tell the two configurations apart. The honest deliverable is a fourth one:
`Job::create()` on unix reports whether the process is actually in a unit cgroup (one
`/proc/self/cgroup` read) and takes the existing WARNING-and-continue path otherwise. That is fewer
moving parts than the doc rewrite it replaces.

**(b) The looser-direction divergence is understated.** `job.rs:12-16` is explicit about *why* the
Job Object is the mechanism: the kernel kills the job *"when the last handle to it closes — which
happens on process teardown for **every** death mode, including a hard kill, **precisely because it
needs no cooperation from the dying process**."* C12's chain needs cooperation at two links: cage
must notice its child died and exit, and systemd must then run a stop job. `spawn.rs:32-36` names
the realistic kiosk failure — *"a process wedged in an uninterruptible kernel wait (a hung GPU/
display driver …)"* — and cage is the process holding the DRM device. If cage wedges, the unit stays
`active`, no stop job runs, the orphan survives **and** the dead launcher is never restarted. On
Windows the job object still fires. C12 declares only the non-systemd dev run as looser. Under C3
that is an incomplete divergence statement, not a wrong one.

**(c) No gate in P2-C can observe it.** C12's own gate is deferred to P2-G. The spec's declared merge
gates cannot substitute: RT-13 passes `None` for the job deliberately (`rt13.rs:145-152` — *"No job
object: `Harness` already reaps the mock explicitly"*), so C14's "primary regression net" has never
covered orphan-kill on Windows either and will not on Linux; and smoke 13-15 (C15) assert restart,
exit-86 and no-zombie, none of which is orphan-kill. So P2-C ships with orphan-kill parity
**reassigned**, not closed. The deferral has a named owner (P2-G), so this is not a frame-C9
feasibility defect — but the Writer's summary line *"closes the U3 gap"* overstates what this spec
delivers, and (a) is a real, cheap, in-scope fix.

**What I do not dispute.** `KillMode=control-group` **is** the systemd default (tier 5), which the
Writer says himself, so making it explicit is documentation and costs nothing — no objection there.
The rejection of `PR_SET_PDEATHSIG` is correct and well-argued: `rt13.rs:163-166` really does run
the loop on a spawned thread, and `job.rs:131-134` really does claim the child's whole process tree.
Rejecting a launcher-owned cgroup on Q2 is right.

**Evidence.** Tier 3: `job.rs:4-16`, `:12-16`, `:131-134`, `:144-147`, `:217-225`;
`main.rs:189-199`, `:222-226`; `sink.rs:393-419`; `rt13.rs:145-152`, `:163-166`;
`spawn.rs:29-39`. Tier 5: `systemd.kill(5)` default (man pages absent on this minimized image —
declared as tier 5, not probed).

---

## OB-3 — "else the data dir" was never checked; on Linux it is `./kiosk` (vs C2 and C13, MED)

**What breaks.** C2(2) specifies `runtime_dir()` as *"`/run/kiosk` when it is a directory, else the
data dir (dev run without systemd)"*, and C13 puts `launcher.lock` in the same directory. Neither
change states what the data dir *is* on Linux. Checked:

```rust
// crates/kiosk-launcher/src/main.rs:48-53
fn resolve_data_dir() -> PathBuf {
    std::env::var_os("ProgramData").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".")).join("kiosk")
}
```

`ProgramData` is unset on Linux, so **today the fallback is `./kiosk` — relative to the launcher's
current working directory.** kiosk-main has the identical function (`kiosk-main/src/main.rs:436`).

**When / why it matters.**
- **C13 is defeated in its own motivating case.** The backstop exists because *"'systemd guarantees
  it' is exactly the kind of claim that is true until someone runs the binary by hand"*. Two hand
  runs from two working directories take two different `./kiosk/launcher.lock` files and both
  acquire. `File::try_lock` is per-inode; a per-CWD path is not a single-instance token.
- If `./kiosk` does not exist, `File::create` fails → C13's own rule maps any non-`WouldBlock` error
  to `Err` → WARN + continue (`job.rs:18-25`). Silent loss of the backstop, as designed, but reached
  by a path C13 does not anticipate.
- Under the unit the CWD is `/` (no `WorkingDirectory=` in C11's shape), so the data dir is
  `/kiosk` — which is also where the spool and every breadcrumb go. Not C's bug, but C is the first
  spec whose mechanisms depend on it.

**The dependency is real and undeclared.** P2-A already owns the fix — `p2a:113`:
*"`resolve_data_dir()` → `/var/lib/kiosk/` on Linux (parent spec §4 …)"*, and `p2a:29` lists it in
A's Unix surface. C2 and C13 therefore have a hard prerequisite on a P2-A change that, exactly like
the `credential_acl.rs` caveat C9 already adopted from verifier row 19, is **designed, not merged**.
C9 states that caveat for one A change and omits it for another that C2/C13 depend on more directly.
With `/var/lib/kiosk/` landed the mechanism is fine (`/var/lib/kiosk/hb-<pid>.sock` = 27 bytes, far
inside the probed 107-byte bound), so the fix is a dependency row, not a redesign.

**Evidence.** Tier 3: `crates/kiosk-launcher/src/main.rs:48-53`, `kiosk-main/src/main.rs:436-438`;
`job.rs:18-25`, `:239-248`. Tier 2: `p2a:29`, `p2a:113`.

---

## OB-4 — C13's backstop is order-dependent (vs C13, MED)

**What breaks.** C13's two mechanisms select *different lock files depending on which process started
first*, because `runtime_dir()` (C2) branches on whether `/run/kiosk` exists, and `RuntimeDirectory=`
creates and destroys it with the unit.

**When.** Hand-run **before** the unit: `/run/kiosk` does not exist (default
`RuntimeDirectoryPreserve=no` removed it on last stop — the verifier's row 40, adopted by C2), so the
hand-run launcher locks `<data_dir>/launcher.lock`. `systemctl start` then creates `/run/kiosk`, and
the unit's launcher locks `/run/kiosk/launcher.lock` — a different inode. Both acquire. Two
launchers, two kiosk-mains, two webviews on one display — the exact outcome `job.rs:6-9` describes.
(The reverse ordering — unit first, then hand run — works, because `/run/kiosk` exists for both.)

**Why it matters.** C13 concedes that the per-PID socket name *"means they will not collide, which
hides the condition rather than preventing it"*. A per-directory lock has the same property: it
silently permits the state it was added to detect. Severity is MED, not HIGH, because the primary
mechanism (unit identity) is sound and this only degrades the backstop.

**Cheapest fix consistent with C13's own doctrine:** lock a path that does not move —
`/run/kiosk` is not required for it; `/tmp/.kiosk-launcher.lock` or an abstract-namespace socket
would do, and either is fewer branches than `runtime_dir()`. I am not prescribing which.

**Evidence.** Tier 3: `job.rs:6-9`, `:236-248`, `:283-288`; in-session probe (`crit/lk.rs`):
`File::try_lock` is stable on the active toolchain and a **second handle in the same process** gets
`Err(WouldBlock)` (flock is per-open-file-description), so the mechanism itself is sound and RT-13 is
unaffected — `rt13.rs` never calls `acquire_single_instance` (grepped; it builds `LauncherSink`
directly at `:138-152`). Tier 5: `RuntimeDirectoryPreserve=no`.

**Toolchain sub-claim, checked and cleared.** The prompt asked me to check the pinned toolchain:
`rust-toolchain.toml` is `channel = "stable"` (floating, **not** 1.94.1), no `rust-version`/MSRV
anywhere in the workspace, and CI uses `dtolnay/rust-toolchain@stable` in all three jobs
(`ci.yml:18,32,50`). `File::try_lock` is stable on every stable ≥ its stabilisation and the channel
only moves forward, so the claim survives the correction. **No objection.**

---

## OB-5 — the `siginfo` size assertion cannot fail, so it mitigates nothing (vs C5, LOW)

**What breaks.** C5 says the local `siginfo` prefix's layout risk *"is mitigated the way
`job.rs:110-111` already mitigates `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`: a compile-time `size_of`
assertion"*. The analogy does not transfer.

`job.rs:105-111` states its own justification: *"`SetInformationJobObject` validates
`cbJobObjectInformationLength` against the kernel's own idea of the struct size and fails
ERROR_BAD_LENGTH on a mismatch … Pin the documented 64-bit size at compile time instead."* The size
there is a real, arch-varying number the kernel independently checks, and it is guarded by
`#[cfg(target_pointer_width = "64")]`.

`siginfo_t` is 128 bytes **by definition** on every Linux ABI (`__SI_MAX_SIZE`). A struct whose tail
is declared as filler to 128 will assert `size_of == 128` on x86_64, i686 and arm alike, while the
fields C5 actually reads move: x86_64 has an `int __pad0` after `si_code` that 32-bit ABIs do not, so
`si_status` sits at offset 24 on x86_64 and 20 elsewhere. The assertion constrains the filler, never
the offsets, and `waitid` performs no length check to catch it.

**Why it matters (little).** Frame C7 pins x86_64, and my probe (`crit/wid.rs`, the Writer's exact
struct shape) reads `si_code`/`si_status` correctly there, so nothing ships broken. The defect is
that the spec presents an inert check as the pin, which is the kind of thing a plan-time implementer
copies forward. Say "x86_64 only, pinned by the probe, `#[cfg(target_arch = "x86_64")]` on the
struct" — or take `libc` (already `Cargo.lock:1946`, v0.2.186, zero new supply chain) and delete both
hand-rolled ABI structs. I am **not** arguing the dependency; I am arguing the mitigation sentence is
false as written.

**Evidence.** Tier 3: `job.rs:105-111`; `Cargo.lock:1946-1948`. Probe: `crit/wid.rs`
(`sizeof siginfo prefix = 128`, all five cases read correctly on x86_64).

---

## OB-6 — the cage gate runs, but not against the pinned version (vs C10/C15, LOW)

**What I checked, and the good news first.** Frame C9 asks whether a declared gate can actually run.
It can: `apt-get install -s cage` resolves cleanly here (pulling `libwlroots12t64`, `libseat1`,
`xwayland`, …), so smoke 13's `cage -- sh -c 'exit 86'; test $? -eq 86` is executable in this
environment. **C15 is feasible; no C9 defect.** The verifier's "cage is not installed" is a state,
not a blocker, and the Writer's conversion of the prose claim into an executable assertion is the
right move.

**What breaks.** `apt-cache policy cage` here → candidate **`0.1.5+20240127-2build1`**
(noble/universe). C10 records the measurement against **cage 0.1.4** *"(the Debian 12 package)"* and
says so precisely because *"propagation behaviour is version-dependent"*. So the version the gate can
run against is not the version the spec pins, and the version the spec pins is the one the platform
floor (frame C7: Debian 12) will actually run. The gate proves the property for 0.1.5; the device
runs 0.1.4.

**Why it matters (a little).** The remedy is one line, and the Writer has already built the slot for
it: smoke 13 is specified to *"record the `cage --version` the run measured"*. Make the recorded
version the assertion's output rather than a spec constant, and route the floor-version check to
P2-G's image validation, where a Debian 12 image with cage 0.1.4 actually exists — which is where
C10 already sends the systemd half.

**Evidence.** Tier 3: `apt-cache policy cage` / `apt-get install -s cage`, run in-session.
Tier 5: Debian 12 ships cage 0.1.4.

---

## OB-7 — the bind-collision concession went too far (vs C2, LOW)

**Stated plainly, as the prompt asks: this is a design the Writer gave up that was defensible.**

C2(4) withdraws unconditional unlink-before-bind and buys a connect-probe (`AddrInUse` →
`UnixStream::connect` → `ECONNREFUSED` ⇒ unlink and rebind once; success ⇒ loud-fail) to preserve
`FILE_FLAG_FIRST_PIPE_INSTANCE`'s loud-failure property (`pipe.rs:100-104`).

The property is worth preserving. The price is not comparable. On Windows it costs one flag bit in an
argument that is already being passed. On Linux it costs an extra branch, an extra syscall path, a
retry arm, and a second failure mode to reason about — guarding a state that requires **all** of:
the name is `/run/kiosk/hb-<our-own-pid>.sock`, so a squatter must hold our exact PID; a predecessor
must have survived into our PID's reuse; and — after C13 — must have done so while not holding the
single-instance lock. C2 itself concedes the `RuntimeDirectory` wipe already covers the accumulation
case and that per-PID naming is what makes it belt-and-braces. Q2 says the simpler design that meets
the requirement wins; unconditional unlink of a name that is ours by construction met it.

I am not asking for the withdrawal to be reversed as a condition of adoption — the probe is
*correct*, just not *earned*. Recording it as "kept for symmetry with `pipe.rs:100-104`, not because
the state is reachable" would be the honest version, and would stop a future reader treating a live
peer on our own PID as a real scenario.

**Evidence.** Tier 3: `pipe.rs:39-44`, `:100-104`, `:370-388`.

---

## Clean passes

Issued deliberately, each after checking the thing that would have made it an objection.

- **C1 — UDS listener transport. Clean.** `ChannelReconnected`'s only producer is `pipe.rs:455`
  behind `awaiting_reconnect_event` (set `:479`, cleared `:445`); `watchdog.rs:257-265` and
  `:212-224` confirm the fault→grace→reconnect states are exactly what a listener-less transport
  would delete. The launcher has no async runtime (`kiosk-launcher/Cargo.toml`: `kiosk-core` +
  `serde_json`, native deps `cfg(windows)`-only), so blocking std is the only option and `serve` is
  already blocking-on-caller-thread (`main.rs:239`). Adopting the verifier's second nuance
  strengthens the rejection record rather than weakening it.
- **C3 — `SO_PEERCRED`. Clean, including the two traps in the prompt.** The mapping is faithful:
  `accept_client` (`pipe.rs:84-89`) is `Some(p) if p != 0 => p == expected || p == current`, `_ =>
  false`, and its own doc already names the Windows failure — *"`None` (Windows won't name the
  client)"* — as a fail-closed case, so a failed `getsockopt` → `None` → reject produces the identical
  FSM event on both platforms. **No accept→check TOCTOU:** `SO_PEERCRED` returns credentials the
  kernel recorded at `connect(2)` time, not a live lookup, so there is no window to race; and PID
  reuse cannot forge, because the recorded PID would have to equal `child_pid`, which only the real
  child holds. Reusing the two-valued seam rather than reimplementing it satisfies frame C1.
- **C4 — `pipe.rs` Unix `serve`. Clean.** Mapping table verified at `pipe.rs:65-71`, `:84-89`,
  `:441-459`, `:467-486`. The bind-failure citation is now direct and correct (`pipe.rs:370-388` +
  the `ponytail:` at `:373-380`), and closing open decision 3 with "no poll/timeout" is right:
  `pipe.rs:322-330` and `main.rs:249-256` both already document `process::exit` as the teardown, and
  `rt13.rs:673-679` does not join the server thread. Mirroring the caveat beats inventing a timeout.
- **C6 — `kill_and_wait` Unix body. Clean, and the re-derivation is the good part.** Verified that
  the bound is `#[cfg(windows)]`-only (`spawn.rs:29-39`) and the existing Unix body
  (`spawn.rs:63-67`) is unbounded. Dropping the `ERROR_SHARING_VIOLATION` rationale is correct —
  POSIX `rename(2)` ignores open descriptors — and keeping the bound on the *second* rationale alone
  is also correct: `sink.rs:374-376`'s `ChannelFault`-after-kill race is transport-independent and
  `pipe.rs:467-481` is still gated on `child_pid == expected`. Stating which of two inherited
  rationales survives the port is exactly what C3 asks for.
- **C7 — `128 + signo`. Clean.** Probed: `si_code ∈ {1,2}`, `si_status` = exit code or signal number.
  128+signo ∈ [129,192] for signals 1–64 — never 86, never the `-1` sentinel. The reason for
  pinning now is sound: `-signo` at SIGHUP really does collide with `sink.rs:434-437`'s synthetic
  `ChildExited{-1}`, which `spawn.rs:100-109` makes a contract. Collision check verified: kiosk-main's
  only explicit exits are `0` (`cli.rs:31`) and `86` (`pinpad.rs:156`), plus 101 on panic.
  **Noted, not an objection:** the encoding is not injective — I probed `exit 137`, which returns
  `si_code=1, si_status=137` and renders identically to SIGKILL. The ambiguity is unreachable for this
  child, so the invariant as stated is fine; one sentence saying "unreachable because kiosk-main emits
  only 0/86/101" would make it airtight. The never-86 invariant holds in both directions.
- **C8 — `heartbeat.rs` Linux client. Clean.** `tokio` features `net` + `io-util` are already present
  (`kiosk-main/Cargo.toml`), `pipe_name_from_env()` (`heartbeat.rs:37-39`) is platform-free, and
  `RECONNECT_BACKOFF` = 1 s against `MISS_LIMIT_S` = 15 leaves the documented margin. The declared
  divergence (`ENOENT`/`ECONNREFUSED` replacing `ERROR_PIPE_BUSY`/`ERROR_FILE_NOT_FOUND`, comment
  changes only because the arm already retries any open error) is honest and in the right direction.
- **C9 — launcher `credential_acl.rs` + A's C12 hand-forward. Clean.** The stub is verbatim fail-open
  (`credential_acl.rs:100-104`), `is_violation` (`:24-26`) makes `Err` a violation for free, and the
  `replace-don't-add-beside` rule matches `p2a:263-265`. The U8 re-derivation is correct on its own
  terms: C11's shape has no `User=`, so root-owned `0o600` still makes mode bits sufficient, and the
  condition A attached is transferred to P2-G rather than dropped. That is what frame §0 asks for.
- **C10 — process shape. Clean** (subject to OB-6's version note). The narrowing — exit codes and
  `ChildExited` see what Windows sees, kill semantics do not — is the correct correction, and both
  rejected shapes are rejected for real reasons.
- **C11 — unit shape. Clean, on all three traps in the prompt.** I re-ran `systemd-analyze verify`
  on the full seven-directive block under systemd 255: **exit 0**. (a) `SuccessExitStatus=86` and
  `RestartPreventExitStatus=86` are independent lists and do not conflict; `RestartPreventExitStatus`
  is documented to apply *"regardless of the restart setting configured with `Restart=`"*, so the pair
  yields exactly the parent's intent — no restart, `inactive` not `failed`. Restoring it is right, and
  the `failed`-state consequence is stated correctly. (b) `RuntimeDirectoryMode=0700` cannot be
  undermined by a service-user fork: cage is the unit's **main process** and the launcher is its
  child, so in this shape they are necessarily the same user, and `RuntimeDirectory` ownership follows
  `User=`/`Group=` if P2-G ever sets them. (c) `Restart=always` does **not** defeat the FSM's
  authority — the parent mandates it verbatim (`2026-07-05-kiosk-browser-design.md:171-175`, which
  also names all three directives, confirming row 39), and in any case the FSM has **no give-up state
  to defeat**: `watchdog.rs:143-149` escalates `SafeModeFailed` by holding backoff at the 60 s ceiling
  and continuing to restart, and the only terminal action in the whole machine is
  `ExitLauncher{86}` (`:196-198`). There is nothing for systemd to launder.
- **C14 — RT-13 cross-platform. Clean.** Citations now correct (`rt13.rs:27` `#![cfg(windows)]`,
  `:104-111` `unique_pipe` = PID + `AtomicU32`, `mock_main.rs:26-30` + `:33`). The transport-name
  seam fits: I measured a worst-case-shaped path at 85 bytes and the real tags are
  `healthy`/`hang`/`crash`/`exit86` (`rt13.rs:292,325,360,385`), so the actual paths are ~55 bytes
  against the probed 107-byte ceiling, with C2's guard failing loudly above it. `TMPDIR` is unset and
  `temp_dir()` is `/tmp` here and on ubuntu-22.04 runners. The budget is derived from constants I
  verified (`MISS_LIMIT_S = 15` at `rt13.rs:46-49`, `HEALTHY_OBSERVE = 20 s` at `:61`) rather than
  measured, which is legitimate — the constants bound it, and four scenarios under default
  parallelism give the stated ~25-35 s. Parallelism containment rests on per-scenario tempdirs
  (`rt13.rs:117-118`) plus PID+counter, both already present, so no `--test-threads` restriction is
  needed. The `-D warnings` consequence of un-gating is correctly claimed as part of the change.
  Withdrawing the "same `UnixStream` branch" claim was right — the launcher crate has no tokio.
  Also verified as a *non*-issue: the socket lands in the scenario's `data_dir`, and `drain_orphan`
  (`sink.rs:271-285`) touches only `spool/main` and `spool.orphaned`, so a socket file there is inert.
- **C15 — smoke 13-15. Clean** apart from OB-6. Non-collision re-verified (`p2a:326` owns 1-7,
  `p2b:174` owns 8-12). The weston→cage harness swap is a real divergence and stating it beats
  claiming continuity. Scenario 15's "no zombie" now has the owner C5 supplies, and that owner is
  correct (see OB-1's concession).

**Response to the verification record.** I checked the dispositions rather than taking them: all
three FALSEs, all seven DRIFTs and both UNVERIFIABLEs are conceded accurately, and the corrected
citations are the right ones. Zero rebuttals with 43 rows checked is a defensible posture here, not a
capitulation — I found no row where the Writer conceded something the record did not require, except
OB-7, which I have logged as such rather than as a defect.
