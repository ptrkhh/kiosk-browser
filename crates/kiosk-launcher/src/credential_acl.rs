//! Win32 edge for SEC-09 (spec §8): read the credential file's owner SID and
//! DACL, extract the SIDs granted read access, and hand them to the pure
//! judgment in `kiosk_core::acl::is_read_owner_only`. This module does no
//! security reasoning of its own — it only mechanically extracts SIDs from the
//! Win32 security descriptor; the owner/SYSTEM comparison lives in kiosk-core
//! so it stays host-testable and is not duplicated here.
//!
//! # Fail closed
//! Any error reading the security info (missing file, access denied, malformed
//! ACL) is `Err`. The caller (boot/reload wiring, not this module) treats
//! `Ok(false)` and `Err` identically — refuse to trust the credential.
//!
//! Wired into `sink::build_telemetry` (Task 3, SEC-09): a bad DACL skips
//! building the GCL client but never stops supervision.

use std::io;
use std::path::Path;

/// Fail-closed judgment on a raw [`credential_is_owner_only`] result (spec §8/
/// SEC-09): only `Ok(true)` is trusted. `Ok(false)` (a non-owner principal can
/// read the file) and `Err` (the security info could not be read at all) are
/// both violations — every caller must refuse to use the credential on either
/// outcome, never just log it.
pub fn is_violation(check: io::Result<bool>) -> bool {
    !matches!(check, Ok(true))
}

/// `Ok(true)` iff the file's DACL grants read to nobody but its owner or
/// SYSTEM; `Ok(false)` if some other SID can read it; `Err` if the security
/// info could not be read at all. Non-Windows dev hosts (the kiosk target is
/// Windows x64 only) stub this to `Ok(true)` — there is no DACL to check.
#[cfg(windows)]
pub fn credential_is_owner_only(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut owner_sid = PSID(std::ptr::null_mut());
    let mut dacl_ptr: *mut ACL = std::ptr::null_mut();
    let mut sd = PSECURITY_DESCRIPTOR(std::ptr::null_mut());

    // Safety: `wide` is a valid null-terminated UTF-16 buffer alive for the
    // call. `owner_sid`, `dacl_ptr` and `sd` are valid local out-params. On
    // success `sd` owns OS-allocated memory (the owner SID and DACL point
    // into it) that must be freed exactly once; on failure (non-zero return)
    // nothing is allocated and there is nothing to free.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(&mut owner_sid as *mut _),
            None,
            Some(&mut dacl_ptr as *mut _),
            None,
            &mut sd as *mut _,
        )
    };
    if status.0 != 0 {
        return Err(io::Error::from_raw_os_error(status.0 as i32));
    }
    // From here on `sd` MUST be freed on every path, including every `?`
    // early return below — `SdGuard::drop` does that unconditionally.
    let _guard = SdGuard(sd);

    if owner_sid.0.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GetNamedSecurityInfoW returned no owner SID",
        ));
    }
    let owner_sid_string = sid_to_string(owner_sid)?;

    // A NULL DACL (the pointer itself, not an empty-but-present one) means
    // "no DACL" — Win32 treats that as granting Everyone full access. That is
    // NOT the same thing as a present DACL with zero ACEs, which denies
    // everyone and is handled by `read_grantee_sids` returning an empty
    // `Vec` below. Conflating the two would fail OPEN, so a null pointer here
    // is an immediate violation.
    if dacl_ptr.is_null() {
        return Ok(false);
    }
    let read_sids = read_grantee_sids(dacl_ptr)?;

    Ok(kiosk_core::acl::is_read_owner_only(
        &read_sids,
        &owner_sid_string,
    ))
}

/// Linux credential gate. The packaged service runs as root and provisions the
/// credential as root-owned `0600`; mode bits are therefore the complete
/// proof needed here. If P2-G ever adds a non-root `User=`, this must gain an
/// owner-UID check at the same time.
#[cfg(unix)]
pub fn credential_is_owner_only(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)?.permissions().mode();
    Ok(mode & 0o077 == 0)
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(mode: u32) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "kiosk-credential-acl-{}-{}-{}.json",
            std::process::id(),
            mode,
            crate::clock::now()
        ));
        std::fs::write(&path, b"{}\n").expect("fixture");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("permissions");
        path
    }

    #[test]
    fn owner_only_mode_passes() {
        let path = fixture(0o600);
        assert!(credential_is_owner_only(&path).expect("mode check"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn group_or_other_read_modes_fail() {
        for mode in [0o640, 0o604, 0o666] {
            let path = fixture(mode);
            assert!(!credential_is_owner_only(&path).expect("mode check"));
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn missing_credential_fails_closed() {
        let path = std::env::temp_dir().join(format!(
            "kiosk-credential-acl-missing-{}-{}",
            std::process::id(),
            crate::clock::now()
        ));
        let check = credential_is_owner_only(&path);
        assert!(check.is_err());
        assert!(is_violation(check));
    }
}

/// Frees the security descriptor `GetNamedSecurityInfoW` allocated, on every
/// path out of `credential_is_owner_only` — including early `?` returns —
/// via `Drop`, so the free can't be missed by adding a new return later.
#[cfg(windows)]
struct SdGuard(windows::Win32::Security::PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl Drop for SdGuard {
    fn drop(&mut self) {
        // Safety: `self.0` was allocated by `GetNamedSecurityInfoW`, which we
        // only reach `SdGuard::drop` after it returned success; freed here
        // exactly once.
        unsafe {
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(self.0 .0),
            ));
        }
    }
}

/// Walks `dacl`'s ACEs and returns the string SID of every one that grants
/// read (see [`mask_grants_read`]). An ACE type this module cannot classify
/// (object ACEs, callback ACEs, conditional ACEs, ...) is treated as granting
/// read — fail closed, since we cannot prove it does not.
#[cfg(windows)]
fn read_grantee_sids(dacl: *const windows::Win32::Security::ACL) -> io::Result<Vec<String>> {
    use windows::Win32::Security::{GetAce, ACCESS_ALLOWED_ACE, ACE_HEADER, PSID};
    use windows::Win32::System::SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE};

    // Safety: `dacl` is the non-null DACL pointer `GetNamedSecurityInfoW`
    // wrote, inside the security descriptor kept alive by the caller's
    // `SdGuard` for the duration of this call.
    let count = unsafe { (*dacl).AceCount } as u32;

    let mut sids = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut ace_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        // Safety: `dacl` is valid (see above); `index` is bounds-checked
        // against `AceCount` by this loop, and `GetAce` itself range-checks
        // it again against the ACL's own bookkeeping.
        unsafe { GetAce(dacl, index, &mut ace_ptr) }.map_err(io::Error::from)?;
        if ace_ptr.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GetAce returned a null ACE",
            ));
        }
        // Safety: every ACE, of every type, begins with an ACE_HEADER — a
        // documented Win32 layout invariant. `ace_ptr` is non-null and was
        // produced by the successful `GetAce` call above.
        let header = unsafe { &*(ace_ptr as *const ACE_HEADER) };
        match header.AceType as u32 {
            ACCESS_ALLOWED_ACE_TYPE => {
                // Safety: `AceType == ACCESS_ALLOWED_ACE_TYPE` guarantees this
                // memory is laid out as `ACCESS_ALLOWED_ACE` (a mask followed
                // by the SID bytes starting at `SidStart`).
                let ace = unsafe { &*(ace_ptr as *const ACCESS_ALLOWED_ACE) };
                if mask_grants_read(ace.Mask) {
                    // Safety: `SidStart`'s address is the start of the ACE's
                    // embedded SID; it is valid for as long as the ACE itself
                    // (i.e. as long as `sd`, kept alive by the caller).
                    let sid = PSID(std::ptr::addr_of!(ace.SidStart) as *mut _);
                    sids.push(sid_to_string(sid)?);
                }
            }
            ACCESS_DENIED_ACE_TYPE => {
                // A deny ACE never grants access; nothing to collect.
            }
            other => {
                // Unclassifiable ACE type: we cannot parse its layout, so we
                // cannot prove it does not grant read. Fail closed by
                // synthesizing a SID string that can never equal the owner or
                // SYSTEM, forcing `is_read_owner_only` to `false`.
                sids.push(format!("unclassified-ace-type-{other}"));
            }
        }
    }
    Ok(sids)
}

/// True iff `mask` grants any bit this module treats as "can read the file's
/// contents": `FILE_GENERIC_READ`, `GENERIC_READ`, `FILE_READ_DATA` (the
/// actual bit set inside `FILE_GENERIC_READ`), any of the generic rights
/// (`GENERIC_ALL`, `GENERIC_READ`, `GENERIC_WRITE`, `GENERIC_EXECUTE` —
/// `GENERIC_ALL` is `0x1000_0000`, which does not intersect the
/// specific-rights bits above as a raw bit), or `MAXIMUM_ALLOWED`
/// (`0x0200_0000`).
///
/// `MapGenericMask` performs the same generic->specific expansion the OS
/// itself does when it evaluates access checks, so a `GENERIC_*` bit in
/// `mask` is judged exactly as Windows would. `MAXIMUM_ALLOWED` is NOT one
/// of the four bits `MapGenericMask` touches (it maps only
/// `GENERIC_{READ,WRITE,EXECUTE,ALL}`), so it is checked directly instead;
/// Windows does not accept `MAXIMUM_ALLOWED` in a stored ACE at all (it is
/// only meaningful in an access-check request), so this is defense in depth
/// against an already-invalid mask rather than a real access grant this
/// function has ever been observed to receive — same as the `GENERIC_*` bits
/// above (verified empirically while building this fix: a real on-disk NTFS
/// ACE never carries them either, since `SetNamedSecurityInfoW`/NTFS maps
/// generic rights to specific rights at write time — see this module's
/// `generic_all_is_a_read_grant` test doc comment for the trace). Kept as
/// cheap, correct defense in depth against a caller or filesystem this
/// module has not been observed to encounter, not a fix for a
/// currently-reachable fail-open.
#[cfg(windows)]
fn mask_grants_read(mask: u32) -> bool {
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Security::{MapGenericMask, GENERIC_MAPPING};
    use windows::Win32::Storage::FileSystem::{
        FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_READ_DATA,
    };

    let mut mapped = mask;
    let mapping = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ.0,
        GenericWrite: FILE_GENERIC_WRITE.0,
        GenericExecute: FILE_GENERIC_EXECUTE.0,
        GenericAll: FILE_ALL_ACCESS.0,
    };
    // Safety: `mapped` is a valid local `u32` out-param; `mapping` is a fully
    // populated, stack-local `GENERIC_MAPPING` alive for the call. Pure bit
    // manipulation on the caller's own memory — no handles, no allocation.
    unsafe { MapGenericMask(&mut mapped, &mapping) };

    const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
    const READ_BITS: u32 =
        FILE_GENERIC_READ.0 | GENERIC_READ.0 | FILE_READ_DATA.0 | MAXIMUM_ALLOWED;
    mapped & READ_BITS != 0
}

/// Converts a Win32 `PSID` to its string form (`S-1-5-...`), freeing the
/// OS-allocated buffer on every path.
#[cfg(windows)]
fn sid_to_string(sid: windows::Win32::Security::PSID) -> io::Result<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;

    let mut buf = PWSTR::null();
    // Safety: `sid` points at a valid SID (the owner SID or an ACE's embedded
    // SID) inside the still-live security descriptor; `buf` receives a
    // LocalAlloc'd string this function frees below on every path.
    unsafe { ConvertSidToStringSidW(sid, &mut buf) }.map_err(io::Error::from)?;

    // Safety: the successful call above guarantees `buf` is non-null and
    // points to a null-terminated UTF-16 string, valid until freed below.
    let result = unsafe { buf.to_string() }.map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "SID string was not valid UTF-16",
        )
    });

    // Safety: `buf.0` was allocated by `ConvertSidToStringSidW` via
    // `LocalAlloc` per its documented contract; freed here exactly once,
    // regardless of whether decoding it above succeeded.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(buf.0 as *mut _)));
    }
    result
}

#[cfg(test)]
mod is_violation_tests {
    use super::is_violation;
    use std::io;

    #[test]
    fn ok_true_is_not_a_violation() {
        assert!(!is_violation(Ok(true)));
    }

    #[test]
    fn ok_false_is_a_violation() {
        assert!(is_violation(Ok(false)));
    }

    #[test]
    fn err_is_a_violation() {
        assert!(is_violation(Err(io::Error::other("access denied"))));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::mask_grants_read;

    #[test]
    fn file_generic_read_is_a_read_grant() {
        use windows::Win32::Storage::FileSystem::FILE_GENERIC_READ;
        assert!(mask_grants_read(FILE_GENERIC_READ.0));
    }

    #[test]
    fn generic_read_is_a_read_grant() {
        use windows::Win32::Foundation::GENERIC_READ;
        assert!(mask_grants_read(GENERIC_READ.0));
    }

    #[test]
    fn file_read_data_alone_is_a_read_grant() {
        use windows::Win32::Storage::FileSystem::FILE_READ_DATA;
        assert!(mask_grants_read(FILE_READ_DATA.0));
    }

    #[test]
    fn write_only_mask_is_not_a_read_grant() {
        use windows::Win32::Storage::FileSystem::FILE_WRITE_DATA;
        assert!(!mask_grants_read(FILE_WRITE_DATA.0));
    }

    #[test]
    fn zero_mask_is_not_a_read_grant() {
        assert!(!mask_grants_read(0));
    }

    /// FIX 1 (SEC-09 final review, Critical): `GENERIC_ALL` (0x1000_0000) does not
    /// intersect any of `FILE_GENERIC_READ`/`GENERIC_READ`/`FILE_READ_DATA` as raw
    /// bits, so a naive bitmask check misses a mask carrying only this bit. Pins
    /// `mask_grants_read`'s contract directly at the unit level; fails against the
    /// pre-fix `READ_BITS`-only check. NOTE: verified empirically while building
    /// this fix that a REAL on-disk NTFS ACE never actually carries this literal
    /// value — `SetNamedSecurityInfoW`/NTFS maps `GENERIC_ALL` to the specific
    /// `FILE_ALL_ACCESS` mask at write time, for both `icacls`'s `(GA)` shorthand
    /// and a hand-built SDDL string passed straight to the Win32 API — so this is
    /// defense in depth against a caller (or filesystem) this module has not been
    /// observed to encounter, not a currently-reachable fail-open.
    #[test]
    fn generic_all_is_a_read_grant() {
        use windows::Win32::Foundation::GENERIC_ALL;
        assert!(mask_grants_read(GENERIC_ALL.0));
    }

    /// Same gap as `GENERIC_ALL`: `MAXIMUM_ALLOWED` (0x0200_0000) also does not
    /// intersect the specific-rights bits directly. Windows does not accept
    /// `MAXIMUM_ALLOWED` inside a stored ACE at all (it is only meaningful in an
    /// access-check request) — this pins `mask_grants_read`'s behavior on the raw
    /// value regardless, since the function has no way to know that.
    #[test]
    fn maximum_allowed_is_a_read_grant() {
        const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
        assert!(mask_grants_read(MAXIMUM_ALLOWED));
    }
}
