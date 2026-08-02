# P1-G — Credential-at-Rest Hardening (SEC-09 Fail-Closed DACL Check) (Design)

> The last P1 code item — closes the SEC-09 release blocker so the Windows MVP is
> shippable-secure. Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §8/SEC-09.
> Builds on P1-F1 (`--safe` / `safe.html` render) and P1-F2 (the MSI that sets the ACL).

**Status:** approved 2026-08-02 (design). The owner-only ACE decision is host-testable
(kiosk-core); the `GetNamedSecurityInfoW` read + the safe-mode wiring are Windows-host.

## Goal

Make the credential's at-rest protection **enforced**, not just installed. The F2 MSI sets an
owner-only DACL on `kiosk-credential.json`; P1-G makes the app **verify** that DACL at boot and
on every config reload and **refuse + enter safe mode** if it is not owner-only — the fail-closed
requirement SEC-09 emphasizes ("refuses to load ... enters safe mode ... rather than only
logging"). Without this, an attacker who loosens the credential's ACL (or a mis-imaged device)
runs with a world-readable service-account key and nothing notices.

## Decision: ACL'd file is the accepted P1 credential-at-rest mode (DPAPI → P2)

SEC-09 says "OS keystore, not a flat file" but **explicitly accepts** a properly-permissioned
file (Linux `root:root 0600`) as a valid restrictive mode. The Windows equivalent is an
**owner-only-ACL'd file + the runtime DACL check** below. Full DPAPI/Credential Manager is
stronger but is deferred to **P2**: the deployment model (MSI ships a placeholder; the operator
drops the real credential per device) makes DPAPI awkward — it would need a first-run
read-flat → `CryptProtectData` → delete-flat import, adding a plaintext window and provisioning
complexity for defense-in-depth the fail-closed ACL check already covers. This is documented in
the packaging README + lockdown runbook, not left implicit.

## Component 1 — the owner-only decision (kiosk-core, pure, host-tested)

A pure function that, given the resolved DACL as a list of allow-ACEs, decides whether the file
is owner-only. This is where the security judgment lives, so it is where the adversarial tests
live.

```rust
// crates/kiosk-core/src/acl.rs  (pure, no Win32)
pub struct Ace { pub sid: Sid, pub rights_read: bool }   // simplified: who can read
pub enum Sid { KioskAccount, System, Administrators, Everyone, BuiltinUsers, Other(String) }
/// Owner-only ⇔ every read-granting ACE is the kiosk account or SYSTEM. Any read grant to
/// Everyone / BUILTIN\Users / Authenticated Users / a non-owner principal ⇒ NOT owner-only
/// (fail closed). An empty/deny-only DACL is owner-only (no one can read but the owner via
/// ownership — conservative: treat "no explicit read ACE for a broad group" as owner-only,
/// "any broad read ACE present" as a violation).
pub fn is_owner_only(aces: &[Ace], kiosk_account: &Sid) -> bool;
```

Host tests (adversarial): owner + SYSTEM only → true; add `BUILTIN\Users` read → false;
`Everyone` read → false; `Authenticated Users` read → false; owner + SYSTEM + Administrators →
false (Administrators is not the kiosk account); empty → true. The exact `Sid` modelling
(well-known SIDs vs the resolved kiosk account SID) is refined in the plan against what the
Win32 read actually yields — the *decision* is the tested part.

## Component 2 — the Win32 DACL read (kiosk-main + kiosk-launcher, Windows edge)

A thin per-binary helper (both load the credential — kiosk-main's telemetry stack and the
launcher's own): `fn credential_is_owner_only(path: &Path, kiosk_account: &Sid) -> io::Result<bool>`
→ `GetNamedSecurityInfoW(path, SE_FILE_OBJECT, DACL_SECURITY_INFORMATION)` → walk the ACL's
ACEs into `acl::Ace`s → `acl::is_owner_only(...)`. `#[cfg(windows)]`; the non-Windows stub returns
`Ok(true)` (Linux uses the `0600` check — P2). Resolve "the kiosk account" as the file's **owner
SID** (the account the MSI ACL'd it to), so the check is "only the owner + SYSTEM can read".

## Component 3 — fail-closed wiring (boot + reload)

- **Boot (both binaries):** before `fs::read_to_string(credential_path)` in the telemetry-stack
  build, call `credential_is_owner_only`. `Ok(false)` (bad DACL) or `Err` (unreadable security
  info) → **do NOT read/build telemetry**; kiosk-main renders `safe.html` (device id + "credential
  DACL not owner-only — refusing to load", reusing the F1/F2 safe path) and spools a `config.error`
  with a distinct reason (`credential_permissions`); the launcher logs to its own spool + does not
  build its GCL client (it still supervises — supervision must not depend on telemetry). Last-good
  content still displays (SEC-09: "last-good display").
- **Reload (kiosk-main):** the config-poll path re-checks the DACL before each fetched-config apply
  cycle that would (re)read the credential; a DACL that went bad since boot → the same
  refuse-and-safe-mode + `config.error`. (Cheap: a stat-like security-info read per poll interval.)
- The `config.error{reason:"credential_permissions"}` is reported distinctly from a
  signature/rollback/device-binding rejection so an operator can tell a *permissions* problem from
  a *config* problem.

## Data flow

boot → `credential_is_owner_only(cred)` → **false/err** → render `safe.html` + spool
`config.error{credential_permissions}` + show last-good, telemetry NOT built → device is visibly
degraded (safe page) instead of silently running with an exposed key. **true** → proceed as today.
Same gate on each reload.

## Error handling

- `GetNamedSecurityInfoW` failure (file missing, access denied to read the SD) → treat as **not
  owner-only** (fail closed) → safe mode. Never proceed on an unverifiable DACL.
- The check must never panic on a malformed/exotic ACL — unknown ACE types are conservatively
  treated as a violation (a read grant we can't classify is a read grant we don't trust).
- Supervision (the launcher's watchdog loop) must keep running even when its telemetry is refused —
  a bad credential DACL degrades telemetry, not the kiosk's crash-recovery.

## Testing

- **Host-testable (kiosk-core `acl::is_owner_only`):** the full ACE truth table — owner+SYSTEM →
  ok; any broad-group read (Everyone / BUILTIN\Users / Authenticated Users) → violation;
  Administrators (non-owner) read → violation; empty/deny-only → ok; unknown ACE type → violation
  (fail closed). This is the security core.
- **Windows-host:** with the MSI-set owner-only ACL → the app runs normally; `icacls
  kiosk-credential.json /grant "BUILTIN\Users:R"` (loosen it) → on next boot/reload the device
  shows `safe.html` "credential DACL not owner-only", spools `config.error{credential_permissions}`,
  and keeps showing last-good; the launcher keeps supervising.

## Scope / defer

P1-G = the fail-closed DACL check + safe-mode wiring + the doc note that the ACL'd file is the
accepted P1 mode. Deferred: **DPAPI/Credential Manager keystore** (P2 hardening); the **Linux
`0600` check** (P2, when the Linux platform lands); Android Keystore (P3). After P1-G + the
outstanding hardware smokes, P1 is shippable-secure.
