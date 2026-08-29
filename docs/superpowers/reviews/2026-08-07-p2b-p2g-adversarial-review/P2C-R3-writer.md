# P2-C — WRITER, Round 3

Six items. **REVISE ×4, CONCEDE ×2, REBUT ×0.** All three NEW claims verified in-session before
disposal; probe sources at `scratchpad/probe/{new1.rs,ob1.rs}`.

I decline one of the Critic's two suggested remedies (NEW-2 option ii) with evidence, while accepting
the defect in full.

---

## NEW-1 — **REVISE**

**He is right, and it is my own R1 argument.** Verified: `spawn.rs:141-144` documents that arm for
*"exceedingly rare (e.g. handle-table exhaustion)"* — transient. `pidfd_open` denial is permanent
(`SystemCallFilter=` from P2-B, container seccomp, or `ENOSYS` pre-5.3). I listed
*"a container-seccomp dependency on syscall 434"* as a reason to reject pidfd in R1, then adopted
pidfd in R2 without disposing of it. His FSM trace is correct against code I already read:
`sink.rs:428-441` → `watchdog.rs:186-190` `restart(-1,…,"exit")` → backoff → rule-7 → `safe` →
`SAFE_FAIL_LIMIT` → `Log(SafeModeFailed)` + 60 s ceiling → forever, with `ExitLauncher{86}`
(`watchdog.rs:196-198`) the only terminal action. One denied syscall = permanent black screen, which
is exactly what `job.rs:18-25` forbids and what C12 leans on in the same round.

**Adopted, as he specifies it.** `ChildHandle.pidfd` becomes `Option<OwnedFd>`:

- `pidfd_open` failure ⇒ `pidfd: None`, `eprintln!` + `breadcrumb_if_absent(data_dir, "pidfd", …)` —
  the existing degraded channel, replayed at `main.rs:222-226` alongside `("job", …)` and
  `("mutex", …)`. Supervision continues.
- The `Err` arm is **kept for waiter-thread-creation failure only** (`spawn.rs:179-191`), where the
  Windows analogy is exact and the failure genuinely is rare.
- `kill_and_wait` with `pidfd: None` degrades to `kill(pid, SIGKILL)` **gated on the waiter's
  `exited` flag** — no kill is issued at all once the child is known reaped.

**Cost of the degraded mode, stated.** The kill loses reuse-immunity: if the child exits and the
waiter reaps between the `exited` load and the `kill` syscall, the signal could land on a recycled
PID. That is the pre-P2-C exposure verbatim — today's `spawn.rs:63-67` is `child.kill()` on a Child
the sink never waits, i.e. an ungated kill-by-pid — so this is status quo, not a regression, and it is
the same bounded-and-documented shape as `sink.rs:393-405`'s ACCEPTED-RACE. Recorded as a
`ponytail:` naming the ceiling and the upgrade (pidfd, when the sandbox permits it).

**Exactly-one-exit-event is unaffected, and I probed it rather than asserting it.** The property comes
from the waiter owning the `Child` and being the sole status consumer; the pidfd was only ever the
*kill* handle. `probe/new1.rs`, degraded path (`pidfd: None`) — four orderings plus a 200× run:

```
live child, killed       kill=kill(pid,SIGKILL)         events=1 code=Some(137) zombie=false
exit 86 then kill        kill=skipped (already reaped)  events=1 code=Some(86)  zombie=false
SIGKILL self then kill   kill=skipped (already reaped)  events=1 code=Some(137) zombie=false
kill races exit          kill=kill(pid,SIGKILL)         events=1 code=Some(137) zombie=false
200x post-exit: gate skipped 200, kill-by-pid issued 0; events==1 in 200/200
```

The 200/200 skip is the useful number: on the already-reaped path — the only one where reuse could
bite — the gate suppresses the kill entirely, so the residual window is not "every post-exit kill" but
"a reap landing inside a two-instruction window", which the stress run never hit.

`kill(2)` is one plain non-variadic libc extern (scalars only, no arch-specific syscall number), so
frame C6 is untouched. The N=200 host test from R2 gains a second parameterisation with
`pidfd: None`, so both modes are gated by the same test.

---

## NEW-2 — **REVISE** (defect accepted; his option (ii) declined with evidence)

**Verified.** `/var/lib` is `root:root drwxr-xr-x`, and as a non-root user
`mkdir -p /var/lib/kiosk-probe` → **Permission denied**. So after C16, OB-4's `create_dir_all` is
`EACCES` for exactly the caller its sentence names, and C2's *"a manual dev run without systemd must
not fail to bind"* rationale is dead — that branch is now `/var/lib/kiosk`, unwritable non-root, so
the bind fails into `pipe.rs:370-388`'s breadcrumb-and-retry and the FSM restarts main at every
startup-grace expiry. Internal inconsistency, as he frames it. C16's value stands — parent §4 `:409`
mandates `/var/lib/kiosk/` (tier 1, re-checked).

**I take his option (i), and decline option (ii).** The rationale is the thing that expired, not the
path:

- C2's fallback rationale is rewritten from *"a manual dev run must not fail to bind"* to *"a run
  without **systemd** — no `RuntimeDirectory`, therefore no `/run/kiosk` — which is still a **root**
  run, the same principal the unit uses."* The branch survives; only its stated reason changes.
- One sentence added: **a non-root manual run is not a supported configuration.** It degrades
  loudly, not silently — bind failure leaves the `pipe.rs:384` breadcrumb, `create_dir_all` failure
  leaves C13's WARN — which is Q3-correct, and consistent with C9's derivation that the launcher runs
  as root because C11's shape declares no `User=`.

**Why not `$XDG_RUNTIME_DIR`.** Two checks, both against it:
1. It is **not guaranteed present**: unset on this box, and it is set by `pam_systemd` at login, so
   it is absent in CI, in containers, and under `su`. It therefore needs its own fallback — a fourth
   branch in `runtime_dir()`, to serve an unsupported configuration.
2. **It rescues nothing.** The socket is not what breaks a non-root run: the spool (`spool/launcher`,
   `spool.orphaned`), every breadcrumb (`sink::breadcrumb`), and C13's lock all live in
   `/var/lib/kiosk` and are equally unwritable. A per-user socket would leave the launcher running
   with no telemetry, no breadcrumb and no backstop — a *silently* half-working dev run, which is
   worse than a loudly failing one.

So the honest fix is to say what is true (root-only) rather than to buy a branch that makes an
unsupported configuration look supported. C13's `create_dir_all` stays — it is correct and useful for
the root case (first boot before the package has created the dir) and its failure arm already WARNs.

---

## NEW-3 — **CONCEDE**

Checked on the cage he installed: `cage --version` → `cage: invalid option -- '-'` + usage, **rc=1**;
`cage -v` → `Cage version 0.1.5`, **rc=0**. Under `set -e` the version line would abort smoke 13
before the assertion that matters. Smoke 13 records `cage -v`. One character; no further argument.

---

## Noted item 1 — `job.rs:221-223` — **CONCEDE**

Correct: `sink.rs:406` passes what `LauncherSink.child` holds, so `#[cfg(not(windows))]`
`assign(&self, _child: &Child)` must take `&ChildHandle`. My R2 file list omitted it; the "zero
lines" claim was scoped to *Windows behaviour* and stays true (`ChildHandle` **is**
`std::process::Child` there, so `job.rs:199`'s Windows signature is byte-identical). Added to C5's
change list, and it lands in the same edit as C12's `#[cfg(unix)] Job::create()` rewrite, which
already touches that impl block.

## Noted item 2 — C7's `-1` totality arm — **REVISE**

He is right that reusing `-1` reintroduces the collision I closed the `-signo` encoding to avoid
(`sink.rs:434-437`, `spawn.rs:100-109`). The arm is unreachable — from `child.wait()` without
`WUNTRACED`/`WCONTINUED`, exactly one of `code()` / `signal()` is `Some` — so it is a latent trap, not
a live bug, and a trap is precisely what a plan-time implementer copies forward. Changed to **`-2`**,
with a one-line comment naming it as the impossible-status sentinel and distinct from the
spawn-failure `-1`. One character, trap removed. (`probe/new1.rs` already uses `-2`.)

## Noted item 3 — raw `syscall` is load-bearing — **CONCEDE**, promoted into the spec

Verified: glibc's `pidfd_open` wrapper is 2.36; Ubuntu 22.04 ships 2.35 (this box is 2.39, which is
why a local `extern { fn pidfd_open }` would appear to work and then fail on half the platform
floor). The syscall numbers themselves are fine — `pidfd_open` 434 / `pidfd_send_signal` 424 are in
the arch-agnostic range, and the kernels clear the floor (Debian 12 = 6.1, Ubuntu 22.04 = 5.15, vs
5.3 / 5.1 required). Added to C5 as a **Q5 note in the spec body, not a plan-time decision**: *"declare
these via `syscall(2)`, not as direct externs — glibc only gained a `pidfd_open` wrapper in 2.36 and
the platform floor includes glibc 2.35."* `kill(2)` is exempt and is declared directly.

---

## Final register — C1…C16

| ID | Change | Final state |
|---|---|---|
| C1 | UDS listener transport | Clean, unchanged since R1 |
| C2 | Socket naming / derivation / seam / `SUN_LEN` | Unconditional unlink (R2, OB-7). **R3:** fallback rationale rewritten — "a run without systemd, still root"; non-root manual runs declared unsupported, degrading loudly. Depends on C16 |
| C3 | `SO_PEERCRED` local `ucred` extern | Clean; probe is the pin, no size assertion claimed |
| C4 | `pipe.rs` Unix `serve` | Clean, unchanged since R1 |
| C5 | Linux `spawn_main` + waiter | Waiter owns the `Child`, sole reaper/reporter (R2). **R3:** `pidfd: Option<OwnedFd>`; open failure ⇒ WARN + `("pidfd", …)` breadcrumb + continue, **not** `Err`; `Err` retained for waiter-thread-creation failure only. File list gains `job.rs:221-223`. Spec states the `syscall(2)`-not-extern requirement (glibc 2.35 floor). One declared cross-platform change (`ChildHandle` alias); zero Windows behaviour diff |
| C6 | `kill_and_wait` Unix body | `pidfd_send_signal(SIGKILL)` + bounded poll on `exited`. **R3:** degraded arm — `kill(pid, SIGKILL)` gated on `exited` when `pidfd` is `None`, with a `ponytail:` naming the reuse ceiling and the upgrade. Cost = pre-P2-C status quo |
| C7 | `128 + signo`, never-86 | Sourced from `ExitStatus::code()` / `signal()`. **R3:** totality sentinel `-1` → **`-2`**, removing the collision with the spawn-failure sentinel |
| C8 | `heartbeat.rs` Linux client | Clean, unchanged since R1 |
| C9 | Launcher `credential_acl.rs` + A's C12 hand-forward | Clean; its root-only derivation is now also what carries C2's R3 rationale |
| C10 | `cage -- kiosk-launcher` shape | **Assumption discharged** — the Critic ran cage 0.1.5 headless: exit 86/7/0 propagate, `kill -9` → rc 137, non-Wayland-client child tolerated. Still no version constant; floor assertion → P2-G |
| C11 | systemd unit shape | Clean; `RuntimeDirectoryMode=0700` carries C2's loud-failure property, `KillMode=control-group` carries C12's |
| C12 | Orphan-kill parity | `#[cfg(unix)] Job::create()` → `Err` when `INVOCATION_ID` unset. **Guard confirmed:** `INVOCATION_ID` survives the cage hop, and cage exits on *abnormal* child death (rc 137) — the chain link this change needed and had not named. Wedged-cage residual risk carried by P2-G; gate owned by P2-G |
| C13 | Single-instance parity | `<data_dir>/launcher.lock` + `create_dir_all`. **R3:** scoped root-only per C2's rewritten rationale; the non-root `EACCES` path is the declared-unsupported case and WARNs |
| C14 | RT-13 cross-platform | Clean; unaffected by C5 (rt13 builds `LauncherSink` directly, never calls `spawn_main`) |
| C15 | Smoke 13-15 | **R3:** version line is `cage -v`, not `cage --version` (rc 1 vs 0) |
| C16 | Launcher `resolve_data_dir()` → `/var/lib/kiosk` | Value confirmed against parent §4 `:409` (tier 1); ownership confirmed (A is kiosk-main only). Hard co-landing constraint with P2-A. Discharges TEL-10 drain co-location |

**Withdrawn this round:** routing `pidfd_open` failure to `spawn.rs:139-151`'s `Err` arm; C2's
dev-run bind rationale; C7's `-1` totality sentinel; `cage --version`.

**Added this round:** the `pidfd: None` degraded arm + its `ponytail:` ceiling; the "unsupported
non-root run" statement; the `syscall(2)`-not-extern requirement; `job.rs:221-223` in C5's file list.
