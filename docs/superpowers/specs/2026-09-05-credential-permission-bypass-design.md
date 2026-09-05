# MVP Credential Permission Bypass Design

> Approved by the operator on 2026-09-05 for the unsecured MVP deployment.

## Goal

Allow an explicitly configured MVP kiosk to load its credential even when the Windows
credential file is not owner-only, while preserving the existing secure default.

## Configuration

Add the optional boolean `[kiosk] allow_insecure_credential_permissions` to
`BootstrapConfig`. A missing value parses as `false`. The local bootstrap file is the only
source for this setting; remote configuration cannot enable it.

## Runtime behavior

`kiosk-main` and `kiosk-launcher` apply the same rule:

- `false`: run the existing owner/SYSTEM ACL check and fail closed on a violation.
- `true`: skip only the ACL check, then continue to read and validate the credential JSON.

Each process emits a warning when the bypass is active. A malformed, missing, or unreadable
credential still fails normally. Supervision behavior is unchanged.

## Testing

Add parser coverage for the default and explicit `true` values. Add boot coverage proving a
bad ACL is bypassed only when the flag is enabled, while existing default-false tests continue
to prove fail-closed behavior. Keep launcher and main checks synchronized.

## Scope

Change only the shared bootstrap schema, the two credential gates, their tests, and the local
MVP configuration. Do not remove the ACL implementation or change remote configuration,
credential parsing, or filesystem permissions.
