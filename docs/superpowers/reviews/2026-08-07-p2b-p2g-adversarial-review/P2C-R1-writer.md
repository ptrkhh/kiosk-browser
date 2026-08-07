# P2-C — WRITER, Round 1 (opening)

No frame dispute.

All in-session probes below were run on this box (`rustc 1.94.1`, `systemd 255 (255.4-1ubuntu8.14)`,
systemd **not** PID 1 → no transient units). Probe sources kept at
`scratchpad/probe/{waitid.rs,misc.rs,u/kiosk.service}`.

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| C1 | UDS listener transport (`std::os::unix::net::UnixListener`); socketpair+inherited-fd recorded as rejected | parent §3.1 arch-03/04/15 (heartbeat channel + channel-grace states); §9 P2 row | E1 FSM (binding); nothing in B/D/E/F/G |
| C2 | Socket path/naming: `/run/kiosk/hb-<launcher-pid>.sock`, pure derivation fn, `instance_name()` cfg seam, `SUN_LEN` bound, bind-collision policy | arch-03; parity with `pipe.rs:39-44` stale-instance discipline | C1; C11 (`RuntimeDirectory`/`RuntimeDirectoryMode`); P2-G installs values |
| C3 | `SO_PEERCRED` peer verification via local `getsockopt`/`ucred` extern, feeding the **existing two-valued** `accept_client` | SEC (forge-proof heartbeat); parity with `GetNamedPipeClientProcessId` | C1, C4 |
| C4 | `pipe.rs` — `#[cfg(unix)] serve`: accept loop, byte-for-byte mapping table, fault/reconnect latch, bind-failure degradation | arch-03/04/15; TEL breadcrumb doctrine | C1, C2, C3 |
| C5 | **NEW** `spawn.rs` Linux `spawn_main` + waiter thread on `waitid(P_PID, …, WEXITED\|WNOWAIT)`; sole-reaper rule. *Replaces the withdrawn "plain `child.wait()`" claim.* | arch-12 (backoff needs one exit event per spawn); arch-05 (exit 86) | C2 (env var), C6, C7, C12 |
| C6 | `kill_and_wait` — Unix body gains the bounded wait it does not have today | E2 kill/drain ordering; TEL-10 | C5 (reaper identity) |
| C7 | Signal-death → `ChildExited{code}` mapping; never-86 invariant; encoding **pinned now** to `128 + signo` | arch-05 (86 is reserved), arch-12, Q3 | C5 |
| C8 | `heartbeat.rs` Linux client (`tokio::net::UnixStream`) | arch-03; parent §9 P2 row | C1, C2 |
| C9 | Launcher-crate `credential_acl.rs` Unix impl + A's C12 hand-forward discharged (no `User=` ⇒ root ⇒ mode bits suffice) | SEC-09 (§4, §8) | P2-A §C12 (precedent); C11 (unit shape decides the service user); P2-G if it adds `User=` |
| C10 | Process shape `kiosk.service → cage -- kiosk-launcher → kiosk-main`; two rejected shapes recorded | arch-05, §3.1 process model; §7.2 Linux locked session | C11; P2-G (seat/DRM); P2-B (hardening runs inside this shape) |
| C11 | systemd unit **shape**: `Type/ExecStart/Restart/RestartPreventExitStatus/SuccessExitStatus/RuntimeDirectory/RuntimeDirectoryMode/KillMode` | arch-05 verbatim (parent §3.1:172-175); orphan-kill (C12); single-instance (C13) | C10; values+install = P2-G |
| C12 | **NEW** Orphan-kill parity for `job.rs`'s Unix no-op: the systemd cgroup is the mechanism; divergence stated in both directions | arch-05 / C3 honest parity; closes the U3 gap | C11, C10; gate at P2-G image validation |
| C13 | **NEW** Single-instance parity for `job::acquire_single_instance`'s Unix no-op: unit identity + a stdlib `File::try_lock` on the runtime dir | C3 honest parity; closes the U4 gap | C11, C2 |
| C14 | RT-13 cross-platform: transport-name seam (per-scenario tempdir), mock-main std-UDS client, un-gate, CI budget | §10 RT-13; §10 "Linux compile check → functional at P2"; C9 merge gate | C1–C8; C15 independent |
| C15 | Smoke 13–15; the cage exit-code claim converted from prose to an executable assertion | C9 merge gates; §7.2 deployment gate | C10, C11; extends A's harness with a stated compositor swap |

---

## C1 — UDS listener transport

**Proposal.** The launcher binds a `std::os::unix::net::UnixListener`; the accept loop replaces
`CreateNamedPipeW`/`ConnectNamedPipe`. Nothing else about the channel changes: same
`kiosk_core::ipc` frames, same `'\n'` framing, same `Event` mapping, same `child_pid` contract.

**Requirement.** arch-03 (READY handshake), arch-15 (liveness disambiguation) and the E1
channel-grace states depend on a channel that can *break and be re-established*.

**Evidence.**
- Tier 3: the only producer of `ChannelReconnected` in the tree is `pipe.rs:455`, inside the
  post-accept reader loop, latched at `pipe.rs:479` and cleared at `pipe.rs:445`. A transport with
  no accept has no producer. (Verifier row 24, re-checked.)
- Tier 3: `watchdog.rs:257-260` (fault sets grace, emits nothing), `:212-224` (grace expiry →
  `restart(0, now, "channel")`), `:262-265` (`ChannelReconnected` clears grace + logs
  `ChannelReset`). So amputating reconnect does not merely lose a log line, it converts every
  transient channel loss into a 30 s-delayed restart with cause `channel`.
- Tier 3, **and I adopt the verifier's second nuance as strengthening the record** (row 25): with a
  socketpair, `ChannelFault` is itself near-unreachable — `pipe.rs:474` gates the fault on
  `child_pid == expected`, and a socketpair only EOFs when the child dies (PID already 0) or
  deliberately closes its fd. So socketpair does not "degenerate fault→restart", it deletes *both*
  channel states and leaves only `hang`. That is a worse trade than the spec recorded, not a better
  one. The rejection record is amended to say this.
- Tier 3: launcher has no async runtime — `crates/kiosk-launcher/Cargo.toml` `[dependencies]` is
  `kiosk-core` + `serde_json`; native deps are `[target.'cfg(windows)'.dependencies]` only. So
  blocking std is the only option, and `serve` is already blocking-on-caller-thread (`main.rs:239`).

**Dependencies.** E1 FSM (binding precedent, not modified). None on siblings.

---

## C2 — Socket naming, path derivation, and the `instance_name()` seam

**Proposal.**
1. `pipe::instance_name()` gains a `cfg` seam: `#[cfg(windows)]` keeps
   `format!("{PIPE_NAME}-{pid}")`; `#[cfg(unix)]` returns
   `<runtime_dir>/hb-<launcher-pid>.sock`. **`main.rs:172`'s unconditional call site is
   unchanged** — that is the point of putting the seam inside the function rather than at the call.
2. `runtime_dir()` is the pure, host-tested function: `/run/kiosk` when it is a directory, else the
   data dir (dev run without systemd).
3. The pure function returns `io::Result` and **rejects a derived path ≥ 108 bytes** rather than
   handing it to `bind()`. The host test asserts the *bind*, not only the derivation.
4. **No unconditional unlink-before-bind.** On `AddrInUse`: probe with `UnixStream::connect`. Connect
   succeeds ⇒ a live peer owns the name ⇒ loud-fail down C4's degradation path. Connect fails
   `ECONNREFUSED` ⇒ corpse ⇒ unlink and rebind once.

**Requirement.** arch-03; parity with the stale-instance discipline at `pipe.rs:39-44`.

**Evidence.**
- Tier 3, corrected citation: `pipe.rs:39-44` is the stale-instance prose; `pipe.rs:53` is
  `PIPE_NAME`; `pipe.rs:55-60` is `instance_name()`. (Verifier row 1 conceded.)
- Tier 3: `main.rs:172` `let pipe_name = pipe::instance_name();` — unconditional, every platform,
  feeding both `LauncherSink::new` and the pipe thread. A Linux launcher publishing
  `\\.\pipe\kiosk-heartbeat-<pid>` as `KIOSK_HEARTBEAT_PIPE` hands the child an unopenable path.
  (U7 conceded; the seam is the fix.)
- **In-session probe** (`probe/misc.rs`): `UnixListener::bind` succeeds at path length 107, fails at
  108 with `"path must be shorter than SUN_LEN"`. std raises a clean `io::Error` — it does **not**
  truncate. `/run/kiosk/hb-<pid>.sock` measures 24 bytes; `TMPDIR=/tmp`, so a
  `tempfile::tempdir()` path plus `hb-<pid>.sock` is ~40 bytes. Both are far inside the bound.
  (U5: the bound is real, the failure is loud not silent, and the check costs one comparison.)
- Tier 5 + tier 3: `RuntimeDirectory=kiosk` wipes `/run/kiosk` on unit stop. I adopt the verifier's
  row-40 nuance: because the name is already per-PID, the wipe only matters after PID reuse across
  a restart — the same point `pipe.rs:41-44` makes. The wipe is therefore belt-and-braces, and the
  corpse-probe in (4) is what actually covers the dev-run path.
- Rationale for (4): `FILE_FLAG_FIRST_PIPE_INSTANCE` (`pipe.rs:100-104`) deliberately chose
  *loud permanent failure* over *silently becoming a second instance of a squatter's object*.
  Unconditional unlink-before-bind inverts that choice into silent theft. C3 (honest parity)
  requires I not do that silently, and the corpse-probe keeps the loud property where it matters
  while still self-healing the only case that actually occurs.

**Dependencies.** C1; C11 for `RuntimeDirectory` + `RuntimeDirectoryMode=0700`; P2-G owns installation.

---

## C3 — `SO_PEERCRED` peer verification

**Proposal.** Per-accept `getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &mut ucred, &mut len)` via a local
extern + a 3×`u32` `#[repr(C)]` `Ucred`. The resulting `Option<u32>` is fed to the **existing,
unmodified** `accept_client(client, expected, current)` (`pipe.rs:84-89`) — snapshot **or** current,
fail-closed otherwise.

**Requirement.** Forge-proof heartbeats (`pipe.rs:26-27`, `pipe.rs:332-336`).

**Evidence.**
- Tier 3 (verifier row 29, re-run): `UnixStream::peer_cred()` is unstable
  (`E0658 … 'peer_credentials_unix_socket'`, issue #42839). No stable-std path exists.
- **In-session probe** (`probe/misc.rs`): the local 3×`u32` extern returns the connecting process's
  PID correctly (`SO_PEERCRED peer pid = Some(12890)`, matching the probe process). The layout risk
  the verifier flagged is real for large structs; `struct ucred` is three `u32`s and is now
  measured, not assumed.
- I adopt the verifier's row-29 caveat verbatim into the spec text: the `spawn.rs:12-14` analogy is
  "the platform C library is already linked into every Rust binary on this target", which is true of
  libc on both `gnu` and `musl` — not "kernel32 is special".
- **U6 conceded in full.** The spec's phrase "per-accept `SO_PEERCRED` gate against `child_pid`"
  reads as a one-valued check. `pipe.rs:76-81` documents why one-valued is a bug: with
  `backoff_s > 2` the pre-accept snapshot is 0 (`await_child_pid` gives up at ~2 s), so
  snapshot-only rejects the legitimate new child and cries impostor on **every** normal restart.
  `pipe.rs:556-566` is the regression test for exactly that. The revision: C reuses
  `accept_client` unchanged — it is already the platform-free seam, so C1's "reimplement no decision
  logic" is satisfied by *calling* it, and the Linux `serve` keeps `await_child_pid` and the
  post-accept `expected = client.unwrap_or(expected)` re-derivation (`pipe.rs:441`) too.

**Divergence, declared.** `SO_PEERCRED`/`struct ucred` is Linux (and Android) ABI; macOS uses
`LOCAL_PEERPID`. The Unix bodies are `#[cfg(unix)]`, so a macOS build of the launcher would fail to
link this extern. Verified: `.github/workflows/ci.yml` has exactly three jobs — `lint-test`
(ubuntu-22.04), `build-windows` (windows-latest), `build-linux` (ubuntu-22.04). macOS is built
nowhere. Recorded as a `ponytail:` with the one-line upgrade if a macOS dev host ever appears.

**Dependencies.** C1, C4.

---

## C4 — `pipe.rs::serve`, Unix body

**Proposal.** Replace (not add beside) the `#[cfg(not(windows))]` stub at `pipe.rs:519-528` with a
`#[cfg(unix)]` `serve` of the identical signature. Structure mirrors `pipe.rs:366-491` one-for-one:
`bind` ↔ `create_pipe`; `await_child_pid` unchanged; `listener.accept()` ↔ `connect_pipe`;
`accept_client` unchanged; `BufReader::read_line` ↔ `LineReader::next_line` with the same
`MAX_LINE_BYTES` cap and the same silent-drop on `decode` error; `Err`-while-`child_pid == expected`
→ `ChannelFault` + latch; re-accept + first frame → `ChannelReconnected` before that frame's own
event; `logged_failure` / `logged_impostor` once-per-streak latches unchanged.

**Requirement.** arch-03/04/15; the E1 event vocabulary.

**Evidence.** Tier 3: mapping table verified at `pipe.rs:65-71`, `:84-89`, `:441-446`, `:450-486`.
`Event` variants exist at `watchdog.rs:29-35`.

**Bind/serve failure — row 43 conceded, deferral withdrawn.** The spec said this "mirrors whatever
E2's Windows serve-failure path does (confirm at plan time)". The verifier is right that the E2 spec
has no such paragraph (`p1e2:105-107` covers spawn failure and orphan-drain failure only). The
behaviour is in code and I now cite it directly, so there is nothing left to confirm at plan time:
`pipe.rs:370-388` — once-per-streak `eprintln!` + `crate::sink::breadcrumb(data_dir, "pipe", …)` +
`sleep_retry()` (100 ms). The `ponytail:` note at `pipe.rs:373-380` calling a squatted name "the only
silent-forever degraded path on the device" carries over verbatim, which is a second reason C2(4)
does not silently steal a live name: the breadcrumb is the operator's only signal, and stealing
would suppress it.

**Shutdown caveat — open decision 3 answered, not deferred.** `UnixListener::accept()` blocks
exactly as `ConnectNamedPipe` does, and `cancel` is checked only between blocking calls. That is the
same shape `pipe.rs:322-330` already documents ("nothing unblocks it … the same shape as
`spawn_main`'s child-waiter thread"), and `main.rs:249-253` already states the launcher relies on
`process::exit` to tear threads down. No poll/timeout is added: adding one would be new mechanism to
fix a caveat the Windows path already lives with, and RT-13 already handles it by not joining the
server thread (`rt13.rs:673-679`). Mirror, don't invent — the mirror is "same caveat, same doc".

**Dependencies.** C1, C2, C3.

---

## C5 — `spawn.rs`: the real waiter design (replaces the withdrawn claim)

**The verifier is right and the spec's sentence is withdrawn.** "the waiter thread is plain
`child.wait()` (std exposes the exit status portably)" is FALSE as a drop-in, for exactly the reason
`spawn.rs:89-95` gives: `Child::wait` takes `&mut self`, and the caller keeps the `Child`
(`sink.rs:423 self.child = Some(child)`, consumed by `sink.rs:377-381 kill_child` →
`spawn::kill_and_wait` and by `sink.rs:406-407 job.assign(&child)`). `std::process::Child` exposes no
unix duplication. Two threads independently `wait()`ing is not merely unsupported, it is a
**PID-reuse kill hazard**: whichever reaps first frees the PID, and the other's later
`kill(pid, SIGKILL)` can land on an unrelated process. So this is a new change with its own evidence.

**Proposal — the sole-reaper rule.**
> Exactly one owner reaps: `LauncherSink`, via its retained `Child`. The waiter thread **observes
> without reaping**.

Concretely, `#[cfg(unix)] spawn_main` keeps the existing signature and returns the live `Child` to
the sink unchanged. It captures `child.id()` and spawns the waiter thread, which blocks in

```
waitid(P_PID, pid, &mut siginfo, WEXITED | WNOWAIT)
```

reads `si_code` / `si_status`, maps them per C7 and sends the one `Event::ChildExited`.
`WNOWAIT` leaves the child in `Z` state, so the PID is pinned (not reusable) and the sink's
`kill_and_wait` remains the reaper. This is *ownership-identical to Windows*: the waiter observes a
handle it alone owns, the caller's `Child` is untouched, and the sink's kill-then-wait sequence
(`sink.rs:461-481`'s "on the `exit` path this kills an already-exited `Child` — a no-op") behaves the
same way it does today.

**Evidence — in-session probe** (`probe/waitid.rs`, local `waitid` extern + `#[repr(C)]` siginfo
prefix with a `const _: () = assert!(size_of == 128)`):

| child | `waitid` result | state after `WNOWAIT` | `Child::kill` after exit | `Child::wait` | `/proc/<pid>` after reap |
|---|---|---|---|---|---|
| `exit 86` | `si_code=1` (CLD_EXITED) `si_status=86` | `Z` | `Ok` | `code=Some(86)` | gone |
| `exit 0` | `si_code=1` `si_status=0` | `Z` | `Ok` | `code=Some(0)` | gone |
| `kill -9 $$` | `si_code=2` (CLD_KILLED) `si_status=9` | `Z` | `Ok` | `code=None` `raw=unix_wait_status(9)` | gone |
| `kill -11 $$` | `si_code=2` `si_status=11` | `Z` | `Ok` | `code=None` | gone |

So: the status is observable without reaping; the sink's `Child::kill()` on an
already-exited-unreaped child is a harmless `Ok` (no reuse hazard, because the zombie holds the PID);
and the reap is complete (`/proc/<pid>` gone) — **U2 discharged and smoke 15's "no zombie" gets a
named owner: `LauncherSink::kill_child`, on every path.**

Zombie window is bounded by the FSM, not by hope: `watchdog.rs:194-199` routes `ChildExited` to
either `ExitLauncher{86}` (→ `sink.rs:483-485 kill_child`) or `restart(...)`, and
`watchdog.rs:126-127` puts `Action::DrainOrphanedSpool` **first** in every restart's action vector
(→ `sink.rs:471 kill_child`). Both reap in the same FSM turn, before any backoff sleep.

**Alternatives considered and rejected (Q2).**
- `pidfd_open(2)` + `poll` + `waitid(P_PIDFD, …)`: same ownership, three externs instead of one, and
  a container-seccomp dependency on syscall 434. `P_PID` is safe here precisely because the zombie
  pins the PID, so the pidfd's only advantage is unneeded.
- Moving the `Child` into the waiter and giving the sink a bare PID: zero externs, but changes
  `spawn_main`'s return type per platform and forces `sink.rs` to grow a `cfg` seam — a
  cross-platform change to shared supervise code, which C8 sets a higher bar for and Q4 penalises.
- Adding the `libc` crate: it is already in `Cargo.lock` (v0.2.186, pulled by `tokio`, `glib`,
  `gtk`, `wry`, `rustix`, `socket2` …), so it costs no new supply chain. I am **not** proposing it,
  on C6 grounds and to keep one convention across `ucred` and `siginfo`, but I record it as the
  cheap escape hatch if the local `siginfo` prefix ever looks like a liability. The layout risk is
  mitigated the way `job.rs:110-111` already mitigates `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`: a
  compile-time `size_of` assertion, plus the probe above as the runtime proof.

**Dependencies.** C2 (the env var value), C6, C7, C12.

---

## C6 — `kill_and_wait`, Unix body

**Row 10 conceded.** The spec said the function "keeps its bounded-wait doctrine
(`spawn.rs:29-39`)". Verified: that doctrine and `KILL_WAIT_MS` are `#[cfg(windows)]`-only
(`spawn.rs:38-39`), and the existing Unix body is `spawn.rs:63-67`:
`let _ = child.kill(); let _ = child.wait();` — **unbounded**. The bound must be *introduced*, and
that body replaced.

**Proposal.** `#[cfg(unix)] kill_and_wait`: `child.kill()` (SIGKILL) then `try_wait()` polled at
20 ms up to a `KILL_WAIT_MS`-equivalent 5 s ceiling; on ceiling expiry, give up and proceed, same
degradation the Windows doc records.

**Requirement.** The wait is load-bearing for one reason on Linux and not the other, and C3 requires
me to say which:
- **Does not apply on Linux:** the `ERROR_SHARING_VIOLATION` rationale (`spawn.rs:44-49`,
  `sink.rs:368-374`). POSIX `rename(2)` does not care about open descriptors, so the orphan-spool
  rename cannot race a live writer's open file the way it does on Windows.
- **Does apply on Linux:** `sink.rs:374-376` — "Waiting also closes the `ChannelFault`-after-kill
  race: the pipe reader's read error can no longer beat the `child_pid.store(0)` that follows."
  That is transport- and platform-independent, and `pipe.rs:467-481` is still gated on
  `child_pid == expected`. So the bound stays, on the second rationale alone.

**Dependencies.** C5 (this is the sole reaper).

---

## C7 — Signal-death mapping; the never-86 invariant; encoding pinned

**Proposal.** Pure, host-tested `fn exit_code(si_code: i32, si_status: i32) -> i32`:
`CLD_EXITED (1)` → `si_status`; anything else → `128 + si_status`. Invariant: a signal death can
never map to 86.

**Requirement.** arch-05 (86 is reserved for the technician exit; `watchdog.rs:196-198` returns
`ExitLauncher{86}` with no restart, and `RestartPreventExitStatus=86` stops systemd on top of it).
The OOM-killer case is real on a kiosk, and a SIGKILLed main reading as a technician exit would
leave a dead device that systemd deliberately refuses to restart.

**Evidence / why the encoding is pinned now rather than at plan time.** The spec deferred
"negative-signal vs sentinel". Verified reason not to defer: `-signo` **collides with the existing
`-1` sentinel** — `sink.rs:434-437` feeds a synthetic `ChildExited{code: -1}` on every `spawn_main`
`Err`, and `spawn.rs:100-109` makes that a contract. SIGHUP is signal 1, so `-signo` would render a
SIGHUP death and a spawn failure identically in `watchdog.restart`'s `code` field — a Q3 silent
failure in the one diagnostic an operator has. `128 + signo` yields 129…192 for signals 1…64:
never 86, never −1, and 137 is the universally recognised SIGKILL/OOM code, which is the greppability
the spec asked for. Probe confirms the inputs are exactly `si_code ∈ {1,2}` and `si_status ∈
{exit code, signal number}`.

Collision check against kiosk-main's own codes: 0, 86, and 101 (Rust panic). No overlap with
129…192.

**Dependencies.** C5.

---

## C8 — `heartbeat.rs`, the Linux client

**Proposal.** Replace the `#[cfg(not(windows))]` stub at `heartbeat.rs:152-155` with a `#[cfg(unix)]`
body: `tokio::net::UnixStream::connect(&path)` in place of `ClientOptions::new()…open()`
(`heartbeat.rs:68-72`); everything else — the `ready_reached` latch, the `logged_failure`
once-per-streak latch, the cancel-wrapped `write_all`s, `tokio::time::interval` with
`MissedTickBehavior::Delay`, `RECONNECT_BACKOFF`, `sleep_or_cancel` — is copied unchanged.
`pipe_name_from_env()` (`heartbeat.rs:37-39`) is already platform-free; the client derives nothing.

**Requirement.** arch-03; parent §9 P2 row.

**Evidence.**
- Tier 3, corrected citation (row 14 conceded): the Windows client is `heartbeat.rs:41-147`
  (`run` 42-137, `sleep_or_cancel` 141-147); `149-151` is the stub's doc comment.
- Tier 3: `crates/kiosk-main/Cargo.toml` `tokio` features include `net` and `io-util`;
  `tokio::net::UnixStream` needs `net` + `cfg(unix)`, `AsyncWriteExt` needs `io-util`. **No Cargo
  change.**
- Tier 3: `RECONNECT_BACKOFF` = 1 s (`heartbeat.rs:31-34`), documented as well under the FSM's 15 s
  miss window; `PING_INTERVAL_S = 5` (`kiosk-core/src/ipc.rs:3`); degradation doctrine
  `heartbeat.rs:16-19`.

**Divergence, declared (it makes the client simpler, not looser).** The Windows comment
`heartbeat.rs:62-67` enumerates `ERROR_PIPE_BUSY` / `ERROR_FILE_NOT_FOUND` because the pipe name
transiently ceases to exist between reconnects (`pipe.rs:46-52`). On Linux the socket **file**
persists across the listener's accept cycle, so the reconnect-gap errors are `ENOENT` (before first
bind) and `ECONNREFUSED` (file present, no listener). The existing arm already retries *any* open
error, so the code is unchanged; only the comment is rewritten to name the two Linux errnos.

**Dependencies.** C1, C2.

---

## C9 — Launcher-crate `credential_acl.rs` (SEC-09), and A's C12 hand-forward

**Proposal.** Replace `crates/kiosk-launcher/src/credential_acl.rs:100-104` (`#[cfg(not(windows))]`
→ `Ok(true)`) with the `#[cfg(unix)]` mode-bits body from A's C12, and rewrite the doc comment in the
same edit. **Replace, do not add beside** — `#[cfg(unix)]` and `#[cfg(not(windows))]` both match on
Linux (`p2a:263-265`).

**Requirement.** SEC-09 (parent §4, §8); C5 (fail-closed security gate).

**Evidence.**
- Tier 3: the stub is verbatim fail-open at `credential_acl.rs:100-104`.
- Tier 3: `is_violation` exists in the **launcher** crate at `credential_acl.rs:24-26`
  (`!matches!(check, Ok(true))`), so `Err` is already a violation — fail-closed comes free.
- Tier 3, **citation corrected (row 17 conceded)**: there is no `boot.rs` in kiosk-launcher. The
  cross-crate reference is `crates/kiosk-main/src/boot.rs:153-156`; the launcher's own gate is
  `sink.rs:73` `fn build_telemetry` (private) with the check-before-read at `sink.rs:84-86`.
- Tier 2 caveat adopted (row 19): kiosk-main's stub is still `Ok(true)` in the tree today
  (`kiosk-main/src/credential_acl.rs:101-104`) — A's C12 is designed, not merged. The spec's "one
  crate was fixed in A" is rewritten to "one crate is A's to fix".

**U8 — A's hand-forward, discharged explicitly.** `p2a:285-286` closes C12 with "`ponytail:` mode
bits only, no uid check — a root-owned `0o600` file is the deployment shape; add an owner check if a
non-root service user lands in P2-C." The verifier is right that C introduces the service shape and
then said nothing. The re-derivation, stated:
> The C11 unit shape declares **no `User=`**, so the launcher, cage and kiosk-main all run as root
> and the credential is root-owned `0o600`. Mode bits alone are therefore still sufficient and the
> owner check is still not needed. **The condition A attached is transferred, not dropped:** if
> P2-G's seat/DRM wiring introduces a non-root `User=`, the uid check lands with it, as a named row
> in P2-G's hand-off list — because at that point the credential's owner and the reader's uid can
> differ and mode bits stop proving anything.

**Dependencies.** P2-A §C12 (precedent); C11; conditional hand-forward to P2-G.

---

## C10 — Process shape: `kiosk.service → cage -- kiosk-launcher → kiosk-main`

**Proposal.** Unchanged from the draft. kiosk-main stays the launcher's direct child.

**Requirement.** arch-05 (exit-86 must survive to systemd); §3.1 process model; §7.2 locked session.

**Evidence + amendment.** The claim "exit codes, `ChildExited`, kill semantics … see exactly what
Windows sees" was too broad and I narrow it: **exit codes and `ChildExited` see exactly what Windows
sees** (C5, C7 pin that). **Kill semantics do not** — see C12. Rejected shapes stand as recorded
(launcher-spawns-`cage -- kiosk-main` launders main's exit code through cage and flashes the
compositor on every restart; a two-unit `BindsTo` split puts two supervisors in one failure domain).

**Row 41 — the cage exit-code pin is downgraded to a declared assumption and converted into a gate.**
`cage` is not installed here (`command -v cage` → empty), so I cannot re-verify the in-session
measurement and I will not argue it as evidence. Revision, adopting the verifier's own three
remedies:
- Declare it: *assumption* — "cage propagates its child's exit status unchanged, and tolerates a
  child that never connects as a Wayland client."
- Pin the version: the measurement is recorded against **cage 0.1.4** (the Debian 12 package), and
  the spec now says so, because propagation behaviour is version-dependent.
- Gate it: smoke 13 gains `cage -- sh -c 'exit 86'; test $? -eq 86` under `WLR_BACKENDS=headless`
  (C15). Residual risk if it fails: the fallback shape is `ExecStart=cage -- …` replaced by a
  two-line wrapper that `exec`s the launcher and whose exit status systemd sees directly — but that
  is P2-G's to build, and the gate fires long before an image exists.

**Dependencies.** C11; P2-G (seat/DRM, install); P2-B runs its hardening inside this shape.

---

## C11 — systemd unit **shape**

**Proposal.**

```ini
[Service]
Type=simple
ExecStart=cage -- /usr/libexec/kiosk/kiosk-launcher
Restart=always
RestartPreventExitStatus=86
SuccessExitStatus=86
RuntimeDirectory=kiosk
RuntimeDirectoryMode=0700
KillMode=control-group
```

**U10 / row 39 conceded — `SuccessExitStatus=86` is restored.** Parent §3.1:172-175 names all three
in one sentence: "systemd `Restart=always` with `RestartPreventExitStatus=86` **and
`SuccessExitStatus=86`**". Since the spec's own framing is "values and installation in P2-G; the
*shape* is this spec's", the set of directives **is** C's claim and omitting one the parent names is
a contradiction, not a value-level detail I can hand to G. Behavioural consequence, stated: without
it `RestartPreventExitStatus` still suppresses the restart, but the unit lands in `failed` with
`status=86` — so `systemctl is-failed` and every dashboard built on it report a healthy technician
exit as a device fault. That is precisely the Q3 silent/misleading-signal class.

**Two directives added beyond the draft, both load-bearing for changes in this spec:**
- `RuntimeDirectoryMode=0700` — `RuntimeDirectory` defaults to 0755, which would leave
  `/run/kiosk/hb-<pid>.sock` connectable by any local user. `SO_PEERCRED` (C3) still refuses them, so
  there is no forgery, but a loop of connect-and-reject is an accept-starvation channel of exactly
  the kind `pipe.rs:390-395` (Finding 1) exists to prevent on Windows. One directive closes it at the
  directory.
- `KillMode=control-group` — systemd's default, made explicit because C12 now depends on it.

**Evidence.** Tier 5 (documentary) for runtime semantics, plus an **in-session structural check**:
`systemd-analyze verify` on the block above under systemd 255 exits 0, and the same file with
`KillMode` misspelled reports `Unknown key name 'KillModeXX' in section 'Service'` — so the tool is
really checking and all seven directives parse. Runtime semantics (that a unit exiting 86 ends
`inactive` rather than `failed`) remain tier 5: systemd here is not PID 1
(`systemctl is-system-running` → `offline`; `/proc/1/comm` is not systemd), so `systemd-run --wait`
cannot run. **Declared assumption**, pinned by P2-G image validation, which C already nominates for
the systemd half of smoke 14.

**Dependencies.** C10; C12; C13; values + install = P2-G.

---

## C12 — **NEW**: orphan-kill parity (the Job Object gap)

**The verifier is right that this is a gap the spec never names.** Established from the code:
`job.rs:217-225` is the whole Unix implementation —
`fn create() -> Ok(Job)` and `fn assign(&self, _child) -> Ok(())`, both no-ops against a unit struct
`pub struct Job;` (`job.rs:146-147`). `main.rs:189-199` therefore always gets `Some(job)` on Linux
and `sink.rs:406-419` always "succeeds", printing nothing. So **today, on Linux, a launcher killed
with `SIGKILL` leaves `kiosk-main` running full-screen and unsupervised** — the exact field failure
`job.rs:4-16` was written to close. The spec's "kill semantics … see exactly what Windows sees" is
false in the *looser* direction and C3 makes silent looseness a defect. C10 is amended accordingly.

**Position: this is C's to close, and it closes with a directive, not with code.**

The Job Object exists on Windows because there is no supervisor above the launcher — parent §3.1
says Windows uses "a boot/logon trigger with no restart-on-exit setting (the launcher owns
crash-restart, not the OS task)". On Linux there **is** one, and it is already in C11: the unit's
cgroup. Under `KillMode=control-group` (the default), when the service stops — including the stop
step that precedes every `Restart=always` restart — systemd signals **every** process remaining in
the unit's cgroup, then `SIGKILL`s the survivors. A launcher death makes cage exit, which makes the
unit's main process exit, which runs that stop step. So the orphan is killed by the cgroup teardown
before the successor launcher starts.

Why this over the alternatives (Q2):
- `PR_SET_PDEATHSIG` in `Command::pre_exec`: rejected. Two defects. (a) It is delivered on the death
  of the spawning **thread**, not the process — the sink dispatches on the main thread today
  (`main.rs:248`) but nothing enforces that, and RT-13 runs the loop on a spawned thread
  (`rt13.rs:163-166`), so the invariant is silently fragile. (b) It kills only `kiosk-main`, not its
  descendants; `job.rs:131-134` explicitly claims "everything the child itself spawned, e.g.
  WebView2's process tree", and WebKitGTK's Network/Web processes are the Linux analogue. Weaker
  parity for strictly more code.
- A launcher-owned cgroup: reimplements what the unit already gives us. Rejected on Q2.

**The cgroup is in fact *stronger* than the Job Object** (it covers grandchildren without the
`AssignProcessToJobObject` failure modes `job.rs:196-198` documents), so this is a divergence in the
*stricter* direction, and C3 requires me to state that too.

**Divergence in the looser direction, declared:** a **non-systemd dev run** (`cargo run` on a
developer box, no unit, no cgroup) has no orphan-kill at all. Residual risk: a dev's stray
kiosk-main. Accepted; it is exactly the pre-P1-F1 status quo and it never reaches a device.

**Deliverables.** (1) `KillMode=control-group` explicit in C11's shape. (2) Rewrite `job.rs:144-147`
and `:217-225` doc comments: they currently say "Linux supervision is P2" and "no supervision
hardening off Windows (P2)" — after this spec they must say the guarantee is provided by the unit's
cgroup, and name the directive, so the next reader does not re-open this. (3) Gate: P2-G image
validation asserts `kill -9` of the launcher leaves no `kiosk-main` — named as a P2-G row here, in
the same place C already defers smoke 14's systemd half.

**Dependencies.** C11 (hard — without `KillMode` this change does not exist); C10; gate owned by P2-G.

---

## C13 — **NEW**: single-instance parity

**U4 conceded.** `job.rs:283-288` `#[cfg(not(windows))] acquire_single_instance() -> Ok(Some(SingleInstance))`
— "every process is 'the one'". `sink.rs:298`'s claim that double-launch is "now prevented upstream,
by `job::acquire_single_instance` in `main`" is Windows-only. Under `Restart=always`, a wedged
predecessor and its successor can both run; the per-PID socket name (C2) means they will not
*collide*, which hides the condition rather than preventing it — two launchers each supervising a
kiosk-main, two webviews on one display.

**Position: mostly discharged by the unit, with a 5-line stdlib backstop.**
- Deployed shape: systemd guarantees one instance of `kiosk.service`. That is the primary mechanism
  and it needs no code.
- Backstop, because "systemd guarantees it" is exactly the kind of claim that is true until someone
  runs the binary by hand next to a live unit: `#[cfg(unix)] acquire_single_instance` takes an
  advisory lock on `<runtime_dir>/launcher.lock`.

**Evidence — in-session probe** (`probe/misc.rs`): `std::fs::File::try_lock()` is **stable** on
rustc 1.94.1 and a second open handle gets `Err(WouldBlock)`. So the entire Unix body is
`File::create(path)?` + `try_lock()`, mapping `WouldBlock` → `Ok(None)` (peer holds it — the
deliberate `exit(0)` arm `job.rs:243-246` documents) and any other error → `Err` (WARN + continue,
per `job.rs:18-25` never-block-boot). No extern, no dependency, no new file. The returned `File`
lives in `SingleInstance` for the process lifetime, exactly as the Windows `OwnedHandle` does
(`job.rs:227-233`).

Divergence, declared: `flock` is advisory and dies with the process; the Windows mutex is a kernel
object in the `Global\` namespace and sees a launcher in another logon session
(`job.rs:36-47`). On a single-seat kiosk with one unit that difference is not reachable.

**Dependencies.** C11, C2 (runtime dir).

---

## C14 — RT-13 cross-platform, as the CI gate

**Citations corrected (rows 33, 34 conceded).** The gate is `#![cfg(windows)]` at **`rt13.rs:27`**,
not `:32` (`:32` is a `use` of `std::sync::atomic`). `rt13.rs:101-112` contains no `cfg` at all — it
is the `unique_pipe` helper (doc `102-103`, fn `104-111`). The mock's gate is a different shape
again: `mock_main.rs:26-30` branches inside `fn main()`, with the real body at `mock_main.rs:33`
`#[cfg(windows)] fn windows_main()`.

**Row 36 conceded; the open decision is withdrawn and replaced with a design.** The spec said "tempdir
per test already implied by the tag scheme at `rt13.rs:107` — verify". Verified: false. The tag
scheme (`rt13.rs:104-111`) is `PID + AtomicU32 counter`, and its own doc (`rt13.rs:102-103`) says so.
Per-scenario tempdirs exist but are `config_dir`/`data_dir` (`rt13.rs:117-118`), and the pipe name is
deliberately not derived from them.

**The real design.** `unique_pipe(tag)` becomes `unique_transport(tag, dir: &Path)`:
- `#[cfg(windows)]` — unchanged, `rt13.rs:107`'s template, PID + counter.
- `#[cfg(unix)]` — `dir.join(format!("hb-{tag}-{}-{}.sock", pid, counter))`, where `dir` is the
  scenario's **existing** `data_dir` tempdir (`rt13.rs:118`), which `Harness::start` already owns and
  already drops at teardown. PID + counter is retained, so the uniqueness argument the Windows scheme
  makes is unchanged and cross-binary collisions in `/tmp` are impossible too.
- Length: probed at ~40 bytes for `/tmp/.tmpXXXXXX/hb-<tag>-<pid>-<n>.sock`, against the measured
  107-byte ceiling. C2's derivation guard applies here as well, so an exotic `TMPDIR` fails loudly at
  bind rather than at `ENAMETOOLONG` inside `serve`'s retry loop. **U5 discharged for the test paths
  too, which is where the verifier correctly said the risk actually lived.**

**U9 — CI affordability and parallelism, answered.**
- Parallelism safety no longer rests on the false row-36 claim: it rests on per-scenario tempdirs
  (already there) plus PID+counter (already there). No `--test-threads` restriction is needed, and
  none is imposed.
- Budget, from the code: four scenarios (`rt13.rs:291, 324, 359, 384`). The floor is set by
  `MISS_LIMIT_S = 15` (a hard-coded FSM constant, per `rt13.rs:46-49`) and `HEALTHY_OBSERVE = 20 s`
  (`rt13.rs:61`). Under cargo's default parallelism they overlap, so the added wall clock on
  `lint-test` is ~25-35 s, not 4×. If that ever wedges the job, the escape is scheduling RT-13 into
  `build-linux` rather than trimming it — the two scenarios that cost the time are the two that
  matter.
- **Mock-main claim withdrawn and corrected.** The spec said the mock gets "the same `UnixStream`
  branch as the real one". False: the launcher crate has no `tokio` (`kiosk-launcher/Cargo.toml`),
  and it never shared the Windows client code either — `mock_main.rs` uses `std::io::Write` on a
  plain handle, not `ClientOptions`. The Linux mock uses `std::os::unix::net::UnixStream` with the
  same `kiosk_core::ipc` frames and the same retry-the-open contract `mock_main.rs:8-13` states. The
  shared thing is the *protocol*, which is where it always was.
- `mock_main.rs:26-30`'s in-`main` branch is replaced by a `#[cfg(unix)] fn unix_main()` sibling of
  `windows_main`, so the `[[bin]]` gains a real Linux body rather than staying a stub the un-gated
  test would spawn and immediately lose.

**Evidence for the gate landing.** Tier 3: `.github/workflows/ci.yml:11-26` — job `lint-test`,
`runs-on: ubuntu-22.04`, running `cargo clippy --workspace --all-targets -- -D warnings` **and**
`cargo test --workspace`, on `pull_request`. Un-gating puts RT-13 in a per-PR Linux run with no
workflow edit. Note the clippy line: `--all-targets -D warnings` means the un-gated test and mock
must be warning-clean on Linux, which is part of this change, not a follow-up.

**Dependencies.** C1–C8 (it exercises all of them). Independent of C15.

---

## C15 — Smoke 13–15

**Proposal.** Scenarios as drafted, with three amendments.

13. **full chain** — prepended assertion, discharging C10's assumption as a gate rather than prose:
    `cage -- sh -c 'exit 86'; test $? -eq 86` under `WLR_BACKENDS=headless`, and record the
    `cage --version` the run measured. Then the chain as drafted.
14. **technician exit** — unchanged; systemd half explicitly P2-G's.
15. **hang path** — unchanged, and the "no zombie" assertion now has the named owner C5 supplies
    (`LauncherSink::kill_child`, reached from `DrainOrphanedSpool`).

**Row 42 conceded — the harness divergence is stated, not papered over.** The spec claimed these
"extend the A/B harness". A's harness is **weston headless** (`p2a:289`); `WLR_BACKENDS` is a
**wlroots** variable and weston does not read it. Scenario 13 therefore *swaps* the compositor for
cage. Corrected framing: scenarios 13-15 extend A's harness in fixtures and assertions, and
**deliberately replace the compositor**, because cage is the object under test in this spec —
asserting the cage contract under weston would assert nothing. A's weston harness remains correct
for A/B's scenarios 1-12. This is a divergence from A's stated harness, declared here rather than
presented as continuity.

**Non-collision.** Verified: A owns 1-7 (`p2a:326`), B owns 8-12 (`p2b:174`). 13-15 are free.

**Dependencies.** C10, C11.

---

## Response to the verification record

Silence is concession, so every row is dispositioned. **Rebuttals: zero.** The three FALSEs and all
ten undeclared assumptions are conceded, declared, or answered with a new change.

### FALSE (3)

| Row | Disposition | Revision |
|---|---|---|
| 31 `spawn.rs` "plain `child.wait()`" | **Concede in full** | Sentence and the "portable at last" heading withdrawn. Replaced by **C5** (waiter observes via `waitid(P_PID, WEXITED\|WNOWAIT)`; `LauncherSink` remains sole reaper), with an in-session probe. |
| 33 `#[cfg(windows)]` at `rt13.rs:32` | **Concede** | Gate is `#![cfg(windows)]` at `rt13.rs:27`; the mock's is `mock_main.rs:26-30` + `:33`, a different shape. **C14.** |
| 36 "tempdir per test already implied" | **Concede** | Open decision withdrawn. Real design in **C14**: `unique_transport(tag, data_dir)`, per-scenario tempdir + PID + counter, with C2's length guard. |

### DRIFT (7)

| Row | Disposition |
|---|---|
| 1 `pipe.rs:53-59` | Concede — correct citation `pipe.rs:39-44` + `55-60`. **C2.** |
| 10 `kill_and_wait` "keeps" its bound | Concede — the bound is `#[cfg(windows)]`-only (`spawn.rs:38-39`); the Unix body `spawn.rs:63-67` is unbounded and must be **replaced**, not kept. **C6**, with the Linux-specific re-derivation of *why* the wait is still needed. |
| 14 `heartbeat.rs:42-151` | Concede — `41-147`. **C8.** |
| 17 `boot.rs:153-156` | Concede — no `boot.rs` in kiosk-launcher; it is `crates/kiosk-main/src/boot.rs:153-156`, and the launcher's own gate is `sink.rs:73` + `:84-86`. **C9.** |
| 32 `try_wait` reaps / double-reap | Concede — this is the substance of **C5**'s sole-reaper rule and **C6**. |
| 34 `rt13.rs:101-112` | Concede — that range is `unique_pipe`, no `cfg`. **C14.** |
| 43 "mirrors E2's serve-failure path" | Concede — E2 has no such paragraph (`p1e2:105-107`). Deferral **withdrawn**; the behaviour is cited directly at `pipe.rs:370-388` + the `ponytail:` at `:373-380`. **C4.** |

### UNVERIFIABLE (2)

| Row | Disposition |
|---|---|
| 41 cage exit-code propagation | **Declared as an assumption.** Pinned three ways per **C10/C15**: version recorded (cage 0.1.4, Debian 12), converted into an executable assertion in smoke 13, systemd half at P2-G image validation. Residual risk + fallback shape named. |
| 42 `WLR_BACKENDS=headless` / harness continuity | **Concede the divergence.** **C15** states the compositor swap explicitly instead of claiming continuity with A's weston harness. |

### VERIFIED-with-contradiction (1)

| Row | Disposition |
|---|---|
| 39 `SuccessExitStatus=86` missing | **Concede.** Parent §3.1:172-175 names all three directives in one sentence; the shape is C's claim, so this is C's omission, not a P2-G value. Restored in **C11**, with the `failed`-state consequence stated. |

### Undeclared assumptions (10)

| # | Disposition |
|---|---|
| U1 waiter can `child.wait()` | **Concede** → **C5**, new design + probe. |
| U2 exactly one reaper | **Concede** → **C5** sole-reaper rule; probe shows `WNOWAIT` leaves `Z`, `Child::kill` after exit is `Ok`, `Child::wait` reaps, `/proc/<pid>` gone. Smoke 15's owner named. |
| U3 orphan-kill survives the port | **Concede the gap** → **C12** (new change). Established from `job.rs:217-225` that a Linux launcher death today leaves kiosk-main running; closed by `KillMode=control-group` + doc rewrite + a P2-G gate; `PR_SET_PDEATHSIG` and a hand-rolled cgroup both rejected with reasons. Both directions of divergence stated. |
| U4 single-instance survives | **Concede the gap** → **C13** (new change): unit identity + a stable-stdlib `File::try_lock` backstop (probed). |
| U5 `sun_path` 108 | **Concede** → **C2** length guard + **C14** applies it to the test paths. Probe: bind OK at 107, `Err` at 108, no truncation. |
| U6 one-valued peer check | **Concede** → **C3** reuses `accept_client(client, expected, current)` unchanged, keeping `await_child_pid` and the `pipe.rs:441` re-derivation. The verifier is right that a literal reading reintroduces a fixed bug (`pipe.rs:76-81`, test at `:556-566`). |
| U7 `instance_name()` seam | **Concede** → **C2**: the seam goes **inside** `instance_name()` so `main.rs:172`'s unconditional call site is untouched. |
| U8 A's C12 hand-forward | **Concede the silence; discharge the condition** → **C9**: no `User=` in the C11 shape ⇒ root ⇒ mode bits suffice; the owner check transfers to P2-G *conditional on* P2-G introducing a service user. |
| U9 RT-13 affordable + parallelism-safe | **Concede** → **C14**: containment restated on per-scenario tempdirs + PID + counter (not row 36); ~25-35 s added wall clock from `MISS_LIMIT_S=15` and `HEALTHY_OBSERVE=20`; the mock's "same `UnixStream` branch as the real one" claim withdrawn (launcher has no tokio; it never shared client code, only the protocol); `mock_main` gains a real `#[cfg(unix)] unix_main()`. |
| U10 `SuccessExitStatus` not C's | **Concede** → **C11**. |

### Also adopted from the verifier's nuances (not objections, but they change the text)

- Row 25's second nuance: socketpair also makes `ChannelFault` near-unreachable (`pipe.rs:474`),
  so the rejection record understated its own case. Folded into **C1**.
- Row 29's caveat: the `spawn.rs:12-14` analogy is "the platform C library is already linked", true
  for `gnu` and `musl` alike. Stated in **C3**.
- Row 40's nuance: `RuntimeDirectory`'s wipe is per-unit-stop and only matters after PID reuse — the
  same point `pipe.rs:41-44` makes. Stated in **C2**, which is why the corpse-probe rather than the
  wipe is what covers the dev-run path.

---

## Withdrawals / restructuring

**Withdrawn outright**

1. **"`spawn.rs` — portable at last"**, heading and claim. `std::process::Child` is not duplicable on
   unix and the sink retains the `Child` (`sink.rs:421-423`, `:377-381`, `:406-407`). Replaced by
   **C5**, which is a new change carrying its own evidence and its own dependencies.
2. **`kill_and_wait` "keeps its bounded-wait doctrine."** It does not have one on unix
   (`spawn.rs:63-67`). Replaced by **C6**, which *introduces* it — and which drops the Windows
   `ERROR_SHARING_VIOLATION` rationale as inapplicable to POSIX `rename(2)` and re-derives the bound
   from the `ChannelFault`-after-kill race (`sink.rs:374-376`) instead.
3. **Open decision "tempdir per test already implied by the tag scheme at `rt13.rs:107`."** Checked;
   false. Replaced by the concrete design in **C14**.
4. **Open decision "signal-death code encoding … pick at plan time."** Closed now as `128 + signo`,
   because `-signo` collides with the `-1` spawn-failure sentinel (`sink.rs:434-437`,
   `spawn.rs:100-109`) at SIGHUP — a Q3 defect a plan-time coin-flip could ship.
5. **Open decision "whether `serve`'s accept loop needs a poll/timeout."** Closed now: it does not.
   `accept()` blocks exactly as `ConnectNamedPipe` does and the caveat is already documented
   (`pipe.rs:322-330`); adding a timeout would be new mechanism to fix a caveat Windows lives with.
6. **"Bind/serve failure mirrors whatever E2's Windows serve-failure path does (confirm at plan
   time)."** The named source does not exist. Replaced by a direct citation, no deferral.
7. **"unlink-before-bind covers a crashed predecessor."** Unconditional unlink silently steals a live
   peer's name, inverting the deliberate loud-fail choice at `pipe.rs:100-104`. Replaced by C2(4)'s
   corpse-probe.
8. **"the mock-main client gets the same `UnixStream` branch as the real one."** The launcher crate
   has no tokio; the mock never shared client code on Windows either. What is shared is
   `kiosk_core::ipc`.
9. **"one crate was fixed in A."** A's C12 is designed, not merged (`kiosk-main/src/credential_acl.rs:101-104`
   is still `Ok(true)`). Reworded to "one crate is A's to fix".
10. **"exit codes, `ChildExited`, kill semantics … see exactly what Windows sees."** Narrowed: exit
    codes and `ChildExited` do; kill semantics do not, until C12.

**Restructuring**

- **Two new changes added** — C12 (orphan-kill) and C13 (single-instance). Both are Windows
  supervision guarantees that are silent Unix no-ops in the tree (`job.rs:217-225`, `:283-288`) and
  that the spec's parity claim asserted without checking. Both close with a directive plus stdlib,
  not with new subsystems.
- **C11 grows two directives** (`RuntimeDirectoryMode=0700`, `KillMode=control-group`) because C3's
  accept-starvation surface and C12's orphan-kill both terminate in the unit shape, which is C's.
  C hands *values* to G; the *set of directives* stays C's, and that is exactly why row 39 was C's
  defect to own.
- **Four open decisions closed** (items 3-6 above); one remains genuinely open and is a value, not a
  mechanism: the exact `ucred` field order confirmed against the libc crate's definition — probed
  working here, and Q5 admits value-level plan-time resolution.
- **The three in-session probes** (`waitid`, `sun_path`/`SO_PEERCRED`/`File::try_lock`,
  `systemd-analyze verify`) are recorded in the spec as reproducible commands, so a reviewer is not
  asked to take a measurement on trust — which is the failure mode row 41 caught.
