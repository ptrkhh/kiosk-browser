# kiosk — universal kiosk browser

A locked-down, single-URL kiosk browser (Rust + Tauri 2 / WebView2) for unattended
displays. Spec of record: [`docs/superpowers/specs/2026-07-05-kiosk-browser-design.md`](docs/superpowers/specs/2026-07-05-kiosk-browser-design.md).

> **Status: P0 skeleton.** Boots a fullscreen/borderless/always-on-top WebView on a
> hardcoded URL; CI; and the Windows input-capture feasibility gate (below). Config,
> telemetry, connectivity, webview hardening, exit gesture/PIN, and the launcher watchdog
> arrive in later plans.

## Workspace layout (spec §4)

| Crate | Role |
|---|---|
| `kiosk-core` | platform-agnostic library (config, telemetry, …); host-testable; never depends on Tauri or any per-OS API |
| `kiosk-main` | the Tauri 2 app — the fullscreen kiosk window |
| `kiosk-launcher` | watchdog stub for now; the real supervise/heartbeat launcher is a later plan |

## Build & run

```bash
cargo build --workspace
cargo run -p kiosk-main -- --url https://example.com   # fullscreen kiosk
cargo run -p kiosk-main -- --windowed                  # dev: decorated 1280×800
cargo test --workspace
```

Lint gates (CI + before every commit): `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

### Building on an ARM64 Windows host

There is **no native ARM64 MSVC toolchain** on some ARM64 dev boxes (the VS installer ships
ARM64→x64/x86 cross tools but not the native `Hostarm64/arm64` compiler/linker), and
VS 2026's `vswhere` may not expose the install to `rustc`'s auto-detection. If `cargo`
fails at the linker with `link: extra operand … Try 'link --help'` (Git Bash's coreutils
`link` shadowing MSVC), build the **x86_64 toolchain under x64 emulation** from inside a VS
dev environment:

```bash
rustup toolchain install stable-x86_64-pc-windows-msvc --force-non-host
# then run cargo from an "ARM64 → x64" VS dev prompt (vcvarsarm64_amd64.bat) so link.exe
# resolves to the MSVC x64 linker, e.g.:
cargo +stable-x86_64-pc-windows-msvc build -p kiosk-main
```

The resulting x64 binary runs under emulation; WebView2 and the Win32 input hooks behave
identically (the P0 gate below was validated this way).

## Security & deployment: OS-level lockdown is **mandatory**

**The application cannot enforce OS security boundaries. A device that is not locked down
at the OS level is _not_ a secure kiosk** (spec §1, §7.2, §12/OD-5). In-app shortcut
blocking is defense-in-depth only — it is **documented-unreliable** on a focused WebView2
(see the P0 gate verdict below). The covering boundary is provisioned at deploy time and
is a hard requirement, not an option.

### Windows — Assigned Access **or** Shell Launcher (required)

Use one of Microsoft's kiosk lockdown mechanisms as the covering boundary:

- **Assigned Access** (single-app kiosk), **or**
- **Shell Launcher** — replaces `explorer.exe` with `kiosk-main.exe` as the shell.

> ⚠️ **SKU requirement (PF-01 / SEC-07 / §12/OD-5):** Shell Launcher / robust Assigned
> Access require **Windows Enterprise, IoT Enterprise, or Education**. **Windows Pro and
> Home cannot use Shell Launcher** — procurement/imaging must target a supported SKU. This
> is the single most important deployment constraint.

Alongside Assigned Access / Shell Launcher, the imaging baseline (spec §7.2) must also:

- **Disable escape surfaces** via GPO/registry: Task Manager, Run, and registry-editing tools.
- **Disable accessibility hotkeys** that break out of the app: Sticky Keys (5×Shift),
  Filter Keys, Toggle Keys.
- **Neutralize reserved shortcuts:** set `DisableLockWorkstation`, enable "Turn off Windows
  Key hotkeys", and disable the Xbox Game Bar (Win+G).
- **Autologon** into a locked, **unprivileged** kiosk-only local account; disable/secure the
  screensaver by policy.
- **Windows Update:** configure **active hours + reboot deferral + reboot-into-kiosk**
  (autologon + Startup) — otherwise a forced WU reboot lands on the lock screen and the
  in-app watchdog never starts (spec M8).

> **Ctrl+Alt+Del** and a handful of other OS-reserved chords (Win+L, VT switch, etc.) are
> outside *any* user-mode app's control; the baseline above is what closes them. See spec
> §7.2 and the §7.2 deployment checklist (shipped as `packaging/windows/` in a later plan).

### Linux / Android (summary — see spec §7.2)

- **Linux:** a **cage** (Wayland) locked session is the supported secure config; disable VT
  switching/zap (`NAutoVTs=0`/`ReserveVT=0` or `DontVTSwitch`/`DontZap`), dedicated seat,
  no other TTYs, DPMS/screensaver off, sleep/suspend masked. (X11/openbox is demo-only.)
- **Android:** **device-owner provisioning** (QR/zero-touch) is required; screen pinning is
  demo-only.

### Why this is mandatory — the P0 input-capture gate

The P0 feasibility gate measured what in-app code can and cannot swallow over a focused
WebView2 window. Full results and evidence:
[`docs/superpowers/plans/2026-07-06-p0-input-gate-report.md`](docs/superpowers/plans/2026-07-06-p0-input-gate-report.md).

**Verdict: KEYBOARD = PARTIAL, POINTER = PASS.**

- ✅ **WebView2 `AcceleratorKeyPressed`** reliably swallows accelerators the webview
  receives (Ctrl+W, F5, F11) — genuine in-app defense-in-depth.
- ❌ **Low-level keyboard hook cannot contain OS shortcuts.** It is blind to keys the
  focused WebView2 consumes (Tauri #13919), the **Start menu still opens** on the Win key
  (the hook swallows key-down; Start fires on key-up), the **Alt+Tab switcher still flashes**,
  and **Alt+F4 is invisible to it** (the window closes). Touch **edge-swipes** (right→Action
  Center, bottom→Start) bypass the mouse hook entirely.
- ➡️ Therefore **Alt+F4 / Alt+Tab / Win / edge-gesture containment must come from Assigned
  Access / Shell Launcher**, not from the app (spec §12/OD-5).
- ✅ **Pointer capture works:** the native top-left corner-tap exit gesture (spec §3.5) is
  reliably seen over the focused webview — no fallback chord needed.
