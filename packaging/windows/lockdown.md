# Windows kiosk lockdown runbook

This checklist is a deployment gate for Windows 10/11 kiosks. The application keyboard hook is defense in depth, not the security boundary. A device that does not pass this runbook is **not a secure kiosk**.

Commands assume an elevated 64-bit Windows PowerShell session and the local account `kiosk`. Replace that name consistently. Apply user policies to the kiosk account, not the technician account. Domain GPO or MDM should own settings on managed fleets; do not also stamp conflicting local registry values.

## 0. Record recovery data before lockdown

1. Create and test a separate named technician administrator. Never add `kiosk` to Administrators, Remote Desktop Users, or another group nested into Administrators.
2. Export current policy and boot state:

   ```powershell
   New-Item C:\KioskRecovery -ItemType Directory -Force
   gpresult /h C:\KioskRecovery\gpresult-before.html
   reg export 'HKLM\SOFTWARE\Policies\Microsoft\Windows' C:\KioskRecovery\hklm-policies.reg /y
   manage-bde -status C: > C:\KioskRecovery\bitlocker-before.txt
   schtasks /Query /TN KioskLauncher /XML > C:\KioskRecovery\KioskLauncher.xml
   ```

3. Store the BitLocker recovery key and UEFI supervisor password in the organization's escrow system. Test the technician login and remote-management path before enabling Shell Launcher or autologon.
4. Keep recovery material off the kiosk. `C:\KioskRecovery` is only a temporary collection location; copy it to protected administration storage, then remove it under the site's retention procedure.

## 1. OS edition and covering lockdown

Check the installed edition and build:

```powershell
Get-ComputerInfo | Select-Object WindowsProductName, WindowsVersion, OsBuildNumber
```

This project's secure Windows baseline requires **Enterprise, Enterprise LTSC, Education, IoT Enterprise, or IoT Enterprise LTSC**. Shell Launcher is supported only on those editions. Assigned Access capabilities vary by Windows release and XML schema; Windows Pro may expose some Assigned Access features, but Pro and Home do not meet this project's SEC-07/PF-01/OD-5 deployment gate. Home also lacks Local Group Policy Editor. Upgrade the image rather than treating app hotkey blocking as equivalent.

Choose exactly one model:

- **Shell Launcher, preferred:** replaces `explorer.exe` for `kiosk` with `kiosk-launcher.exe`. No desktop shell exists to escape into. Shell Launcher owns initial shell launch; disable the MSI-installed `KioskLauncher` Scheduled Task to prevent a duplicate launcher.
- **Assigned Access plus Scheduled Task:** Assigned Access supplies the restricted user experience; the MSI-installed task starts `kiosk-launcher.exe` at `kiosk` logon with `InteractiveToken`, `LeastPrivilege`, and no `RestartOnFailure`. Use this where the organization's MDM/provisioning system already owns Assigned Access. Do not use legacy `Set-AssignedAccess` single-app syntax for this desktop launcher.

Microsoft references: [Shell Launcher edition requirements and enablement](https://learn.microsoft.com/windows/configuration/shell-launcher/configure), [Shell Launcher WMI provider](https://learn.microsoft.com/windows/configuration/shell-launcher/wesl-usersetting), and [Assigned Access configuration](https://learn.microsoft.com/windows/configuration/assigned-access/configure-multi-app-kiosk).

### Option A: Shell Launcher

Enable the feature, assign the launcher to the kiosk SID, preserve technician exit code `86`, then enable Shell Launcher:

```powershell
Enable-WindowsOptionalFeature -Online -FeatureName Client-DeviceLockdown,Client-EmbeddedShellLauncher -All -NoRestart

$account = New-Object System.Security.Principal.NTAccount("$env:COMPUTERNAME", 'kiosk')
$sid = $account.Translate([System.Security.Principal.SecurityIdentifier]).Value
$shell = [wmiclass]'\\localhost\root\standardcimv2\embedded:WESL_UserSetting'

# Shell Launcher actions: 0 restart shell, 1 restart device, 2 shut down, 3 do nothing.
# Normal/crash exit restarts kiosk-launcher. Technician exit 86 does not restart it.
$result = $shell.SetCustomShell($sid, '"C:\Program Files\kiosk\kiosk-launcher.exe"', [int[]]@(86), [int[]]@(3), 0)
if ($result.ReturnValue -ne 0) { throw "SetCustomShell failed: $($result.ReturnValue)" }
$result = $shell.SetEnabled($true)
if ($result.ReturnValue -ne 0) { throw "SetEnabled failed: $($result.ReturnValue)" }

Disable-ScheduledTask -TaskName KioskLauncher
Restart-Computer
```

Exit code 86 deliberately leaves the kiosk shell stopped; it does **not** reveal Explorer. Technician work therefore uses the separate admin/remote-management path. If field workflow requires an Explorer desktop after code 86, use Assigned Access plus Scheduled Task instead.

Verify after signing in as `kiosk`:

```powershell
Get-CimInstance -Namespace root\standardcimv2\embedded -ClassName WESL_UserSetting |
  Select-Object Sid, Shell, DefaultAction, CustomReturnCodes, CustomReturnCodesAction
Get-ScheduledTask -TaskName KioskLauncher | Select-Object TaskName, State
$launcher = @(Get-Process kiosk-launcher -ErrorAction SilentlyContinue)
$main = @(Get-Process kiosk-main -ErrorAction SilentlyContinue)
if ($launcher.Count -ne 1 -or $main.Count -ne 1) {
  throw "Expected one launcher and one main; found launcher=$($launcher.Count), main=$($main.Count)"
}
```

Expected: the kiosk SID maps to the quoted launcher path, default action is `0`, code `86` maps to `3`, task is disabled, and one launcher/main pair runs.

Rollback from the technician administrator or management channel:

```powershell
$account = New-Object System.Security.Principal.NTAccount("$env:COMPUTERNAME", 'kiosk')
$sid = $account.Translate([System.Security.Principal.SecurityIdentifier]).Value
$shell = [wmiclass]'\\localhost\root\standardcimv2\embedded:WESL_UserSetting'
$null = $shell.RemoveCustomShell($sid)
$null = $shell.SetEnabled($false)
Enable-ScheduledTask -TaskName KioskLauncher
Restart-Computer
```

Do not remove the Windows feature until this rollback and an Explorer login have succeeded.

### Option B: Assigned Access plus Scheduled Task

Configure a Windows 11 multi-app/restricted-user Assigned Access profile through one authoritative channel:

- Intune: **Devices > Configuration > Create > Windows 10 and later > Templates > Kiosk**, assign it to the device group, choose the local `kiosk` account, and allow `C:\Program Files\kiosk\kiosk-launcher.exe` plus only site-approved support applications.
- CSP/provisioning package: apply the version-correct Assigned Access XML at `./Vendor/MSFT/AssignedAccess/Configuration`. Validate the XML against the schema for the deployed Windows build; do not copy an XML from another release blindly.

The MSI task remains enabled and is the only startup entry. Verify its exact contract:

```powershell
schtasks /Query /TN KioskLauncher /V /FO LIST
[xml]$task = schtasks /Query /TN KioskLauncher /XML
$task.Task.Triggers.LogonTrigger.UserId
$task.Task.Principals.Principal.LogonType
$task.Task.Principals.Principal.RunLevel
$task.Task.Settings.RestartOnFailure
```

Expected: logon trigger belongs to `kiosk`, logon type is `InteractiveToken`, run level is `LeastPrivilege`, launcher path is `C:\Program Files\kiosk\kiosk-launcher.exe`, and `RestartOnFailure` is absent. The launcher owns crash restart; adding task restart would defeat technician exit code 86.

After kiosk logon, assert process counts instead of accepting mere process presence:

```powershell
$launcher = @(Get-Process kiosk-launcher -ErrorAction SilentlyContinue)
$main = @(Get-Process kiosk-main -ErrorAction SilentlyContinue)
if ($launcher.Count -ne 1 -or $main.Count -ne 1) {
  throw "Expected one launcher and one main; found launcher=$($launcher.Count), main=$($main.Count)"
}
```

Rollback: remove the Assigned Access profile through the same MDM/provisioning channel. On a locally configured recovery machine, use **Settings > Accounts > Other users > Kiosk > Remove kiosk**, then run `gpupdate /force` and sign out/in. Do not use `Clear-AssignedAccess` unless the profile was originally created by the matching legacy cmdlet.

## 2. Kiosk account and autologon

Create an unprivileged local account with a unique, random password entered as a secure prompt:

```powershell
$password = Read-Host 'Unique kiosk password' -AsSecureString
New-LocalUser -Name kiosk -Password $password -Description 'Kiosk interactive account' -AccountNeverExpires
Set-LocalUser -Name kiosk -PasswordNeverExpires $true
Remove-Variable password
Get-LocalGroupMember Administrators
Get-LocalGroupMember 'Remote Desktop Users'
```

Confirm `kiosk` is absent from both outputs and from any nested administrator group. The non-expiring password prevents an unattended reboot from stranding the device; compensate with a unique high-entropy password, BitLocker, no remote logon, and a scheduled per-device rotation procedure. Apply **Deny access to this computer from the network** and **Deny log on through Remote Desktop Services** to the kiosk account at `Computer Configuration > Windows Settings > Security Settings > Local Policies > User Rights Assignment`, unless a documented support dependency requires otherwise. Do not grant service-account, install-driver, backup, debug, or shutdown rights.

Autologon is needed so an update or power-loss reboot returns to kiosk operation. Preferred method is Microsoft Sysinternals [Autologon](https://learn.microsoft.com/sysinternals/downloads/autologon) in its GUI: enter `kiosk`, local computer name, and password, then select **Enable**. It stores the password as an LSA secret instead of the Winlogon `DefaultPassword` value. An administrator can still recover an LSA secret; full-disk encryption, firmware controls, a unique low-privilege password, and restricted admin access remain mandatory. Do not pass the password on the Autologon command line because process inspection and shell history can expose it.

The legacy alternative uses these real values under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon`:

- `AutoAdminLogon` (`REG_SZ`) = `1`
- `DefaultUserName` (`REG_SZ`) = `kiosk`
- `DefaultDomainName` (`REG_SZ`) = local computer name
- `DefaultPassword` (`REG_SZ`) = the password

**Warning:** the legacy alternative stores `DefaultPassword` as clear text in the registry and may expose it remotely to authenticated users. Use it only when the device is physically secured, remote registry access is blocked, and the organization explicitly accepts the risk. Never put the password in a script, MSI property, ticket, or command line.

Verify only non-secret values:

```powershell
$winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Get-ItemProperty $winlogon | Select-Object AutoAdminLogon, DefaultUserName, DefaultDomainName
```

Reboot twice, including once after an update test, and confirm `kiosk-launcher` and `kiosk-main` return without intervention. To roll back, select **Disable** in Sysinternals Autologon. For legacy configuration, set `AutoAdminLogon` to `0` and remove `DefaultPassword`; do not export that key while it contains a password.

### Kiosk-account password rotation

Treat the local password and Autologon LSA secret as one maintenance transaction. They are not updated by one Windows API, so never reboot, sign out, or permit an update restart between these steps:

1. Start a technician-admin maintenance session, pause scheduled restarts, and retain the old password in the approved password vault until validation finishes.
2. Obtain a new unique password through the vault, then change the account without putting the secret in command history:

   ```powershell
   $newPassword = Read-Host 'New unique kiosk password' -AsSecureString
   Set-LocalUser -Name kiosk -Password $newPassword
   Remove-Variable newPassword
   ```

3. Immediately open the Sysinternals Autologon GUI, enter the same new password, and select **Enable**. Never use its password-bearing command-line form.
4. Check only `AutoAdminLogon`, `DefaultUserName`, and `DefaultDomainName` with the non-secret query above. Perform one controlled reboot and assert exactly one launcher/main pair returns.
5. If Autologon cannot be updated or the reboot test fails, remain in the technician session: restore the old local password, re-enable Autologon with the old password in the GUI, and diagnose before any unattended reboot. Retire the old vault entry only after the new reboot test passes.

## 3. Per-user shell and keyboard policies

Use a domain/MDM user policy scoped only to `kiosk`, or create a user-specific Multiple Local GPO: open `mmc.exe` as a technician, select **File > Add/Remove Snap-in > Group Policy Object Editor > Add > Browse > Users**, select the concrete local **kiosk** account, then **Finish > OK**. Confirm the console root names that user before configuring these enabled policies. Do not select **Non-Administrators**: that broader LGPO also restricts technician-standard accounts.

| Control | Group Policy path | Registry mapping for audit |
|---|---|---|
| Remove Task Manager | User Configuration > Administrative Templates > System > Ctrl+Alt+Del Options > Remove Task Manager | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System\DisableTaskMgr` DWORD `1` |
| Remove Run menu | User Configuration > Administrative Templates > Start Menu and Taskbar > Remove Run menu from Start Menu | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer\NoRun` DWORD `1` |
| Prevent registry editing tools | User Configuration > Administrative Templates > System > Prevent access to registry editing tools | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System\DisableRegistryTools` DWORD `1` |
| Remove Lock Computer | User Configuration > Administrative Templates > System > Ctrl+Alt+Del Options > Remove Lock Computer | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System\DisableLockWorkstation` DWORD `1` |
| Remove Change Password | User Configuration > Administrative Templates > System > Ctrl+Alt+Del Options > Remove Change Password | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System\DisableChangePassword` DWORD `1` |
| Remove Logoff | User Configuration > Administrative Templates > Start Menu and Taskbar > Remove Logoff on the Start Menu | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer\NoLogoff` DWORD `1` |
| Turn off Windows Key hotkeys | User Configuration > Administrative Templates > Windows Components > File Explorer > Turn off Windows Key hotkeys | `HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer\NoWinKeys` DWORD `1` |

`DisableLockWorkstation` is the registry value behind the policy named **Remove Lock Computer**. The policy removes the lock action; the name is not a command to lock the device.

For an unmanaged reference image, sign in once as `kiosk`, run this block in that user's non-elevated PowerShell before applying the covering lockdown, then sign out:

```powershell
$system = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\System'
$explorer = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer'
New-Item $system,$explorer -Force | Out-Null
New-ItemProperty $system DisableTaskMgr -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $system DisableRegistryTools -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $system DisableLockWorkstation -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $system DisableChangePassword -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $explorer NoRun -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $explorer NoLogoff -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $explorer NoWinKeys -PropertyType DWord -Value 1 -Force | Out-Null
```

Do not run that block from the technician profile: `HKCU` means the process owner. Verify as `kiosk` with `reg query` against the mappings above. For rollback, set each mapped GPO to **Not Configured** through its owner, or remove only these seven named values from the kiosk profile.

### Accessibility activation shortcuts

While signed in as `kiosk`, open **Settings > Accessibility > Keyboard** and turn off both the feature and its keyboard shortcut for:

- Sticky Keys: feature off; shortcut for pressing Shift five times off.
- Filter Keys: feature off; shortcut for holding Right Shift off.
- Toggle Keys: feature off; shortcut for holding Num Lock off.

These settings are per-user bit fields, not policy DWORDs. On the validated Windows 10/11 reference image, the conventional disabled/no-hotkey values are:

```powershell
reg add 'HKCU\Control Panel\Accessibility\StickyKeys' /v Flags /t REG_SZ /d 506 /f
reg add 'HKCU\Control Panel\Accessibility\Keyboard Response' /v Flags /t REG_SZ /d 122 /f
reg add 'HKCU\Control Panel\Accessibility\ToggleKeys' /v Flags /t REG_SZ /d 58 /f
```

Export those three keys before automation and re-check them after every OS feature update. Microsoft defines the values as bit masks; do not treat the decimal strings as universal across unvalidated images or overwrite site-required accessibility settings. The supported fallback is the Settings UI above.

### Xbox Game Bar and capture

Enable `Computer Configuration > Administrative Templates > Windows Components > Windows Game Recording and Broadcasting > Enables or disables Windows Game Recording and Broadcasting`, then choose **Disabled**. Registry mapping:

```powershell
New-Item 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR' -Force | Out-Null
New-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR' AllowGameDVR -PropertyType DWord -Value 0 -Force | Out-Null
```

Verify:

```powershell
Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR' -Name AllowGameDVR
```

Rollback by setting the GPO to **Not Configured** or removing only `AllowGameDVR` if local registry stamping owned it.

### Reserved chord test

`NoWinKeys` does not suppress every OS-reserved chord. Windows provides no supported registry value that removes the Secure Attention Sequence itself. Never claim a GPO closes `Ctrl+Alt+Del`; it only removes actions exposed from that screen. Assigned Access/Shell Launcher plus the policies above must make each chord a non-escape path:

| Test as `kiosk` | Pass condition |
|---|---|
| `Win+L` | No usable lock-screen escape; **Remove Lock Computer** is enforced. |
| `Win+G`, `Win+Alt+R` | Game Bar/recording UI does not open. |
| `Win+K` | No usable Cast/shell surface opens; if the deployed build exposes one, block it with the device's Assigned Access policy/MDM and fail deployment until retested. |
| `Ctrl+Alt+Del` | SAS may appear, but Task Manager, Lock, password-change, and other escape actions are absent or unusable under the selected covering lockdown. |
| `Alt+Tab`, `Alt+F4`, `Ctrl+W`, Windows key, five presses of Shift, held Right Shift, held Num Lock | Kiosk remains the only usable application and no accessibility dialog appears. |

Test physical and on-screen keyboards. Any usable shell, Task Manager, Settings, Cast, recorder, sign-in bypass, or file picker is a deployment failure.

## 4. Screensaver and power

Preferred unattended-display policy is to disable the screen saver for `kiosk`:

- `User Configuration > Administrative Templates > Control Panel > Personalization > Enable screen saver` = **Disabled**.
- Registry mapping: `HKCU\Software\Policies\Microsoft\Windows\Control Panel\Desktop\ScreenSaveActive` (`REG_SZ`) = `0`.

Unmanaged reference-image command, run as `kiosk`:

```powershell
$desktopPolicy = 'HKCU:\Software\Policies\Microsoft\Windows\Control Panel\Desktop'
New-Item $desktopPolicy -Force | Out-Null
New-ItemProperty $desktopPolicy ScreenSaveActive -PropertyType String -Value '0' -Force | Out-Null
```

If site policy requires a screen saver, enable **Password protect the screen saver**, set a timeout, and prove the kiosk can recover unattended; otherwise a password-protected saver contradicts reboot-into-kiosk availability. Also set the device's AC power plan so display/sleep do not interrupt service:

```powershell
powercfg /change monitor-timeout-ac 0
powercfg /change standby-timeout-ac 0
powercfg /getactivescheme
```

Treat battery/DC behavior as a site policy for UPS-equipped or mobile hardware. Verify after the longest configured idle period.

## 5. Windows Update and reboot into kiosk

Do not disable Windows Update. Define a site maintenance window outside staffed/display hours. Apply these **Computer Configuration** policies through one GPO/MDM owner; settings below are a concrete reference-ring choice, not universal business policy:

1. `Administrative Templates > Windows Components > Windows Update > Manage end user experience > Configure Automatic Updates` = **Enabled**, option **4 - Auto download and schedule the install**, **Every day**, `03:00`. Older ADMX versions place the same named policy directly under **Windows Update**.
2. `... > Manage end user experience > Turn off auto-restart for updates during active hours` = **Enabled**, start `07`, end `22`. Change all three reference times together for the site's real quiet window.
3. Windows 11 22H2+: configure both `... > Manage end user experience > Specify deadline for automatic updates and restarts for quality update` = **Enabled**, deadline `2` days, grace `1` day, **Don't auto-restart until end of grace period** unchecked; and the matching **...for feature update** = **Enabled**, deadline `7` days, grace `2` days, same checkbox unchecked.
4. Windows 10 22H2/reference images with the combined policy: `... > Manage end user experience > Specify deadlines for automatic updates and restarts` = **Enabled**, quality deadline `2` days, feature deadline `7` days, grace `2` days, **Don't auto-restart until end of grace period** unchecked. Use either split or combined deadline policy as exposed by the deployed ADMX, never both.
5. `... > Manage end user experience > No auto-restart with logged on users for scheduled automatic updates installations` = **Not Configured**. Enabling it conflicts with the bounded forced-restart gate on an account that stays signed in indefinitely.

Deadline and grace values are the approved deferral window: Windows may restart outside active hours earlier; after the effective deadline it forces restart regardless of active hours. Security/operations owners may choose different bounded values after documenting patch SLA and display impact. See Microsoft's [deadline behavior](https://learn.microsoft.com/windows/deployment/update/wufb-compliancedeadlines) and [restart/active-hours policy](https://learn.microsoft.com/windows/deployment/update/waas-restart).

For unmanaged images only, these documented policy values set active hours (example `07:00` through `22:00`); GPO/MDM remains preferred:

```powershell
$wu = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate'
New-Item $wu -Force | Out-Null
New-ItemProperty $wu SetActiveHours -PropertyType DWord -Value 1 -Force | Out-Null
New-ItemProperty $wu ActiveHoursStart -PropertyType DWord -Value 7 -Force | Out-Null
New-ItemProperty $wu ActiveHoursEnd -PropertyType DWord -Value 22 -Force | Out-Null
gpupdate /force
```

Choose site hours; do not copy the example blindly. Verify effective policy and reboot path:

```powershell
Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate' |
  Select-Object SetActiveHours, ActiveHoursStart, ActiveHoursEnd
gpresult /h C:\Windows\Temp\kiosk-gpresult.html
Get-WinEvent -FilterHashtable @{LogName='System'; Id=1074} -MaxEvents 10 |
  Select-Object TimeCreated, ProviderName, Message
```

In a maintenance test ring, install an update requiring restart. Pass only when the device restarts outside active hours, autologon succeeds, and this assertion passes:

```powershell
$launcher = @(Get-Process kiosk-launcher -ErrorAction SilentlyContinue)
$main = @(Get-Process kiosk-main -ErrorAction SilentlyContinue)
if ($launcher.Count -ne 1 -or $main.Count -ne 1) {
  throw "Expected one launcher and one main; found launcher=$($launcher.Count), main=$($main.Count)"
}
```

Roll back registry-stamped active hours by removing only the three values above; roll back GPO/MDM through its owning system.

## 6. Physical and boot controls (SEC-08)

These are firmware, procurement, and key-management controls; no universal Windows registry key can configure them.

### BitLocker full-volume encryption

Check TPM readiness and encryption:

```powershell
Get-Tpm
Get-BitLockerVolume -MountPoint C: | Select-Object MountPoint, VolumeStatus, EncryptionPercentage, ProtectionStatus
manage-bde.exe -status C:
```

**Warning:** enabling BitLocker without a tested escrowed recovery key can make device data unrecoverable after firmware/TPM changes. Prefer an Intune/AD policy that automatically escrows recovery material. Never print a recovery password, dump the full `KeyProtector` object, redirect it to a file, or put it in this runbook's evidence. Standalone sequence below suppresses secret-bearing output and escrows directly; run the one backup command matching device join state:

```powershell
Enable-BitLocker -MountPoint C: -EncryptionMethod XtsAes256 -RecoveryPasswordProtector | Out-Null
$recovery = @(Get-BitLockerVolume -MountPoint C: | Select-Object -ExpandProperty KeyProtector |
  Where-Object KeyProtectorType -eq RecoveryPassword)
if ($recovery.Count -ne 1) { throw "Expected one recovery protector; found $($recovery.Count)" }

# Microsoft Entra joined: use this one.
BackupToAAD-BitLockerKeyProtector -MountPoint C: -KeyProtectorId $recovery[0].KeyProtectorId | Out-Null
# Active Directory joined alternative: use this instead, not as well.
# Backup-BitLockerKeyProtector -MountPoint C: -KeyProtectorId $recovery[0].KeyProtectorId | Out-Null

Add-BitLockerKeyProtector -MountPoint C: -TpmProtector | Out-Null
Get-BitLockerVolume -MountPoint C: | Select-Object MountPoint, VolumeStatus, ProtectionStatus
Get-BitLockerVolume -MountPoint C: | Select-Object -ExpandProperty KeyProtector |
  Select-Object KeyProtectorType, KeyProtectorId
Remove-Variable recovery
```

Do not reboot or continue until escrow presence and a recovery drill succeed through the management system. Microsoft distinguishes **Fully Encrypted** from **Used Space Only Encrypted**, even though both can show 100 percent; see the [BitLocker operations guide](https://learn.microsoft.com/windows/security/operating-system-security/data-protection/bitlocker/operations-guide). This gate requires `manage-bde.exe -status C:` to report `Conversion Status: Fully Encrypted` and `Protection Status: Protection On`. `EncryptionPercentage = 100` alone does not pass.

### UEFI and procurement baseline

In vendor firmware setup, protected by a unique or centrally managed supervisor password:

- Enable Secure Boot; use production keys, not Setup Mode.
- Put Windows Boot Manager first and disable boot from USB/removable media.
- Disable network/PXE/IPv4/IPv6 boot on every NIC.
- Lock firmware setup and one-time boot menus with the supervisor password.
- Procure hardware whose firmware supports all four controls and whose password/recovery workflow is supportable at fleet scale. Record approved model and firmware revision in the asset record.

Verify from Windows, then perform a physical negative test:

```powershell
Confirm-SecureBootUEFI
msinfo32.exe
```

`Confirm-SecureBootUEFI` must return `True`; System Information must show **BIOS Mode: UEFI** and **Secure Boot State: On**. With approved test media and network boot service available, cold boot and invoke the one-time boot menu: USB and PXE must be unavailable or require the supervisor credential. Disk removal must not reveal plaintext because BitLocker is active. Firmware changes require a maintenance ticket and full retest.

## 7. Per-device credential, ACL, and rotation (SEC-03/SEC-09)

Production target is a token proxy: each device authenticates with a per-device certificate/key and receives short-lived, downscoped access only to its config object plus a logging token. No long-lived GCP service-account key remains on disk. Protect the device authenticator with TPM/DPAPI/Credential Manager when that client is implemented. Pilot/interim deployments may use one service account per device with an IAM condition limiting reads to `devices/<device_id>` and `logging.logWriter`; never use a shared fleet key or bucket-wide `roles/storage.objectViewer`. Per-device service accounts stop scaling near GCP's default project service-account quota and are not the production design.

Current MSI/application contract uses the ACL-protected JSON file below. It implements only SEC-09's installer-time owner-only ACL portion; it does not implement runtime fail-closed DACL validation and is not an OS keystore. Do not describe it as equivalent to the parent spec's DPAPI/Credential Manager requirement. Token-proxy/keystore migration remains an explicit production hardening gap until implemented or formally waived.

The MSI installs an obviously fake placeholder at `C:\Program Files\kiosk\kiosk-credential.json` and gives only the local kiosk SID read access plus `SYSTEM` full access. Provision the real file from protected removable/management staging by overwriting the existing file, not deleting and recreating it:

```powershell
$credential = 'C:\Program Files\kiosk\kiosk-credential.json'
$staged = Read-Host 'Full path to protected per-device credential JSON'
Copy-Item -LiteralPath $staged -Destination $credential -Force
icacls $credential
```

Expected ACL: only `NT AUTHORITY\SYSTEM` and the device's local `kiosk` identity; inheritance disabled; no `Users`, `Authenticated Users`, `Everyone`, technician group, or stale SID. Copy tools can change a destination DACL, so the `icacls` recheck is mandatory after every provision, rotation, repair, upgrade, restore, or file-copy tool change.

If the ACL was lost, stop kiosk processes and restore it from the technician administrator:

```powershell
$credential = 'C:\Program Files\kiosk\kiosk-credential.json'
if ((Resolve-Path $credential).Path -ne $credential) { throw 'Unexpected credential path' }
$kioskSid = (New-Object System.Security.Principal.NTAccount("$env:COMPUTERNAME", 'kiosk')).Translate([System.Security.Principal.SecurityIdentifier])
$systemSid = New-Object System.Security.Principal.SecurityIdentifier('S-1-5-18')
$acl = New-Object System.Security.AccessControl.FileSecurity
$acl.SetAccessRuleProtection($true, $false)
$acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($kioskSid, 'Read', 'Allow'))
$acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($systemSid, 'FullControl', 'Allow'))
Set-Acl -LiteralPath $credential -AclObject $acl
icacls $credential
```

Also verify the anti-rollback/spool directory:

```powershell
icacls "$env:ProgramData\kiosk"
```

Expected directory ACL: only `SYSTEM` and local `kiosk`, inheritable to children; no world-readable/writeable ACE. This protects the disk-derived `config-lastgood.json` anti-rollback floor, but does not eliminate its known reboot-reset limitation. SEC-08 boot controls prevent offline ACL bypass; a TPM monotonic counter is outside v1.

**UNMET SEC-09 release blocker:** current `kiosk-main` `boot::load` only reads and parses the credential; it does not inspect its Windows DACL. No current reload path proves the required DACL check either. Therefore a permissive credential ACL is not rejected at boot or every reload, and no `config.error`/safe-mode claim may be made. The MSI ACL and mandatory operator `icacls` checks above still reduce exposure, but they do not satisfy the parent spec's runtime fail-closed requirement. A secure-image gate cannot pass until code implements and tests the boot/reload DACL check, or the owner formally waives/changes SEC-09. The separately noted flat-file-versus-DPAPI/Credential Manager gap also remains open.

Rotation procedure:

1. Mint the replacement per-device identity/key with the same per-object read boundary; never widen to bucket-level viewer.
2. Stop/exit the kiosk under an approved maintenance session.
3. Overwrite the existing credential, recheck ACL, and start the launcher.
4. Confirm config fetch and telemetry from that device.
5. Revoke the old service-account key/device credential, then confirm it can no longer authenticate.
6. Record device ID, new credential identifier, activation time, revocation time, and operator; never record private key material.

If compromise is suspected, revoke first, accept that the single kiosk goes offline, then re-provision. Token-proxy short-lived tokens make routine rotation automatic; rotate the device authenticator through the same per-device revoke/re-enroll flow.

## 8. Application-layer security evidence

These §8 controls are not firmware/GPO settings. Release engineering must attach passing evidence from [the signed-config smoke runbook](../../docs/testing/p1d2-signed-config-smoke.md) and the current release test report before this OS image is approved:

- **SEC-11 config integrity and binding:** remote config has an Ed25519 signature over RFC 8785 JCS, signed `device_id`, and monotonic `revision`; validation order is signature, device binding, anti-rollback, then schema on fetch and boot/last-good paths. Wrong-device, stale, unsigned, and invalid-signature cases keep last-good and emit distinct errors. The public key is pinned in the signed binary, not beside the read credential.
- **SEC-01 injection:** non-empty `inject_js`/`inject_css` is rejected unless signature verification works; CSP and the subresource/navigation allowlist constrain egress. Treat config-bucket write as fleet remote-code-execution authority and restrict it accordingly.
- **SEC-08 repoint resistance:** a changed local `config_url` cannot make a forged config pass. Device binding must also reject another kiosk's genuinely signed config. This complements, not replaces, Secure Boot and disk/boot protection.
- **Remote-origin boundary:** remote pages have no Tauri IPC/navigation-sentinel bridge; remote `kiosk://` navigation is logged and blocked.
- **SEC-05 exit secret:** `pin_hash` is a per-device Argon2id PHC string outside fleet-readable config; persisted exponential lockout passes restart testing. Use a longer alphanumeric secret or hardware token where four digits are insufficient.
- **Native privacy controls:** main-frame navigation allowlist and idle reset remain native and cannot be disabled by page JavaScript.
- **Repository secret hygiene:** installed credential begins as the obviously fake `dist-template/kiosk-credential.json`; verify `.gitignore` still blocks real credential files and scan the release payload before signing.

These checks are application/release gates. OS provisioning cannot repair a build missing them; quarantine that build.

## 9. Fresh-install black-screen diagnostic

A fresh device that remains black may have an unreadable or invalid `kiosk.ini`, a placeholder/malformed credential, wrong credential ACL, missing WebView2, or a task/shell startup fault. Do not assume `safe.html` must appear: configuration faults historically occurred before safe rendering and can still indicate a broken/mismatched build.

From the technician account or management channel:

```powershell
Test-Path 'C:\Program Files\kiosk\kiosk.ini'
Test-Path 'C:\Program Files\kiosk\kiosk-credential.json'
icacls 'C:\Program Files\kiosk\kiosk-credential.json'
Get-Content 'C:\ProgramData\kiosk\crash-panic.txt' -ErrorAction SilentlyContinue
schtasks /Query /TN KioskLauncher /V /FO LIST
Get-ScheduledTaskInfo -TaskName KioskLauncher
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -Name pv
Get-WinEvent -LogName 'Microsoft-Windows-TaskScheduler/Operational' -MaxEvents 50 |
  Where-Object Message -Match 'KioskLauncher' |
  Select-Object TimeCreated, Id, LevelDisplayName, Message
```

Do not print credential JSON into logs or tickets. Validate `kiosk.ini` against the shipped template and check that its `credential` names the file beside it. Re-provision the per-device credential by the procedure above. If using Shell Launcher, query `WESL_UserSetting` instead of expecting the task to be enabled. A healthy current build should show `safe.html` for credential read/parse errors. It does **not** currently detect a permissive DACL; black output remains an escalation signal, not proof that config or ACL is valid.

## 10. Final deployment gate

Run after provisioning, after every base-image/feature-update change, and after firmware replacement:

- [ ] Approved Enterprise/Education/IoT edition and build recorded.
- [ ] Exactly one covering model configured: Shell Launcher, or Assigned Access plus the no-restart Scheduled Task.
- [ ] `kiosk` is local, unique-password, unprivileged, denied remote/RDP login, and autologon returns through two reboots.
- [ ] Task Manager, Run, registry tools, Lock, Windows shell hotkeys, accessibility activation shortcuts, and Xbox capture are blocked for `kiosk`.
- [ ] Reserved-chord physical test has no usable escape; SAS itself is not falsely claimed disabled.
- [ ] Screen saver/power policy permits unattended display.
- [ ] Update active hours, deadlines/deferral, maintenance reboot, autologon, and launcher return are proven.
- [ ] `manage-bde -status C:` reports **Conversion Status: Fully Encrypted** and **Protection Status: Protection On**; protector type/ID-only evidence, escrow confirmation, and recovery drill passed.
- [ ] UEFI supervisor password is escrowed; Secure Boot on; USB and PXE boot disabled and physically tested.
- [ ] Per-device credential scope is documented; no shared production key or bucket-wide viewer exists.
- [ ] Credential and `%ProgramData%\kiosk` ACLs contain only local `kiosk` and `SYSTEM` by operator verification.
- [ ] **RELEASE BLOCKER:** app DACL validation at boot and every reload is implemented and tested, or SEC-09 has a recorded owner waiver/spec change. Current code cannot check this box.
- [ ] **RELEASE BLOCKER:** credential is protected by the required OS keystore, or the flat-file exception has a recorded owner waiver/spec change. Current MSI JSON cannot check this box.
- [ ] Fresh-install black-screen diagnostics and recovery administrator path are tested.

### Spec cross-check

| Requirement | Runbook control |
|---|---|
| §7.2 covering lockdown; SEC-07/PF-01/OD-5 editions | §1, both explicit models and edition gate |
| Task Manager, Run, registry tools | §3 policy table and verification |
| Sticky/Filter/Toggle shortcuts | §3 accessibility subsection |
| `DisableLockWorkstation`, `NoWinKeys`, Xbox Game Bar | §3 policy table and Game Bar subsection |
| OS-reserved chords, including Ctrl+Alt+Del limitation | §3 reserved-chord test |
| Unprivileged local autologon account | §2, LSA-secret preferred and clear-text risk documented |
| Disable/secure screen saver | §4 |
| Update active hours, reboot deferral, reboot into kiosk (M8) | §5 |
| SEC-08 full-disk encryption, UEFI password, Secure Boot, USB/PXE disabled | §6; Windows, firmware, procurement ownership separated |
| SEC-03 per-device interim identity, token-proxy target, revoke/re-provision rotation | §7 |
| SEC-04 per-object scope/no bucket-wide viewer | §7 |
| SEC-09 owner-only credential/data ACL | §7 MSI/operator check; runtime boot/reload validation explicitly **unmet** |
| SEC-11 disk-floor residual and physical mitigation | §7 |
| SEC-11 signed config, device binding, anti-rollback, pinned key | §8 release evidence |
| SEC-01 authenticated injection and bounded egress | §8 release evidence |
| Remote-origin IPC/sentinel boundary | §8 release evidence |
| SEC-05 per-device exit secret and persisted lockout | §8 release evidence |
| Native navigation allowlist and idle reset | §8 release evidence |
| Fake installer credential and repository secret hygiene | §§7–8 |
| Fresh-install black diagnostic | §9 |
