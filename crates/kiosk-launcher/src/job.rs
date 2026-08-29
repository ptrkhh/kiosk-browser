//! Supervision hardening: a kill-on-close Job Object and a single-instance
//! mutex.
//!
//! # Why
//! If the launcher dies in any way that skips its own cleanup — `taskkill /F`,
//! a panic, a fast shutdown — `kiosk-main.exe` SURVIVES: full-screen,
//! unsupervised, on a device with no keyboard and nobody in front of it. A
//! relaunched launcher then spawns a SECOND one and two webviews fight for the
//! display, and because `pipe::serve` authenticates heartbeats by PID, the
//! orphan is never even noticed. That is the one field failure P1-E2 left open.
//!
//! A Job Object carrying `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the KERNEL
//! kill every process in the job when the last handle to it closes — which
//! happens on process teardown for *every* death mode, including a hard kill,
//! precisely because it needs no cooperation from the dying process. The mutex
//! stops a second supervisor from ever reaching the point of spawning.
//!
//! # Never block boot
//! A device that refuses to start because a hardening feature failed is a black
//! screen, which is strictly worse than a device running unhardened — the same
//! trade `load_bootstrap` and `build_telemetry` already make. So every failure
//! in this module is WARNING-and-continue, surfaced through the launcher's
//! existing `startup-degraded.txt` breadcrumb. The ONE exception is "a peer
//! already holds the mutex", which is a deliberate, successful `exit(0)` and
//! not a failure at all.
//!
//! # Style
//! Raw `extern "system"` declarations against kernel32, matching `spawn.rs` and
//! `pipe.rs`. All five entry points used here live in kernel32.lib, which is
//! already linked into every Windows Rust binary, so this needs no new
//! dependency (the crate has none for Win32 today and gains none here).

#[cfg(unix)]
use crate::spawn::ChildHandle;
use std::io;
#[cfg(unix)]
use std::path::Path;
#[cfg(windows)]
use std::process::Child;

/// The mutex name. `Global\` is the machine-wide namespace, so a launcher
/// started in another session (a technician's RDP login while the kiosk session
/// runs) is still seen.
///
/// ponytail: no `Local\` fallback. Creating in the `Global\` namespace needs
/// SeCreateGlobalPrivilege, which a service or a SYSTEM-run Scheduled Task has
/// but a plain interactive user may not; without it `CreateMutexW` returns
/// ERROR_ACCESS_DENIED and this device runs with NO double-start protection
/// (loudly — see `acquire_single_instance`'s `Err` arm and its caller). If the
/// deployed autostart mechanism (P1-F2) turns out to run unprivileged, retry
/// the create under `Local\kiosk-launcher`, which needs no privilege and is
/// still correct for a single-session kiosk.
#[cfg(windows)]
const MUTEX_NAME: &str = r"Global\kiosk-launcher";

/// Raw kernel32 declarations — see the module docs for why these are hand
/// written rather than pulled from a bindings crate.
#[cfg(windows)]
#[allow(non_snake_case, non_camel_case_types)]
mod win32 {
    use std::ffi::c_void;
    use std::os::windows::io::RawHandle;

    /// Kill every process in the job when its last handle closes. The whole
    /// point of this module.
    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    /// `JobObjectExtendedLimitInformation`, the `JOBOBJECTINFOCLASS` value that
    /// selects the struct below.
    pub const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    pub const ERROR_ALREADY_EXISTS: u32 = 183;

    #[repr(C)]
    #[derive(Default)]
    pub struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        pub PerProcessUserTimeLimit: i64,
        pub PerJobUserTimeLimit: i64,
        pub LimitFlags: u32,
        pub MinimumWorkingSetSize: usize,
        pub MaximumWorkingSetSize: usize,
        pub ActiveProcessLimit: u32,
        pub Affinity: usize,
        pub PriorityClass: u32,
        pub SchedulingClass: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct IO_COUNTERS {
        pub ReadOperationCount: u64,
        pub WriteOperationCount: u64,
        pub OtherOperationCount: u64,
        pub ReadTransferCount: u64,
        pub WriteTransferCount: u64,
        pub OtherTransferCount: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        pub BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION,
        pub IoInfo: IO_COUNTERS,
        pub ProcessMemoryLimit: usize,
        pub JobMemoryLimit: usize,
        pub PeakProcessMemoryUsed: usize,
        pub PeakJobMemoryUsed: usize,
    }

    /// `SetInformationJobObject` validates `cbJobObjectInformationLength`
    /// against the kernel's own idea of the struct size and fails
    /// ERROR_BAD_LENGTH on a mismatch, so a layout mistake here would be loud
    /// rather than silent — but it would still cost a device its kill-on-close.
    /// Pin the documented 64-bit size at compile time instead. (Both the x64
    /// kiosk target and the ARM64 dev host are LLP64, so one number covers
    /// them; 32-bit Windows is not a target.)
    #[cfg(target_pointer_width = "64")]
    const _: () = assert!(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() == 144);

    extern "system" {
        pub fn CreateJobObjectW(lp_job_attributes: *mut c_void, lp_name: *const u16) -> RawHandle;
        pub fn SetInformationJobObject(
            h_job: RawHandle,
            job_object_information_class: i32,
            lp_job_object_information: *const c_void,
            cb_job_object_information_length: u32,
        ) -> i32;
        pub fn AssignProcessToJobObject(h_job: RawHandle, h_process: RawHandle) -> i32;
        pub fn CreateMutexW(
            lp_mutex_attributes: *mut c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> RawHandle;
        pub fn GetLastError() -> u32;
    }
}

/// A kill-on-close Job Object. Every spawned `kiosk-main` is assigned to it, so
/// the kernel tears the child (and everything the child itself spawned, e.g.
/// WebView2's process tree) down the instant this handle closes.
///
/// **Must outlive every supervised child.** Dropping it early does not disable
/// the feature, it FIRES it: the kiosk dies on the spot. `LauncherSink` owns
/// the only instance and lives for the whole of `main`.
///
/// The launcher itself is deliberately NOT in the job — a self-assigned
/// supervisor would kill itself.
#[cfg(windows)]
pub struct Job(std::os::windows::io::OwnedHandle);

/// On Linux the service manager owns the child cgroup. Refuse to report an
/// armed job when this process is not running inside a systemd service; the
/// caller already has the WARNING + breadcrumb degraded path for this case.
#[cfg(not(windows))]
pub struct Job;

#[cfg(windows)]
impl Job {
    /// Creates an unnamed job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    ///
    /// Unnamed on purpose: nothing else needs to find it, and a named kernel
    /// object is one more thing another process could open or squat.
    pub fn create() -> io::Result<Job> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};

        // Safety: both arguments are null (default security attributes, no
        // name), matching the documented `CreateJobObjectW` signature.
        let handle = unsafe { win32::CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // Safety: `handle` is a fresh, non-null, exclusively-owned kernel
        // handle; from here on `OwnedHandle` is solely responsible for closing
        // it, including on the early return below.
        let owned = unsafe { OwnedHandle::from_raw_handle(handle) };

        let info = win32::JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: win32::JOBOBJECT_BASIC_LIMIT_INFORMATION {
                LimitFlags: win32::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                ..Default::default()
            },
            ..Default::default()
        };
        // Safety: `handle` is the live job handle owned by `owned`; `info` is a
        // correctly-laid-out (see the size assertion in `win32`) value alive
        // for the call, and the length passed is its own size.
        let ok = unsafe {
            win32::SetInformationJobObject(
                handle,
                win32::JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        };
        if ok == 0 {
            // A job without the flag is worse than no job: it silently looks
            // armed while killing nothing. Drop it and report the failure.
            return Err(io::Error::last_os_error());
        }
        Ok(Job(owned))
    }

    /// Puts `child` in the job. Fails on some CI/container/debugger job
    /// configurations (a process can be in several jobs on modern Windows, but
    /// not in every combination), which is exactly why the caller treats this
    /// as WARNING-and-continue rather than fatal.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::{AsHandle, AsRawHandle};

        // `as_handle()` first so the borrow keeps `child` alive for the call.
        let child_handle = child.as_handle();
        // Safety: both handles are live and owned by values that outlive this
        // call — the job by `self`, the process by `child`.
        let ok = unsafe {
            win32::AssignProcessToJobObject(self.0.as_raw_handle(), child_handle.as_raw_handle())
        };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(not(windows))]
impl Job {
    pub fn create() -> io::Result<Job> {
        if std::env::var_os("INVOCATION_ID").is_some() {
            Ok(Job)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "launcher is not running under a systemd service cgroup",
            ))
        }
    }
    pub fn assign(&self, _child: &ChildHandle) -> io::Result<()> {
        Ok(())
    }
}

/// Proof that this process is the only launcher. Owns the mutex HANDLE and
/// nothing else: the handle is never read, only held, because releasing it
/// frees the name for the next launcher.
///
/// **Must live for the whole process.** Bound in `main` and never dropped.
#[cfg(windows)]
pub struct SingleInstance(#[allow(dead_code)] std::os::windows::io::OwnedHandle);

/// Linux backstop for manual launches. The production path uses the absolute
/// `/var/lib/kiosk` data directory so a hand-run launcher and the systemd unit
/// contend on the same inode rather than selecting different runtime dirs.
#[cfg(not(windows))]
pub struct SingleInstance(#[allow(dead_code)] std::fs::File);

/// Claims the machine-wide launcher slot.
///
/// Three distinct outcomes, and conflating any two of them is a bug:
/// * `Ok(Some(_))` — this process is the launcher. Hold the token for life.
/// * `Ok(None)` — a peer already supervises. The caller logs and `exit(0)`s;
///   this is a SUCCESSFUL outcome, not a failure.
/// * `Err(_)` — the mutex could not be created at all (see `MUTEX_NAME` on the
///   `Global\` privilege). The caller WARNs and CONTINUES: treating this as
///   already-held would make the launcher silently refuse to start, which is
///   the black screen this whole crate exists to prevent.
#[cfg(windows)]
pub fn acquire_single_instance() -> io::Result<Option<SingleInstance>> {
    acquire_named(MUTEX_NAME)
}

/// The body of [`acquire_single_instance`], parameterised by name so the tests
/// can exercise it without touching (or leaking) the production `Global\` name.
#[cfg(windows)]
fn acquire_named(name: &str) -> io::Result<Option<SingleInstance>> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // Safety: `wide` is a valid null-terminated UTF-16 buffer alive for the
    // call; the other arguments are a null SECURITY_ATTRIBUTES and a plain
    // BOOL, matching the documented `CreateMutexW` signature.
    let handle = unsafe { win32::CreateMutexW(std::ptr::null_mut(), 1, wide.as_ptr()) };
    // Read the error immediately: anything else on this thread could clobber it.
    let err = unsafe { win32::GetLastError() };
    if handle.is_null() {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    // Safety: `handle` is a fresh, non-null, exclusively-owned kernel handle.
    let owned = unsafe { OwnedHandle::from_raw_handle(handle) };
    if err == win32::ERROR_ALREADY_EXISTS {
        // A peer holds it. `CreateMutexW` still returned a valid handle to the
        // EXISTING object — dropping `owned` closes it, so this branch leaks
        // nothing and, critically, does not keep the name alive after the real
        // holder exits.
        drop(owned);
        return Ok(None);
    }
    Ok(Some(SingleInstance(owned)))
}

#[cfg(not(windows))]
pub fn acquire_single_instance() -> io::Result<Option<SingleInstance>> {
    acquire_single_instance_at(Path::new("/var/lib/kiosk"))
}

#[cfg(not(windows))]
pub fn acquire_single_instance_at(data_dir: &Path) -> io::Result<Option<SingleInstance>> {
    std::fs::create_dir_all(data_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data_dir.join("launcher.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(SingleInstance(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(unix)]
#[cfg(test)]
mod unix_tests {
    use super::*;

    #[test]
    fn job_create_reports_missing_supervision_when_invocation_id_is_absent() {
        std::env::remove_var("INVOCATION_ID");
        assert!(Job::create().is_err());
    }

    #[test]
    fn a_second_launcher_does_not_acquire_the_lock() {
        let dir = std::env::temp_dir().join(format!(
            "kiosk-launcher-lock-{}-{}",
            std::process::id(),
            crate::clock::now()
        ));
        let first = acquire_single_instance_at(&dir).expect("first lock attempt");
        assert!(first.is_some());
        let second = acquire_single_instance_at(&dir).expect("second lock attempt");
        assert!(second.is_none());
        drop(first);
        std::fs::remove_dir_all(dir).expect("test lock directory cleanup");
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A `Local\` name unique to this test process: no SeCreateGlobalPrivilege
    /// needed, no collision with a real launcher running on the dev box, and
    /// nothing left behind for the rest of the suite.
    fn test_name(tag: &str) -> String {
        format!(r"Local\kiosk-launcher-test-{tag}-{}", std::process::id())
    }

    /// The single-instance contract, end to end: the first caller wins, a
    /// second caller sees the peer (and does NOT mistake it for a creation
    /// failure), and the name is only released when the winner's handle closes.
    ///
    /// The third acquire is what proves the second one did not quietly steal or
    /// close the first token — if it had, the name would already be free.
    #[test]
    fn a_second_acquire_sees_the_peer_and_the_first_token_still_holds_the_name() {
        let name = test_name("single");

        let first = acquire_named(&name)
            .expect("creating a Local\\ mutex must succeed")
            .expect("nothing else can hold a per-PID name");

        assert!(
            acquire_named(&name)
                .expect("an already-held name is Ok(None), never Err")
                .is_none(),
            "a peer holding the name must report as Ok(None), so the caller exits 0"
        );

        drop(first);
        assert!(
            acquire_named(&name).expect("still creatable").is_some(),
            "the name is only free once the holder's handle closes — so the \
             second acquire above did not disturb the first token"
        );
    }

    /// Kill-on-close, for real: a child assigned to the job must die when the
    /// `Job` is dropped, with no cooperation from the child and no `kill()`
    /// call. This is the behaviour the whole module exists for, and the only
    /// part of it a host test can reach.
    ///
    /// `ping -n 30` is a ~30 s process with no display, no network egress
    /// (loopback) and no arguments to get wrong.
    #[test]
    fn dropping_the_job_kills_the_child_assigned_to_it() {
        let job = Job::create().expect("job object creation");

        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("ping.exe spawns");
        job.assign(&child)
            .expect("assigning a fresh child succeeds");

        assert!(
            child.try_wait().expect("try_wait").is_none(),
            "the child must still be running before the job closes"
        );

        drop(job); // last handle closed => the kernel kills the job's processes

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait().expect("try_wait") {
                Some(_) => break,
                None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                None => {
                    let _ = child.kill();
                    panic!("the child outlived the job object: kill-on-close is not armed");
                }
            }
        }
    }
}
