# P2-C — Linux Launcher Supervision Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** Linux. Tasks 1–9 are host-testable (`cargo test --workspace` on any Linux host, including CI). Task 10 (smoke 13–15) runs under **cage**, not weston — cage is the object under test.

**Goal:** `kiosk-launcher` supervises `kiosk-main` on Linux with the guarantees Windows actually enforces: spawn, watch a heartbeat channel, restart per the FSM, drain a dead main's spool, exit 86 on technician exit, kill orphans, refuse a second instance — under a cage compositor started by one systemd unit.

**Architecture:** The transport becomes a Unix domain socket listener with the same `kiosk_core::ipc` frames, framing, `Event` mapping and `child_pid` contract — so the FSM's channel states keep their producers. On Unix a child's exit status is a **single-consumer resource**, so the waiter thread owns the `Child` and is sole reaper, sole status consumer and sole reporter, while the sink holds a `pidfd` it uses to kill and observe but which cannot consume a status. Orphan-kill is reassigned to the unit's cgroup with code-level detection.

**Tech Stack:** Rust 2021, std `UnixListener`/`UnixStream`, `SO_PEERCRED` via a local extern, `syscall(434)`/`syscall(424)` for pidfd, systemd + cage.

**Spec:** `docs/superpowers/specs/2026-08-06-p2c-linux-launcher-supervision-design.md` (rev 2)

**Depends on:** P2-A (order-independent with P2-B). **C16 has a hard co-landing constraint with P2-A's `/var/lib/kiosk/`.**

## Global Constraints

- **Merge gates:** RT-13 running in **per-PR Linux CI**, the **N=200** spawn-and-kill host test, and smoke scenarios 13–15.
- **Mirror, don't invent.** Every Linux body mirrors its Windows counterpart one-for-one. `accept_client`, `frame_to_event`, `await_child_pid`, the `expected = client.unwrap_or(expected)` re-derivation, the once-per-streak latches and the `MAX_LINE_BYTES` cap are **used unchanged**, not reimplemented.
- **Declare `pidfd_open`/`pidfd_send_signal` via `syscall(2)`, never as direct externs.** glibc gained a `pidfd_open` wrapper only in 2.36 and Ubuntu 22.04 ships 2.35 — a direct extern links on a modern dev box and fails on half the platform floor. Syscall numbers: **434** (`pidfd_open`), **424** (`pidfd_send_signal`). `kill(2)` is exempt and is declared directly.
- **Replace `#[cfg(not(windows))]` stubs, never add `#[cfg(unix)]` beside them** — both match on Linux and you get a duplicate definition.
- **No new dependency.** `SO_PEERCRED`, `INVOCATION_ID` and `File::try_lock` keep both crates dependency-free. New `unsafe` surface is exactly three bounded FFI sites: `SO_PEERCRED`, `syscall(434)`/`syscall(424)`, `kill(2)`.
- **Never block boot.** Every degraded path is WARN + breadcrumb + continue (`job.rs:18-25`): "a device that refuses to start because a hardening feature failed is a black screen."
- **Windows behaviour diff is zero lines.** `ChildHandle` *is* `std::process::Child` there.
- **A non-root manual run is not a supported configuration.** It degrades loudly, never silently.

## File Structure

| File | Responsibility |
|---|---|
| `crates/kiosk-launcher/src/pipe.rs` | `instance_name` gains a `cfg` seam inside the function plus a `data_dir` argument and an `io::Result`; new `runtime_dir()`; `#[cfg(unix)] serve` replaces the stub |
| `crates/kiosk-launcher/src/spawn.rs` | `ChildHandle` type; `#[cfg(unix)] spawn_main` + waiter thread; `#[cfg(unix)] kill_and_wait`; exit-status mapping |
| `crates/kiosk-launcher/src/job.rs` | `#[cfg(unix)] Job::create` detects `INVOCATION_ID`; `#[cfg(unix)] acquire_single_instance` takes a file lock |
| `crates/kiosk-launcher/src/sink.rs` | `child` field's type becomes `ChildHandle` (alias-only change on Windows) |
| `crates/kiosk-launcher/src/main.rs:48-53` | `resolve_data_dir` → `/var/lib/kiosk` on Unix (C16) |
| `crates/kiosk-launcher/src/credential_acl.rs:100-104` | fail-open stub replaced by the mode-bits check (C9) |
| `crates/kiosk-launcher/tests/rt13.rs`, `tests/mock_main.rs` | un-gated from `#![cfg(windows)]`; `unique_transport(tag, dir)` |
| `crates/kiosk-main/src/heartbeat.rs` | `#[cfg(unix)]` client (C8) + the arch-04 JS-ping (C17) |
| `packaging/systemd/kiosk.service` | the unit **shape** (values, `[Install]` content and start-limit numbers are P2-G's) |

---

### Task 1: Socket naming, `runtime_dir()` and the `SUN_LEN` guard (C2)

**Files:**
- Modify: `crates/kiosk-launcher/src/pipe.rs:55-60` (`instance_name`), add `runtime_dir`
- Test: `crates/kiosk-launcher/src/pipe.rs` (`mod tests` — currently `#[cfg(all(test, windows))]` in places; the new tests must run on Linux)

**Interfaces:**
- Produces: `pub fn runtime_dir(data_dir: &Path) -> PathBuf` (pure, host-tested) and `pub fn instance_name(data_dir: &Path) -> io::Result<String>`
- Consumes: `resolve_data_dir` (Task 8)

> **The call site does change, and the spec's two requirements are why.** The spec prefers `main.rs:172`'s call site untouched, but it also requires `runtime_dir()` to fall back to *the data dir* and the derivation to return `io::Result` so a path ≥ 108 bytes is rejected before `bind()`. You cannot have both: the function needs the data dir as input and its error has to be handled somewhere. Checked at the tree — `main.rs:172` is `let pipe_name = pipe::instance_name();` with `data_dir` already in scope (it is cloned eight lines later at `:180`, and `pipe::serve` already takes `&data_dir`), so the cost is **one argument and one error arm**, not new plumbing.
>
> Handle the `Err` the way every other launcher degradation is handled (`job.rs:18-25`, never block boot): `eprintln!` + `crate::sink::breadcrumb(data_dir, "pipe", …)` and keep going with the un-bindable name, which then takes `pipe.rs:370-388`'s existing retry-and-breadcrumb path. **Do not** `unwrap()` and do not abort — a launcher that refuses to start is a black screen. `#[cfg(windows)]` keeps the infallible body and ignores the argument.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(unix)]
mod unix_naming {
    use super::*;

    /// `/run/kiosk` when it is a directory (the unit's RuntimeDirectory), else the data
    /// dir. The second branch is for a run without systemd — still a ROOT run, the same
    /// principal the unit uses. `$XDG_RUNTIME_DIR` is deliberately not a third branch.
    #[test]
    fn the_runtime_dir_falls_back_to_the_data_dir() {
        let data = std::path::Path::new("/var/lib/kiosk");
        let got = runtime_dir(data);
        assert!(got == std::path::Path::new("/run/kiosk") || got == data);
    }

    /// Probed: `UnixListener::bind` succeeds at 107 bytes and fails at 108 with
    /// "path must be shorter than SUN_LEN". std raises a clean io::Error, it does NOT
    /// truncate — so the derivation rejects rather than handing it to bind().
    #[test]
    fn a_path_at_or_over_sun_len_is_rejected_by_the_derivation() {
        let long = std::path::PathBuf::from("/tmp").join("x".repeat(120));
        assert!(instance_name(&long).is_err());
    }

    /// The host test asserts the BIND, not only the derivation.
    #[test]
    fn the_derived_path_actually_binds() {
        let dir = std::env::temp_dir().join(format!("kiosk-bind-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = instance_name(&dir).expect("derives");
        let _l = std::os::unix::net::UnixListener::bind(&path).expect("binds");
        std::fs::remove_dir_all(&dir).ok();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-launcher pipe`
Expected: FAIL — `runtime_dir`/`instance_name`'s new signature do not exist.

- [ ] **Step 3: Implement**

`#[cfg(windows)]` keeps `format!("{PIPE_NAME}-{pid}")`. `#[cfg(unix)]` returns `<runtime_dir>/hb-<launcher-pid>.sock`. Per-PID naming mirrors the stale-instance discipline `pipe.rs:39-44` states. Reject a path ≥ 108 bytes with an `io::Error` before `bind()`. Reference sizes: `/run/kiosk/hb-<pid>.sock` is 24 bytes; `/var/lib/kiosk/hb-<pid>.sock` is 27; RT-13's tempdir paths measure ~55.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-launcher pipe`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/pipe.rs
git commit -m "feat(launcher): unix socket naming, runtime_dir and the SUN_LEN guard"
```

---

### Task 2: `SO_PEERCRED` peer verification (C3)

**Files:**
- Modify: `crates/kiosk-launcher/src/pipe.rs` — add a `#[cfg(unix)] mod peercred`

**Interfaces:**
- Produces: `#[cfg(unix)] fn peer_pid(stream: &UnixStream) -> Option<u32>`
- Consumes: nothing. Feeds the **existing, unmodified** `accept_client(client, expected, current)`.

`UnixStream::peer_cred()` is unstable (`E0658`, rust issue #42839), so there is no stable-std path. No new dependency: the platform C library is already linked into every Rust binary on this target — true of `gnu` and `musl` alike — which is the correct reading of the convention `spawn.rs:12-14` set for kernel32.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(unix)]
#[test]
fn peer_pid_reports_the_connecting_process() {
    let dir = std::env::temp_dir().join(format!("kiosk-peer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("peer.sock");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let (server, _) = listener.accept().unwrap();
    assert_eq!(peer_pid(&server), Some(std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
}

/// No accept→check TOCTOU exists: SO_PEERCRED returns credentials the kernel recorded
/// at connect(2) time, not a live lookup. Pinned as a comment on the test above.
#[cfg(unix)]
#[test]
fn the_existing_accept_client_seam_is_reused_unchanged() {
    // Snapshot OR current — a one-valued check against child_pid reintroduces a fixed
    // bug: with backoff_s > 2 the pre-accept snapshot is 0, so snapshot-only rejects the
    // legitimate new child and cries impostor on every normal restart.
    assert!(accept_client(Some(42), 0, 42));
    assert!(accept_client(Some(42), 42, 0));
    assert!(!accept_client(Some(7), 42, 43));
    assert!(!accept_client(None, 42, 42));
    assert!(!accept_client(Some(0), 0, 0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p kiosk-launcher peer`
Expected: FAIL — `peer_pid` does not exist.

- [ ] **Step 3: Implement**

A local `extern "C" { fn getsockopt(...) -> c_int; }` and a 3×`u32` `#[repr(C)] struct Ucred { pid: u32, uid: u32, gid: u32 }`. `SOL_SOCKET = 1`, `SO_PEERCRED = 17` on Linux. Confirm the `ucred` field order against the libc crate's definition (spec §Open decisions — this is the one value left to confirm). Claim **no size assertion**; the probe above is the pin.

Add the declared divergence as a `ponytail:` comment: `SO_PEERCRED`/`struct ucred` is Linux (and Android) ABI; macOS uses `LOCAL_PEERPID`, so a macOS build would fail to link. CI has exactly three jobs — `lint-test` (ubuntu-22.04), `build-windows`, `build-linux` — and macOS is built nowhere.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-launcher`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/pipe.rs
git commit -m "feat(launcher): SO_PEERCRED peer verification feeding the existing accept_client"
```

---

### Task 3: The `#[cfg(unix)] serve` accept loop (C4)

**Files:**
- Modify: `crates/kiosk-launcher/src/pipe.rs:519-528` — **replace** the `#[cfg(not(windows))]` stub

**Interfaces:**
- Consumes: `runtime_dir`/`instance_name` (Task 1), `peer_pid` (Task 2), `accept_client`, `frame_to_event`, `await_child_pid`, `sink::breadcrumb`
- Produces: the Linux `serve(pipe_name, data_dir, tx, cancel, child_pid)` with the identical signature

**Structure mirrors `pipe.rs:366-491` one-for-one:**

| Windows | Linux |
|---|---|
| `create_pipe` | `bind` |
| `connect_pipe` | `listener.accept()` |
| `LineReader::next_line` | `BufReader::read_line` (same `MAX_LINE_BYTES` cap, same silent drop on `decode` error) |
| `await_child_pid` | unchanged |
| `accept_client` | unchanged |

- [ ] **Step 1: Implement the loop**

Keep, byte-for-byte in behaviour: `Err`-while-`child_pid == expected` → `ChannelFault` + latch; re-accept + first frame → `ChannelReconnected` **before** that frame's own event; `logged_failure` / `logged_impostor` once-per-streak latches; the post-accept `expected = client.unwrap_or(expected)` re-derivation.

**Unconditional `remove_file` before `bind`** — one line, no collision branch. The name is `/run/kiosk/hb-<our-own-pid>.sock`: for a live peer to hold it, some other process must be listening on a path named after a PID this process currently holds, which is impossible for a peer launcher and impossible for a squatter (`RuntimeDirectoryMode=0700` on a root-owned directory). A connect-probe collision branch was designed and **withdrawn** — do not re-add it.

Bind failure for any other cause takes `pipe.rs:370-388` unchanged: once-per-streak `eprintln!` + `crate::sink::breadcrumb(data_dir, "pipe", …)` + `sleep_retry()`, carrying the `ponytail:` at `:373-380` over verbatim.

**No poll/timeout on `accept()`** — it blocks exactly as `ConnectNamedPipe` does, and `cancel` is checked only between blocking calls.

- [ ] **Step 2: Verify against RT-13 locally**

Run: `cargo test -p kiosk-launcher --test rt13` (will still be Windows-gated until Task 9 — if so, verify by building and moving on)
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add crates/kiosk-launcher/src/pipe.rs
git commit -m "feat(launcher): unix accept loop mirroring the Windows serve one-for-one"
```

---

### Task 4: Exit-status mapping (C7)

**Files:**
- Modify: `crates/kiosk-launcher/src/spawn.rs` — add the pure mapping fn

**Interfaces:**
- Produces: `#[cfg(unix)] pub(crate) fn exit_code_of(status: std::process::ExitStatus) -> i32`

```
code()   => that code
signal() => 128 + signo          // 129…192 for signals 1…64
neither  => -2                   // unreachable from a wait()ed status
```

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(unix)]
mod exit_mapping {
    use super::exit_code_of;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    #[test]
    fn a_normal_exit_maps_to_its_code() {
        assert_eq!(exit_code_of(ExitStatus::from_raw(0)), 0);
    }

    /// arch-05 reserves 86 for the technician exit, and RestartPreventExitStatus=86 stops
    /// systemd on top of it. A SIGKILLed main reading as a technician exit would leave a
    /// dead device systemd deliberately refuses to restart. The OOM-killer case is real
    /// on a kiosk.
    #[test]
    fn no_signal_death_can_ever_map_to_86() {
        for signo in 1..=64 {
            let status = ExitStatus::from_raw(signo); // wait(2) encoding: low byte = signal
            assert_ne!(exit_code_of(status), 86, "signal {signo}");
        }
    }

    #[test]
    fn sigkill_maps_to_137() {
        assert_eq!(exit_code_of(ExitStatus::from_raw(9)), 137);
    }

    /// `-signo` would collide with the existing `-1` sentinel: sink.rs:434-437 feeds a
    /// synthetic ChildExited{code: -1} on every spawn_main Err, so SIGHUP would render a
    /// signal death and a spawn failure identically in the one diagnostic an operator has.
    #[test]
    fn the_mapping_never_produces_minus_one() {
        for signo in 1..=64 {
            assert_ne!(exit_code_of(ExitStatus::from_raw(signo)), -1);
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-launcher exit_mapping`
Expected: FAIL — `exit_code_of` does not exist.

- [ ] **Step 3: Implement**

```rust
/// The third arm is **-2**, not -1: it is unreachable (from `child.wait()` without
/// WUNTRACED/WCONTINUED exactly one of code()/signal() is Some), and a trap is precisely
/// what a plan-time implementer copies forward. Ambiguity with a literal `exit 137` is
/// likewise unreachable — kiosk-main emits only 0 (cli.rs:31), 86 (pinpad.rs:156) and 101
/// on panic.
#[cfg(unix)]
pub(crate) fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => code,
        (None, Some(signo)) => 128 + signo,
        (None, None) => -2,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-launcher exit_mapping`
Expected: PASS (4 tests, one looping all 64 signals).

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/spawn.rs
git commit -m "feat(launcher): 128+signo exit mapping with the never-86 invariant"
```

---

### Task 5: `ChildHandle`, `spawn_main` and the sole-reaper waiter (C5)

**Files:**
- Modify: `crates/kiosk-launcher/src/spawn.rs:198-210` — replace the stub
- Modify: `crates/kiosk-launcher/src/sink.rs:421` etc. — `child`'s type becomes `ChildHandle`
- Modify: `crates/kiosk-launcher/src/job.rs:221-223` — `assign` takes `&ChildHandle`

**Interfaces:**
- Produces:
  ```rust
  #[cfg(windows)] pub type ChildHandle = std::process::Child;
  #[cfg(unix)]    pub struct ChildHandle { pidfd: Option<OwnedFd>, exited: Arc<AtomicBool>, pid: u32 }
  ```
  and `pub fn spawn_main(exe, config_dir, safe, pipe_name, tx) -> io::Result<ChildHandle>`
- Consumes: `exit_code_of` (Task 4)

**The settled design:** the **waiter thread owns the `Child`** and is sole reaper, sole status consumer and sole reporter (`child.wait()`, stdlib). The **sink holds a `pidfd`** — a reuse-immune handle it uses to kill and to observe death, and which cannot consume a status. Exactly-one-exit-event holds **by construction**, not by guard: one resource, one consumer.

> **Two withdrawn designs. Do not reintroduce either.** (1) "plain `child.wait()` in the waiter" is not implementable as a drop-in — `Child::wait` takes `&mut self` and the caller keeps the `Child`; `std::process::Child` exposes no unix duplication API, and two threads independently `wait()`ing is a PID-reuse kill hazard. (2) `waitid(P_PID, …, WEXITED|WNOWAIT)` is a query against a PID another thread is racing to reap: with the sink reaping first it returns −1 `ECHILD` with an untouched buffer, losing the exit event *and* fabricating a `ChildExited{128}`. The `size_of::<siginfo>() == 128` "mitigation" is struck with it — `siginfo_t` is 128 bytes by `__SI_MAX_SIZE` on every Linux ABI, so it constrains the filler and never the offsets that move.

- [ ] **Step 1: Implement `spawn_main`**

`Command` with `KIOSK_HEARTBEAT_PIPE=<socket path>`, the unchanged `--safe` chain, and **`--config <config_dir>`, byte-identical to `spawn.rs:121`**. Then spawn → `pidfd_open(pid)` → move the `Child` into the waiter thread → return `ChildHandle`.

**`--config` is load-bearing (INT-9).** The existing stub takes `_config_dir` and drops it. Without the flag, `kiosk-main`'s `resolve_config_dir` falls back to `current_exe().parent()` = `/usr/lib/kiosk`, where `kiosk.ini`, `kiosk-credential.json` and `kiosk-offline.mp4` do not exist under P2-G's layout. Fail-closed one process downstream.

- [ ] **Step 2: Implement the waiter thread**

`child.wait()` → `ExitStatus` → `exit_code_of` → `exited.store(true)` → send **one** `ChildExited`. **No error branch exists**, because there is no competitor.

- [ ] **Step 3: Implement the `pidfd_open` degrade path**

```rust
// pidfd_open failure does NOT fail the spawn. Routing it to the existing Err arm is
// withdrawn: that arm is documented for a *transient* failure ("exceedingly rare, e.g.
// handle-table exhaustion"), whereas pidfd_open denial is permanent and environmental
// (a SystemCallFilter=, a container seccomp profile denying syscall 434, or ENOSYS below
// kernel 5.3). Traced through the FSM, Err → ChildExited{-1} → restart(-1, …, "exit") →
// backoff → rule-7 → safe = true → SAFE_FAIL_LIMIT → Log(SafeModeFailed) with backoff
// pinned at 60 s → forever. One denied syscall would mean a device that never renders,
// never exits and never stops trying.
```

`pidfd: None` + `eprintln!` + `breadcrumb_if_absent(data_dir, "pidfd", …)` on the existing degraded channel (replayed at `main.rs:222-226` alongside `("job", …)` and `("mutex", …)`), **and supervision continues**. The `Err` arm is retained for **waiter-thread-creation failure only**, where the Windows analogy is exact.

- [ ] **Step 4: Verify Windows is unchanged**

Run: `cargo check -p kiosk-launcher --target x86_64-pc-windows-msvc` if a Windows target is installed; otherwise verify by inspection that `ChildHandle` is a type alias for `std::process::Child` on Windows so `job.rs:199`, `sink.rs:377-381`, `:406-407` and `:421-423` compile unchanged. This is C's **one** declared cross-platform change and its Windows behaviour diff must be **zero lines**.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/spawn.rs crates/kiosk-launcher/src/sink.rs crates/kiosk-launcher/src/job.rs
git commit -m "feat(launcher): sole-reaper waiter thread, pidfd handle, --config forwarding"
```

---

### Task 6: Bounded `kill_and_wait` and the N=200 gate (C6)

**Files:**
- Modify: `crates/kiosk-launcher/src/spawn.rs:63-67` — replace the unbounded Unix body
- Test: `crates/kiosk-launcher/src/spawn.rs` (`mod tests`, un-gated from `#[cfg(all(test, windows))]` for the new tests)

**Interfaces:**
- Consumes: `ChildHandle` (Task 5)

**Correction to carry:** the bounded-wait doctrine and `KILL_WAIT_MS` are `#[cfg(windows)]`-only (`spawn.rs:29-39`); the existing Unix body is `let _ = child.kill(); let _ = child.wait();` — unbounded, and replaced. The bound must be **introduced**, not kept.

*Which inherited rationale survives the port:* the `ERROR_SHARING_VIOLATION` rationale does **not** apply (POSIX `rename(2)` does not care about open descriptors). `sink.rs:374-376` **does** apply — waiting closes the `ChannelFault`-after-kill race, so the reader's error can no longer beat `child_pid.store(0)`. The bound stays on that rationale alone.

- [ ] **Step 1: Write the N=200 gate test**

```rust
/// The gate for the ownership rule. This test fails intermittently under the withdrawn
/// `waitid` design and cannot fail under the sole-reaper design without the rule being
/// broken. RT-13 cannot cover it — rt13.rs:145-152 passes `job: None` and never exercises
/// the kill/exit race — so this host test is the gate, not RT-13.
#[cfg(unix)]
#[test]
fn exactly_one_exit_event_over_200_spawn_and_kill_iterations() {
    for pidfd_mode in [true, false] {
        for i in 0..200 {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut handle = spawn_test_child(tx, pidfd_mode);
            kill_and_wait(&mut handle);
            let events: Vec<_> = rx.try_iter().collect();
            assert_eq!(events.len(), 1, "iteration {i}, pidfd={pidfd_mode}: {events:?}");
            assert!(!is_zombie(handle.pid()), "iteration {i}: zombie left behind");
        }
    }
}
```

Write `spawn_test_child` to spawn a trivial `sleep`-style child through the real `spawn_main` path (with `pidfd` forced to `None` in the second mode), and `is_zombie` to read `/proc/<pid>/stat`'s state field.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p kiosk-launcher exactly_one_exit_event -- --nocapture`
Expected: FAIL — the unbounded Unix `kill_and_wait` still calls `child.wait()` from the sink side, which is exactly the double-consumer the design removes.

- [ ] **Step 3: Implement**

`pidfd_send_signal(pidfd, SIGKILL)` then a bounded poll on the waiter's `exited` flag up to the same 5 s ceiling; on expiry give up and proceed, the same degradation the Windows doc records. `exited` is set *after* `child.wait()` returns — i.e. after the reap — so it is a strictly **stronger** postcondition than the `WaitForSingleObject(KILL_WAIT_MS)` it mirrors.

**Degraded arm (`pidfd: None`):** `kill(pid, SIGKILL)` **gated on `exited`** — no kill is issued at all once the child is known reaped.

```rust
// ponytail: the ceiling is a two-instruction window between the atomic load and the
// syscall in which a reap could recycle the PID; upgrade is pidfd, when the sandbox
// permits it. Strictly better than the only Unix kill path the tree has ever had
// (an *ungated* child.kill()), not a new exposure.
```

- [ ] **Step 4: Run the gate to verify it passes**

Run: `cargo test -p kiosk-launcher exactly_one_exit_event -- --nocapture`
Expected: PASS — 400 iterations (200 × two modes), `events == 1` in every ordering, no zombies.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/spawn.rs
git commit -m "feat(launcher): bounded unix kill_and_wait with the N=200 exactly-one-event gate"
```

---

### Task 7: Orphan-kill and single-instance parity (C12, C13)

**Files:**
- Modify: `crates/kiosk-launcher/src/job.rs:217-225` (`Job::create`/`assign`), `:283-288` (`acquire_single_instance`)

**Interfaces:**
- Consumes: `resolve_data_dir` (Task 8) for the lock path

**C12 — the honest parity statement:** this **reassigns** enforcement to the unit cgroup, **adds** detection, and **defers** the gate to P2-G G15 (`pkill -9 kiosk-launcher; sleep 2; ! pgrep kiosk-main`). It does **not** close the gap inside this spec. Today, on Linux, a launcher killed with `SIGKILL` leaves `kiosk-main` running full-screen and unsupervised — the exact field failure `job.rs:4-16` was written to close.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(unix)]
#[test]
fn job_create_reports_missing_supervision_when_invocation_id_is_absent() {
    // INVOCATION_ID is set by systemd for every service since v232, inherited by the whole
    // unit tree, and verified to survive the cage hop intact. Its absence means no unit
    // cgroup is enforcing orphan-kill, which must not be reported as armed supervision.
    // ponytail: INVOCATION_ID is env-settable, so the only misreport this permits is a
    // false *negative* warning on a box where someone exported it by hand.
    std::env::remove_var("INVOCATION_ID");
    assert!(Job::create().is_err());
}

#[cfg(unix)]
#[test]
fn a_second_launcher_does_not_acquire_the_lock() {
    let dir = std::env::temp_dir().join(format!("kiosk-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let first = acquire_single_instance_at(&dir).unwrap();
    assert!(first.is_some());
    let second = acquire_single_instance_at(&dir).unwrap();
    assert!(second.is_none(), "WouldBlock must map to Ok(None), the deliberate exit(0) arm");
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-launcher job`
Expected: FAIL — the Unix `Job::create` returns `Ok(Job)` unconditionally and `acquire_single_instance` returns `Ok(Some(SingleInstance))` ("every process is 'the one'").

- [ ] **Step 3: Implement C12 detection**

`#[cfg(unix)] Job::create()` returns `Err` when `std::env::var_os("INVOCATION_ID").is_none()`, firing the **existing** WARNING-and-continue path with its existing message and its existing `("job", …)` breadcrumb (`main.rs:189-199`, replayed at `:222-226`). **Zero new plumbing.**

Do **not** parse `/proc/self/cgroup` (v1/v2/hybrid parsing is more code than the thing it guards) and do **not** use `PR_SET_PDEATHSIG` (delivered on the death of the spawning *thread*, and it kills only `kiosk-main`, not its descendants).

- [ ] **Step 4: Implement C13's backstop**

`#[cfg(unix)] acquire_single_instance` takes `std::fs::File::try_lock()` (stable) on **`<data_dir>/launcher.lock`** — C16's absolute path, preceded by `create_dir_all`. Mapping: `WouldBlock` → `Ok(None)` (the deliberate `exit(0)` arm `job.rs:239-248` documents), any other error → `Err` (WARN + continue). The returned `File` lives in `SingleInstance` for the process lifetime, exactly as the Windows `OwnedHandle` does.

**The lock deliberately does not use `runtime_dir()`** — that branches on whether `/run/kiosk` exists, and `RuntimeDirectory=` creates and destroys it with the unit, so a hand-run-then-unit ordering would take two different lock inodes and both would acquire. `File::try_lock` is per-inode; a path that moves is not a token.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p kiosk-launcher job`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-launcher/src/job.rs
git commit -m "feat(launcher): cgroup orphan-kill detection and the single-instance file lock"
```

---

### Task 8: Launcher `resolve_data_dir` and SEC-09 (C16, C9)

**Files:**
- Modify: `crates/kiosk-launcher/src/main.rs:48-53`
- Modify: `crates/kiosk-launcher/src/credential_acl.rs:100-104`

**Interfaces:**
- Produces: `fn resolve_data_dir() -> PathBuf` → `/var/lib/kiosk`; `#[cfg(unix)] pub fn credential_is_owner_only(path: &Path) -> io::Result<bool>`

> **Hard co-landing constraint with P2-A.** The two `resolve_data_dir` functions must agree; whichever merges second matches the first. Today on Linux the launcher's data dir is `./kiosk` relative to CWD — under the unit that is `/kiosk`. If P2-A lands `/var/lib/kiosk` in kiosk-main and C leaves the launcher at `./kiosk`, the launcher drains an empty `./kiosk/spool/main` and **TEL-10 dies silently on Linux**.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(unix)]
#[test]
fn the_launcher_data_dir_matches_kiosk_mains() {
    // sink.rs's own doc: "the launcher's spool/launcher partition and the spool/main
    // partition it drains have to land in the same place."
    assert_eq!(resolve_data_dir(), std::path::PathBuf::from("/var/lib/kiosk"));
}
```

Plus the same four `credential_is_owner_only` tests P2-A Task 3 uses (0600 passes; 0640/0604/0666 fail; missing file is `Err`; the `Err` is a violation) — the launcher's copy is a separate file and needs its own coverage.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p kiosk-launcher data_dir credential`
Expected: FAIL.

- [ ] **Step 3: Implement both**

`#[cfg(unix)] resolve_data_dir() -> PathBuf::from("/var/lib/kiosk")`, doc rewritten in the same edit. **Replace** the `credential_acl.rs` fail-open stub with `metadata.permissions().mode() & 0o077 == 0`, doc comment rewritten in the same edit.

Record P2-A's transferred condition: the C11 shape declares **no `User=`**, so launcher, cage and kiosk-main all run as root and the credential is root-owned `0600`; mode bits alone remain sufficient. **If P2-G's seat/DRM wiring introduces a non-root `User=`, the uid check lands with it** — at that point the credential's owner and the reader's uid can differ and mode bits stop proving anything.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p kiosk-launcher`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/kiosk-launcher/src/main.rs crates/kiosk-launcher/src/credential_acl.rs
git commit -m "fix(launcher): /var/lib/kiosk data dir and the SEC-09 mode check"
```

---

### Task 9: Heartbeat client (C8) and the arch-04 JS-ping (C17)

**Files:**
- Modify: `crates/kiosk-main/src/heartbeat.rs:149-155` — replace the stub
- Modify: `crates/kiosk-main/src/main.rs:941` — one new argument

**Interfaces:**
- Produces: `#[cfg(unix)] pub async fn run(pipe_name: String, ready: Arc<Notify>, cancel: CancellationToken, window: tauri::WebviewWindow)`
- Consumes: `pipe_name_from_env()` (already platform-free)

**C8:** `tokio::net::UnixStream::connect(&path)` in place of `ClientOptions::new()…open()`; **everything else in the Windows client (`heartbeat.rs:41-147`) is copied unchanged** — the `ready_reached` latch, the once-per-streak `logged_failure` latch, the cancel-wrapped `write_all`s, `tokio::time::interval` with `MissedTickBehavior::Delay`, `RECONNECT_BACKOFF` (1 s), `sleep_or_cancel`. `tokio` features `net` + `io-util` are already declared: **no Cargo change.**

Only the *comment* changes: on Linux the reconnect-gap errors are `ENOENT` (before first bind) and `ECONNREFUSED` (file present, no listener), not `ERROR_PIPE_BUSY`/`ERROR_FILE_NOT_FOUND`. The existing arm already retries *any* open error.

**C17 — why this exists:** `heartbeat::run` is a `tokio::spawn`ed task that never touches the GTK loop, so **a wedged GTK main loop keeps pinging** and the FSM's 3-missed rule never arms. Composed with P2-D claiming no covering control and P2-G removing VT, getty and SSH from a conforming image, that is parent §3.5's un-exitable device reached without any single spec being wrong.

- [ ] **Step 1: Implement the C8 client**

- [ ] **Step 2: Implement the JS ping**

In `heartbeat::run`'s `tick.tick()` arm, `#[cfg(not(windows))]`, **before each `Frame::Ping` write**:

```rust
// arch-04 / RT-02 / OD-1. Round-trip a no-op through the webview:
// AppHandle::run_on_main_thread → WebviewWindow::with_webview(|w| w.inner()
//   .run_javascript("0", None, cb)), cb resolving a tokio::sync::oneshot, awaited under
// the parent's own 3 s cap. Timeout or error ⇒ the ping is WITHHELD — not an error, not a
// log storm: one WARN on the first withheld ping of a run. Three withheld pings = 15 s =
// the FSM's existing 3-missed rule → watchdog.hang → restart.
//
// Both of arch-15's uncovered halves fall to this one mechanism: a wedged GTK main loop
// never dispatches run_on_main_thread, and a wedged renderer with a live loop never
// delivers the run_javascript reply. Either way the cap expires.
#[allow(deprecated)] // run_javascript carries #[cfg_attr(feature = "v2_40", deprecated)]
```

Use `run_javascript` (`web_view.rs:1469`), which carries **no `#[cfg(feature = …)]` gate**, so it compiles at the declared `v2_32` floor. Do **not** use `evaluate_javascript` — it is `#[cfg(feature = "v2_40")]` and using it would make the declared floor unreal.

`heartbeat::run` gains one parameter (the `WebviewWindow`); `main.rs:941` gains one argument. C17 is `cfg(not(windows))` — **Windows is byte-unchanged**, because it has `ProcessFailed`/`RenderProcessUnresponsive` from P1.

Record the residual: **a wedged cage is still unrecoverable** — C17 cannot reach it, because the compositor holds the DRM device. Carried by P2-G H11 plus the runbook power-cycle line.

- [ ] **Step 3: Verify**

Run: `cargo build -p kiosk-main && cargo clippy -p kiosk-main --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-main/src/heartbeat.rs crates/kiosk-main/src/main.rs
git commit -m "feat(linux): unix heartbeat client plus the arch-04 JS-ping hang gate"
```

---

### Task 10: RT-13 cross-platform and the per-PR Linux CI gate (C14)

**Files:**
- Modify: `crates/kiosk-launcher/tests/rt13.rs:27` — remove `#![cfg(windows)]`; `unique_pipe` → `unique_transport(tag, dir)`
- Modify: `crates/kiosk-launcher/tests/mock_main.rs:26-30` — replace the in-`main` branch with a `#[cfg(unix)] fn unix_main()` sibling of `windows_main`

**Interfaces:**
- Produces: `fn unique_transport(tag: &str, dir: &Path) -> String`

**Correction to carry:** the draft claimed "tempdir per test already implied by the tag scheme at `rt13.rs:107`". **False** — the tag scheme is PID + counter and its own doc says so; the per-scenario tempdirs exist but are `config_dir`/`data_dir` and the transport name is deliberately not derived from them. Parallelism containment rests on those tempdirs plus PID + counter, both already present; **no `--test-threads` restriction is needed and none is imposed.** A socket in `data_dir` is inert: `drain_orphan` touches only `spool/main` and `spool.orphaned`.

**Correction to carry:** the draft said the mock gets "the same `UnixStream` branch as the real one". **False** — the launcher crate has no `tokio` and the mock never shared client code on Windows either (`mock_main.rs` writes with `std::io::Write`). The Linux mock uses `std::os::unix::net::UnixStream` with the same `kiosk_core::ipc` frames and the same retry-the-open contract. What is shared is the *protocol*, which is where it always was.

- [ ] **Step 1: Un-gate and parameterise**

`#[cfg(windows)]` keeps `rt13.rs:107`'s template; `#[cfg(unix)]` returns `dir.join(format!("hb-{tag}-{pid}-{n}.sock"))`, where `dir` is the scenario's **existing** `data_dir` tempdir (`rt13.rs:117-118`), which `Harness::start` already owns and drops at teardown. C2's length guard applies to the test paths too (~55 bytes, well under 108).

- [ ] **Step 2: Run RT-13 on Linux**

Run: `cargo test -p kiosk-launcher --test rt13 -- --nocapture`
Expected: all four scenarios PASS — fault, reconnect, miss-restart, safe-mode chain, drain. Budget from the constants: floor set by `MISS_LIMIT_S = 15` and `HEALTHY_OBSERVE = 20 s`; under cargo's default parallelism they overlap, so ~25–35 s added wall clock.

- [ ] **Step 3: Verify the CI gate lands with no workflow edit**

Run: `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean. These are exactly what `.github/workflows/ci.yml:11-26`'s `lint-test` job (ubuntu-22.04, on `pull_request`) runs, so un-gating puts RT-13 in a per-PR Linux run with **no workflow edit**. `-D warnings` means the un-gated test and mock must be warning-clean on Linux — that is part of this change.

> If RT-13 ever wedges the job, the escape is moving it into `build-linux`, **not** trimming it.

- [ ] **Step 4: Commit**

```bash
git add crates/kiosk-launcher/tests/rt13.rs crates/kiosk-launcher/tests/mock_main.rs
git commit -m "test(launcher): RT-13 cross-platform, running per-PR on Linux CI"
```

---

### Task 11: The systemd unit shape (C11) and smoke 13–15 (C15)

**Files:**
- Create: `packaging/systemd/kiosk.service`
- Modify: `packaging/smoke/run-smoke.sh` — scenarios 13–15 under **cage**

**Interfaces:**
- Produces: the unit *shape*. **Values, installation, `[Install]` content and start-limit numbers are P2-G's.**

- [ ] **Step 1: Write the unit shape**

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

Three directives are load-bearing for changes in this spec:
- **`SuccessExitStatus=86`** — without it the unit lands in `failed` with `status=86`, so `systemctl is-failed` and every dashboard on top of it report a healthy technician exit as a device fault.
- **`RuntimeDirectoryMode=0700`** — the default 0755 would leave `/run/kiosk/hb-<pid>.sock` connectable by any local user. `SO_PEERCRED` still refuses them, so there is no forgery, but connect-and-reject in a loop is an accept-starvation channel. This directive also carries C2's unconditional-unlink property.
- **`KillMode=control-group`** — systemd's default, made explicit because C12 depends on it.

- [ ] **Step 2: Verify the structure**

Run: `systemd-analyze verify packaging/systemd/kiosk.service`
Expected: exits 0. Confirm the tool is really parsing by temporarily misspelling a key and seeing it reported. Runtime semantics (a unit exiting 86 ends `inactive`, not `failed`) stay a **declared assumption** pinned by P2-G image validation.

- [ ] **Step 3: Scenario 13 — full chain under cage**

```bash
# Scenarios 13-15 deliberately replace weston with cage: cage is the object under test,
# and asserting the cage contract under weston would assert nothing. A's weston harness
# remains correct for 1-12. WLR_BACKENDS is a wlroots variable weston does not read.
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1
cage -v          # NOT `cage --version` — that exits 1 with "invalid option -- '-'" and
                 # would abort the script under `set -e`. The emitted version is what this
                 # run proved; the floor assertion is P2-G's.
cage -- sh -c 'exit 86'; test $? -eq 86
```

Then the chain: `cage -- kiosk-launcher` headless → main up, home rendered, `watchdog.*` events spooling; `kill -9` main → launcher restarts it within the FSM's window; main back up.

- [ ] **Step 4: Scenario 14 — technician exit**

Drive the pinpad exit → assert the **launcher process** exits 86. The systemd half is P2-G's H2.

**Driver on the floor:** cage 0.1.4 exposes no virtual-pointer and no virtual-keyboard protocol, so the app path is driven by running `kiosk-main` inside cage with `GDK_BACKEND=x11` — an Xwayland client — and `xdotool`. *Declared divergence:* that exercises GTK's X11 GDK backend, not the Wayland one, which is faithful for what 14 asserts (exit-86 propagation over GTK widget signals) and is **not** a substitute for the Wayland input path. *Fallback:* if even that fails, 14's app-path half moves to the deferred hardware list against P2-G H2 and H4a — record it, do not silently drop it.

- [ ] **Step 5: Scenario 15 — hang paths**

(a) `SIGSTOP` main past the heartbeat-miss window → launcher kills/restarts (`kill_and_wait` exercised for real); `SIGCONT`ed corpse reaped, **no zombie**.
(b) **C17 variant:** block the **GTK main thread only** (a `run_on_main_thread` closure that sleeps past the window), leaving the process running and the tokio task alive → assert `watchdog.hang` is emitted and main is restarted. That is the first Linux exercise of arch-15 case (c).

- [ ] **Step 6: Record the expected degraded breadcrumb**

Smoke runs outside systemd, so C12's `INVOCATION_ID` guard **correctly fires** and every run writes a `("job", …)` breadcrumb to `startup-degraded.txt`. Scenarios 13–15 expect it and **must not read it as a failure**. Note in the smoke README that neither P2-A nor P2-B asserts on that file.

- [ ] **Step 7: Run and commit**

Run: `bash packaging/smoke/run-smoke.sh`
Expected: 13–15 PASS under cage headless, with the proved `cage -v` recorded.

```bash
git add packaging/systemd/kiosk.service packaging/smoke
git commit -m "feat(launcher): systemd unit shape; smoke 13-15 under cage headless"
```

---

## Self-Review

**Spec coverage:** C1/C4 → T3; C2 → T1; C3 → T2; C5 → T5; C6 → T6; C7 → T4; C8/C17 → T9; C9/C16 → T8; C10/C11 → T11 Steps 1–2; C12/C13 → T7; C14 → T10; C15 → T11 Steps 3–7.

**Open decision to resolve during T2:** the exact `ucred` field order confirmed against the libc crate's definition (probed working; we still declare locally).

**Residual risks carried out, each with a named carrier:** wedged cage → P2-G H11 + runbook power-cycle line; orphan-kill has no gate inside P2-C → P2-G G15; cage 0.1.4 behaviour (measured only on 0.1.5) → P2-G image validation; degraded-mode kill's two-instruction PID-reuse window → the `ponytail:` in T6; non-root manual run → declared unsupported; "unit exits 86 ends `inactive`" → P2-G image validation with smoke 14's systemd half.
