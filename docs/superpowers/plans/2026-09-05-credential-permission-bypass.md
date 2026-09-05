# MVP Credential Permission Bypass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an explicit local bootstrap setting that lets the unsecured MVP skip the Windows credential ACL check while retaining secure-by-default behavior.

**Architecture:** Parse `allow_insecure_credential_permissions` once in shared `BootstrapConfig`. Both binaries consult it before their existing ACL gate. `true` skips only ACL evaluation; file access and JSON validation remain mandatory. The existing ACL implementation remains intact.

**Tech Stack:** Rust, `rust-ini`, Windows DACL checks, WSL Ubuntu Cargo for tests and Windows GNU release builds.

## Global Constraints

- The setting exists only in local `[kiosk]` bootstrap configuration.
- Missing/false preserves fail-closed behavior.
- True does not bypass missing or malformed credentials.
- Main and launcher behavior must stay synchronized.
- Do not remove or weaken the ACL helper.

---

### Task 1: Parse the opt-in setting

**Files:** `crates/kiosk-core/src/config/bootstrap.rs`, `crates/kiosk-core/src/config/mod.rs`, `crates/kiosk-main/src/telemetry.rs`.

- [x] Add parser tests proving omission defaults to `false` and `allow_insecure_credential_permissions = true` parses as `true`.
- [x] Run `wsl.exe -d Ubuntu -e bash -lc 'cd /mnt/d/git/kiosk-browser && cargo test -p kiosk-core insecure_credential_permission_bypass'` and confirm the new tests fail before implementation.
- [x] Add `pub allow_insecure_credential_permissions: bool` to `BootstrapConfig`, parse it with the existing boolean values (`true/1/yes` and `false/0/no`), defaulting to `false`, and initialize all struct fixtures.
- [x] Run `cargo test -p kiosk-core --quiet`; expect zero failures.

### Task 2: Gate kiosk-main

**Files:** `crates/kiosk-main/src/boot.rs` and its existing Windows ACL tests.

- [x] Add a Windows test that grants `BUILTIN\\Users` read access to a valid credential, enables the new flag, and expects `BootOutcome::Ready`.
- [x] Change the existing boot check to run only when `!bootstrap.allow_insecure_credential_permissions`; keep credential reading and `ServiceAccount::from_json` validation unchanged.
- [x] Emit an explicit `kiosk-main` warning when the flag is enabled.
- [x] Run `wsl.exe -d Ubuntu -e bash -lc 'cd /mnt/d/git/kiosk-browser && cargo test -p kiosk-main --quiet'`; expect zero failures and unchanged default-false ACL coverage.

### Task 3: Gate kiosk-launcher

**Files:** `crates/kiosk-launcher/src/sink.rs` and its existing Windows ACL tests.

- [x] Add the launcher counterpart: bad DACL plus the enabled flag must not return the ACL-permission error.
- [x] Change `build_telemetry` to skip only the ACL check when the flag is true, and emit an explicit `kiosk-launcher` warning.
- [x] Leave JSON parsing, telemetry creation, and watchdog supervision unchanged.
- [x] Run `cargo test -p kiosk-launcher --quiet` and `cargo test -p kiosk-main --quiet`; expect zero failures.

### Task 4: Enable the local MVP and verify the Windows artifact

**Files:** `dist-template/kiosk.ini` and ignored local `target/x86_64-pc-windows-gnu/release/kiosk.ini`.

- [x] Add `allow_insecure_credential_permissions = true` under `[kiosk]` in the two MVP deployment configs. Parser omission remains secure by default.
- [x] Build with the repository’s WSL MinGW environment:

```powershell
wsl.exe -d Ubuntu -e bash -lc 'set -eu; export PATH=/mnt/d/git/kiosk-browser/target/kiosk-mingw-wrapper:/tmp/kiosk-mingw-bin:/tmp/kiosk-mingw-root/usr/bin:$PATH; export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc; export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc; export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar; cd /mnt/d/git/kiosk-browser; cargo build --release --target x86_64-pc-windows-gnu -p kiosk-main -p kiosk-launcher'
```

- [x] Restage x64 `WebView2Loader.dll` beside `kiosk-main.exe`.
- [x] Run the complete relevant test suites, `git diff --check`, verify the six runtime files, and launch the executable with `--config <release-dir> --windowed`. Confirm startup no longer reports the ACL refusal and does report the explicit bypass warning.
- [x] Commit the implementation and tracked template with `git commit -m "feat: add explicit MVP credential permission bypass"`.
