# P2-C — Linux Launcher Shell: UDS Heartbeat + cage/systemd Supervision (Design)

> Third sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.1 (process
> model, arch-04/05/12/15), §4 (Linux paths), §10 (RT-13). **Builds on P1-E1/E2** (the pure
> `watchdog` FSM + the launcher actor loop — `2026-07-31-p1e2-launcher-shell-design.md`)
> and P2-A/B. Reimplements NO supervise logic: the E1 FSM, the actor loop, the sink, the
> spool drain, and the safe-mode chain are already portable. This ports the three Windows
> edges — pipe server, spawn/wait, heartbeat client — closes the three supervision
> guarantees that are silent Unix no-ops today, and defines the systemd/compositor contract
> around the launcher.

**Status:** rev 2, 2026-08-07 — adversarial design review; see
`docs/superpowers/reviews/2026-08-07-p2b-p2g-adversarial-review/`.

## Goal

`kiosk-launcher` supervises `kiosk-main` on Linux with the guarantees Windows actually
enforces: spawn, watch a heartbeat channel, restart per the FSM, drain a dead main's spool,
exit 86 on technician exit, kill orphans, refuse a second instance — under a cage compositor
started by one systemd unit. Merge gates: **RT-13 running in per-PR Linux CI** (the full
supervise loop, tested on every PR — coverage the Windows-only pipe never allowed), the
N=200 spawn-and-kill host test, and smoke scenarios 13–15.

## Scope

**In:** Linux bodies for `kiosk-launcher/src/{pipe,spawn,job}.rs` and
`kiosk-main/src/heartbeat.rs` (including the arch-04 JS-ping, C17); the launcher's own
`credential_acl.rs` and `resolve_data_dir` Unix implementations; RT-13 made cross-platform;
the systemd unit *contract* (the installed unit file, seat/DRM permissions, `[Install]`, and
start-limit numbers are P2-G's).

**Out:** idle/gesture (P2-D), video (P2-E), update/CI-harness (P2-F), packaging/image/
logind/seatd (P2-G).

**Change register:** C1–C17. Cross-spec edges are tabulated at the end; every one is
declared in both directions.

## Architecture — the three approved decisions

### C1 — Transport: Unix domain socket listener, same contract, same FSM semantics

The launcher binds `std::os::unix::net::UnixListener`. Nothing else about the channel
changes: same `kiosk_core::ipc` frames, same `'\n'` framing, same `Event` mapping, same
`child_pid` contract. The launcher has no async runtime (`kiosk-launcher/Cargo.toml`:
`kiosk-core` + `serde_json`; native deps are `cfg(windows)`-only), and `serve` is already
blocking-on-caller-thread (`main.rs:239`), so blocking std is the only option and the shape
is unchanged.

**Why not socketpair + inherited fd (rejected, recorded).** Forgery-proof by construction
and no filesystem state — strictly better on those two axes — but there is no listener, so
the FSM's channel states have no producer. `ChannelReconnected`'s **only** producer in the
tree is `pipe.rs:455`, inside the post-accept reader loop, latched at `:479` and cleared at
`:445`; `watchdog.rs:257-260` (fault sets grace), `:212-224` (grace expiry →
`restart(0, now, "channel")`), `:262-265` (reconnect clears grace, logs `ChannelReset`).
And `ChannelFault` is itself near-unreachable over a socketpair: `pipe.rs:474` gates the
fault on `child_pid == expected`, and a socketpair only EOFs when the child dies (PID
already 0) or deliberately closes its fd. So socketpair does not degrade fault→restart, it
**deletes both channel states** and leaves only `hang`.

### C10 — Process shape: `kiosk.service` → `cage -- kiosk-launcher` → `kiosk-main`

The compositor wraps the launcher; the launcher spawns `kiosk-main` with the inherited
`WAYLAND_DISPLAY`. kiosk-main stays the launcher's **direct child**, so exit codes and
`ChildExited` see exactly what Windows sees, and a main restart never churns the compositor.
**Kill semantics do not see what Windows sees** — that is C12, stated there rather than
asserted here.

**Measured, cage 0.1.5+20240127 (`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1`):**
`cage -- sh -c 'exit 86'` → rc 86; `exit 7`/`exit 0` → rc 7/0; `kill -9 $$` → **rc 137**
(cage exits when its child dies *abnormally*, not only on clean exit — the link C12's chain
depends on); a child that never connects as a Wayland client (`sh`) is tolerated and runs to
completion; `INVOCATION_ID` is inherited intact across the cage hop. The first property makes
`RestartPreventExitStatus=86` sound with cage as the unit's main process; the second makes
the launcher (not itself a Wayland client) a legitimate cage child.

**No cage version constant is pinned in this spec.** Frame floor C7 is Debian 12 *and*
Ubuntu 22.04 and the dev/CI box is neither, so a constant would be wrong wherever it was
read. The measurements above are stamped **cage 0.1.5 (as run in-session)**; the floor
assertion — **cage 0.1.4 (Debian 12)** — is P2-G image validation's, which also asserts the
image's `cage -v` equals the recorded floor. Smoke 13 records the version each run actually
proved. Fallback if 0.1.4 diverges: replace `ExecStart=cage -- …` with a two-line wrapper
that `exec`s the launcher so systemd sees its status directly (P2-G's to build; the gate
fires long before an image exists).

**Rejected shapes, recorded:** launcher-spawns-`cage -- kiosk-main` (compositor flash on
every restart; the FSM's child becomes cage and main's exit code is laundered through it),
and a two-unit `BindsTo` split (two supervisors owning one failure domain — the launcher FSM
is the restart authority, systemd only supervises the launcher itself).

### C11 — systemd unit **shape**

Values, installation and start-limit numbers are P2-G's; the *set of directives* is C's,
because three of them are load-bearing for changes in this spec.

```ini
[Unit]
# StartLimitIntervalSec / StartLimitBurst belong HERE, not in [Service] — systemd 255
# silently discards them from [Service]. Values are P2-G's.

[Service]
Type=simple
ExecStart=cage -- /usr/lib/kiosk/kiosk-launcher --config /etc/kiosk
Restart=always
RestartPreventExitStatus=86
SuccessExitStatus=86
RuntimeDirectory=kiosk
RuntimeDirectoryMode=0700
KillMode=control-group

[Install]
WantedBy=multi-user.target
```

- **`SuccessExitStatus=86`** — parent §3.1:169-175 names all three directives in one
  sentence. Without it `RestartPreventExitStatus` still suppresses the restart, but the unit
  lands in `failed` with `status=86`, so `systemctl is-failed` and every dashboard on top of
  it report a healthy technician exit as a device fault.
- **`RuntimeDirectoryMode=0700`** — `RuntimeDirectory` defaults to 0755, which would leave
  `/run/kiosk/hb-<pid>.sock` connectable by any local user. `SO_PEERCRED` (C3) still refuses
  them, so there is no forgery, but connect-and-reject in a loop is an accept-starvation
  channel of exactly the kind `pipe.rs:390-395` exists to prevent on Windows. This directive
  also carries C2's loud-failure property.
- **`KillMode=control-group`** — systemd's default, made explicit because C12 depends on it.
- **Install path** — `/usr/lib/kiosk` + `/etc/kiosk` per P2-G's layout (parent §4's
  `/opt/kiosk/` cell is recorded as an erratum: `/opt` trips lintian `dir-or-file-in-opt` at
  severity *error*). `[Install]` is G's to own; the shape is stated here so the two halves
  cannot drift.
- Structural check: `systemd-analyze verify` on the `[Service]` block exits 0 under
  systemd 255, and a deliberately misspelled key is reported — so the tool is really parsing.
  Runtime semantics (a unit exiting 86 ends `inactive`, not `failed`) remain a **declared
  assumption**, pinned by P2-G image validation, which C already nominates for the systemd
  half of smoke 14.

## Components

### `pipe.rs` — `#[cfg(unix)] serve` (C2, C3, C4)

**Naming and derivation (C2).** `pipe::instance_name()` gains a `cfg` seam *inside the
function*, so `main.rs:172`'s unconditional call site is untouched: `#[cfg(windows)]` keeps
`format!("{PIPE_NAME}-{pid}")` (`pipe.rs:55-60`); `#[cfg(unix)]` returns
`<runtime_dir>/hb-<launcher-pid>.sock`. Per-PID naming mirrors the stale-instance discipline
`pipe.rs:39-44` states. `runtime_dir()` is a pure, host-tested function: `/run/kiosk` when it
is a directory (the unit's `RuntimeDirectory`), else the data dir.

The second branch is for **a run without systemd** — no `RuntimeDirectory`, therefore no
`/run/kiosk` — which is still a **root** run, the same principal the unit uses. **A non-root
manual run is not a supported configuration.** It degrades loudly, not silently: after C16
the data dir is `/var/lib/kiosk` (root-owned, unwritable non-root), so the bind fails into
`pipe.rs:370-388`'s breadcrumb path and C13's lock WARNs. `$XDG_RUNTIME_DIR` was considered
and rejected — it is unset in CI, containers and under `su`, so it needs its own fourth
branch, and it rescues nothing: the spool, every breadcrumb and C13's lock are all equally
unwritable, so a per-user socket would produce a *silently* half-working dev run, which is
worse than a loud failure. Non-root Linux launcher debugging is served by RT-13 (C14), which
builds `LauncherSink` directly under tempdirs.

**`SUN_LEN` bound.** The derivation returns `io::Result` and rejects a path ≥ 108 bytes
rather than handing it to `bind()`. Probed: `UnixListener::bind` succeeds at 107 bytes and
fails at 108 with `"path must be shorter than SUN_LEN"` — std raises a clean `io::Error`, it
does **not** truncate. `/run/kiosk/hb-<pid>.sock` is 24 bytes; `/var/lib/kiosk/hb-<pid>.sock`
is 27; RT-13's tempdir paths measure ~55. The host test asserts the *bind*, not only the
derivation.

**Unconditional `remove_file` before `bind`.** The name is `/run/kiosk/hb-<our-own-pid>.sock`.
For a live peer to hold it, some other process must be *listening* on a path named after a
PID this process currently holds — impossible for a peer launcher (we hold the PID) and
impossible for a squatter, because `RuntimeDirectoryMode=0700` on a root-owned directory
means no other principal can create the file at all. So the loud-failure property
`FILE_FLAG_FIRST_PIPE_INSTANCE` buys on Windows for one flag bit (`pipe.rs:100-104`) is
bought on Linux by a directive C11 already carries. **A connect-probe collision branch was
designed and then withdrawn (recorded):** it guarded a state unreachable by construction, at
the cost of an extra syscall path, a retry arm and a second failure mode — one line beats
that. Bind failure for any other cause takes `pipe.rs:370-388` unchanged: once-per-streak
`eprintln!` + `crate::sink::breadcrumb(data_dir, "pipe", …)` + `sleep_retry()`, with the
`ponytail:` at `:373-380` (a permanently unbindable name is the only silent-forever degraded
path on the device) carrying over verbatim.

**Peer verification (C3).** Per-accept `getsockopt(fd, SOL_SOCKET, SO_PEERCRED, …)` via a
local extern and a 3×`u32` `#[repr(C)] Ucred`. `UnixStream::peer_cred()` is unstable
(`E0658`, rust issue #42839), so there is no stable-std path. No new dependency: the platform
C library is already linked into every Rust binary on this target — true of `gnu` and `musl`
alike — which is the correct reading of the convention `spawn.rs:12-14` set for kernel32.
No size assertion is claimed; the probe is the pin (the extern returns the connecting
process's PID correctly, measured).

The resulting `Option<u32>` feeds the **existing, unmodified** `accept_client(client,
expected, current)` (`pipe.rs:84-89`) — snapshot **or** current, fail-closed otherwise —
and the Linux `serve` keeps `await_child_pid` and the post-accept
`expected = client.unwrap_or(expected)` re-derivation (`pipe.rs:441`). A one-valued check
against `child_pid` would reintroduce a fixed bug: with `backoff_s > 2` the pre-accept
snapshot is 0 (`pipe.rs:76-81`), so snapshot-only rejects the legitimate new child and cries
impostor on **every** normal restart (regression test at `pipe.rs:556-566`). No accept→check
TOCTOU exists: `SO_PEERCRED` returns credentials the kernel recorded at `connect(2)` time,
not a live lookup.

*Divergence, declared:* `SO_PEERCRED`/`struct ucred` is Linux (and Android) ABI; macOS uses
`LOCAL_PEERPID`, so a macOS build would fail to link. `.github/workflows/ci.yml` has exactly
three jobs — `lint-test` (ubuntu-22.04), `build-windows`, `build-linux` — and macOS is built
nowhere. Recorded as a `ponytail:` with the one-line upgrade if a macOS dev host appears.

**The accept loop (C4).** The `#[cfg(not(windows))]` stub at `pipe.rs:519-528` is **replaced,
not added beside** (`#[cfg(unix)]` and `#[cfg(not(windows))]` both match on Linux). Structure
mirrors `pipe.rs:366-491` one-for-one: `bind` ↔ `create_pipe`; `await_child_pid` unchanged;
`listener.accept()` ↔ `connect_pipe`; `accept_client` unchanged; `BufReader::read_line` ↔
`LineReader::next_line` with the same `MAX_LINE_BYTES` cap and the same silent drop on
`decode` error; `Err`-while-`child_pid == expected` → `ChannelFault` + latch; re-accept +
first frame → `ChannelReconnected` before that frame's own event; `logged_failure` /
`logged_impostor` once-per-streak latches unchanged. The mapping table is byte-for-byte the
Windows one (`pipe.rs:65-71`, `:84-89`, `:441-446`, `:450-486`); only the accept/read calls
change.

**No poll/timeout on `accept()`.** It blocks exactly as `ConnectNamedPipe` does, and `cancel`
is checked only between blocking calls — the shape `pipe.rs:322-330` already documents, with
`main.rs:249-256` stating the launcher relies on `process::exit` for teardown. Adding a
timeout would be new mechanism to fix a caveat the Windows path already lives with. Mirror,
don't invent.

### `spawn.rs` — the sole-reaper design (C5, C6, C7)

**Withdrawn, recorded: "the waiter thread is plain `child.wait()`."** It is not implementable
as a drop-in, for the reason `spawn.rs:89-95` gives: `Child::wait` takes `&mut self` and the
caller keeps the `Child` (`sink.rs:421` `self.child = Some(child)`, consumed by `sink.rs:377-381`
`kill_child` and `sink.rs:406-407` `job.assign`). `std::process::Child` exposes no unix
duplication API. Two threads independently `wait()`ing is not merely unsupported, it is a
PID-reuse kill hazard.

**Also withdrawn, recorded: `waitid(P_PID, …, WEXITED|WNOWAIT)` + a hand-rolled `siginfo`.**
It is *not* ownership-identical to Windows — it is a query against a PID a different thread
is racing to reap. Measured: with the sink reaping first, the waiter's `waitid` returns −1
`ECHILD` with an untouched buffer (`si_code=0, si_status=0`), which loses the exit event
*and*, under C7's total mapping, fabricates a `ChildExited{128}`. A guard returning early on
`ECHILD` is **not** an admissible fix: it makes `spawn.rs:100-109`'s contract read *at most
one* exit event, and arch-12's sliding restart window (`watchdog.rs:149-156`, entered without
a phase guard at `:186-190`) is built on *exactly one*. The `size_of::<siginfo>() == 128`
mitigation is struck with the struct: `siginfo_t` is 128 bytes by `__SI_MAX_SIZE` on every
Linux ABI, so the assertion constrains the filler and never the offsets that move, and
`waitid` performs no length check — an inert check presented as a pin.

**The settled design.** On Unix a child's exit status is a **single-consumer resource** —
there is no analogue of Windows' duplicated process handle, which is precisely what
`spawn.rs:89-95` works around. So there is one consumer, and it is the one that reports:

> The **waiter thread owns the `Child`** and is the sole reaper, sole status consumer and
> sole reporter (`child.wait()`, stdlib). The **sink holds a `pidfd`** — a reuse-immune
> handle it uses to kill and to observe death, and which cannot consume a status.

```rust
#[cfg(windows)] pub type ChildHandle = std::process::Child;
#[cfg(unix)]    pub struct ChildHandle { pidfd: Option<OwnedFd>, exited: Arc<AtomicBool>, pid: u32 }
```

- `#[cfg(unix)] spawn_main`: `Command` with `KIOSK_HEARTBEAT_PIPE=<socket path>`, the
  unchanged `--safe` chain, and **`--config <config_dir>`, byte-identical to `spawn.rs:121`**
  — spawn → `pidfd_open(pid)` → move the `Child` into the waiter thread → return
  `ChildHandle`.
- Waiter: `child.wait()` → `ExitStatus` → C7 mapping → `exited.store(true)` → send one
  `ChildExited`. No error branch exists, because there is no competitor.
- Exactly-one-exit-event holds **by construction**, not by guard: one resource, one consumer.
  On `Err` the caller supplies the one synthetic `ChildExited{-1}` (`sink.rs:434-437`) and no
  supervised child exists; on `Ok` the waiter is the only party that can observe the status.

**`--config` is load-bearing (INT-9).** `main.rs:22-26` already states the contract —
*"`spawn::spawn_main` passes this directory to the child as its own `--config`"* — and the
`#[cfg(not(windows))]` stub at `spawn.rs:198-210` takes `_config_dir` and drops it. Without
the flag, `kiosk-main`'s `resolve_config_dir` (`main.rs:423-431`) falls back to
`current_exe().parent()` = `/usr/lib/kiosk`, where `kiosk.ini`, `kiosk-credential.json` and
`kiosk-offline.mp4` do not exist under P2-G's layout. Fail-closed one process downstream.

**`pidfd_open` failure does not fail the spawn.** Routing it to the existing `Err` arm
(`spawn.rs:139-151`) is wrong and is **withdrawn**: that arm is documented for a *transient*
failure — `spawn.rs:141-144`, *"exceedingly rare (e.g. handle-table exhaustion)"* — whereas
`pidfd_open` denial is **permanent and environmental** (a `SystemCallFilter=` from P2-B, a
container seccomp profile denying syscall 434, or `ENOSYS` below kernel 5.3). Traced through
the FSM: `Err` → `sink.rs:428-441` `ChildExited{-1}` → `watchdog.rs:186-190`
`restart(-1, …, "exit")` → backoff → rule-7 → `safe = true` → `SAFE_FAIL_LIMIT` →
`Log(SafeModeFailed)` with backoff pinned at 60 s → **forever**; the only terminal action in
the machine is `ExitLauncher{86}` (`watchdog.rs:196-198`). One denied syscall would mean a
device that never renders, never exits and never stops trying — exactly what `job.rs:18-25`
forbids (*"a device that refuses to start because a hardening feature failed is a black
screen"*), and the same doctrine C12 leans on.

Settled: `pidfd_open` failure ⇒ `pidfd: None`, `eprintln!` + `breadcrumb_if_absent(data_dir,
"pidfd", …)` on the existing degraded channel (replayed at `main.rs:222-226` alongside
`("job", …)` and `("mutex", …)`), **and supervision continues**. The `Err` arm is retained
for **waiter-thread-creation failure only** (`spawn.rs:179-191`), where the Windows analogy
is exact and the failure genuinely is rare.

**Spec requirement, not a plan-time choice: declare `pidfd_open`/`pidfd_send_signal` via
`syscall(2)`, never as direct externs.** glibc gained a `pidfd_open` wrapper only in **2.36**
and Ubuntu 22.04 ships **2.35**, so a direct extern links on a modern dev box and fails on
half the platform floor. The syscall numbers are in the arch-agnostic range (434 / 424) and
the kernels clear it (Debian 12 = 6.1, Ubuntu 22.04 = 5.15, vs 5.3 / 5.1 required). `kill(2)`
is exempt and is declared directly. Recorded here so it survives a later "simplification".

**Cross-platform change, declared (frame C8).** `LauncherSink.child`'s type becomes the
`ChildHandle` alias and `job.rs:221-223`'s `#[cfg(not(windows))] assign` takes `&ChildHandle`
(it lands in the same edit as C12's `Job::create()` rewrite, which already touches that impl
block). Windows behaviour diff is **zero lines**: there `ChildHandle` *is*
`std::process::Child`, so `job.rs:199`, `sink.rs:377-381`, `:406-407` and `:421-423` compile
unchanged. This is C's one declared cross-platform change.

#### `kill_and_wait` — the bound must be introduced, not kept (C6)

**Correction, recorded:** the bounded-wait doctrine and `KILL_WAIT_MS` are
`#[cfg(windows)]`-only (`spawn.rs:29-39`); the existing Unix body (`spawn.rs:63-67`) is
`let _ = child.kill(); let _ = child.wait();` — unbounded, and replaced.

`#[cfg(unix)] kill_and_wait`: `pidfd_send_signal(pidfd, SIGKILL)` then a bounded poll on the
waiter's `exited` flag up to the same 5 s ceiling; on expiry, give up and proceed, same
degradation the Windows doc records. `exited` is set *after* `child.wait()` returns — i.e.
after the reap — so it is a strictly **stronger** postcondition than the
`WaitForSingleObject(KILL_WAIT_MS)` it mirrors: process gone, fds closed, no zombie.

*Which inherited rationale survives the port, stated because C3 requires it:*

- **Does not apply on Linux:** the `ERROR_SHARING_VIOLATION` rationale (`spawn.rs:44-49`,
  `sink.rs:368-374`). POSIX `rename(2)` does not care about open descriptors, so the
  orphan-spool rename cannot race a live writer's open file.
- **Does apply:** `sink.rs:374-376` — waiting closes the `ChannelFault`-after-kill race, so
  the reader's error can no longer beat `child_pid.store(0)`. Transport- and
  platform-independent, and `pipe.rs:467-481` is still gated on `child_pid == expected`. The
  bound stays on this rationale alone.

**Degraded arm (`pidfd: None`):** `kill(pid, SIGKILL)` **gated on `exited`** — no kill is
issued at all once the child is known reaped. Measured over 200 zero-delay
spawn/reap/kill iterations: the gate skipped the kill 200/200 and zero kill-by-pid was ever
issued post-reap; `events == 1` in every ordering, `zombie=false` throughout. `ponytail:` the
ceiling is a two-instruction window between the atomic load and the syscall in which a reap
could recycle the PID; upgrade is pidfd, when the sandbox permits it. This is strictly better
than the only Unix kill path the tree has ever had (`spawn.rs:63-67`, an *ungated*
`child.kill()`), not a new exposure.

**Reuse-immunity in the pidfd mode is a construction property, not a lucky ordering:**
`pidfd_send_signal` on an already-reaped pidfd returns `ESRCH` — 200/200 in the stress run.

#### Exit-status mapping (C7)

Pure, host-tested, sourced from `ExitStatus::code()` / `ExitStatusExt::signal()`:

```
code()   => that code
signal() => 128 + signo          // 129…192 for signals 1…64
neither  => -2                   // impossible from a wait()ed status; see below
```

**Invariant: a signal death can never map to 86.** arch-05 reserves 86 for the technician
exit (`watchdog.rs:196-198` returns `ExitLauncher{86}` with no restart, and
`RestartPreventExitStatus=86` stops systemd on top of it). The OOM-killer case is real on a
kiosk, and a SIGKILLed main reading as a technician exit would leave a dead device systemd
deliberately refuses to restart.

**Encoding pinned here, not at plan time.** `-signo` **collides with the existing `-1`
sentinel**: `sink.rs:434-437` feeds a synthetic `ChildExited{code: -1}` on every `spawn_main`
`Err` and `spawn.rs:100-109` makes that a contract, so SIGHUP (signal 1) would render a
signal death and a spawn failure identically in `watchdog.restart`'s `code` field — a silent
failure in the one diagnostic an operator has. `128 + signo` is never 86, never −1, and 137
is the universally recognised SIGKILL/OOM code. Collision check against kiosk-main's own
codes — 0 (`cli.rs:31`), 86 (`pinpad.rs:156`), 101 on panic — is clean. The third arm is
**`-2`**, not `-1`: it is unreachable (from `child.wait()` without `WUNTRACED`/`WCONTINUED`
exactly one of `code()`/`signal()` is `Some`), and a trap is precisely what a plan-time
implementer copies forward. Ambiguity with a literal `exit 137` is likewise unreachable —
kiosk-main emits only 0, 86 and 101.

**Gate.** A host test in `spawn.rs` asserts `events == 1` over **N=200** iterations of
spawn-and-immediately-kill, parameterised over both `pidfd: Some` and `pidfd: None`. That
test fails intermittently under the withdrawn `waitid` design and cannot fail under this one
without the ownership rule being broken. RT-13 cannot cover it — `rt13.rs:145-152` passes
`job: None` and never exercises the kill/exit race — so the host test is the gate, not RT-13.

### `heartbeat.rs` — the Linux client (C8) and the arch-04 JS-ping (C17)

**C8.** Replace the `#[cfg(not(windows))]` stub at `heartbeat.rs:149-155` with a
`#[cfg(unix)]` body: `tokio::net::UnixStream::connect(&path)` in place of
`ClientOptions::new()…open()`; everything else in the Windows client (`heartbeat.rs:41-147`)
is copied unchanged — the `ready_reached` latch, the once-per-streak `logged_failure` latch,
the cancel-wrapped `write_all`s, `tokio::time::interval` with `MissedTickBehavior::Delay`,
`RECONNECT_BACKOFF` (1 s, `:31-34`, well under the FSM's 15 s miss window), `sleep_or_cancel`.
`pipe_name_from_env()` (`:37-39`) is already platform-free; the client derives nothing — it
connects to whatever `KIOSK_HEARTBEAT_PIPE` says, which is what keeps the contract one-sided,
as on Windows. `tokio` features `net` + `io-util` are already declared in
`kiosk-main/Cargo.toml`: **no Cargo change.**

*Divergence, declared (it makes the client simpler, not looser).* The Windows comment
(`heartbeat.rs:62-67`) enumerates `ERROR_PIPE_BUSY` / `ERROR_FILE_NOT_FOUND` because the pipe
name transiently ceases to exist between reconnects. On Linux the socket file persists across
the listener's accept cycle, so the reconnect-gap errors are `ENOENT` (before first bind) and
`ECONNREFUSED` (file present, no listener). The existing arm already retries *any* open
error, so only the comment changes.

**C17 — webview round-trip gate on the heartbeat (arch-04 / RT-02 / OD-1).** The parent puts
this in P2 verbatim (§3.1:133-141), and without it a **wedged GTK main loop is invisible**:
`heartbeat::run` is a `tokio::spawn`ed task (`main.rs:941`, cadence at `heartbeat.rs:110`)
that never touches the GTK loop, so a wedged UI thread keeps pinging and the FSM's 3-missed
rule never arms. Composed with P2-D claiming no covering control and P2-G removing VT, getty
and SSH from a conforming image, that is parent §3.5's un-exitable device reached without any
single spec being wrong. C owns the fix because C already owns this file's Linux body and the
hang scenario; it is one arm of one function C is rewriting.

> `#[cfg(not(windows))]`, in `heartbeat::run`'s `tick.tick()` arm: before each `Frame::Ping`
> write, round-trip a no-op through the webview — `AppHandle::run_on_main_thread` →
> `WebviewWindow::with_webview(|w| w.inner().run_javascript("0", None, cb))`, `cb` resolving a
> `tokio::sync::oneshot`, awaited under the parent's own **3 s cap**. Timeout or error ⇒
> **the ping is withheld** — not an error, not a log storm: one WARN on the first withheld
> ping of a run. Three withheld pings = 15 s = the FSM's existing 3-missed rule →
> `watchdog.hang` → restart. `heartbeat::run` gains one parameter (the `WebviewWindow`);
> `main.rs:941` gains one argument.

Both of arch-15's uncovered halves fall to one mechanism: a **wedged GTK main loop** never
dispatches `run_on_main_thread` (P2-A states the premise — the `with_webview` closure runs on
the GTK main thread), and a **wedged renderer** with a live loop never delivers the
`run_javascript` reply. Either way the cap expires.

*Feasibility at P2-B's declared WebKitGTK floor, verified here:* `run_javascript`
(`web_view.rs:1469`) carries **no `#[cfg(feature = …)]` gate** — only
`#[cfg_attr(feature = "v2_40", deprecated)]` at `:1466` — so it compiles at `v2_32` and at the
`v2_40` tauri unifies in; the call site takes `#[allow(deprecated)]`. `evaluate_javascript`
is `#[cfg(feature = "v2_40")]` (`web_view.rs:788`) and is deliberately **not** used, so the
declared floor stays real. The `webkit2gtk` dependency declaration is shared with P2-B and
P2-D (union of features, first writer wins); **no ordering edge**.

*Scope narrowing, declared not silent (frame C8).* The parent calls the ping
"cross-platform" but scopes its landing to *"P2 (WebKitGTK/Android, where no native
unresponsive signal exists)"*. Windows has `ProcessFailed`/`RenderProcessUnresponsive` from
P1, so C17 is `cfg(not(windows))` and **Windows is byte-unchanged**.

*Residual after C17:* a wedged **cage** is still unrecoverable — C17 cannot reach it, because
the compositor holds the DRM device. Carried by **P2-G H11** (the hardware row that observes
it) plus a P2-G runbook line making the power cycle the documented supported recovery rather
than a discovered one.

### `credential_acl.rs` (launcher crate) — SEC-09 (C9)

**Replace** the fail-open stub at `credential_acl.rs:100-104` (`Ok(true)`) with the
`#[cfg(unix)]` mode-bits body — `metadata.permissions().mode() & 0o077 == 0` — and rewrite
the doc comment in the same edit. Replace, do not add beside: `#[cfg(unix)]` and
`#[cfg(not(windows))]` both match on Linux. Fail-closed comes free: `is_violation`
(`credential_acl.rs:24-26`, `!matches!(check, Ok(true))`) already treats `Err` as a violation.
The launcher's SEC-09 gate is `sink.rs:73` `fn build_telemetry` (private) with the
check-before-read at `sink.rs:84-86`; the cross-crate comment describing that ordering is
`crates/kiosk-main/src/boot.rs:153-156`. This is the launcher's copy — **kiosk-main's is
P2-A's to fix**, and is still `Ok(true)` in the tree today.

**P2-A's hand-forward, discharged explicitly.** P2-A closes its C12 with *"`ponytail:` mode
bits only, no uid check — a root-owned `0o600` file is the deployment shape; add an owner
check if a non-root service user lands in P2-C."* The C11 shape declares **no `User=`**, so
the launcher, cage and kiosk-main all run as root and the credential is root-owned `0o600`;
mode bits alone remain sufficient. The condition is **transferred, not dropped**: if P2-G's
seat/DRM wiring introduces a non-root `User=`, the uid check lands with it, as a named row in
P2-G's hand-off list — at that point the credential's owner and the reader's uid can differ
and mode bits stop proving anything.

### `job.rs` — orphan-kill (C12) and single-instance (C13) parity

Both are Windows supervision guarantees that are silent Unix no-ops in the tree, and the
draft's blanket parity claim asserted them without checking.

**C12 — orphan-kill.** `job.rs:217-225` is the entire Unix implementation: `create() -> Ok(Job)`
and `assign(&self, _child) -> Ok(())`, both no-ops against a unit struct. So `main.rs:189-199`
always gets `Some(job)` on Linux and `sink.rs:406-419` always "succeeds", printing nothing —
**today, on Linux, a launcher killed with `SIGKILL` leaves `kiosk-main` running full-screen
and unsupervised**, the exact field failure `job.rs:4-16` was written to close.

Enforcement is reassigned to the unit's cgroup. The Job Object exists on Windows because
there is no supervisor above the launcher (parent §3.1: a boot/logon trigger with no
restart-on-exit setting). On Linux there is one. Under `KillMode=control-group` (C11), when
the service stops — including the stop step preceding every `Restart=always` restart —
systemd signals every process remaining in the unit's cgroup and `SIGKILL`s the survivors. A
launcher death makes cage exit (measured: cage exits rc 137 when its child dies abnormally),
which ends the unit's main process, which runs that stop step, so the orphan is killed before
the successor launcher starts.

Detection is code, because the deliverable must not leave the launcher reporting armed
supervision it does not have: `#[cfg(unix)] Job::create()` returns `Err` when
`std::env::var_os("INVOCATION_ID").is_none()`, firing the **existing** WARNING-and-continue
path with its existing message and its existing `("job", …)` breadcrumb
(`main.rs:189-199`, replayed at `:222-226`). Zero new plumbing. `INVOCATION_ID` is set by
systemd for every service since v232, inherited by the whole unit tree, and **verified to
survive the cage hop intact**. `/proc/self/cgroup` was considered and rejected: on this box it
is a legacy-hybrid listing, and parsing v1/v2/hybrid correctly is more code than the thing it
guards. `ponytail:` the ceiling is that `INVOCATION_ID` is env-settable, so the only misreport
it permits is a false *negative* warning on a box where someone exported it by hand.

**Honest parity statement.** This change **reassigns** enforcement to the unit cgroup,
**adds** detection, and **defers** the gate to P2-G. It does **not** close the gap inside this
spec: RT-13 passes `job: None` deliberately (`rt13.rs:145-152`), so it never covered
orphan-kill on Windows either, and smoke 13–15 assert restart, exit-86 and no-zombie. The
gate is **P2-G's G15 assertion** — `pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main` —
named there, not left as "a P2-G row".

*Divergence, stricter:* the cgroup covers grandchildren (WebKitGTK's Network/Web processes,
the Linux analogue of `job.rs:131-134`'s WebView2 tree) without
`AssignProcessToJobObject`'s failure modes (`job.rs:196-198`).

*Divergence, looser — both cases, per C3:* (a) a **non-systemd dev run** has no orphan-kill
at all; accepted, it is the pre-P1-F1 status quo and never reaches a device. (b) **A wedged
cage.** `job.rs:12-16` is explicit that the Job Object fires *"precisely because it needs no
cooperation from the dying process"*; this chain needs cooperation at two links — cage must
notice its child died and exit, and systemd must then run a stop job. `spawn.rs:31-37` names
the realistic kiosk failure (a process wedged in an uninterruptible kernel wait behind a hung
GPU/display driver) and cage is the process holding the DRM device. If cage wedges: the unit
stays `active`, no stop job runs, the orphan survives **and** the dead launcher is never
restarted; on Windows the job object still fires. No in-scope fix exists — `WatchdogSec` needs
`Type=notify` and cage does not `sd_notify`; `RuntimeMaxSec` would restart a healthy kiosk on
a timer. Carried by **P2-G H11** + the runbook power-cycle line.

*Rejected, recorded:* `PR_SET_PDEATHSIG` in `Command::pre_exec` — delivered on the death of
the spawning **thread**, not the process (nothing enforces that the sink dispatches on the
main thread; `rt13.rs:163-166` runs the loop on a spawned thread), and it kills only
`kiosk-main`, not its descendants. Weaker parity for strictly more code. A launcher-owned
cgroup reimplements what the unit already gives us.

**C13 — single-instance.** `job.rs:283-288` returns `Ok(Some(SingleInstance))` on non-Windows
— "every process is 'the one'" — so `sink.rs:298`'s claim that double-launch is prevented
upstream is Windows-only. Under `Restart=always` a wedged predecessor and its successor can
both run; C2's per-PID socket name means they will not *collide*, which hides the condition
rather than preventing it — two launchers, two kiosk-mains, two webviews on one display.

Primary mechanism: systemd guarantees one instance of `kiosk.service`; no code. Backstop,
because "systemd guarantees it" is true until someone runs the binary by hand next to a live
unit: `#[cfg(unix)] acquire_single_instance` takes `std::fs::File::try_lock()` (stable) on
**`<data_dir>/launcher.lock`** — C16's absolute path, preceded by `create_dir_all`. Mapping:
`WouldBlock` → `Ok(None)` (peer holds it — the deliberate `exit(0)` arm `job.rs:239-248`
documents), any other error → `Err` (WARN + continue, per `job.rs:18-25`). The returned `File`
lives in `SingleInstance` for the process lifetime, exactly as the Windows `OwnedHandle` does.
No extern, no dependency.

**The lock deliberately does not use `runtime_dir()`** (recorded, because the first design
did): `runtime_dir()` branches on whether `/run/kiosk` exists and `RuntimeDirectory=` creates
and destroys it with the unit, so a hand-run-then-unit ordering would take two different lock
inodes and both would acquire. `File::try_lock` is per-inode; a path that moves is not a
token. `/tmp/.kiosk-launcher.lock` was declined — a world-writable lock directory with no
`RuntimeDirectoryMode` protection, to solve a problem C16 already removes.

*Divergence, declared:* `flock` is advisory and dies with the process; the Windows mutex is a
kernel object in the `Global\` namespace and sees a launcher in another logon session
(`job.rs:36-47`). On a single-seat kiosk with one unit that difference is not reachable.

### `resolve_data_dir` — the launcher's own (C16)

`crates/kiosk-launcher/src/main.rs:48-53` is `ProgramData` else `PathBuf::from(".")`, joined
with `"kiosk"` — so **on Linux today the data dir is `./kiosk`, relative to CWD**. Under the
unit the CWD is `/` (no `WorkingDirectory=`), so it would be `/kiosk`.

`#[cfg(unix)] resolve_data_dir() -> PathBuf::from("/var/lib/kiosk")`, with the doc rewritten
in the same edit. The value is the parent's (§4:409, tier 1). **This is C's, not P2-A's:**
P2-A's scope is `crates/kiosk-main/src/…` and it mentions the launcher exactly once, deferring
"launcher/heartbeat/systemd (P2-C)" — the same ownership split C9 applies to the launcher's
`credential_acl.rs`.

**Why it is a correctness item and not a citation fix.** The launcher's own doc says why
(`main.rs:44-47`): *"the launcher's `spool/launcher` partition and the `spool/main` partition
it drains have to land in the same place."* If P2-A lands `/var/lib/kiosk` in kiosk-main and
C leaves the launcher at `./kiosk`, the launcher drains an empty `./kiosk/spool/main` and
**TEL-10 dies silently on Linux** — the exact silent-loss class `sink.rs:365-376` exists to
prevent.

**Hard co-landing constraint with P2-A.** The two functions must agree: whichever merges
second must match the first, and if P2-A's value changes, C's follows.

### RT-13 — cross-platform, and the CI gate (C14)

`tests/rt13.rs` is gated by `#![cfg(windows)]` at **`rt13.rs:27`**; the mock's gate is a
different shape — `mock_main.rs:26-30` branches inside `fn main()`, with the real body at
`:33` `#[cfg(windows)] fn windows_main()`.

`unique_pipe(tag)` (`rt13.rs:104-111`, PID + `AtomicU32`) becomes
`unique_transport(tag, dir: &Path)`: `#[cfg(windows)]` keeps `rt13.rs:107`'s template;
`#[cfg(unix)]` returns `dir.join(format!("hb-{tag}-{pid}-{n}.sock"))`, where `dir` is the
scenario's **existing** `data_dir` tempdir (`rt13.rs:117-118`), which `Harness::start`
already owns and drops at teardown. PID + counter is retained, so cross-binary collisions in
`/tmp` remain impossible, and C2's length guard applies to the test paths too.

**Correction, recorded:** the draft's open decision claimed "tempdir per test already implied
by the tag scheme at `rt13.rs:107`". Checked, false — the tag scheme is PID + counter and its
own doc (`:102-103`) says so; the per-scenario tempdirs exist but are `config_dir`/`data_dir`
and the transport name is deliberately not derived from them. Parallelism containment
therefore rests on those tempdirs plus PID + counter, both already present; no
`--test-threads` restriction is needed and none is imposed. A socket in `data_dir` is inert:
`drain_orphan` (`sink.rs:271-285`) touches only `spool/main` and `spool.orphaned`.

**Correction, recorded:** the draft said the mock gets "the same `UnixStream` branch as the
real one". False — the launcher crate has no `tokio` and the mock never shared client code on
Windows either (`mock_main.rs` writes with `std::io::Write`). The Linux mock uses
`std::os::unix::net::UnixStream` with the same `kiosk_core::ipc` frames and the same
retry-the-open contract `mock_main.rs:8-13` states. What is shared is the *protocol*, which is
where it always was. `mock_main.rs:26-30`'s in-`main` branch is replaced by a
`#[cfg(unix)] fn unix_main()` sibling of `windows_main`.

**Where the gate lands.** `.github/workflows/ci.yml:11-26` — job `lint-test`,
`runs-on: ubuntu-22.04`, running `cargo clippy --workspace --all-targets -- -D warnings`
**and** `cargo test --workspace`, on `pull_request`. Un-gating puts RT-13 in a per-PR Linux
run with no workflow edit; `-D warnings` means the un-gated test and mock must be
warning-clean on Linux, which is part of this change. Budget from the constants: four
scenarios, floor set by `MISS_LIMIT_S = 15` (`rt13.rs:46-49`) and `HEALTHY_OBSERVE = 20 s`
(`rt13.rs:61`); under cargo's default parallelism they overlap, so ~25–35 s added wall clock.
If that ever wedges the job, the escape is moving RT-13 into `build-linux`, not trimming it.
RT-13 is unaffected by C5 — it builds `LauncherSink` directly (`rt13.rs:138-152`) and never
calls `spawn_main` or `acquire_single_instance`.

## Smoke additions (C15) — scenarios 13–15

**Harness, stated rather than papered over.** P2-A's harness is **weston headless**;
`WLR_BACKENDS` is a **wlroots** variable weston does not read. Scenarios 13–15 extend A's
harness in fixtures and assertions and **deliberately replace the compositor with cage**,
because cage is the object under test here — asserting the cage contract under weston would
assert nothing. A's weston harness remains correct for scenarios 1–12.

**Runner (INT-3, resolved).** P2-F's nightly `debian:12` container gains **`cage`,
`xwayland`, `xdotool`**, and P2-F states the scenario→compositor map explicitly: A 1–7,
B 8–12 and D 16–17 under weston headless; **C 13–15 under `cage -- kiosk-launcher` with
`WLR_BACKENDS=headless`**; no scenario runs under an unnamed compositor. Numbering does not
collide — P2-A owns 1–7, P2-B owns 8–12, P2-D owns 16–17.

13. **full chain.** First: `cage -- sh -c 'exit 86'; test $? -eq 86` under
    `WLR_BACKENDS=headless`, and **emit `cage -v`** — not `cage --version`, which exits 1 with
    `invalid option -- '-'` and would abort the script under `set -e`. The emitted version is
    what the run proved; the floor assertion is P2-G's. Then the chain: `cage -- kiosk-launcher`
    headless → main up, home rendered, `watchdog.*` events spooling; `kill -9` main → launcher
    restarts it within the FSM's window; main back up.
14. **technician exit.** Drive the pinpad exit → assert the *launcher process* exits 86. The
    systemd half is P2-G's H2. **Driver on the floor:** cage 0.1.4 exposes no virtual-pointer
    and no virtual-keyboard protocol, so the app path is driven by running `kiosk-main` inside
    cage with `GDK_BACKEND=x11` — an Xwayland client — and `xdotool`. *Declared divergence:*
    that exercises GTK's X11 GDK backend, not the Wayland one, which is faithful for what 14
    asserts (exit-86 propagation over GTK widget signals) and is **not** a substitute for the
    Wayland input path. *Fallback, recorded not silently dropped:* if even that fails, 14's
    app-path half moves to the deferred hardware list against P2-G H2 (systemd half) and H4a
    (touch half).
15. **hang path.** (a) `SIGSTOP` main past the heartbeat-miss window → launcher
    kills/restarts (`kill_and_wait` exercised for real); `SIGCONT`ed corpse reaped, no zombie —
    owner named: `LauncherSink::kill_child`, reached from `Action::DrainOrphanedSpool`
    (`watchdog.rs:126-127` → `sink.rs:471`) or from `ExitLauncher` (`sink.rs:483-485`), both in
    the same FSM turn, before any backoff sleep. (b) **C17 variant:** block the **GTK main
    thread only** (a `run_on_main_thread` closure that sleeps past the window), leaving the
    process running and the tokio task alive → assert `watchdog.hang` is emitted and main is
    restarted. That is the first Linux exercise of arch-15 case (c).

**Expected degraded breadcrumb.** Smoke runs outside systemd, so C12's `INVOCATION_ID` guard
correctly fires and every run writes a `("job", …)` breadcrumb to `startup-degraded.txt`.
Scenarios 13–15 expect it; they must not read it as a failure. Neither P2-A nor P2-B asserts
on that file.

## Testing

- **RT-13 on Linux CI (gate)** — fault, reconnect, miss-restart, safe-mode chain, drain:
  whatever E2's scenario list runs on Windows runs identically here.
- **Host tests:** exit-status → `ChildExited{code}` mapping including the never-86 invariant
  and the `-2` arm; the **N=200 spawn-and-kill exactly-one-event test**, run in both
  `pidfd: Some` and `pidfd: None` modes; socket-path derivation *and bind* including the
  `SUN_LEN` rejection; the peer-cred accept/reject decision as a pure function.
- **Smoke 13–15** as above, under cage headless.
- Existing gates unchanged; the launcher crate now compiles fully on Linux CI (its dead-code
  warnings shrink to zero).

## Error handling

Unchanged doctrine, now with the Linux paths cited rather than deferred. Bind/serve failure:
once-per-streak `eprintln!` + `sink::breadcrumb` + 100 ms retry (`pipe.rs:370-388`).
`pidfd_open` failure: WARN + `("pidfd", …)` breadcrumb + continue degraded — never `Err`,
which would be a permanent black screen. `Job::create` failure (no `INVOCATION_ID`): the
existing WARN + `("job", …)` breadcrumb + continue. Single-instance lock failure other than
`WouldBlock`: WARN + continue. Heartbeat client failures cost heartbeats, never the browser
(`heartbeat.rs:16-19`). Drain and kill paths keep their bounded waits. Every one of these is
`job.rs:18-25`'s never-block-boot rule.

## Residual risks — each with a named carrier

| Risk | Carrier |
|---|---|
| Wedged cage: unit stays `active`, orphan survives, launcher not restarted (Windows' job object would still fire); C17 cannot reach it | **P2-G H11** + the runbook power-cycle line |
| Orphan-kill has no gate inside P2-C | **P2-G G15**: `pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main` |
| cage behaviour on the floor's 0.1.4 (measured only on 0.1.5) | **P2-G** image validation asserts `cage -v` against the floor; smoke 13 records what each run proved |
| Degraded-mode kill loses reuse-immunity (two-instruction window between the `exited` load and `kill(2)`) | **`ponytail:` ceiling in C6**; upgrade is pidfd, when the sandbox permits it |
| Non-root manual run | **Declared unsupported** in C2; degrades loudly (`pipe.rs:384` breadcrumb, C13 WARN) |
| Unit exits 86 ends `inactive`, not `failed` (systemd is not PID 1 on the dev/CI box) | **P2-G** image validation, with smoke 14's systemd half |

## Open decisions to resolve at plan time

Values and shims only; no mechanism is left unpinned.

- The exact `ucred` field order confirmed against the libc crate's definition (probed working
  here; we still declare locally).

## Change register and cross-spec edges

| ID | Change | Discharges | Depends on |
|---|---|---|---|
| C1 | UDS listener transport; socketpair+inherited-fd rejected | arch-03/04/15; §9 P2 row | E1 FSM (binding) |
| C2 | Socket naming, `runtime_dir()`, `instance_name()` seam, `SUN_LEN` guard, unconditional unlink | arch-03 | C11 (`RuntimeDirectoryMode`), **C16** |
| C3 | `SO_PEERCRED` via local `ucred` extern, feeding the existing `accept_client` | SEC (forge-proof heartbeat) | C1, C4 |
| C4 | `pipe.rs` `#[cfg(unix)] serve` | arch-03/04/15; TEL breadcrumb doctrine | C1, C2, C3 |
| C5 | `spawn_main` + waiter thread owning the `Child`; sink holds `Option<pidfd>`; `--config` forwarding | arch-12, arch-05; **P2-G G1** | C2, C6, C7, C12, C16 |
| C6 | `kill_and_wait` Unix body, bounded; degraded `kill(pid)` arm | E2 kill/drain ordering; TEL-10 | C5 |
| C7 | `128 + signo`, never-86, `-2` sentinel | arch-05, arch-12 | C5 |
| C8 | `heartbeat.rs` Linux client | arch-03 | C1, C2 |
| C9 | Launcher `credential_acl.rs`; P2-A's uid-check condition transferred to P2-G | SEC-09 | P2-A (precedent), C11 |
| C10 | `kiosk.service → cage -- kiosk-launcher → kiosk-main`; two shapes rejected | arch-05; §3.1; §7.2 | C11; P2-G (seat/DRM, install) |
| C11 | systemd unit **shape** (8 directives + `[Install]`; `StartLimit*` in `[Unit]`) | arch-05 verbatim | C10; values/install = P2-G |
| C12 | Orphan-kill parity: cgroup enforcement + `INVOCATION_ID` detection | arch-05; C3 honest parity | C11 (hard); gate = P2-G G15 |
| C13 | Single-instance parity: unit identity + `File::try_lock` backstop | C3 honest parity | C11, C16 |
| C14 | RT-13 cross-platform, per-PR Linux CI | §10 RT-13; §10 "functional at P2" | C1–C8 |
| C15 | Smoke 13–15 (`cage -v`, X11/`xdotool` driver, C17 variant) | merge gates; §7.2 | C10, C11, C17; runner = P2-F |
| C16 | Launcher `resolve_data_dir()` → `/var/lib/kiosk` | parent §4:409; TEL-10 drain co-location | **hard co-landing with P2-A** |
| C17 | JS-ping webview round-trip on the heartbeat | **arch-04 / RT-02 / OD-1** (parent §3.1:133-141) | C8; shared `webkit2gtk` declaration |

**Edges, both directions:** P2-A → C16 (`/var/lib/kiosk` must match kiosk-main's);
C5 → P2-G G1 (`--config`); C11 → P2-G G8 (the installed unit); C12 → P2-G G15 (orphan-kill
assertion); C10/C15 → P2-G G15 (`cage -v` floor); C12/C17 residual → P2-G H11 + runbook
recovery step; C14 → P2-F (RT-13 in CI); C15 → P2-F F5 (cage/xwayland/xdotool in the nightly
container, and the scenario→compositor map). The `webkit2gtk` declaration is **shared** with
P2-B and P2-D — union of features, first writer wins, **no ordering edge**.

## Scope / defer

Unit file installation, `[Install]`, seat/DRM/seatd or logind session wiring, start-limit
numbers, the cage floor-version assertion, the orphan-kill gate, H11, and the OS image that
runs all of this → P2-G. Idle/gesture → P2-D. Nightly harness and the compositor map → P2-F.
The `zbus` ponytail from P2-B remains parked: nothing here needed D-Bus either —
`SO_PEERCRED`, `INVOCATION_ID` and `systemd-inhibit` kept both crates dependency-free. New
`unsafe` surface introduced by this spec is three bounded FFI sites: `SO_PEERCRED`,
`syscall(434)`/`syscall(424)`, and `kill(2)`.
