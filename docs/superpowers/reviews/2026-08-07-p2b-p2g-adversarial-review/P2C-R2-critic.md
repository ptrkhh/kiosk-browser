# P2-C — CRITIC, Round 2

Probes re-run in-session; sources at `scratchpad/crit/{pfd.rs,wid.rs,sun.rs,lk.rs}`.
**Seven objections, seven ACCEPTED, zero countered, zero escalated.** The replacements hold; I
verified each rather than taking the disposition.

Two things I did this round that change the record in the Writer's favour and that I state before my
own new objections, because they are evidence he did not have:

1. **I installed cage and ran the assumptions.** `apt-get update` (the lists here were stale, which is
   why my R1 `-s` resolve did not translate into an install first try) then `apt-get install cage` →
   **cage 0.1.5+20240127-2build1**. Under `WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1`:

   | probe | result |
   |---|---|
   | `cage -- sh -c 'exit 86'` | **rc=86** |
   | `cage -- sh -c 'exit 7'` / `'exit 0'` | **rc=7 / rc=0** |
   | `cage -- sh -c 'kill -9 $$'` | **rc=137** — cage exits when its child dies *abnormally* |
   | `INVOCATION_ID=deadbeef cage -- sh -c 'echo $INVOCATION_ID $WAYLAND_DISPLAY'` | **`INV=[deadbeef] WD=[wayland-0]`** |
   | child that never connects as a Wayland client (`sh`) | tolerated; runs and exits cleanly |

   That discharges C10's declared assumption empirically on 0.1.5 (both halves), supplies the link
   C12's chain needed but never named — *cage exits on abnormal child death, not merely on clean
   exit* — and answers the Moderator's OB-2 question directly: **`INVOCATION_ID` survives the cage
   hop intact.** The guard does not misfire.

2. **I reproduced the OB-1 replacement, including a reuse-immunity stress run.** See OB-1 below.

---

## Disposition of Round-1 objections

| OB | Writer's move | My response | Status |
|---|---|---|---|
| OB-1 | Waiter owns the `Child` and is sole reaper/reporter (`child.wait()`); sink holds a `pidfd`; `waitid`/`WNOWAIT`/`siginfo` withdrawn | Reproduced all four orderings **plus** a 200× post-reap stress: `ESRCH` 200/200. The single-consumer diagnosis is the right root cause and the fix is by construction, not by guard. Refusing the "no event on `ECHILD`" patch on *exactly-one* vs *at-most-one* grounds is correct | **ACCEPTED** |
| OB-2 | `#[cfg(unix)] Job::create()` → `Err` when `INVOCATION_ID` unset, firing the existing WARN + `("job", …)` breadcrumb; wedged-cage divergence carried as documented risk to P2-G; "closes the gap" withdrawn | Verified `INVOCATION_ID` is inherited through cage (above), so the guard reads what he claims. `/proc/self/cgroup` here really is legacy-hybrid (`9:name=systemd:/`, `0::/`, eight v1 controllers) — his mechanism is the lazier correct one and I withdraw mine. Checked his `WatchdogSec` rejection and it holds | **ACCEPTED** |
| OB-3 | New **C16**: launcher-owned `#[cfg(unix)] resolve_data_dir() -> /var/lib/kiosk`; no P2-A dependency edge; TEL-10 drain-mismatch identified | Ownership claim **verified**: `p2a:25-31` scopes A to `crates/kiosk-main/src/…`, and `grep -n launcher p2a` returns **exactly one line** (`:349`, deferring "launcher/heartbeat/systemd (P2-C)"). Value **verified against tier 1**: parent §4 `:409` mandates `/var/lib/kiosk/` for Linux. His TEL-10 finding is real and better than my objection | **ACCEPTED** |
| OB-4 | Lock moves to `<data_dir>/launcher.lock` (C16's absolute path), `create_dir_all` first; my `/tmp` suggestion declined | Accepted, and the decline is right — a world-writable lock directory is worse. One fixed inode in both orderings; the branch that caused the defect is off this path. But see **NEW-2** on `create_dir_all` | **ACCEPTED** |
| OB-5 | Resolved by deletion — the struct is gone with `waitid` | Confirmed **gone, not moved**: C5/C6/C7 now source status from `ExitStatus::code()`/`ExitStatusExt::signal()`; the only remaining local FFI is C3's `ucred` (no size assertion claimed, probe is the pin) and scalar-only `syscall` externs. The false mitigation sentence is struck, not reworded | **ACCEPTED** |
| OB-6 | No cage version constant; behaviour is a declared assumption; smoke 13 emits the version it proved; floor assertion → P2-G | Better than what I asked for — I asked him to fix a version pin, he correctly observed that frame C7's floor is *two* distros and the dev/CI box is neither, so no constant is right anywhere. My cage probe now discharges the behavioural assumption itself. But see **NEW-3** on the flag | **ACCEPTED** |
| OB-7 | Unconditional unlink reinstated; loud-failure property re-derived from `RuntimeDirectoryMode=0700` | **Yes — this restores exactly what I wanted**, and the re-derivation is stronger than my argument: I argued the branch was unreachable; he shows *why* (no other principal can create a file in a 0700 root-owned dir, so "connect succeeds ⇒ live peer" cannot fire). Checked the dev fallback too: `create_dir_all` gives `/var/lib/kiosk` root-owned 0755 — still not writable by another principal, so the property survives there as well. One line where R1 had a branch, a syscall path and a retry arm | **ACCEPTED** |

### OB-1 — what I actually ran

`scratchpad/crit/pfd.rs` implements his design (waiter owns `Child` + sole reporter; sink holds a
raw-`syscall` pidfd; `kill_and_wait` = `pidfd_send_signal(SIGKILL)` + bounded poll on `exited`):

```
B1 live child, killed     : kill rc=0  errno=ok    events=1 code=Some(137) extra=false zombie=false
B2 exit 86 then kill      : kill rc=-1 errno=ESRCH events=1 code=Some(86)  extra=false zombie=false
B3 SIGKILL self then kill : kill rc=-1 errno=ESRCH events=1 code=Some(137) extra=false zombie=false
B4 kill races exit        : kill rc=0  errno=ok    events=1 code=Some(137) extra=false zombie=false
post-reap pidfd kill: rc==0 0 times, ESRCH 200 times   (200× zero-delay spawn/reap/kill)
```

His table reproduces exactly, and the 200× run I added makes `ESRCH` a construction property rather
than a lucky ordering. **Consequences the Moderator asked me to chase, all cleared:**

- **`spawn.rs:29-39`'s ceiling semantics survive.** The bound is now a poll on `exited`, which the
  waiter sets *after* `child.wait()` returns — i.e. after the reap — so `exited == true` is a strictly
  **stronger** postcondition than the Windows `WaitForSingleObject(KILL_WAIT_MS)` it mirrors: process
  gone, fds closed, no zombie. C6's surviving rationale (`sink.rs:374-376`, the `ChannelFault`-after-
  kill race against `child_pid.store(0)`, still gated at `pipe.rs:467-481`) is satisfied a fortiori.
  Timeout behaviour is unchanged: give up, proceed, same degradation. And the late-`ChildExited`
  consequence of a timeout (waiter fires during the *next* child's era, `watchdog.rs:186-190` has no
  phase guard) is **pre-existing on Windows and therefore parity-preserving**, not a new defect.
- **Platform floor: fine, and the raw-`syscall` choice is load-bearing.** `pidfd_send_signal` is
  Linux 5.1, `pidfd_open` is 5.3; Debian 12 ships 6.1 and Ubuntu 22.04 ships 5.15 — both clear.
  But glibc's `pidfd_open` *wrapper* arrived in **2.36**, and Ubuntu 22.04 ships **2.35**, so a
  plan-time implementer who "simplifies" to `extern { fn pidfd_open(…) }` gets a link failure on half
  the floor. His `syscall(434/424)` extern is the correct call and is what I probed (this box:
  kernel 6.18, glibc 2.39). **Requested as a Q5 note, not an objection:** say that sentence in the
  spec, or the simplification will happen.
- **C6 stays dependency-free.** Two scalar-only externs against the already-linked libc — the same
  convention as `spawn.rs:12-14` and C3. No frame-C6 issue.
- **`sink.rs:406-407` / `:421` / E2 shutdown: undisturbed.** `:421` takes the alias; `:413`'s
  `child.id()` is provided; `kill_child` (`:377-381`) drops the `ChildHandle` (and the pidfd) after
  the wait while the waiter independently owns the `Child` — no double-free. On the `ExitLauncher`
  path `kill_child` now returns only once the waiter has reaped, so the technician exit leaves neither
  zombie nor orphan. **One item missing from his change list, noted not objected:** `job.rs:221-223`'s
  unix `assign(&self, _child: &Child)` also has to take `&ChildHandle`, since `sink.rs:406` now passes
  one. His "zero lines" claim is scoped to *Windows behaviour*, which is true, so this is an
  incomplete file list, not a wrong claim.
- **The host test is the right gate.** N=200 spawn-and-immediately-kill asserting `events == 1` is a
  test that genuinely fails under R1 and cannot fail under R2 without the ownership rule breaking; my
  200-iteration run took milliseconds, so the CI cost is nil. Correctly reasoned that RT-13 cannot
  cover it (`rt13.rs:145-152`, `job: None`).
- **Noted, not objected:** C7's new totality arm is `code()` else `signal()+128` else **`-1`** — and
  `-1` is the spawn-failure sentinel he closed the `-signo` encoding to avoid (`sink.rs:434-437`,
  `spawn.rs:100-109`). The arm is unreachable for a `wait()`ed status, so this is a latent trap rather
  than a live collision; any other value (`-2`, or a `debug_assert!`) removes it.

---

## New objections

Both arise from the replacements and did not exist in Round 1.

### NEW-1 — `pidfd_open` failure is routed to a path that produces a permanent black screen (vs C5/C6, **HIGH**)

**What breaks.** C5 specifies: *"`pidfd_open` failure takes the **existing** `Err` arm verbatim
(`spawn.rs:139-151` `kill_orphan` + `Err`, caller supplies the one synthetic `ChildExited{-1}`), which
is the precise analogue of the handle-duplication failure that arm was written for."* It is not the
analogue, and the routing is unsafe.

**When.** `pidfd_open` failing is, unlike `try_clone_to_owned` failing, a **permanent environmental**
condition: a `SystemCallFilter=` from P2-B or a container seccomp profile denying syscall 434
(**the Writer's own R1 reason for rejecting pidfd — *"a container-seccomp dependency on syscall 434"* —
which he has not disposed of now that he has adopted it**), or `ENOSYS` on a pre-5.3 kernel. The
Windows arm it copies is documented for the opposite case: `spawn.rs:141-144` says duplication failure
is *"exceedingly rare (e.g. handle-table exhaustion)"* — transient, and retrying is the right answer.

**Why it matters.** Traced end to end against the FSM I already read:

`spawn_main` `Err` → `sink.rs:428-441` `ChildExited{-1}` → `watchdog.rs:186-190` `restart(-1, …, "exit")`
→ `Phase::BackingOff`, backoff doubles → tick → `SpawnMain` → `Err` again → after >5 in `WINDOW_S`,
`safe = true`, `SpawnSafe` → `Err` again → `safe_fails >= SAFE_FAIL_LIMIT` → `Log(SafeModeFailed)`,
backoff pinned at 60 s → **repeat forever**. There is no terminal action in the machine except
`ExitLauncher{86}` (`watchdog.rs:196-198`) — I established the FSM has no give-up state in R1 and the
Writer did not dispute it. So one denied syscall = a device that never renders a page, never exits,
and never stops trying, at 60 s intervals, indefinitely.

That is the outcome `job.rs:18-25` exists to forbid, in the Writer's own citation: *"A device that
refuses to start because a hardening feature failed is a black screen, which is strictly worse than a
device running unhardened … every failure in this module is WARNING-and-continue."* C12's revision
leans on precisely that doctrine to justify its `Err`-on-`INVOCATION_ID` design; C5 violates it in the
same round. `pidfd` is a supervision-hardening facility (it buys reuse-immunity for the kill), not a
prerequisite for running a browser.

**Falsifiable form.** If `pidfd_open` returns `EPERM`/`ENOSYS`, does the kiosk render? Under C5 as
written: no, ever. Under `job.rs`'s doctrine it must, degraded.

**The fix is one arm, and the ingredients are already in the spec.** `pidfd_open` failure ⇒
`ChildHandle { pidfd: None, … }`, `eprintln!` + `breadcrumb_if_absent(data_dir, "pidfd", …)` on the
existing channel, and `kill_and_wait` degrades to `kill(pid, SIGKILL)` gated on the waiter's `exited`
flag — which is the pre-P2-C exposure, i.e. the status quo, not a regression, and is the same
bounded-and-documented shape as `sink.rs:393-405`'s ACCEPTED-RACE. Keep the `Err` arm for the
**waiter-thread-creation** failure, where it is genuinely the right analogue and genuinely rare.

**Evidence.** Tier 3: `spawn.rs:139-151`, `:141-144`, `:100-109`; `sink.rs:428-441`, `:393-405`;
`job.rs:18-25`; `watchdog.rs:121-165`, `:186-198`. Tier 2: the Writer's own R1 C5 alternatives list.
Probe: `crit/pfd.rs`.

### NEW-2 — OB-4's `create_dir_all` fix cannot run for the caller it was added for (vs C13/C16/C2, MED)

**What breaks.** OB-4's replacement is `<data_dir>/launcher.lock` with *"`fs::create_dir_all(data_dir)`
precedes the lock so the 'directory absent ⇒ silent loss of backstop' path he identified does not
occur on a first dev run."* After C16, `data_dir` is `/var/lib/kiosk`. `/var/lib` is root-owned 0755,
so `create_dir_all("/var/lib/kiosk")` returns `EACCES` for a non-root dev run — the exact caller the
sentence names. The lock then takes C13's non-`WouldBlock` arm: WARN + continue, no backstop. My OB-4
defect is fixed for root and unfixed for the case the fix cites.

**The same edge hits C2, and there it contradicts C2's own stated requirement.** C2 justifies the
`runtime_dir()` fallback as *"else the data dir (a manual dev run without systemd must not fail to
bind)"*. After C16 that branch is `/var/lib/kiosk`, which a non-root dev cannot write, so the bind
fails and `serve` enters `pipe.rs:370-388`'s breadcrumb-and-retry loop — no heartbeat, and the FSM
restarts kiosk-main at every startup-grace expiry. The fallback no longer discharges the requirement
it was introduced for. Under frame §4.3 that is an internal inconsistency, not a value question.

**What I am *not* arguing.** C16's value is correct and non-negotiable — parent §4 `:409` mandates
`/var/lib/kiosk/` for Linux (tier 1), and the ownership analysis (A is kiosk-main only) is verified.
The defect is that two Round-2 replacements inherited a rationale that C16 invalidated.

**Two one-line exits, either fine.** (i) Withdraw the dev-run rationale and say plainly that manual
runs are root, collapsing `runtime_dir()`'s second branch to a documented root-only path; or (ii)
point the *dev* socket and lock at `$XDG_RUNTIME_DIR` when `/run/kiosk` is absent — one env lookup,
already per-user, already 0700, and it keeps both mechanisms honest without touching C16.

**Evidence.** Tier 1: parent §4 `:409`. Tier 2: `p2a:25-31`, `p2a:349`. Tier 3: `job.rs:18-25`,
`:239-248`; `pipe.rs:370-388`; `main.rs:44-53`. Filesystem: `/var/lib` is `root:root drwxr-xr-x`.

### NEW-3 — smoke 13's version-recording step names a flag cage does not have (vs C15/OB-6, LOW)

OB-6's replacement specifies that smoke 13 *"emits `cage --version` as part of its output"*. On the
cage the gate can actually run against — the one I installed, 0.1.5+20240127-2build1 — there is no
long option:

```
$ cage --version   → "cage: invalid option -- '-'" + usage,  rc=1
$ cage -v          → "Cage version 0.1.5",                   rc=0
$ cage -h          →  " -v   Show the version number and exit"
```

`rc=1` means a smoke script with `set -e` fails at the version line before it ever reaches the
assertion that matters. `-v`, not `--version`. Mechanically checkable, so I checked it.

---

## Standing summary

Every Round-1 objection is accepted; nothing from Round 1 remains open. Of the three new items, only
**NEW-1** is load-bearing — it is a one-arm fix to a replacement, not a redesign, and everything else
about C5's new ownership algebra is right and now probed twice. **NEW-2** is a rationale that two
replacements inherited past its expiry date. **NEW-3** is a one-character correction.

I also record, unprompted, that C10's cage assumptions and C12's `INVOCATION_ID` guard are no longer
assumptions: I ran them against a real cage this round and both hold, including the abnormal-death
case C12's chain depends on and never named.
