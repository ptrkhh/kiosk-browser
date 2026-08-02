# P1-G — Credential-at-Rest Hardening (SEC-09 DACL Check) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST:** T1 (kiosk-core `acl::is_read_owner_only`) is host-testable (`cargo test -p kiosk-core`). T2–T3 (`GetNamedSecurityInfoW` read + the boot/reload safe-mode wiring) are Windows-host build + smoke.

**Goal:** Enforce the credential's at-rest protection — verify `kiosk-credential.json`'s DACL is owner-only at boot and every config reload, and refuse + enter safe mode (render `safe.html`, spool `config.error`, keep last-good) if it is not. Closes the SEC-09 release blocker.

**Architecture:** The security decision (which SIDs may read) is a pure kiosk-core function, host-tested adversarially. The Win32 `GetNamedSecurityInfoW` read that extracts the read-granting SIDs is a thin per-binary edge. The failure reuses the existing F1/F2 `config_error → safe.html` render path.

**Tech Stack:** Rust, the `windows` crate (`Win32_Security`), `kiosk-core`.

**Design spec:** `docs/superpowers/specs/2026-08-02-p1g-credential-at-rest-design.md`. Requirement: parent spec §8/SEC-09.

## Global Constraints

- **Fail closed.** A bad DACL (`is_read_owner_only == false`) OR any error reading the security info (missing file, access denied, malformed ACL) → treat as a violation → safe mode. Never proceed on an unverifiable DACL, never panic.
- **The security decision lives in the pure, host-tested function** — the Win32 layer only mechanically extracts the read-granting SIDs + the owner SID. `SYSTEM` = `S-1-5-18`; owner = the file's owner SID (the account the MSI ACL'd it to). Any other read grantee (Everyone `S-1-1-0`, BUILTIN\Users `S-1-5-32-545`, Authenticated Users `S-1-5-11`, Administrators `S-1-5-32-544`, …) ⇒ NOT owner-only.
- **Supervision must not depend on telemetry.** A bad credential DACL degrades telemetry (kiosk-main → safe page; launcher → no GCL client) but the launcher keeps supervising.
- **Reuse the merged safe path** (F1/F2): a DACL violation produces a `config_error` that feeds the existing `safe = args.safe || config_error.is_some()` render at `main.rs:~380` — do NOT invent a second safe-mode mechanism.
- **`config.error` reason is `credential_permissions`**, reported distinctly from signature/rollback/device-binding rejections.
- Windows bits `#[cfg(windows)]`; the non-Windows stub returns `Ok(true)` (Linux `0600` check = P2).

## Interfaces this plan uses (merged)

```rust
// kiosk-main/src/telemetry.rs
pub fn build(bootstrap, credential_path: &Path, device_id, clock, app_version, revision, data_dir)
    -> Result<(Telemetry, Logger, Receiver<LogReq>), Box<dyn Error>>   // reads credential at :202
// kiosk-main/src/main.rs: ~:380  let (booted, config_error) = ...;  let safe = args.safe || config_error.is_some();
//   safe_error feeds bundled_url("safe.html")?device=&err=  (the render-safe path to reuse)
// kiosk-main/src/boot.rs: safe_boot(data_dir, machine_id) -> Booted; APP_SAFE_URL; assert_render_safe (test helper)
// kiosk-launcher builds its own telemetry stack (its own credential read) — same gate needed there.
// windows crate 0.61 already a kiosk-main dep (add Win32_Security feature); add to kiosk-launcher too.
```

---

### Task 1: kiosk-core `acl::is_read_owner_only` (host-tested security core)

**Files:** Create `crates/kiosk-core/src/acl.rs`; modify `lib.rs` (`pub mod acl;`); test in `acl.rs`

**Interfaces:**
- Produces: `fn is_read_owner_only(read_grantee_sids: &[String], owner_sid: &str) -> bool`; `const SYSTEM_SID: &str = "S-1-5-18"`.

- [ ] **Step 1: failing tests.**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const OWNER: &str = "S-1-5-21-1111-2222-3333-1001"; // the kiosk account
    fn v(sids: &[&str]) -> Vec<String> { sids.iter().map(|s| s.to_string()).collect() }

    #[test] fn owner_and_system_only_is_owner_only() {
        assert!(is_read_owner_only(&v(&[OWNER, SYSTEM_SID]), OWNER));
    }
    #[test] fn owner_only_is_owner_only() { assert!(is_read_owner_only(&v(&[OWNER]), OWNER)); }
    #[test] fn empty_dacl_is_owner_only() { assert!(is_read_owner_only(&v(&[]), OWNER)); }
    #[test] fn everyone_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&[OWNER, "S-1-1-0"]), OWNER));           // Everyone
    }
    #[test] fn builtin_users_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&[OWNER, SYSTEM_SID, "S-1-5-32-545"]), OWNER)); // BUILTIN\Users
    }
    #[test] fn authenticated_users_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&["S-1-5-11"]), OWNER));                 // Authenticated Users
    }
    #[test] fn administrators_read_is_a_violation() {
        assert!(!is_read_owner_only(&v(&[OWNER, "S-1-5-32-544"]), OWNER));      // Administrators != owner
    }
}
```

Run `cargo test -p kiosk-core acl::` → FAIL.

- [ ] **Step 2: implement.**

```rust
//! The owner-only credential-DACL decision (spec §8/SEC-09). Pure: the Win32 layer
//! (kiosk-main/kiosk-launcher) reads the DACL and hands us the read-granting SIDs; the
//! security judgment — only the file's owner and SYSTEM may read — lives here so it is
//! host-testable and adversarially covered.

/// The well-known local SYSTEM account.
pub const SYSTEM_SID: &str = "S-1-5-18";

/// True iff every SID granted read is the file's owner or SYSTEM. Any other read grantee
/// (Everyone, BUILTIN\Users, Authenticated Users, Administrators, a stray account) makes the
/// credential NOT owner-only → the caller fails closed. An empty read set is owner-only.
pub fn is_read_owner_only(read_grantee_sids: &[String], owner_sid: &str) -> bool {
    read_grantee_sids
        .iter()
        .all(|sid| sid == owner_sid || sid == SYSTEM_SID)
}
```

Run → PASS. fmt/clippy clean, commit `feat(core): acl::is_read_owner_only — the SEC-09 credential-DACL decision`.

---

### Task 2: Win32 DACL read (kiosk-main + kiosk-launcher, Windows)

**Files:** Create `crates/kiosk-main/src/credential_acl.rs` + `crates/kiosk-launcher/src/credential_acl.rs` (or one shared module referenced by both — a small `mod` in each is acceptable given the crate boundary); modify both `Cargo.toml` (`windows` `Win32_Security` feature)

**Interfaces:**
- Produces: `fn credential_is_owner_only(path: &Path) -> io::Result<bool>` — `Ok(true)`/`Ok(false)` for a readable DACL, `Err` when the security info can't be read (caller fails closed on both `Ok(false)` and `Err`).

- [ ] **Step 1: the read.** `#[cfg(windows)]`: `GetNamedSecurityInfoW(path, SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION)` → the owner SID + the DACL. Walk the DACL's ACEs; for each **allow** ACE whose mask grants read (`FILE_GENERIC_READ` / `GENERIC_READ` / `FILE_READ_DATA` bits), convert its SID to a string (`ConvertSidToStringSidW`); collect these into `Vec<String>`. Convert the owner SID to a string. Then `kiosk_core::acl::is_read_owner_only(&read_sids, &owner_sid_string)`. An **unknown/unclassifiable ACE that grants read** counts as a read grantee (fail closed — it becomes a non-owner SID). Free the security descriptor (`LocalFree`). Non-Windows stub: `Ok(true)`.

- [ ] **Step 2: no-panic + free.** All Win32 handles/pointers freed on every path; a null/failed `GetNamedSecurityInfoW` → `Err` (never panic, never deref null). Guard the ACE walk against a malformed ACL (bounds-check `AceCount`).

- [ ] **Step 3: Windows smoke (in the report).** On a file the MSI ACL'd owner-only → `Ok(true)`; after `icacls <file> /grant "BUILTIN\Users:R"` → `Ok(false)`; on a path with no read access to the SD → `Err`.

- [ ] **Step 4:** fmt/clippy (`cargo clippy -p kiosk-main --no-deps`, `-p kiosk-launcher`), commit `feat(main,launcher): read the credential DACL (GetNamedSecurityInfoW) -> owner-only check`.

---

### Task 3: fail-closed wiring — boot + reload + launcher (Windows)

**Files:** Modify `crates/kiosk-main/src/main.rs` (boot + reload), `crates/kiosk-main/src/boot.rs` (the `config_error` source), `crates/kiosk-launcher/src/main.rs` / `sink.rs` (launcher gate)

- [ ] **Step 1: boot gate (kiosk-main).** Before `telemetry::build` reads the credential, call `credential_is_owner_only(credential_path)`. `Ok(false)` or `Err(_)` → produce a `config_error` with reason `credential_permissions` and a message "credential file permissions are not owner-only — refusing to load" (the same `config_error: Option<...>` that `main.rs:~380` already turns into `safe = ... || config_error.is_some()` + `safe.html?err=`). Do NOT call `telemetry::build` in that case (no credential read). Last-good content still displays via the existing safe path; spool a `config.error{reason:"credential_permissions"}` (reuse the boot spool path the config-parse fault uses).

- [ ] **Step 2: reload gate (kiosk-main).** In the config-poll cycle, before any path that (re)reads the credential / rebuilds telemetry, re-check `credential_is_owner_only`. A DACL that went bad since boot → emit `config.error{credential_permissions}` and transition to the safe render (navigate `safe.html`) — the same fail-closed outcome as boot. (A cheap security-info read per poll interval; do not read the credential contents.)

- [ ] **Step 3: launcher gate.** Before the launcher builds its own telemetry stack (its credential read), call `credential_is_owner_only`. Bad/Err → log to the launcher's own spool + **skip building the GCL client** (run telemetry-less) but **keep supervising** (spawn/watch/restart unaffected — supervision must survive a bad credential). A `#[cfg(not(windows))]` build keeps today's behaviour.

- [ ] **Step 4: Windows smoke (composition).** MSI-installed owner-only ACL → app runs normally, telemetry flows. `icacls kiosk-credential.json /grant "BUILTIN\Users:R"` then reboot/reload → kiosk-main shows `safe.html` "credential permissions" + spools `config.error{credential_permissions}` + keeps last-good; the launcher logs the refusal but still supervises (kill main → it restarts). Restore the ACL → next boot runs normally.

- [ ] **Step 5:** fmt/clippy, commit `feat(main,launcher): fail closed to safe mode on a non-owner-only credential DACL (SEC-09)`.

---

## Self-Review

**Spec coverage (P1-G design / §8 SEC-09):** owner-only decision → T1; Win32 DACL read → T2; boot + reload + launcher fail-closed-to-safe-mode wiring → T3; `config.error{credential_permissions}` distinct reason + last-good display + supervision-survives → T3 constraints. The ACL'd-file-is-accepted-mode decision + DPAPI-deferral is documented in the design + the F2 packaging README (no code). **Covered.** Deferred: DPAPI keystore (P2), Linux `0600` check (P2), Android Keystore (P3).

**Placeholder scan:** T1 has full runnable host code + the adversarial SID truth table (Everyone / BUILTIN\Users / Authenticated Users / Administrators all violations). T2/T3 name real Win32 APIs (`GetNamedSecurityInfoW`, `ConvertSidToStringSidW`, `LocalFree`) + the exact merged reuse points (`telemetry::build`, `main.rs:~380` `config_error`, `boot.rs` safe path) with "confirm against the crate at impl time" pointers, as prior Windows plans did. No invented interfaces.

**Type consistency:** `is_read_owner_only`/`SYSTEM_SID` (T1) consumed by `credential_is_owner_only` (T2), consumed by the boot/reload/launcher gates (T3). The `config_error{credential_permissions}` reason threads T3 → the existing safe render. `credential_is_owner_only` signature identical in both binaries.

**Scope:** One sub-project (SEC-09 credential-at-rest). Three tasks; T1 host-tested (the security core), T2/T3 Windows with smoke. DPAPI + Linux/Android deferred. After P1-G + the outstanding hardware smokes, P1 is shippable-secure.
