# Windows MSI

Build x64 release binaries first, then build MSI from repository root:

```powershell
cargo build --release -p kiosk-main -p kiosk-launcher
dotnet build packaging/windows/kiosk.wixproj -c Release
```

`ProductVersion` defaults to workspace version `0.1.0`; override it for a release:

```powershell
dotnet build packaging/windows/kiosk.wixproj -c Release -p:ProductVersion=1.2.3
```

Equivalent WiX CLI command:

```powershell
wix extension add WixToolset.Util.wixext/5.0.2
wix build packaging/windows/kiosk.wxs -arch x64 -ext WixToolset.Util.wixext -d ProductVersion=0.1.0 -d RepoRoot=. -o kiosk-0.1.0.msi
```

Install for pre-created local account `kiosk`:

```powershell
msiexec /i kiosk-0.1.0.msi KIOSK_ACCOUNT=kiosk
```

If omitted, `KIOSK_ACCOUNT` defaults to Windows Installer's `LogonUser`. Replace the fake offline video as needed before production packaging, but keep the obviously-fake credential in the MSI. After installation, the operator provisions each device's real credential and `kiosk.ini`, preserves the installed ACL, then rechecks it with `icacls`. `kiosk.ini` and `kiosk-credential.json` are permanent, never-overwrite components, so repair, upgrade, and uninstall preserve operator-provisioned values.

MSI 5.0 `PermissionEx` first installs a protected SYSTEM-only DACL; WiX Util `PermissionEx` then resolves and adds `KIOSK_ACCOUNT`. Result leaves credential read access for `KIOSK_ACCOUNT`, full access for `SYSTEM`, and no inherited ACEs. `%ProgramData%\kiosk` gives inheritable full access only to those identities. That data directory is permanent: uninstall removes packaged binaries/assets but retains device state. Verify both ACLs with `icacls` on a clean Windows VM before deployment; no VM ACL result is recorded from this development host.

WebView2 bootstrap and launcher autostart belong to plan Task 2 and are intentionally absent here.
