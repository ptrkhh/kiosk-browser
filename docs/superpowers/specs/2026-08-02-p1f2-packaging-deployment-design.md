# P1-F2 — WiX MSI + Autostart + §7.2 Lockdown Runbook (Design)

> Sub-project of P1-F (the deployable Windows MVP finish line). Parent spec of record:
> `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2), §4, §7.2, §8 (SEC-08/09),
> §9. Sibling **P1-F1** is the supervision/lifecycle CODE; F2 is the installer + the
> OS-lockdown documentation. No product-code changes.

**Status:** approved 2026-08-02 (design). The WiX authoring compiles cross-platform-ish but the
install/uninstall dry-run + Authenticode signing are Windows-host; the runbook is validated by a
line-by-line check against §7.2/§8 and a deploy dry-run.

## Goal

Ship the actual deployment mechanism: a signed WiX MSI that installs the two binaries + assets,
tightens the credential ACL, bootstraps the WebView2 runtime, and sets the launcher to autostart;
plus the §7.2 Windows OS-lockdown runbook that is the *covering* security boundary. After F1 + F2,
P1 is a deployable Windows MVP.

## Components

### 1. WiX MSI — `packaging/windows/`

Authored in WiX (v4/5 CLI, `wix build`; `util:PermissionEx` from the WiX Util extension for the
ACL). Produces `kiosk-<version>.msi`.

- **Payload → `C:\Program Files\kiosk\`:** `kiosk-main.exe`, `kiosk-launcher.exe`, the bundled
  web assets (`offline.html`, `splash.html`, `error.html`, `pinpad.html`, `safe.html`) if not
  embedded, and the offline video `kiosk-offline.mp4`. Ship **placeholder** `kiosk.ini` and
  `kiosk-credential.json` (the obviously-fake placeholder from `dist-template/`) for the operator
  to replace per device — the MSI must NOT ship real secrets.
- **Credential ACL (SEC-09):** `util:PermissionEx` on `kiosk-credential.json` granting read only
  to the kiosk local account + `SYSTEM` and **removing the inherited world-readable ACE** (the
  `Program Files` default is world-readable). The app already fails closed at boot/reload if the
  credential lacks its restrictive mode (§8) — the MSI is what makes that mode the default.
- **Data dir:** create `%ProgramData%\kiosk\` with an ACL restricting write to the kiosk account +
  SYSTEM (defends the last-good/anti-rollback floor + the spool; the residual SEC-11 reboot
  variant is OS-layer, this is that layer).
- **WebView2 evergreen bootstrap:** a CustomAction (or a Burn bundle wrapping the MSI) runs the
  Evergreen `MicrosoftEdgeWebview2Setup.exe` when the runtime is absent (spec §9: "MSI bootstraps
  evergreen installer") — closes the "WebView2 missing on older Win10" gap.
- **Authenticode:** a build/CI step `signtool sign`s both PE binaries **and** the MSI (and, if
  used, the Burn bundle) with an operator/CI-supplied code-signing cert. F2 provides the invocation
  + docs; the cert itself is not in the repo. Unsigned installers are blocked by SmartScreen /
  enterprise GPO (spec §9), so signing is required for real deployment.

### 2. Autostart — Scheduled Task

The MSI installs a **logon/boot Scheduled Task** that launches `kiosk-launcher` with **no
restart-on-exit setting** (the launcher owns crash-restart, not the OS; a technician exit — code 86
— must reach the desktop, arch-05). Runs as the locked, unprivileged kiosk account.

The runbook documents the **stronger** option: configure `kiosk-launcher` as the **Shell Launcher**
custom shell (replaces `explorer.exe` entirely) on Enterprise/IoT/Education — the most locked-down
autostart, where there is no desktop shell to escape to at all.

### 3. §7.2 Windows OS-lockdown runbook — `packaging/windows/lockdown.md`

The app cannot enforce OS boundaries; this doc is a provisioning checklist. A device not meeting it
is **not** a secure kiosk (spec §1/§7.2). Content, each item with the concrete registry key / GPO /
command:

- **Covering lockdown:** Assigned Access **or** Shell Launcher (requires Windows Enterprise / IoT /
  Education — **Pro/Home cannot**, SEC-07/PF-01). This is the boundary the in-app keyboard hook is
  only defense-in-depth for.
- **GPO / registry:** disable Task Manager, Run, registry tools; disable Sticky Keys (5×Shift),
  Filter Keys, Toggle Keys; `DisableLockWorkstation`; "Turn off Windows Key hotkeys"; disable Xbox
  Game Bar (H4/PF-03). OS-reserved chords (Win+L/G/K, Ctrl+Alt+Del, Win+Alt+R) are closed only here.
- **Autologon** to a locked, unprivileged kiosk local account; disable/secure the screensaver via
  policy.
- **Windows Update:** active hours + reboot deferral + reboot-into-kiosk (autologon + the Scheduled
  Task / Shell Launcher) — else a forced WU reboot lands on the lock screen and the watchdog never
  starts (M8).
- **Physical & boot prereqs (§8/SEC-08):** full-disk encryption (BitLocker), a BIOS/UEFI supervisor
  password, Secure Boot, and disabled USB + network (PXE) boot — without these an attacker reads the
  credential or rewrites `kiosk.ini`'s `config_url` from removable media, bypassing every OS ACL.
- **Credential provisioning + rotation** (SEC-03): per-device service account (interim) or the
  token-proxy (target); rotation = revoke + re-provision; the credential file's owner-only ACL is
  set by the MSI and re-checked by the app every reload.

## Data flow — an install

operator runs the signed MSI → files land in `Program Files\kiosk` → credential ACL tightened →
`%ProgramData%\kiosk` created + ACL'd → WebView2 evergreen ensured → Scheduled Task registered →
(operator edits `kiosk.ini` + drops the real credential per device) → at next logon/boot the task
starts `kiosk-launcher` → it supervises `kiosk-main`. The lockdown runbook is applied by the
provisioning tech separately (GPO/Assigned Access/BIOS).

## Error handling / edge cases

- WebView2 bootstrap offline (no network at install) → the MSI should carry the offline evergreen
  installer or document the prerequisite; a device with no runtime and no network can't self-heal.
- The MSI must be idempotent on upgrade (major-upgrade table) and must NOT overwrite an
  operator-edited `kiosk.ini` / real credential on upgrade (mark them as never-overwrite / permanent
  components).
- Uninstall removes binaries + the task but should leave `%ProgramData%\kiosk` (spool/last-good)
  unless a full purge is requested — document the choice.
- **Inherited from P1-F1: safe mode does not cover config faults.** `kiosk-main` reads and parses
  `kiosk.ini` (and the credential) *before* the `--safe` branch, and both sites `panic!`. A device
  installed with an unreadable/invalid `kiosk.ini` or a bad credential therefore crash-loops:
  escalate to `--safe` → panic in the same place → `SAFE_FAIL_LIMIT` → `watchdog.safe_mode_failed`,
  leaving a 60 s black-screen loop with **no safe page**. F2 owns the fix (render `safe.html`
  before config is parsed, so a config fault shows the device id + error instead of a black
  screen) and the **runbook must state it**: on a freshly installed device stuck black, suspect
  `kiosk.ini` / the credential first — `safe.html` appearing is *not* a prerequisite for a config
  problem.

## Testing / validation (packaging — no unit tests)

- **Compile:** `wix build` produces the MSI without error (the WiX authoring is well-formed).
- **Windows install dry-run:** install the MSI on a clean Windows VM → assert the files are in
  `Program Files\kiosk`; `icacls kiosk-credential.json` shows **only** the kiosk account + SYSTEM
  (no `BUILTIN\Users`); `%ProgramData%\kiosk` exists + ACL'd; the Scheduled Task exists and is
  logon-triggered with no restart-on-exit; WebView2 present; launching the task starts the launcher
  which starts main. Uninstall reverses the binaries + task.
- **Signature:** `signtool verify /pa kiosk.msi` and the two exes pass.
- **Runbook check:** every §7.2 + §8/SEC-08 item appears in `lockdown.md` with a concrete key /
  command; a reviewer diffs the doc against the spec sections line-by-line.

## Scope / defer

Windows only. Deferred: the **Linux** `.deb` + systemd units + cage session docs + `RestartPrevent
ExitStatus=86` (P2); **Android** device-owner provisioning (P3); the **touch keyboard** (PF-02,
separate). The code-signing **certificate** is operator/CI-supplied — F2 ships the signing steps and
the placeholder-only `kiosk-credential.json`, never a real secret.
