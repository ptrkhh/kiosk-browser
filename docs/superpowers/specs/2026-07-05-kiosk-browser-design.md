# Kiosk Browser — Design Specification

Date: 2026-07-05
Status: Approved pending user review
Owner: ptrkhh (github.com/ptrkhh)

## 1. Overview

A cross-platform kiosk browser for Windows, Linux, and Android. Each device runs a
locked-down fullscreen WebView pointed at a configured URL, falls back to a looping
local video when offline, fetches per-device configuration from Google Cloud Storage,
pushes structured telemetry to Google Cloud Logging, and is supervised by a watchdog
that restarts it after crashes or hangs.

### Goals

- One codebase, three platforms, identical fleet behavior and telemetry.
- Survive unattended 24/7 operation: crashes, hangs, network loss, bad config pushes.
- Zero-touch fleet visibility through Google Cloud Logging (no custom dashboard code).
- Field-serviceable: replaceable offline video, replaceable credential, bootstrap INI,
  secret exit gesture for technicians.

### Non-goals

- Fleet dashboard UI (use Cloud Logging / Cloud Monitoring; we ship queries and docs).
- CMS or content authoring (the content is a website we point at).
- iOS and macOS targets (Tauri would make macOS cheap later; explicitly out of scope).
- Peripheral integrations (receipt printers, scanners) — future work.
- MDM/EMM replacement (device enrollment, app distribution stay with existing tooling).

## 2. Decision record

### 2.1 Build vs. buy (2026-07-05)

Surveyed FOSS/commercial landscape: OpenKiosk (Win/Linux, Firefox-based, no Android,
no cloud telemetry), Porteus Kiosk (Linux-only OS), FreeKiosk (Android-only, remote
config still open), Fully Kiosk (Android-only, closed, ~€8/device), Xibo/Anthias
(signage players, not interactive kiosk browsers), Edge Assigned Access / Chromium
`--kiosk` / cage+Cog (building blocks only). No project covers the cross-cutting spec:
3 platforms + offline-video state machine + per-device GCS config + Google Cloud
Logging + unified watchdog telemetry. Decision: build (option A), reuse patterns from
FreeKiosk (Android Lock Task / watchdog service; verify license before copying code)
and Fully Kiosk (feature checklist).

### 2.2 Stack: Rust + Tauri 2

Chosen over .NET (Linux webview story weak: community CEF, ~150 MB), Flutter
(flutter_inappwebview has no Linux support), Electron+Kotlin (two codebases, ~200 MB,
bundled Chromium contradicts the WebView2 requirement), native-per-platform (3×
maintenance).

Tauri 2 gives: system webviews everywhere (WebView2 / WebKitGTK / Android System
WebView), Android build tooling (`cargo tauri android`), small binaries (~10 MB),
single Rust core shared by app and watchdog. Accepted risks: team Rust ramp-up; Tauri
mobile is the newest leg (mitigated by scheduling Android last).

### 2.3 Deltas from the original request

1. `kiosk.mp4` / `kiosk-offline.mp4` naming inconsistency → unified as `kiosk-offline.mp4`.
2. Watchdog is the parent process and entry point (`kiosk-launcher` spawns
   `kiosk-main`), not a sidecar.
3. Android has no separate launcher binary (platform forbids the model). Lock Task
   Mode + foreground service + `BOOT_COMPLETED` receiver + in-process watchdog thread
   fill the role and emit the same watchdog log events.
4. `.exe` suffix only on Windows.
5. Config bucket kept private; config fetched with the same service account
   (`roles/storage.objectViewer`) instead of public-read. Public URLs still work.
6. Service-account key scoped to `logging.logWriter` + `storage.objectViewer` only.
7. Added: secret exit gesture with PIN, and idle session reset (privacy on shared
   kiosks). Both were absent from the original list; field operations require them.

## 3. Architecture

### 3.1 Process model — Windows & Linux

```
autostart (Startup shortcut / Scheduled Task / systemd unit)
        │
        ▼
kiosk-launcher            ── watchdog, no UI
        │  spawns + supervises
        │  heartbeat server (named pipe on Windows, unix socket on Linux)
        │  restart w/ exponential backoff; crash-loop → safe mode
        ▼
kiosk-main (Tauri 2)      ── fullscreen borderless always-on-top window
   ├─ app state machine  Boot → ConfigLoad → Online(url) ⇄ Offline(video) | Safe
   ├─ connectivity prober (drives Online ⇄ Offline)
   ├─ config manager      fetch → validate → apply → cache; last-known-good fallback
   ├─ GCL logger          batch, disk spool, JWT auth
   ├─ heartbeat client    ping every 5 s
   └─ platform glue       cursor hide, key blocking, keep-awake, kiosk window
```

- Launcher restarts main on: process exit (any code except intentional-exit code 86),
  3 consecutive missed heartbeats (15 s), and later memory-cap breach.
- Backoff 1 s → 2 → 4 → … → 60 s. More than 5 restarts in 10 minutes → launcher
  starts `kiosk-main --safe` (bundled error page showing device ID + last error, no
  remote load) and retries normal mode every 10 minutes.
- Exit code 86 = intentional exit (technician PIN menu); launcher exits too.
- Launcher logs watchdog events to GCL directly via the shared core client, so
  restart evidence survives main's death.

### 3.2 Process model — Android

`kiosk-main` core compiles into the Tauri Android app. A small Kotlin layer (Tauri
plugin) provides: Lock Task Mode (full kiosk when provisioned as device owner; screen
pinning fallback otherwise), foreground service with `START_STICKY`, `BOOT_COMPLETED`
receiver, `FLAG_KEEP_SCREEN_ON`. An in-process watchdog thread monitors webview
responsiveness. OS service-restart semantics replace the launcher; events use the
same `watchdog.*` schema. Min SDK: API 26.

### 3.3 State machine

```
Boot ──▶ ConfigLoad ──▶ Online(url) ◀──▶ Offline(video)
              │              │
              │              └── nav/site error, fallback=error_page ──▶ ErrorPage(retry countdown)
              └── no cached config + no network ──▶ Offline(video), keep trying
Safe (entered only via kiosk-main --safe)
```

- Connectivity prober: HTTP GET to `connectivity_check_url` (default
  `https://www.gstatic.com/generate_204`), every 10 s while offline, 30 s while
  online. Two consecutive successes flip online; two failures flip offline (damping —
  no flapping between video and site). Captive portals fail the 204 check correctly.
- `fallback` config selects behavior when the internet is up but the site fails:
  `video` (default) or `error_page` (bundled, auto-retry countdown).
- Reconnect triggers immediate config refetch, then navigation back to the site.

### 3.4 Offline video

Rendered in the same webview: bundled `offline.html` with
`<video loop autoplay muted playsinline>` pointing at the local
`kiosk-offline.mp4` via the Tauri asset protocol. One rendering path on all
platforms; no native media code. The file sits next to the binaries and is
user-replaceable; absence → bundled static "offline" splash instead. H.264
baseline recommended; Linux needs gstreamer plugins (declared as .deb deps).

### 3.5 Injected JS ↔ native bridge

Tauri IPC stays disabled for remote origins (security). Injected JS communicates
with native code through navigation sentinels: script sets
`window.location = "kiosk://idle-reset"` (etc.); the native navigation guard
intercepts the `kiosk://` scheme, performs the action, cancels the navigation.
Used by: idle reset, exit gesture. Bundled local pages (PIN pad, safe mode) may
use full Tauri IPC — they are app-origin.

### 3.6 Navigation guard

Every navigation checked against `content.allowlist` (glob patterns). `kiosk://`
sentinels handled natively. New-window requests open in the same webview.
Downloads blocked. Violations logged (`nav.blocked`).

## 4. Project structure

```
kiosk-browser/                      # Cargo workspace
├── Cargo.toml
├── crates/
│   ├── kiosk-core/                 # platform-agnostic, fully unit-tested
│   │   └── src/
│   │       ├── config/             # INI bootstrap, remote JSON, validation, cache, last-good
│   │       ├── logging/            # GCL REST client, JWT signing, batcher, disk spool
│   │       ├── net/                # connectivity prober state machine
│   │       ├── metrics/            # health sampling (sysinfo)
│   │       ├── ipc/                # heartbeat protocol (shared by main + launcher)
│   │       └── identity.rs         # device id: machine GUID / machine-id / Android ID
│   ├── kiosk-main/                 # Tauri 2 app — desktop + android targets
│   │   ├── src/
│   │   │   ├── state.rs            # app state machine
│   │   │   ├── webview.rs          # hardening settings, injection, navigation guard
│   │   │   └── platform/           # windows.rs, linux.rs, android.rs
│   │   ├── assets/                 # offline.html, error.html, splash.html, pinpad.html
│   │   └── gen/android/            # Tauri Android scaffold + Kotlin kiosk plugin
│   └── kiosk-launcher/             # watchdog binary (Windows/Linux only)
├── packaging/
│   ├── windows/                    # WiX MSI, autostart, WebView2 evergreen bootstrap
│   ├── linux/                      # .deb, systemd units, cage (Wayland kiosk) session docs
│   └── android/                    # provisioning docs, device-owner QR
├── dist-template/                  # kiosk.ini, kiosk-offline.mp4, kiosk-credential.json placeholders
└── docs/
```

Layering rule: `kiosk-core` has no Tauri or platform dependencies and is fully
testable on any host. `kiosk-main/platform/` contains the only per-OS code.

### File & directory conventions

| Item | Windows | Linux | Android |
|---|---|---|---|
| Install dir (read-only) | `C:\Program Files\kiosk\` | `/opt/kiosk/` | APK |
| `kiosk.ini`, credential, mp4 | next to binaries (override: `--config <path>`) | same | app files dir |
| Data dir (cache, spool, last-good) | `%ProgramData%\kiosk\` | `/var/lib/kiosk/` | app files dir |

## 5. Configuration

### 5.1 `kiosk.ini` — local bootstrap (per-device, written at install time)

```ini
[kiosk]
config_url = https://storage.googleapis.com/kiosk/devices/lobby-01.json
device_id  =                        ; empty → auto (machine GUID / machine-id / Android ID)
site       = jakarta-hq
project_id = my-gcp-project
credential = kiosk-credential.json

[bootstrap]
url = https://app.example.com/kiosk ; used until first remote config succeeds
```

### 5.2 Remote config — JSON v1

Fetched with `If-None-Match` (GCS ETag); 304 → no-op. Strict validation
(unknown fields warn, invalid values reject). Applied config persisted as cache;
last valid config kept separately as `config-lastgood.json`. Invalid fetch →
`config.error` log + keep running last-good. `{device_id}` templating available in
`content.url`.

```jsonc
{
  "version": 1,
  "revision": "r42",                      // opaque; echoed in every log entry
  "content": {
    "url": "https://app.example.com/kiosk?device={device_id}",
    "allowlist": ["https://app.example.com/*"],
    "fallback": "video",                  // video | error_page
    "zoom": 1.0,
    "inject_css": "",
    "inject_js": "",
    "idle_reset_seconds": 180,            // 0 = off; returns to home URL
    "clear_data_on_reset": true           // also clears cookies/storage on idle reset
  },
  "display": {
    "cursor_autohide_seconds": 5,
    "monitor": 0,
    "keep_awake": true
  },
  "input": {
    "touch_keyboard": "auto",             // auto | on | off
    "allow_context_menu": false,
    "allow_text_selection": false,        // inputs remain selectable
    "exit_gesture": { "taps": 7, "region": "top-left", "pin_hash": "argon2id:..." }
  },
  "network": {
    "connectivity_check_url": "https://www.gstatic.com/generate_204",
    "probe_online_s": 30,
    "probe_offline_s": 10,
    "config_poll_s": 300
  },
  "maintenance": {
    "nightly_reload": "04:00",            // local time; null = off
    "restart_app": null,                  // "04:30" = daily full restart
    "max_webview_mem_mb": 1500            // 0 = off (P2)
  },
  "logging": {
    "level": "info",
    "health_sample_s": 60,
    "spool_max_mb": 50
  }
}
```

Defaults exist for every field; an empty `{}` remote config is valid. Bootstrap
URL from `kiosk.ini` is used until the first successful remote fetch; cached
config is used on boot when the network is down.

## 6. Telemetry — Google Cloud Logging

- Auth: service-account JSON → RS256 JWT → OAuth2 token → `entries:write` REST.
  Crates: `jsonwebtoken`, `reqwest` (rustls). No GCP SDK dependency.
- Resource `generic_node`; labels: `device_id`, `site`, `app_version`
  (semver+git sha); log name `projects/{project_id}/logs/kiosk`.
- Batching: flush every 10 s or 100 entries, whichever first. On failure →
  disk spool (JSONL segments, 5 MB rotation, `spool_max_mb` cap, drop-oldest),
  drained oldest-first on reconnect.
- Timestamps UTC RFC3339. Every entry carries `config_revision`.
- Severity: DEBUG/INFO/WARNING/ERROR/CRITICAL mapped from event type.

Event taxonomy (`jsonPayload.event`):

| Event | When |
|---|---|
| `app.start` / `app.stop` | main lifecycle (includes version, uptime on stop) |
| `config.applied` / `config.error` | remote config accepted / rejected (revision, reason) |
| `net.online` / `net.offline` | prober state flips (downtime duration on online) |
| `nav.error` / `nav.blocked` | site load failure / allowlist violation |
| `webview.crash` | renderer process died (auto-reload) |
| `watchdog.restart` / `watchdog.hang` / `watchdog.safe_mode` | launcher actions (exit code, backoff) |
| `health.sample` | CPU %, mem, disk free, uptime, webview RSS (P2) |
| `crash.panic` | Rust panic hook: writes panic file, best-effort flush; launcher attaches file to next `watchdog.restart` |

Known trap: JWT requires a sane clock. Dead CMOS battery → token failure →
entries spool locally and retry; NTP is documented as a deployment requirement.

## 7. Webview hardening

The full control set ships with P1 on Windows; Linux and Android columns land with
their platform phases (P2, P3).

| Control | Mechanism |
|---|---|
| Context menu off | WebView2 `AreDefaultContextMenusEnabled=false`; WebKitGTK `context-menu` signal; Android `setOnLongClickListener` consume + `contextmenu` JS preventDefault |
| DevTools off | per-webview settings; release builds compile without devtools feature |
| Zoom lock | WebView2 `IsZoomControlEnabled=false`; WebKitGTK fixed `zoom-level`; Android `setSupportZoom(false)`; plus injected `touch-action: pan-x pan-y` CSS. `content.zoom` sets fixed factor |
| Text selection off | injected `* { user-select: none }` with `input, textarea { user-select: text }` (config flag) |
| Drag/drop off | injected `dragstart`/`drop` preventDefault + platform drop-target disable |
| Shortcut blocking | Windows: low-level keyboard hook (`WH_KEYBOARD_LL`) swallowing Alt+F4, Alt+Tab, Win, Ctrl+W, F11 etc. while kiosk focused. Linux: compositor owns keys — cage session has none; X11/openbox keybinding docs. Android: Lock Task Mode. `Ctrl+Alt+Del` is an OS security boundary — cannot and will not block in-app; OS-level lockdown documented separately |
| Cursor auto-hide | injected JS: `cursor: none` after `cursor_autohide_seconds` idle, restore on move |
| Keep-awake | `SetThreadExecutionState` / systemd-inhibit + compositor idle-inhibit / `FLAG_KEEP_SCREEN_ON` |
| Touch keyboard | Android: system IME. Windows: TabTip auto-invoke on editable focus. Linux: squeekboard/onboard deployment docs |
| Downloads / popups / file pickers | blocked; new windows navigate in place; file picker allowed only via config flag (future) |

## 8. Security

- Service account scoped to `roles/logging.logWriter` + `roles/storage.objectViewer`
  on the config bucket only. Key rotation procedure documented. Token-proxy mode
  (no key on device) noted as future option.
- Config bucket is the fleet control plane: write access to it = control of every
  kiosk's URL. Restrict bucket IAM tightly; optional HMAC config signing listed as
  future hardening.
- Remote origins get no Tauri IPC. Injected-JS → native only via `kiosk://`
  sentinels (one-way, enumerated actions).
- Exit PIN stored as argon2id hash in remote config.
- Navigation allowlist enforced natively (not in JS).
- Credential/config file permission check at boot; wrong perms → `config.error` log.
- `dist-template/kiosk-credential.json` is an obviously-fake placeholder;
  `.gitignore` blocks real credentials.

## 9. Roadmap

| Phase | Deliverable | Contents |
|---|---|---|
| **P0** | skeleton | workspace, Tauri app boots fullscreen with hardcoded URL on Windows, CI (GitHub Actions) builds Windows+Linux |
| **P1** | Windows MVP — deployable | kiosk.ini, hardening set (§7), offline video + connectivity FSM, remote config cycle, GCL client + spool, launcher watchdog + heartbeat + safe mode, WiX MSI with WebView2 bootstrap, touch keyboard, splash/error pages |
| **P2** | Linux + robustness | WebKitGTK parity, .deb + systemd + cage docs, exit gesture + PIN pad, idle reset, webview hang detection (JS ping), memory cap restart, nightly reload/restart, health metrics, CSS/JS injection, remote log level |
| **P3** | Android | Tauri android target, Kotlin plugin (Lock Task, foreground service, boot receiver, keep-screen-on), in-process watchdog, APK + device-owner provisioning docs |
| **P4** | fleet niceties | auto-update (staged, signed; launcher performs swap), one-shot remote commands (reload/clear-cache/screenshot, executed-ID dedup), URL playlist rotation, display on/off schedule, Cloud Monitoring dashboard + log-based-metrics docs |

Platform floors: Windows 10 1809+ / Windows 11 (incl. IoT), Ubuntu 22.04 / Debian 12
(webkit2gtk-4.1, X11 or Wayland; cage recommended), Android 8.0 (API 26)+.

## 10. Testing

- `kiosk-core`: TDD, pure unit tests — config parse/validate/last-good, prober FSM
  transitions (damping), spool rotation/drain, JWT signing against fixtures,
  heartbeat protocol.
- Integration: fake GCL server + fake config server (local HTTP, in test harness);
  launcher↔main restart cycle driven by a mock main that exits/hangs on command.
- Webview/UI layer stays thin; per-platform manual smoke checklist in
  `docs/testing.md` (hardening set, video loop, reconnect, watchdog kill test).
- CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, release builds
  Windows+Linux (P0), Android build (P3).

## 11. Risks

| Risk | Mitigation |
|---|---|
| Tauri Android maturity | Android scheduled last (P3); core stays framework-independent |
| WebKitGTK codec deps for mp4 | declare gstreamer packages in .deb; document H.264 baseline for replaceable video |
| Windows touch keyboard (TabTip) quirks | dedicated P1 task with fallback (JS keyboard listed as future option) |
| WebView2 runtime missing on older Win10 | MSI bootstraps evergreen installer |
| Webview memory leaks over weeks | nightly reload/restart + P2 memory cap |
| Clock skew breaks GCL JWT | spool + retry; NTP documented as deployment requirement |
