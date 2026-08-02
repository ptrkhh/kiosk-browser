# P1-F2 — WiX MSI + Autostart + §7.2 Lockdown Runbook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **EXECUTION HOST: Windows.** This is packaging + docs — no unit tests. "Done" = `wix build` compiles, an install/uninstall dry-run on a clean Windows VM passes the assertions below, `signtool verify` passes, and the runbook checks line-by-line against §7.2/§8. All validation is Windows-host.

**Goal:** Ship the deployment mechanism — a signed WiX MSI that installs the binaries + assets, tightens the credential ACL, bootstraps WebView2, autostarts the launcher — plus the §7.2 OS-lockdown runbook. After F1 + F2, P1 is a deployable Windows MVP.

**Architecture:** WiX v4/5 authoring under `packaging/windows/`; the credential ACL via the WiX Util extension `PermissionEx`; WebView2 + the Scheduled Task as install-time behaviors; Authenticode as a build/CI signing step; the lockdown runbook as markdown mapped 1:1 to §7.2/§8.

**Tech Stack:** WiX Toolset v4/5 (`wix build`, `WixToolset.Util.wixext`), `signtool`, `schtasks`, the Evergreen WebView2 bootstrapper.

**Design spec:** `docs/superpowers/specs/2026-08-02-p1f2-packaging-deployment-design.md`. Requirement authority: parent spec §4, §7.2, §8 (SEC-08/09), §9.

## Global Constraints

- **No real secrets in the MSI.** Ship the obviously-fake placeholder `kiosk-credential.json` (from `dist-template/` / spec §8) and a placeholder `kiosk.ini`. The operator replaces them per device.
- **Credential ACL is owner-only (SEC-09):** `kiosk-credential.json` readable ONLY by the kiosk local account + `SYSTEM`; the inherited world-readable ACE (Program Files default) MUST be removed. This is the default the app's fail-closed boot check (§8) relies on.
- **Autostart owns NO restart-on-exit** — the launcher owns crash-restart; a technician exit (code 86) must reach the desktop (arch-05).
- **Idempotent upgrade:** a `MajorUpgrade`; operator-edited `kiosk.ini` + the real credential are **never-overwrite** (permanent components / `NeverOverwrite`), so an MSI upgrade does not clobber per-device config.
- **Signed:** binaries + MSI Authenticode-signed with an operator/CI cert (not in the repo).
- **Windows only.** Linux `.deb`/systemd/cage = P2; Android = P3; the code-signing cert + the touch keyboard (PF-02) are out of scope.

## Interfaces / inputs

```
Build outputs (from `cargo build --release -p kiosk-main -p kiosk-launcher`):
  target\release\kiosk-main.exe, target\release\kiosk-launcher.exe
Assets: crates\kiosk-main\bundled\*.html, the offline mp4 (dist-template\kiosk-offline.mp4),
  dist-template\kiosk-credential.json (placeholder), a placeholder kiosk.ini.
Install dir: C:\Program Files\kiosk\        Data dir: %ProgramData%\kiosk\
```

---

### Task 0: config-fault safe render (kiosk-main code — the F1 carry-forward)

**Files:** Modify `crates/kiosk-main/src/main.rs` (+ `boot.rs` if the parse lives there)

**Why:** P1-F1 left a field failure the F2 design owns (§"safe mode does not cover config faults"): `kiosk-main` parses `kiosk.ini`/the credential **before** the `--safe` branch and both sites `panic!`, so a device installed with an unreadable/invalid `kiosk.ini` crash-loops → `--safe` panics in the same place → `watchdog.safe_mode_failed` → a 60 s **black screen with no diagnostics**. This is the exact "black screen, can't tell why" failure the whole safe-mode design exists to prevent.

- [ ] **Step 1:** make the config-parse failure render, not panic. When `kiosk.ini`/credential read-or-parse fails at boot (any mode), build the webview and navigate to `bundled_url("safe.html")` with the device id (best-effort: the machine id, since `kiosk.ini` may be unreadable) + the parse error string, keep window+input hardening, and DO NOT `panic!`/exit — sit on the safe page so the launcher's heartbeat still arms and the operator sees the fault. Reuse the Task-2-from-F1 `--safe` render path (safe.html + last-error substitution) — this is the same page, sourced from the parse error instead of `crash-panic.txt`.
- [ ] **Step 2: host/Windows check.** A unit test that the config-load error path returns a "render safe" outcome (factor the decision to a pure fn if practical); Windows smoke: install with a deliberately-corrupt `kiosk.ini` → the device shows `safe.html` with the parse error, NOT a black screen, and does not crash-loop.
- [ ] **Step 3:** commit `fix(main): render safe.html on a config-parse fault instead of panicking`.

(Then the Task-4 runbook still notes the operational hint — "freshly-installed device black? suspect kiosk.ini/credential" — as belt-and-suspenders.)

---

### Task 1: WiX MSI core — layout, components, credential ACL, upgrade

**Files:** Create `packaging/windows/kiosk.wxs`, `packaging/windows/kiosk.wixproj` (or a `wix build` invocation), `packaging/windows/README.md` (build steps)

- [ ] **Step 1: the product + directory layout.** `kiosk.wxs`: `<Package>` (name "Kiosk Browser", a stable `UpgradeCode` GUID, version from the build), `<MajorUpgrade DowngradeErrorMessage=... />`, the `WixToolset.Util.wixext` namespace. Directory tree → `ProgramFiles64Folder\kiosk` for binaries+assets, and a `CommonAppDataFolder\kiosk` (`%ProgramData%\kiosk`) component that creates the data dir.

- [ ] **Step 2: components.** One component per file: `kiosk-main.exe`, `kiosk-launcher.exe`, each bundled `*.html`, `kiosk-offline.mp4`. Two **never-overwrite** components for the placeholder `kiosk.ini` and `kiosk-credential.json` (`Permanent="yes"` + `NeverOverwrite="yes"` so an upgrade preserves the operator's real files). A `<Feature>` pulling them all in.

- [ ] **Step 3: credential ACL (SEC-09).** On the `kiosk-credential.json` component's file, `<util:PermissionEx>` granting `GenericRead` to the kiosk account (the account name is an installer property, e.g. `KIOSK_ACCOUNT`, defaulting to the current install account) + `LocalSystem`, and set the component/file so the inherited `BUILTIN\Users` read ACE is NOT present (WiX `PermissionEx` replaces the DACL for that object). Same restrictive ACL on the `%ProgramData%\kiosk` directory component (write only for the kiosk account + SYSTEM).

- [ ] **Step 4: build + dry-run.** `wix build packaging/windows/kiosk.wxs -ext WixToolset.Util.wixext -o kiosk-<ver>.msi` compiles. Install on a clean VM: assert files in `C:\Program Files\kiosk`; **`icacls "C:\Program Files\kiosk\kiosk-credential.json"` shows ONLY the kiosk account + SYSTEM (no `BUILTIN\Users`)**; `%ProgramData%\kiosk` exists + ACL'd; uninstall removes binaries but leaves `%ProgramData%\kiosk` (document the choice). Record the `icacls` output in the report.

- [ ] **Step 5: commit** `feat(packaging): WiX MSI core — layout, components, credential ACL (SEC-09)`.

---

### Task 2: WebView2 evergreen bootstrap + autostart Scheduled Task

**Files:** Modify `packaging/windows/kiosk.wxs` (or add a Burn bundle `bundle.wxs`)

- [ ] **Step 1: WebView2 evergreen bootstrap.** Ensure the Evergreen runtime is present. Preferred: a **Burn bundle** (`bundle.wxs`) chaining `MicrosoftEdgeWebView2RuntimeInstaller` (the evergreen standalone/bootstrapper) before the MSI, with a `DetectCondition` on the runtime's registry key (`HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`) so it's skipped when present. Alternative (simpler, MSI-only): a deferred `CustomAction` that runs `MicrosoftEdgeWebview2Setup.exe /silent /install` when that key is absent. Carry the offline installer or document the online prerequisite (spec §9: MSI bootstraps evergreen).

- [ ] **Step 2: autostart Scheduled Task.** Register a **logon-triggered** Scheduled Task "KioskLauncher" running `C:\Program Files\kiosk\kiosk-launcher.exe` as the kiosk account, **no restart-on-failure setting** (Settings: `RestartCount=0`; the launcher owns restart, and code 86 must reach the desktop). Via a WiX `util:` task element if available, else a deferred `CustomAction` invoking `schtasks /Create /TN KioskLauncher /TR "...kiosk-launcher.exe" /SC ONLOGON /RU <account> /RL LIMITED /F` (+ a rollback/uninstall CA `/Delete`). Ship the task XML in the repo for auditability.

- [ ] **Step 3: dry-run.** After install: `schtasks /Query /TN KioskLauncher /V /FO LIST` shows a logon trigger, the kiosk account, no restart; WebView2 present (the registry key exists); logging on starts the launcher which starts kiosk-main. Record outputs.

- [ ] **Step 4: commit** `feat(packaging): WebView2 evergreen bootstrap + autostart Scheduled Task`.

---

### Task 3: Authenticode signing (build/CI step)

**Files:** Create `packaging/windows/sign.ps1`, `packaging/windows/README.md` (signing section), a CI note

- [ ] **Step 1: signing script.** `sign.ps1`: `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256 /f <cert.pfx or /sha1 <thumbprint>>` over `kiosk-main.exe`, `kiosk-launcher.exe` **before** packaging, then over the built `kiosk-<ver>.msi` (and the Burn bundle if used) **after**. Cert path/thumbprint + timestamp URL are parameters (operator/CI-supplied; NOT in the repo). Fail loudly if the cert is absent.

- [ ] **Step 2: CI + docs.** Document the order (sign PE binaries → `wix build` → sign MSI/bundle) in `README.md`; note the CI wiring (the Authenticode secret is a CI secret). Reference spec §9 (unsigned installers blocked by SmartScreen/GPO) so the "why" is captured.

- [ ] **Step 3: verify (Windows).** `signtool verify /pa /all kiosk-<ver>.msi` and both exes pass; record output. (Skippable when no cert is available in the dev environment — note it as verified-with-a-test-cert or deferred-to-CI, don't fake it.)

- [ ] **Step 4: commit** `feat(packaging): Authenticode signing script + CI wiring`.

---

### Task 4: §7.2 Windows OS-lockdown runbook

**Files:** Create `packaging/windows/lockdown.md`

- [ ] **Step 1: write the runbook** — a provisioning checklist, each item with the concrete registry key / GPO path / command, mapped 1:1 to spec §7.2 + §8/SEC-08:
  - **Covering lockdown:** Assigned Access **or** Shell Launcher (Enterprise/IoT/Education only — Pro/Home cannot, SEC-07/PF-01); document the Shell-Launcher-as-`kiosk-launcher`-shell option (the strongest — no desktop to escape to) vs the Scheduled-Task-under-Assigned-Access model from Task 2.
  - **GPO / registry:** disable Task Manager (`DisableTaskMgr`), Run (`NoRun`), registry tools (`DisableRegistryTools`); Sticky/Filter/Toggle keys off; `DisableLockWorkstation`; "Turn off Windows Key hotkeys" (`NoWinKeys`); disable Xbox Game Bar. Note the OS-reserved chords (Win+L/G/K, Ctrl+Alt+Del, Win+Alt+R) closed only here.
  - **Autologon** to a locked, unprivileged kiosk local account (the `DefaultUserName`/`AutoAdminLogon` keys or a documented safer alternative); screensaver disabled/secured by policy.
  - **Windows Update:** active hours + reboot deferral + reboot-into-kiosk (autologon + autostart), so a forced WU reboot doesn't strand the device on the lock screen (M8).
  - **Physical/boot (§8/SEC-08):** BitLocker full-disk encryption, BIOS/UEFI supervisor password, Secure Boot on, USB + PXE boot disabled.
  - **Credential provisioning + rotation (SEC-03):** per-device service account (interim) / token-proxy (target); rotation = revoke + re-provision; the MSI sets the owner-only ACL, the app re-checks it every reload.

- [ ] **Step 2: cross-check.** Diff `lockdown.md` against spec §7.2 + §8 line-by-line; every requirement in those sections has a corresponding runbook item with a concrete action. Note any item that is a pure procurement/policy decision vs a settable key.

- [ ] **Step 3: commit** `docs(packaging): §7.2 Windows OS-lockdown runbook`.

---

## Self-Review

**Spec coverage (F2 design / §4, §7.2, §8):** MSI layout + components + credential ACL + ProgramData + upgrade → T1; WebView2 evergreen bootstrap + autostart Scheduled Task → T2; Authenticode signing → T3; §7.2 + §8/SEC-08 lockdown runbook → T4. Placeholder-only secrets, never-overwrite operator files, no-restart-on-exit — all in the constraints + T1/T2. **Covered.** Deferred: Linux `.deb`/systemd (P2), Android (P3), the cert, touch keyboard (PF-02).

**Placeholder scan:** the WiX/schtasks/signtool/registry snippets are real, versioned toolset invocations with "confirm the exact element/key at impl time" pointers (the WebView2 registry GUID, the Util `PermissionEx` element, the WU keys) — resolved by the Windows implementer, as prior Windows plans did. No product code; validation is compile + install dry-run + `icacls`/`schtasks`/`signtool` assertions + a spec-diff of the runbook. No invented interfaces.

**Type consistency:** the install/data dirs (`C:\Program Files\kiosk`, `%ProgramData%\kiosk`), the task name (`KioskLauncher`), the credential filename (`kiosk-credential.json`), and the account property (`KIOSK_ACCOUNT`) are consistent across T1–T4 and match the parent spec §4.

**Scope:** One sub-project (Windows packaging + lockdown docs). Four tasks; all Windows-validated (no unit tests — it's WiX XML + markdown). Cert + Linux packaging + touch keyboard explicitly out. After F1 + F2, P1 (deployable Windows MVP) is complete.
