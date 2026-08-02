# Windows installer

Build x64 release binaries, MSI, then Burn setup bundle from repository root:

```powershell
cargo build --release -p kiosk-main -p kiosk-launcher
dotnet build packaging/windows/bundle.wixproj -c Release
```

The bundle project builds the x64 MSI first, downloads Microsoft's Evergreen WebView2 bootstrapper to `obj` when absent, verifies its Authenticode signature is valid and issued to Microsoft Corporation, then embeds both. Release the `kiosk-setup-<version>.exe` bundle, not the bare MSI: Burn rejects 32-bit Windows, checks the per-machine `pv` value under `HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`, installs WebView2 per-machine only when missing, and stops before MSI installation if the vital prerequisite fails. Per-user runtime registrations are intentionally ignored because the installer account can differ from `KIOSK_ACCOUNT`.

The default Evergreen bootstrapper needs internet access on a machine missing WebView2. For fully offline deployment, download Microsoft's x64 Evergreen Standalone Installer during release preparation and override the build input:

```powershell
dotnet build packaging/windows/bundle.wixproj -c Release `
  -p:WebView2InstallerPath=C:\release-inputs\MicrosoftEdgeWebView2RuntimeInstallerX64.exe
```

That larger standalone installer is embedded in the resulting bundle. Neither Microsoft installer is committed to this repository.

`ProductVersion` defaults to workspace version `0.1.0`; override it for a release:

```powershell
dotnet build packaging/windows/bundle.wixproj -c Release -p:ProductVersion=1.2.3
```

Equivalent WiX CLI command:

```powershell
wix extension add WixToolset.Util.wixext/5.0.2
wix build packaging/windows/kiosk.wxs -arch x64 -ext WixToolset.Util.wixext -d ProductVersion=0.1.0 -d RepoRoot=. -o kiosk-0.1.0.msi
wix extension add WixToolset.Bal.wixext/5.0.2
powershell.exe -NoProfile -File packaging/windows/verify-webview2.ps1 -Path C:\release-inputs\MicrosoftEdgeWebview2Setup.exe
wix build packaging/windows/bundle.wxs -arch x64 -ext WixToolset.Util.wixext -ext WixToolset.Bal.wixext -d ProductVersion=0.1.0 -d MsiPath=kiosk-0.1.0.msi -d WebView2InstallerPath=C:\release-inputs\MicrosoftEdgeWebview2Setup.exe -o kiosk-setup-0.1.0.exe
```

Install for pre-created local account `kiosk`:

```powershell
msiexec /i kiosk-0.1.0.msi KIOSK_ACCOUNT=kiosk
```

For the bundle, set the same overridable Burn variable:

```powershell
.\kiosk-setup-0.1.0.exe KIOSK_ACCOUNT=kiosk
```

`KIOSK_ACCOUNT` is required on first install. Use a bare local account name containing only letters, digits, dot, underscore, or hyphen. Installation fails unless that account exists, is enabled, and cannot reach built-in Administrators through direct or nested local-group membership. The MSI stores the validated name under HKLM for repair, uninstall, and major upgrade; never put a password in the MSI command line. Account validation runs after helper installation but explicitly before WiX Util schedules credential/data ACL changes. Replace the fake offline video as needed before production packaging, but keep the obviously-fake credential in the MSI. After installation, the operator provisions each device's real credential and `kiosk.ini`, preserves the installed ACL, then rechecks it with `icacls`. `kiosk.ini` and `kiosk-credential.json` are permanent, never-overwrite components, so repair, upgrade, and uninstall preserve operator-provisioned values.

MSI 5.0 `PermissionEx` first installs a protected SYSTEM-only DACL; WiX Util `PermissionEx` then resolves and adds `KIOSK_ACCOUNT`. Result leaves credential read access for `KIOSK_ACCOUNT`, full access for `SYSTEM`, and no inherited ACEs. `%ProgramData%\kiosk` gives inheritable full access only to those identities. That data directory is permanent: uninstall removes packaged binaries/assets but retains device state. Verify both ACLs with `icacls` on a clean Windows VM before deployment; no VM ACL result is recorded from this development host.

The MSI registers `KioskLauncher` from the shipped `KioskLauncher.xml`. It resolves `KIOSK_ACCOUNT` to a SID, XML-escapes substitutions, triggers only at that account's logon, runs `kiosk-launcher.exe` with `InteractiveToken` and `LeastPrivilege`, and deliberately has no `RestartOnFailure` element (`RestartCount` is therefore zero). Before repair, upgrade, or uninstall it exports any prior definition to a SYSTEM/Administrators-only transaction directory, stops an active task, then replaces or unregisters it. Commit deletes the backup; rollback restores the exact prior definition and running state, while a new-install rollback removes only the newly-created task. Major upgrades run `RemoveExistingProducts` after `InstallInitialize`, keeping old-product removal in the transaction so a failed new install restores the old product and task. Stopping the task terminates the launcher; its kill-on-close Job Object terminates the supervised `kiosk-main` child.

Validate on a clean Windows VM after install:

```powershell
schtasks /Query /TN KioskLauncher /V /FO LIST
schtasks /Query /TN KioskLauncher /XML
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -Name pv
```

Confirm logon trigger, kiosk account, `Run Level: Limited`, launcher path, no `RestartOnFailure`, then log on as the kiosk account and confirm launcher starts `kiosk-main`. Uninstall and confirm `schtasks /Query /TN KioskLauncher` returns not found. No VM result is recorded from this development host.

## Authenticode signing

Production release order matters because Burn embeds the MSI:

```powershell
# 1. Build and sign PE binaries.
cargo build --release -p kiosk-main -p kiosk-launcher
$env:KIOSK_SIGNING_PFX_PASSWORD = '<from your CI secret store>'
.\packaging\windows\sign.ps1 -Stage Binaries `
  -Path target\release\kiosk-main.exe,target\release\kiosk-launcher.exe `
  -PfxPath C:\release-inputs\codesign.pfx -TimestampUrl https://timestamp.digicert.com

# 2. Build and sign the MSI.
dotnet build packaging\windows\kiosk.wixproj -c Release -p:ProductVersion=1.2.3
.\packaging\windows\sign.ps1 -Stage Installers `
  -Path packaging\windows\bin\Release\kiosk-1.2.3.msi `
  -NewerThan target\release\kiosk-main.exe,target\release\kiosk-launcher.exe `
  -PfxPath C:\release-inputs\codesign.pfx -TimestampUrl https://timestamp.digicert.com

# 3. Build the bundle from the signed MSI, then sign the bundle.
dotnet build packaging\windows\bundle.wixproj --no-dependencies -c Release -p:ProductVersion=1.2.3
.\packaging\windows\sign.ps1 -Stage Installers `
  -Path packaging\windows\bin\Release\kiosk-setup-1.2.3.exe `
  -NewerThan packaging\windows\bin\Release\kiosk-1.2.3.msi `
  -PfxPath C:\release-inputs\codesign.pfx -TimestampUrl https://timestamp.digicert.com
Remove-Item Env:KIOSK_SIGNING_PFX_PASSWORD
```

`-Path` is explicit; `-NewerThan` rejects an installer older than its inputs, avoiding silent signing of an old glob match. For a certificate already installed with its private key, replace `-PfxPath ...` with `-Thumbprint <40-hex-thumbprint>`; Current User and Local Machine personal stores are searched. The PFX password comes only from the named environment variable (override with `-PfxPasswordEnvironmentVariable`), keeping it out of shell history and native process arguments. PFX mode preflights one code-signing leaf, then imports it into Current User's personal store and signs by thumbprint. A pre-installed same-thumbprint certificate with a private key is reused without importing; one without a private key fails before import. Certificates newly imported by the invocation are removed afterward, including their private keys; pre-installed certificates are never removed. Clear the environment variable after signing.

Every signature is immediately checked with `signtool verify /pa /all`. The script fails on a missing certificate, target, Windows SDK `signtool.exe`, signing error, or verification error. A real certificate is operator/CI-supplied and never committed.
