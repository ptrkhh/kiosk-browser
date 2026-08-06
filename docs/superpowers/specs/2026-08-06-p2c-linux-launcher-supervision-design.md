# P2-C — Linux Launcher Shell: UDS Heartbeat + cage/systemd Supervision (Design)

> Third sub-project of P2. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §3.1 (process
> model), §10 (RT-13). **Builds on P1-E1/E2** (the pure `watchdog` FSM + the launcher
> actor loop — `2026-07-31-p1e2-launcher-shell-design.md`) and P2-A/B. Reimplements NO
> supervise logic: the E1 FSM, the actor loop, the sink, the spool drain, and the safe-mode
> chain are already portable. This ports the three Windows edges — pipe server, spawn/wait,
> heartbeat client — and defines the systemd/compositor contract around the launcher.

**Status:** draft, 2026-08-06 (awaiting review). Approach approved in-session: Unix domain
socket transport (over socketpair-fd) and `cage -- kiosk-launcher` as the service
process shape (over launcher-spawns-cage and two-unit splits).

## Goal

`kiosk-launcher` supervises `kiosk-main` on Linux exactly as on Windows: spawn, watch a
heartbeat channel, restart per the FSM, drain a dead main's spool, exit 86 on technician
exit — under a cage compositor started by one systemd unit. Merge gates: **RT-13 running
in per-PR Linux CI** (the full supervise loop, tested on every PR — coverage the
Windows-only pipe never allowed) plus smoke scenarios 13–15.

## Scope

**In:** Linux bodies for `kiosk-launcher/src/{pipe,spawn}.rs` and
`kiosk-main/src/heartbeat.rs`; the launcher's own `credential_acl.rs` Unix
implementation (the same fail-open stub kiosk-main's C12 fixed — one crate was fixed in
A, this one is C's); RT-13 made cross-platform; the systemd unit *contract* (the
installed unit file, seat/DRM permissions, and start-limit numbers are P2-G's).

**Out:** idle/gesture (P2-D), video (P2-E), update/CI-harness (P2-F), packaging/image/
logind/seatd (P2-G).

## Architecture — the two approved decisions

### Transport: Unix domain socket, same contract, same FSM semantics

The launcher binds `std::os::unix::net::UnixListener` at
`/run/kiosk/hb-<launcher-pid>.sock` — per-PID naming mirrors `instance_name()`'s
stale-instance discipline (`pipe.rs:53-59`); `RuntimeDirectory=kiosk` in the unit both
creates `/run/kiosk` and wipes it on stop, so stale sockets cannot accumulate;
unlink-before-bind covers a crashed predecessor within the same boot. The
`KIOSK_HEARTBEAT_PIPE` env contract is unchanged in shape (`heartbeat.rs:27-29`): the
launcher computes the concrete name and publishes it on the child; on Linux the value is
a filesystem path and the kiosk-main client connects `tokio::net::UnixStream` (tokio's
UDS support is stable) with the same `RECONNECT_BACKOFF` (`heartbeat.rs:31-34`) and the
same Ready-then-Ping frame discipline over shared `kiosk_core::ipc`. Path derivation is
one pure, host-tested function: `/run/kiosk` when it exists (the unit's
`RuntimeDirectory`), else the data dir (a manual dev run without systemd must not fail
to bind); the client never derives anything — it connects to whatever the env var says,
which is what keeps the contract one-sided, as on Windows.

**Peer verification:** `SO_PEERCRED` on the accepted stream gives the connecting PID —
direct parity with `GetNamedPipeClientProcessId` (`pipe.rs:129,225`) against the shared
`child_pid` atomic (`pipe.rs:18-30`'s cross-task contract, unchanged). No new
dependency: a local `getsockopt`/`ucred` extern declaration, the same no-extra-dependency
convention `spawn.rs:12-14` established for kernel32 (the launcher's `Cargo.toml` has no
unix-side native deps today and gains none).

**Why not socketpair + inherited fd** (recorded): forgery-proof by construction and no
filesystem state — strictly better on those two axes — but there is no listener, so a
faulted channel can never re-accept: `Event::ChannelReconnected` becomes unreachable and
every `ChannelFault` degenerates into a restart. The E1 FSM's channel-grace semantics
are behavior the operator already reasons about; a transport that silently amputates one
of its states is the wrong trade. The UDS listener keeps fault→reconnect intact.

### Process shape: `kiosk.service` → `cage -- kiosk-launcher` → `kiosk-main`

The compositor wraps the launcher; the launcher spawns `kiosk-main` with the inherited
`WAYLAND_DISPLAY`. kiosk-main stays the launcher's **direct child**, so exit codes,
`ChildExited`, kill semantics, and the FSM see exactly what Windows sees; a main restart
never churns the compositor.

**Empirical pin (in-session, headless backend):** cage propagates its child's exit code
exactly — child exits 0/7/86 → cage exits 0/7/86 — and runs fine with a child that never
connects as a Wayland client (the probe child was `sh`). Both properties are load-bearing:
the first makes `RestartPreventExitStatus=86` sound with cage as the unit's main process;
the second means the launcher (not itself a Wayland client) is a legitimate cage child.

Unit contract (values and installation in P2-G; the *shape* is this spec's):

```ini
[Service]
Type=simple
ExecStart=cage -- /usr/libexec/kiosk/kiosk-launcher
Restart=always
RestartPreventExitStatus=86
RuntimeDirectory=kiosk
```

Technician exit end-to-end: pinpad exit → kiosk-main exits 86 → launcher FSM
`ExitLauncher{86}` (E2, unchanged) → cage exits 86 → systemd stops restarting. Rejected
shapes, recorded: launcher-spawns-`cage -- kiosk-main` (compositor flash on every
restart; the FSM's child becomes cage and main's code is laundered through it), and a
two-unit `BindsTo` split (two supervisors owning one failure domain — the launcher FSM
is the restart authority, systemd only supervises the launcher itself).

## Components

### `pipe.rs` — `linux_impl::serve`

Same blocking-`serve`-on-caller-thread contract (`pipe.rs:338`, threading per its module
doc): accept loop on the `UnixListener`; per-accept `SO_PEERCRED` gate against
`child_pid` (0 = no live child → reject, as on Windows); `'\n'`-delimited `ipc::decode`
→ `Event::{Ready, Heartbeat{at}}`; garbage lines dropped; EOF/reset while the child
lives → `Event::ChannelFault{at}`, re-accept + first frame → `Event::ChannelReconnected`
— the mapping table is byte-for-byte the Windows one; only the accept/read calls change.
The `#[cfg(not(windows))]` stub at `pipe.rs:519` is replaced, not added beside.

### `spawn.rs` — portable at last

`spawn_main` (`spawn.rs:111`, stub `:198`) becomes almost entirely `std::process`:
`Command` with `KIOSK_HEARTBEAT_PIPE=<socket path>` and the unchanged `--safe` argument
chain; the waiter thread is plain `child.wait()` (std exposes the exit status portably —
the kernel32 externs eat the `#[cfg(windows)]` gate they already wear).
`kill_and_wait` keeps its bounded-wait doctrine (`spawn.rs:29-39`): `Child::kill`
(SIGKILL) + `try_wait` polling up to the same ceiling. One genuinely new rule:
**signal-death mapping** — `ExitStatus::code()` is `None` when the child died to a
signal (the OOM-killer case is real on a kiosk); the mapping to `ChildExited{code}` is a
pure, host-tested function whose one invariant is *a signal death can never map to 86*
(a SIGKILLed main must not read as a technician exit). Exact encoding chosen at plan
time; the invariant is spec.

### `heartbeat.rs` — the Linux client

Replace the stub at `heartbeat.rs:152`: `UnixStream::connect` in the same
retry/backoff/reconnect loop as the Windows `ClientOptions` client
(`heartbeat.rs:42-151`), same `PING_INTERVAL_S` from `kiosk_core::ipc`, same
degradation doctrine (`heartbeat.rs:16-19`: failures cost heartbeats, never the
browser), same fold on the app `CancellationToken`.

### `credential_acl.rs` (launcher crate)

Same `#[cfg(unix)]` mode-bits implementation and doc rewrite as kiosk-main's C12
(A spec) — `metadata.permissions().mode() & 0o077 == 0`, `Err` stays a violation via the
existing `is_violation`. The launcher's SEC-09 gate (`sink::build_telemetry`'s
check-before-read ordering, cited in `boot.rs:153-156`'s comment) then enforces on
Linux instead of failing open.

### RT-13 — cross-platform, becomes the CI gate

`tests/rt13.rs` + the `rt13-mock-main` bin are `#[cfg(windows)]`-gated today
(`rt13.rs:32,101-112`) with a Windows pipe-name template (`rt13.rs:107`). C makes the
transport-name construction a platform seam (UDS path under the test tempdir on unix),
gives the mock-main client the same `UnixStream` branch as the real one, and un-gates
the test. Result: the real launcher, real sink, real transport, scriptable child — on
every PR, in the existing ubuntu CI job. This is the primary regression net for
everything above.

## Smoke additions (extend the A/B harness)

13. **full chain:** `cage -- kiosk-launcher` headless (`WLR_BACKENDS=headless`, as
    pinned in-session) → main up, home rendered, `watchdog.*` events spooling;
    `kill -9` main → launcher restarts it within the FSM's window; main back up.
14. **technician exit:** drive the pinpad exit → assert the *launcher process* exits 86
    (the systemd half of the contract is asserted at P2-G's image validation).
15. **hang path:** `SIGSTOP` main past the heartbeat-miss window → launcher
    kills/restarts (the `kill_and_wait` path exercised for real); `SIGCONT`ed corpse
    reaped, no zombie.

## Testing

- **RT-13 on Linux CI** (gate) — fault, reconnect, miss-restart, safe-mode chain,
  drain: whatever E2's scenario list runs on Windows runs identically here.
- **Host tests:** signal-death → `ChildExited` mapping (incl. the never-86 invariant);
  socket-path derivation; the peer-cred accept/reject decision as a pure function.
- Existing gates unchanged; launcher crate now compiles fully on Linux CI (its
  dead-code warnings shrink to zero).

## Error handling

Unchanged doctrine. Bind/serve failure mirrors whatever E2's Windows serve-failure path
does (confirm at plan time — do not invent a new degradation here); heartbeat client
failures cost heartbeats, never the browser; drain and kill paths keep their bounded
waits.

## Open decisions to resolve at plan time

- The exact `ucred` extern layout + `getsockopt` shim (musl/glibc agreement — both
  targets use the same `struct ucred`; confirm against the libc crate's definition even
  though we declare locally).
- Signal-death code encoding (negative-signal vs sentinel) — pick whichever the sink's
  `watchdog.*` log fields render most greppably.
- Whether `serve`'s accept loop needs a poll/timeout to observe launcher shutdown, or
  whether the E2 shutdown path already unblocks it another way (mirror, don't invent).
- RT-13 socket paths under `cargo test` parallelism (tempdir per test already implied by
  the tag scheme at `rt13.rs:107` — verify).

## Scope / defer

Unit file installation, seat/DRM/seatd or logind session wiring, start-limit numbers,
and the OS image that runs all of this → P2-G. Idle/gesture → P2-D. The `zbus` ponytail
from P2-B remains parked (nothing here needed D-Bus either — `SO_PEERCRED` and
`systemd-inhibit` kept both crates dependency-free).
