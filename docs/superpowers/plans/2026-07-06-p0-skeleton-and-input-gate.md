# P0 — Workspace Skeleton, Fullscreen Kiosk Window, CI, and the Windows Input-Capture Gate

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Cargo workspace, a Tauri 2 app that boots fullscreen/borderless/always-on-top into a hardcoded URL on Windows, CI (Windows release build + Linux compile check + lint/test), and the P0 feasibility spike that proves or disproves in-app keyboard swallowing and tap capture over a focused WebView2 window (spec §9 P0 gate, §2.2, §3.5).

**Architecture:** Three-crate workspace per spec §4 — `kiosk-core` (platform-agnostic lib, host-testable), `kiosk-main` (Tauri 2 app), `kiosk-launcher` (stub for now; real watchdog is a later plan). The spike lives in `kiosk-main` behind a `--spike-input` runtime flag, Windows-only code behind `#[cfg(windows)]`, and produces a written gate report that decides whether P1 shortcut blocking relies on `AcceleratorKeyPressed`/hooks or mandates OS-level Assigned Access / Shell Launcher as the only boundary.

**Tech Stack:** Rust stable, Tauri 2 (`tauri`, `tauri-build`), `windows` + `webview2-com` crates (spike only), GitHub Actions.

## Global Constraints

- Spec of record: `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (Revision 2). On conflict, the spec wins; log the conflict in the PR/commit message.
- Rust: pinned `stable` via `rust-toolchain.toml`; edition 2021; workspace resolver "2".
- Binary names: `kiosk-main`, `kiosk-launcher` (`.exe` suffix appears on Windows only) — spec §2.3.
- App identifier: `io.github.ptrkhh.kiosk`. Product/name prefix in paths: `kiosk` — spec §4.
- Platform floors: Windows 10 1809+; Linux CI image Ubuntu 22.04 (webkit2gtk-4.1) — spec §9.
- Lint gates (CI and locally before every commit): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` — spec §10.
- No new runtime dependencies beyond those named in a task without noting why in the commit message.
- This environment is WSL2; the repo is also visible to Windows at `D:\git\kiosk-browser`. Steps tagged **[WINDOWS HOST]** must run in a Windows shell (or via `powershell.exe -Command '...'` from WSL if Windows-side Rust exists). If the executor cannot run them, STOP at that step and hand the exact command to the human — do not mark the step complete on a WSL-only substitute.
- Commit messages: conventional prefix (`feat:`, `chore:`, `ci:`, `docs:`), body only when "why" isn't obvious, ending with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Cargo workspace + `kiosk-core` + `kiosk-launcher` stub

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/kiosk-core/Cargo.toml`
- Create: `crates/kiosk-core/src/lib.rs`
- Create: `crates/kiosk-launcher/Cargo.toml`
- Create: `crates/kiosk-launcher/src/main.rs`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: workspace layout `crates/*`; `kiosk_core::app_version() -> &'static str` (semver string, used later in telemetry labels and `--version` output); `kiosk-launcher` binary that prints `kiosk-launcher <version>` and exits 0 (placeholder until the watchdog plan).

- [ ] **Step 1: Write workspace + toolchain files**

`Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/kiosk-core", "crates/kiosk-main", "crates/kiosk-launcher"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "UNLICENSED"
repository = "https://github.com/ptrkhh/kiosk-browser"

[workspace.dependencies]
kiosk-core = { path = "crates/kiosk-core" }
```

(`crates/kiosk-main` is created in Task 2; the workspace won't build until then, which is fine — this step only writes files.)

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
```

- [ ] **Step 2: Write the failing test for `app_version`**

`crates/kiosk-core/Cargo.toml`:

```toml
[package]
name = "kiosk-core"
version.workspace = true
edition.workspace = true

[dependencies]
```

`crates/kiosk-core/src/lib.rs`:

```rust
//! Platform-agnostic core: config, telemetry, connectivity, navigation,
//! identity. This crate must never depend on Tauri or any per-OS API
//! (spec §4 layering rule).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_is_semver_from_cargo() {
        let v = app_version();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {v}");
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p kiosk-core`
Expected: FAIL to compile with ``cannot find function `app_version` in this scope``.

- [ ] **Step 4: Implement `app_version`**

Add above the `tests` module in `crates/kiosk-core/src/lib.rs`:

```rust
/// The crate/product version, sourced from Cargo. Later plans extend this
/// with the git sha for telemetry labels (spec §6 TEL-04).
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
```

- [ ] **Step 5: Add the launcher stub**

`crates/kiosk-launcher/Cargo.toml`:

```toml
[package]
name = "kiosk-launcher"
version.workspace = true
edition.workspace = true

[dependencies]
kiosk-core.workspace = true
```

`crates/kiosk-launcher/src/main.rs`:

```rust
//! Watchdog stub. The real launcher (spawn/supervise/heartbeat/safe-mode,
//! spec §3.1) is implemented in a later plan; this exists so the workspace,
//! packaging, and CI shapes are final from P0.

fn main() {
    println!("kiosk-launcher {}", kiosk_core::app_version());
}
```

- [ ] **Step 6: Temporarily narrow the workspace, verify build + test pass**

Edit root `Cargo.toml` members to `["crates/kiosk-core", "crates/kiosk-launcher"]` (Task 2 restores `kiosk-main`).

Run: `cargo test --workspace && cargo run -p kiosk-launcher`
Expected: `app_version_is_semver_from_cargo` PASS; launcher prints `kiosk-launcher 0.1.0`.

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml crates/
git commit -m "feat: cargo workspace with kiosk-core and launcher stub

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `kiosk-main` — Tauri 2 fullscreen kiosk window on a hardcoded URL

**Files:**
- Create: `crates/kiosk-main/Cargo.toml`
- Create: `crates/kiosk-main/build.rs`
- Create: `crates/kiosk-main/tauri.conf.json`
- Create: `crates/kiosk-main/capabilities/default.json`
- Create: `crates/kiosk-main/assets/index.html`
- Create: `crates/kiosk-main/src/main.rs`
- Create: `crates/kiosk-main/src/cli.rs`
- Modify: `Cargo.toml` (restore `crates/kiosk-main` to members)

**Interfaces:**
- Consumes: `kiosk_core::app_version()`.
- Produces: `kiosk-main` binary. CLI (parsed by `cli::Args::parse(std::env::args())`): `--url <URL>` (override hardcoded URL), `--windowed` (dev: decorated 1280×800 window instead of kiosk fullscreen), `--spike-input` (Task 4; parsed now, no-op until then), `--version`. `cli::Args { url: Option<String>, windowed: bool, spike_input: bool }` — later plans replace `--url` with `kiosk.ini` bootstrap.

- [ ] **Step 1: Write the failing CLI parser test**

`crates/kiosk-main/src/cli.rs`:

```rust
//! Minimal hand-rolled CLI (YAGNI: no clap). Replaced by kiosk.ini in the
//! config plan; --windowed and --spike-input remain dev/diagnostic flags.

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub url: Option<String>,
    pub windowed: bool,
    pub spike_input: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(items: &[&str]) -> Args {
        Args::parse(items.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_all_flags() {
        let a = parse(&["kiosk-main", "--url", "https://x.test/a", "--windowed", "--spike-input"]);
        assert_eq!(
            a,
            Args { url: Some("https://x.test/a".into()), windowed: true, spike_input: true }
        );
    }

    #[test]
    fn defaults_are_off() {
        assert_eq!(parse(&["kiosk-main"]), Args::default());
    }

    #[test]
    fn url_without_value_is_ignored() {
        assert_eq!(parse(&["kiosk-main", "--url"]), Args::default());
    }
}
```

- [ ] **Step 2: Write crate scaffolding so the test can run**

`crates/kiosk-main/Cargo.toml`:

```toml
[package]
name = "kiosk-main"
version.workspace = true
edition.workspace = true

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
kiosk-core.workspace = true
tauri = { version = "2", features = [] }
```

`crates/kiosk-main/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`crates/kiosk-main/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "kiosk",
  "identifier": "io.github.ptrkhh.kiosk",
  "build": {
    "frontendDist": "./assets"
  },
  "app": {
    "withGlobalTauri": false,
    "security": { "csp": null },
    "windows": []
  },
  "bundle": { "active": false }
}
```

(`windows: []` — the window is built in code so `--windowed`/`--url` can shape it before creation.)

`crates/kiosk-main/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "App-origin pages only; remote origins get no IPC (spec §8)",
  "windows": ["kiosk"],
  "permissions": ["core:default"]
}
```

`crates/kiosk-main/assets/index.html`:

```html
<!doctype html>
<meta charset="utf-8">
<title>kiosk</title>
<p>kiosk placeholder (app origin). Real bundled pages arrive with the P1 plan.</p>
```

`crates/kiosk-main/src/main.rs` (minimal so the crate compiles for the test run; window code lands in Step 4):

```rust
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod cli;

fn main() {
    let args = cli::Args::parse(std::env::args());
    println!("kiosk-main {} {:?}", kiosk_core::app_version(), args);
}
```

Restore root `Cargo.toml` members to `["crates/kiosk-core", "crates/kiosk-main", "crates/kiosk-launcher"]`.

- [ ] **Step 3: Run the test to verify it fails**

WSL prerequisite (once): `sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev build-essential curl wget file libssl-dev`
(If sudo is unavailable, run tests on the Windows host instead — same cargo commands.)

Run: `cargo test -p kiosk-main`
Expected: FAIL to compile with ``no function or associated item named `parse` found for struct `Args` ``.

- [ ] **Step 4: Implement the parser, verify tests pass**

Add to `crates/kiosk-main/src/cli.rs` (inside `impl Args`):

```rust
impl Args {
    pub fn parse(mut items: impl Iterator<Item = String>) -> Args {
        let mut args = Args::default();
        let _argv0 = items.next();
        while let Some(item) = items.next() {
            match item.as_str() {
                "--url" => args.url = items.next(),
                "--windowed" => args.windowed = true,
                "--spike-input" => args.spike_input = true,
                "--version" => {
                    println!("kiosk-main {}", kiosk_core::app_version());
                    std::process::exit(0);
                }
                other => eprintln!("kiosk-main: ignoring unknown argument {other:?}"),
            }
        }
        args
    }
}
```

Run: `cargo test -p kiosk-main`
Expected: 3 tests PASS.

- [ ] **Step 5: Implement the kiosk window**

Replace `crates/kiosk-main/src/main.rs` `main` with:

```rust
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod cli;

const HARDCODED_URL: &str = "https://example.com/";

fn main() {
    let args = cli::Args::parse(std::env::args());
    let url: tauri::Url = args
        .url
        .as_deref()
        .unwrap_or(HARDCODED_URL)
        .parse()
        .expect("target URL must be a valid absolute URL");

    tauri::Builder::default()
        .setup(move |app| {
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                "kiosk",
                tauri::WebviewUrl::External(url.clone()),
            );
            builder = if args.windowed {
                builder.inner_size(1280.0, 800.0).decorations(true)
            } else {
                builder
                    .fullscreen(true)
                    .decorations(false)
                    .always_on_top(true)
                    .focused(true)
            };
            let _window = builder.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start kiosk-main");
}
```

- [ ] **Step 6: Verify Linux compile + lints**

Run: `cargo check -p kiosk-main && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all clean/PASS.

- [ ] **Step 7 [WINDOWS HOST]: Verify the kiosk window on Windows**

Run (from `D:\git\kiosk-browser`): `cargo run -p kiosk-main -- --url https://example.com`
Expected, verified by a human looking at the screen: fullscreen, borderless, always-on-top window rendering example.com via WebView2; no title bar; covers the taskbar. Then `cargo run -p kiosk-main -- --windowed` shows a normal 1280×800 decorated window. Close via Alt+F4 (still allowed in P0).
Record the observed result in the Task 5 report (§ "environment sanity" row).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/kiosk-main/
git commit -m "feat: kiosk-main boots fullscreen borderless WebView on hardcoded URL

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: CI — lint + test + Linux compile check + Windows release build

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: workspace from Tasks 1–2.
- Produces: `ci.yml` with jobs `lint-test` (Ubuntu 22.04) and `build-windows` (windows-latest, uploads `kiosk-main.exe` + `kiosk-launcher.exe` artifacts). Later plans append jobs (Android, MSI, soak) to this same file.

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  lint-test:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - name: Install webkit2gtk and system deps
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
            libayatana-appindicator3-dev librsvg2-dev libssl-dev
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
      - name: Linux compile check (kiosk-main)
        run: cargo check -p kiosk-main

  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release -p kiosk-main -p kiosk-launcher
      - uses: actions/upload-artifact@v4
        with:
          name: kiosk-windows-${{ github.sha }}
          path: |
            target/release/kiosk-main.exe
            target/release/kiosk-launcher.exe
          if-no-files-found: error
```

- [ ] **Step 2: Validate YAML locally**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('ok')"`
Expected: `ok`. (If PyYAML absent: `ruby -ryaml -e "YAML.load_file('.github/workflows/ci.yml'); puts 'ok'"` or skip with a note — GitHub validates on push.)

- [ ] **Step 3: Commit and confirm CI**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: lint, test, linux compile check, windows release build

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

If a GitHub remote exists: push and confirm both jobs green (`gh run watch`). If no remote yet, STOP and ask the human whether to create one; CI confirmation then becomes part of that follow-up.

---

### Task 4: Windows input-capture spike (`--spike-input`)

**Files:**
- Create: `crates/kiosk-main/src/spike.rs`
- Modify: `crates/kiosk-main/src/main.rs` (wire the flag)
- Modify: `crates/kiosk-main/Cargo.toml` (Windows-only deps)

**Interfaces:**
- Consumes: `cli::Args.spike_input` from Task 2.
- Produces: `spike::install(window: &tauri::WebviewWindow)` (Windows; no-op stub on other OSes). Emits `SPIKE[A|B|C]` lines on stderr; consumed by the Task 5 report. Not shipped behavior — this module is deleted or repurposed by the P1 hardening plan once the gate verdict is recorded.

The three vectors, mapped to spec §9 P0 gate / §2.2 / §3.5:

| Vector | Question it answers |
|---|---|
| A — WebView2 `AcceleratorKeyPressed`, `Handled=true` | can we swallow accelerator keys the webview receives (Ctrl+W, F5, F11)? |
| B — `WH_KEYBOARD_LL` hook, dedicated pumped thread | do LL keyboard events arrive at all while the WebView2 window is focused (Tauri #13919), and can Alt+F4/Alt+Tab/Win be swallowed? |
| C — `WH_MOUSE_LL` hook, tap counter | does OS-level tap capture work over a focused webview (exit-gesture dependency, spec §3.5)? |

- [ ] **Step 1: Pin the COM crate versions to wry's**

Run: `cargo tree -p kiosk-main --target x86_64-pc-windows-msvc -i webview2-com 2>/dev/null | head -5` and `cargo tree -p kiosk-main --target x86_64-pc-windows-msvc | grep -E "windows v|webview2-com v" | sort -u`
Note the exact `webview2-com` and `windows` versions wry resolves to, and use those in Step 2 — a mismatched `windows` version fails to compile against the controller types. (If the grep shows nothing on WSL, run the same on the Windows host or inspect `Cargo.lock`.)

- [ ] **Step 2: Add Windows-only dependencies**

Append to `crates/kiosk-main/Cargo.toml` (versions from Step 1 — the ones below are the expected resolution at plan time; correct them if `Cargo.lock` disagrees, and note the change in the commit body):

```toml
[target.'cfg(windows)'.dependencies]
webview2-com = "0.38"
windows = { version = "0.61", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
  "Win32_UI_Input_KeyboardAndMouse",
  "Win32_System_LibraryLoader",
] }
```

- [ ] **Step 3: Write the spike module**

`crates/kiosk-main/src/spike.rs`:

```rust
//! P0 feasibility gate (spec §9): can in-app code see and swallow keyboard
//! and pointer input while the WebView2 window is focused?
//! Diagnostic only — enabled by --spike-input, never in normal operation.
//! Every observation is a SPIKE[vector] line on stderr; a human transcribes
//! the outcome into docs/superpowers/plans/2026-07-06-p0-input-gate-report.md.

#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow) {
    eprintln!("SPIKE: only implemented on Windows; nothing to do");
}

#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow) {
    windows_impl::install(window);
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_F4, VK_LWIN, VK_RWIN, VK_TAB,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
        TranslateMessage, KBDLLHOOKSTRUCT, LLKHF_ALTDOWN, MSG, MSLLHOOKSTRUCT,
        WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN, WM_SYSKEYDOWN,
    };

    /// Vector A: WebView2 AcceleratorKeyPressed. Swallows Ctrl+W (0x57),
    /// F5 (0x74), F11 (0x7A) and logs every accelerator seen.
    fn install_accelerator_handler(window: &tauri::WebviewWindow) {
        let result = window.with_webview(|platform_webview| unsafe {
            use webview2_com::AcceleratorKeyPressedEventHandler;
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2AcceleratorKeyPressedEventArgs,
                COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
                COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
            };
            let controller = platform_webview.controller();
            let handler = AcceleratorKeyPressedEventHandler::create(Box::new(
                move |_controller,
                      args: Option<ICoreWebView2AcceleratorKeyPressedEventArgs>|
                      -> windows_core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                    args.KeyEventKind(&mut kind)?;
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }
                    let mut vk: u32 = 0;
                    args.VirtualKey(&mut vk)?;
                    let swallow = matches!(vk, 0x57 /* W: with Ctrl */ | 0x74 /* F5 */ | 0x7A /* F11 */);
                    if swallow {
                        args.SetHandled(true)?;
                    }
                    eprintln!("SPIKE[A] accelerator vk=0x{vk:02X} swallowed={swallow}");
                    Ok(())
                },
            ));
            let mut token = windows::Win32::System::WinRT::EventRegistrationToken::default();
            controller
                .add_AcceleratorKeyPressed(&handler, &mut token)
                .expect("add_AcceleratorKeyPressed failed");
            eprintln!("SPIKE[A] handler installed");
        });
        if let Err(e) = result {
            eprintln!("SPIKE[A] INSTALL FAILED: {e}");
        }
    }

    static KB_EVENTS: AtomicU64 = AtomicU64::new(0);

    unsafe extern "system" fn kb_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            let alt = (k.flags.0 & LLKHF_ALTDOWN.0) != 0;
            KB_EVENTS.fetch_add(1, Ordering::Relaxed);
            let swallow = (alt && k.vkCode == VK_F4.0 as u32)
                || (alt && k.vkCode == VK_TAB.0 as u32)
                || k.vkCode == VK_LWIN.0 as u32
                || k.vkCode == VK_RWIN.0 as u32;
            eprintln!("SPIKE[B] key vk=0x{:02X} alt={alt} swallowed={swallow}", k.vkCode);
            if swallow {
                return LRESULT(1); // non-zero = swallow (do not pass on)
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    static TAP_COUNT: AtomicU32 = AtomicU32::new(0);
    static FIRST_TAP_MS: AtomicU64 = AtomicU64::new(0);
    const TAP_REGION_PX: i32 = 200; // top-left corner, spec §3.5 exit gesture
    const TAPS_REQUIRED: u32 = 7;
    const WINDOW_MS: u64 = 3000;

    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && wparam.0 as u32 == WM_LBUTTONDOWN {
            let m = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let POINT { x, y } = m.pt;
            let in_region = x >= 0 && x < TAP_REGION_PX && y >= 0 && y < TAP_REGION_PX;
            eprintln!("SPIKE[C] tap at ({x},{y}) in_region={in_region}");
            if in_region {
                let now = m.time as u64;
                let first = FIRST_TAP_MS.load(Ordering::Relaxed);
                if first == 0 || now.saturating_sub(first) > WINDOW_MS {
                    FIRST_TAP_MS.store(now, Ordering::Relaxed);
                    TAP_COUNT.store(1, Ordering::Relaxed);
                } else if TAP_COUNT.fetch_add(1, Ordering::Relaxed) + 1 >= TAPS_REQUIRED {
                    eprintln!("SPIKE[C] EXIT-GESTURE DETECTED ({TAPS_REQUIRED} taps < {WINDOW_MS} ms)");
                    TAP_COUNT.store(0, Ordering::Relaxed);
                    FIRST_TAP_MS.store(0, Ordering::Relaxed);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Vectors B + C: low-level hooks on a dedicated thread with its own
    /// message pump (spec §3.1 M2 — never on the webview UI thread).
    fn install_ll_hooks() {
        std::thread::Builder::new()
            .name("spike-ll-hooks".into())
            .spawn(|| unsafe {
                let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), None, 0);
                let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0);
                eprintln!("SPIKE[B] WH_KEYBOARD_LL installed: {}", kb.is_ok());
                eprintln!("SPIKE[C] WH_MOUSE_LL installed: {}", ms.is_ok());
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .expect("failed to spawn hook thread");
    }

    pub fn install(window: &tauri::WebviewWindow) {
        eprintln!("SPIKE starting — focus the webview, then exercise each vector");
        install_accelerator_handler(window);
        install_ll_hooks();
        // Periodic proof-of-life so "no SPIKE[B] lines" is distinguishable
        // from "stderr swallowed".
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            eprintln!("SPIKE[hb] alive; kb_events_total={}", KB_EVENTS.load(Ordering::Relaxed));
        });
    }
}
```

- [ ] **Step 4: Wire the flag in `main.rs`**

In `crates/kiosk-main/src/main.rs`, add `mod spike;` under `mod cli;`, and in `setup` replace `let _window = builder.build()?;` with:

```rust
            let window = builder.build()?;
            if args.spike_input {
                spike::install(&window);
            }
```

- [ ] **Step 5: Verify it still compiles everywhere reachable**

Run (WSL): `cargo check -p kiosk-main && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo fmt --check`
Expected: clean (non-Windows path compiles the no-op stub).

Run **[WINDOWS HOST]**: `cargo check -p kiosk-main`
Expected: clean. If the `windows`/`webview2-com` API surface drifted from the code above (COM types move between versions), fix signatures against the versions recorded in Step 1 — the vectors and log format must not change.

- [ ] **Step 6: Commit**

```bash
git add crates/kiosk-main/
git commit -m "feat: --spike-input P0 gate instrumentation (accelerator, LL hooks, tap capture)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Run the gate on real Windows, record the verdict

**Files:**
- Create: `docs/superpowers/plans/2026-07-06-p0-input-gate-report.md`

**Interfaces:**
- Consumes: `SPIKE[...]` stderr output from Task 4.
- Produces: the filled gate report. The P1 hardening plan reads its Verdict section to choose: in-app swallowing as defense-in-depth (spec §7 shortcut-blocking row) vs Assigned Access/Shell Launcher as a hard P1 requirement (spec §9 P0 gate, §12/OD-5).

- [ ] **Step 1: Write the report template**

`docs/superpowers/plans/2026-07-06-p0-input-gate-report.md`:

```markdown
# P0 Input-Capture Gate Report (spec §9 P0 gate)

Run: `cargo run -p kiosk-main -- --spike-input --url https://example.com 2> spike.log`
on physical Windows hardware (WebView2 evergreen), window fullscreen, webview focused
(click the page first). Date/hardware/Windows build: ___

| # | Probe (webview focused) | Expected SPIKE line | Observed? | Swallowed (nothing escaped to OS/page)? |
|---|---|---|---|---|
| 0 | environment sanity: Task 2 Step 7 fullscreen behavior | n/a | ___ | n/a |
| 1 | type letters into the page | `SPIKE[B] key vk=...` per keypress | ___ | n/a (arrival test — Tauri #13919) |
| 2 | Ctrl+W | `SPIKE[A] ... vk=0x57 swallowed=true` | ___ | window stayed open? ___ |
| 3 | F5 / F11 | `SPIKE[A]` swallowed=true | ___ | no reload / no fullscreen toggle? ___ |
| 4 | Alt+F4 | `SPIKE[B] ... alt=true swallowed=true` | ___ | window stayed open? ___ |
| 5 | Alt+Tab | `SPIKE[B]` swallowed=true | ___ | no task switch? ___ |
| 6 | Win key | `SPIKE[B]` swallowed=true | ___ | Start menu did not open? ___ |
| 7 | 7 fast clicks in top-left corner | `SPIKE[C] EXIT-GESTURE DETECTED` | ___ | n/a |
| 8 | 7 fast touch taps top-left (touch hardware) | `SPIKE[C] EXIT-GESTURE DETECTED` | ___ | n/a |

`SPIKE[hb] kb_events_total` after the session: ___ (0 while typing ⇒ hook is blind, Tauri #13919 confirmed)

## Verdict (fills spec §12/OD-5 and shapes the P1 hardening plan)

- [ ] KEYBOARD PASS — rows 1–6 observed+swallowed ⇒ in-app blocking is real defense-in-depth; OS lockdown remains the boundary per spec §7.2.
- [ ] KEYBOARD PARTIAL — vector A works, B blind ⇒ accelerators in-app; Alt+F4/Alt+Tab/Win closed ONLY by Assigned Access / Shell Launcher, which becomes a hard P1 deployment requirement.
- [ ] KEYBOARD FAIL — neither arrives ⇒ drop in-app blocking from P1 code scope; §7.2 OS lockdown is the only mechanism.
- [ ] POINTER PASS — rows 7 (and 8 on touch) fire ⇒ native exit gesture per spec §3.5 is viable.
- [ ] POINTER FAIL — no SPIKE[C] over the webview ⇒ P1 exit gesture uses the reserved AcceleratorKeyPressed technician chord fallback (spec §3.5).

Notes / anomalies: ___
```

- [ ] **Step 2 [WINDOWS HOST]: Execute the probe run and fill the report**

Human-in-the-loop: run the command from the template on real hardware (touch hardware if available for row 8), transcribe observations, tick exactly one KEYBOARD verdict and one POINTER verdict. The executor STOPS here until the filled report is back.

- [ ] **Step 3: Commit the filled report**

```bash
git add docs/superpowers/plans/2026-07-06-p0-input-gate-report.md
git commit -m "docs: P0 input-capture gate verdict

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Plan Self-Review (done at write time)

1. **Spec coverage (P0 row, §9):** workspace ✓ (T1), fullscreen hardcoded URL on Windows ✓ (T2), CI Windows release + Linux compile check ✓ (T3), gate: accelerator/LL-hook swallowing ✓ (T4 A/B), native tap capture ✓ (T4 C), gate verdict recorded with consequences wired to OD-5 ✓ (T5).
2. **Placeholders:** none — every code step carries full content; the two human-run steps are explicitly [WINDOWS HOST]-tagged stops, not placeholders.
3. **Type consistency:** `cli::Args{url,windowed,spike_input}` consistent T2→T4; `spike::install(&tauri::WebviewWindow)` matches call site; crate names match workspace members. Known risk (stated in T4 Step 5): `windows`/`webview2-com` COM signatures may need version alignment — Step 1 pins versions before code is written.

## What this plan deliberately excludes (later plans, in order)

1. kiosk-core config: INI bootstrap, remote JSON, validation, Ed25519 signing + anti-rollback, last-good (spec §5, §8)
2. kiosk-core telemetry: GCL client, JWT + trusted time, tiered spool, insertId, rate limits (spec §6)
3. Connectivity FSM + offline video + state machine (spec §3.3–3.4)
4. Webview hardening + navigation guard + injection engine + exit gesture/PIN pad (spec §7, §3.5–3.6)
5. Launcher watchdog: heartbeat, READY, disambiguation, backoff, safe mode (spec §3.1)
6. WiX MSI + Authenticode + §7.2 Windows lockdown docs (spec §9 P1 tail)
