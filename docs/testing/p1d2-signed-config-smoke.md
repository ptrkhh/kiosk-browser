# Signed-config smoke runbook — closes deferred 3b/4 (+ exercises D2b gaps)

Deferred D2a/D2b hardware steps that need a **signed** remote config:
- **3b** error_page retry ladder (validates the C1 fix on hardware)
- **4** config-change navigation (RT-04 navigate-half)
- Bonus: exercises D2b nav-guard / egress / PDF-block against a real config.

Signing tool: `crates/kiosk-core/examples/kioskctl.rs` (Linux-runnable; reuses the exact
JCS+Ed25519 recipe `signature::verify_signed` checks — `kioskctl selftest` proves the
roundtrip).

## D2c native-input smoke (PIN pad / exit gesture / idle reset) — NO GCP, NO signing needed

The exit gesture reads `pin_hash` from `kiosk.ini [exit_gesture]`, so PIN/exit/lockout are
testable with just a local `kiosk.ini` — no signed config, no GCP.

1. Make a PIN hash: `cargo run -p kiosk-core --example kioskctl -- hash-pin 4291`
   → paste the `$argon2id$…` line into `kiosk.ini`:
   ```ini
   [exit_gesture]
   pin_hash = $argon2id$v=19$m=19456,t=2,p=1$...   ; from hash-pin
   taps     = 7
   region   = top-left
   ```
2. Build + run the kiosk (see below). Then, at the device:
   - **Exit gesture (tap):** 7 fast taps in the top-left corner → PIN pad opens. Record
     whether tap-capture worked over the focused webview (the P0-unconfirmed question). If
     it does NOT open, use the **technician chord** (the reserved combo wired in D2c/T4) —
     one of the two must open it.
   - **PIN → exit 86:** type `4291` → the process exits. Confirm `echo %ERRORLEVEL%` = `86`.
   - **Lockout survives restart:** type a wrong PIN 4+ times → "blocked until …"; kill +
     relaunch the app → still blocked (reads the persisted lockout, SEC-05).
   - **No pin_hash ⇒ disabled:** remove the `[exit_gesture]` section → the gesture never
     opens the pad (cfg-12; the device is exitable only via §7.2 OS lockdown).
   - **Idle reset:** with defaults the reset fires after 180 s idle (or set a shorter
     `idle_reset_seconds` via a signed config, below). Leave the kiosk untouched → it
     reloads home; if `clear_data_on_reset`, a cookie/autofill entry set beforehand is gone
     and the screen does NOT flash home before the clear completes (the privacy gate).

### Run log — 2026-07-27 (Windows 11 ARM64 dev host, x64 build, no GCP)

Setup: `D:\kiosk-smoke\kiosk.ini` (device_id `lobby-01`, bootstrap url `https://example.com/`,
PIN `4291`), `kiosk-main.exe --config D:/kiosk-smoke`, telemetry disabled (no credential).

| Step | Result |
|---|---|
| PIN pad opens; `4291` → exit code | **PASS** — `EXITCODE=86` |
| Lockout survives restart (SEC-05) | **PASS** — `exit-lockout.json` `failures:3 → 4` across restart; wait 5 s → 10 s (`BACKOFF_BASE_S` doubling), so the counter came off disk |
| No `pin_hash` ⇒ gesture disabled (cfg-12) | **PASS** — `gesture: … not configured (cfg-12); tap capture disabled` + `open_pin_pad no-op`; neither 7 taps nor Ctrl+Alt+Shift+K opened the pad |
| Idle reset (180 s default) | **PASS** — reloaded home |
| Off-allowlist nav (`example.com` → `iana.org`) | **PASS** — with telemetry on, `spool/high` shows `nav.blocked{reason:not_allowlisted}` AND `nav.blocked{reason:egress}` for `https://iana.org` (WARNING events go to `spool/high`, not `low`) |
| Telemetry upload (real GCP key, project `ubm-gen-ai`) | **PASS** — `logName projects/ubm-gen-ai/logs/kiosk`, `generic_node/lobby-01`; `app.start`/`config.applied`/`net.online`/`app.stop` written, cursor `committed` advances to full count. Entries pending at exit are drained on the next boot (spool replay), `dropped:0` |

Open finding — the egress guard blocks the app's OWN IPC origin:
`nav.blocked{reason:egress, url:"http://ipc.localhost"}`. Nothing user-visible broke (the PIN
pad IPC still worked), but the internal origin should not be reported as blocked egress —
check the egress allow list against `APP_ORIGIN`/`ipc.localhost`.

Open finding — WebView2 hardening degraded on this host, logged every boot:
```
hardening: CoreWebView2Settings does not implement Settings4, autofill/password-autosave will stay on: Access is denied. (0x80004002)
hardening: CoreWebView2Settings does not implement Settings5, pinch zoom will stay on: No such interface supported (0x80004002)
```
⇒ autofill/password-save and pinch zoom remain ON. Check the WebView2 Runtime version on the
deployment image before shipping; re-run this on real kiosk hardware.

### Run log — 2026-07-28 (Windows 11 ARM64 dev host, x64 build, evergreen WebView2 **150.0.4078.99**, signed config, real GCP)

Same host as the 2026-07-27 run: Snapdragon X Plus X1P42100; the x64 build (and the x64
PowerShell used to measure windows) runs under emulation, so `PROCESSOR_ARCHITECTURE`
reads `AMD64` — do not mistake this for an x64 box.

Setup: build `KIOSK_CONFIG_PUBKEY_B64` pinned (fresh keypair, seed not recorded here);
`D:\kiosk-smoke\kiosk.ini` (device_id `lobby-01`, project `ubm-gen-ai`, credential present);
signed configs served from a local `127.0.0.1:8000` static server at the device's
`config_url`. Two configs: **rev 10** (`logging.health_sample_s=15`, `display.monitor=5`)
and **rev 11** (`display.monitor=1`). Note both fields are read **once at boot** from the
*cached* config, so each takes effect on the boot *after* the one that fetched it.
Monitors on this host: `DISPLAY1` primary 1536x960 @ (0,0), `DISPLAY2` 1920x1080 @ (-1920,0).

| Check | Result |
|---|---|
| Settings4/Settings5 warnings on a current runtime | **FAIL then FIXED** — see finding below. Evergreen 150.0.4078.99 still logged both; root cause was a wrong-object `cast`, not the runtime. After the one-line fix, boot stderr is clean and no `config.warn{hardening.*}` is emitted |
| `http://ipc.localhost` no longer `nav.blocked{egress}` | **PASS** — 0 occurrences of `ipc.localhost` across `spool/high` + `spool/low` over 4 boots (D2e classifier fix holds) |
| `health.sample` every ~15 s with all 6 keys | **PASS** — 6 samples in an 80 s run at `10:48:17 / :31.5 / :46.5 / 10:49:01.5 / :16.6 / :31.6` (Δ 14.5–15.0 s). Payload: `cpu_percent`, `mem_used_mb` (+`mem_total_mb`), `disk_free_mb`, `uptime_secs`, `spool_dropped_expired`. `severity:INFO` ⇒ `spool/low`, cursor `committed` advanced to full count, `dropped:0` ⇒ delivered to Cloud Logging (`projects/ubm-gen-ai/logs/kiosk`, `generic_node/lobby-01`) |
| `display.monitor=5` ⇒ primary + `config.warn` | **PASS** — `config.warn{field:"display.monitor", reason:"index beyond available displays; using primary"}`, window opened on primary, nothing else degraded |
| Panic ⇒ `<ProgramData>\kiosk\crash-panic.txt` | **PASS** — forced via `kiosk-main.exe --config D:/kiosk-smoke-does-not-exist` (exit 101). File contains `panicked at crates\kiosk-main\src\main.rs:294:9: kiosk-main: cannot read …\kiosk.ini (…os error 3); pass --config <dir> in dev`. Confirms the *early*, file-only hook (fires before telemetry exists) |
| **I1** — `display.monitor=1` window size/placement | **FAIL then FIXED** — before: `L=-1920 T=0 R=0 B=1200` ⇒ 1920x**1200** on a 1920x**1080** panel (right monitor, wrong size, 120 px overhang). After the build-hidden → position → fullscreen → show reorder: `L=-1920 T=0 R=0 B=1080` ⇒ **exactly `DISPLAY2`'s 1920x1080, no overhang**. See finding below |
| I1 regression — `display.monitor=5` still falls back correctly | **PASS** — rev 12, `GetWindowRect` = `L=0 T=0 R=1536 B=960` = the primary's full extent, plus the `config.warn{display.monitor}`. The reorder did not break the fallback branch |

**FIXED — Settings4/5 was never a runtime problem (root cause).** `hardening.rs` called
`webview2.cast::<ICoreWebView2Settings4>()` / `…Settings5` on the **`ICoreWebView2`**
object. Those interfaces live on the **settings** object (`ICoreWebView2Settings`), so the
QI returns `E_NOINTERFACE` on *every* runtime version — which is exactly what the
2026-07-27 ARM64 run and the first boot of this run both saw. Fixed to `settings.cast::<…>()`
(two lines, `crates/kiosk-main/src/hardening.rs`); after rebuild, boot stderr has no
`hardening:` lines and no `config.warn{hardening.autofill|hardening.pinch_zoom}` reaches the
spool ⇒ **password-autosave, general autofill and pinch-zoom are now actually OFF.**
The 2026-07-27 "check the WebView2 Runtime version on the deployment image" note is
superseded — no runtime-version prerequisite exists for these flags.

**FIXED — I1: the window moved, but kept the primary's size.**
Measured with both panels attached, `display.monitor=1`, rev 11 applied:

| | logical (Screen.AllScreens) | physical | scale |
|---|---|---|---|
| `DISPLAY1` (internal, primary, `SDC4187`) | 1536x960 @ (0,0) | **1920x1200** | 125% |
| `DISPLAY2` (external, `TSB010B` ~24") | 1920x1080 @ (-1920,0) | 1920x1080 | 100% |

Window rect: `L=-1920 T=0 R=0 B=1200`. The origin is `DISPLAY2`'s — so `set_position` **does**
work and the earlier "never moves" reading was wrong. The *size*, 1920x1200, is the
**primary's physical** extent: fullscreen was applied while the window still belonged to
`DISPLAY1`, so it captured that monitor's size, and the later `set_position` moved the window
without re-deriving the extent for the target. On a mixed-resolution / mixed-DPI pair this
means 120 px hang off the bottom of the external panel — invisible to the operator (it looks
correctly full-screen) but real, and it would be much more visible on a target monitor
*smaller* than the primary.

Fix applied in `crates/kiosk-main/src/main.rs` `setup()`: drop `.fullscreen(true)` from the
`WebviewWindowBuilder`, add `.visible(false)`, and after the `display.monitor` positioning
block call `window.set_fullscreen(true)` → `show()` → `set_focus()`. A window that is
*already* fullscreen when `set_position` runs keeps the extent it captured from the monitor
it was born on; fullscreening *after* the move makes tao re-evaluate against the monitor the
window is now on. `visible(false)` covers the gap between `build()` and the fullscreen call
so there's no default-size flash at boot. The `set_fullscreen` call sits outside the
`available_monitors()` block, so a failed monitor query still yields a fullscreen kiosk
rather than a small floating window.

Re-measured after the fix, `display.monitor=1`, both panels attached:
`L=-1920 T=0 R=0 B=1080` ⇒ **1920x1080, exactly `DISPLAY2`, no overhang.** Fallback branch
re-checked with `display.monitor=5` (rev 12): `L=0 T=0 R=1536 B=960` = the primary's full
extent, `config.warn{display.monitor}` still emitted. `cargo test -p kiosk-main --release`:
**121 passed, 0 failed.** Side effect worth noting: `focus.lost` events over a ~40 s run
dropped from 16 to 1 — the old build was fighting the desktop for focus during the
fullscreen-then-move window.

Note on measurement units: the `GetWindowRect` figures above come from a DPI-unaware x64
PowerShell, so they are in the same logical coordinate space `Screen.AllScreens` reports.
The pre-fix `1200` was the primary's *physical* height leaking through unscaled — which is
the bug's signature, not a measurement artifact.

Operator observation during the same run: the kiosk filled the external monitor, while the
internal panel showed nothing **and accepted no input** — clicks/keys on the primary desktop
did nothing while the kiosk held focus (the run also logged 16 `focus.lost` events as the
focus-lock fought the desktop). Expected for a kiosk that owns input, but worth pinning as
behaviour: with `display.monitor` pointing at a secondary, the other monitor is dead space,
not a usable second desktop.

Correction to the pre-monitor-replug measurement in this same session: an earlier boot
measured `1536x960` on the primary plus a `config.warn{display.monitor,"index beyond
available displays"}` at `config_revision:11`. That was taken while the external monitor was
physically **unplugged** — `available_monitors()` correctly reported 1. Not a bug; discard
that reading.

**Open finding — steady-state `config.error` noise.** A poll that returns the *already
applied* revision logs `severity:ERROR` `config.error{"anti-rollback: revision 10 <= last
applied 10"}`. Anti-rollback rejecting an equal revision is correct, but a device sitting on
the current config will emit an ERROR every `config_poll_s` (300 s ⇒ ~288/day/device) for a
non-event. `revision == last_applied` should be a silent no-op; only `<` deserves the error.

**Minor — first `health.sample` reads `cpu_percent: 100.0`.** `uptime_secs:0` sample only:
`sysinfo` has no prior refresh to diff against, so the first CPU reading is meaningless.
Subsequent samples are sane. Either skip the first tick or prime the refresh at startup.

**Minor — `labels.config_revision` is `""`** on events emitted before the first
`config.applied` (e.g. `app.start`, the boot `health.sample`). Cosmetic; noted so a Cloud
Logging filter on that label doesn't silently miss boot-time entries.

Host prerequisites learned (this run): x64 host, `cargo` must run inside
`vcvarsall.bat x64` (plain `cargo` fails with `cl.exe: program not found`). `python` on this
box is the Microsoft Store alias stub, not a real interpreter — `python -m http.server` fails;
used a one-line node `http` server instead.

Host prerequisites learned: VS BuildTools needs the **VCTools** workload; this ARM64 dev host has
no ARM64 MSVC target toolset, so the repo is pinned via `rustup override set
stable-x86_64-pc-windows-msvc` and cargo must run inside
`vcvarsall.bat arm64_x64` (plain `cargo` fails with `cl.exe: program not found`).

## 1. Keys (once)
```
cargo run -p kiosk-core --example kioskctl -- keygen
# → KIOSK_SIGNING_KEY_B64=<PRIVATE seed — keep secret, never commit/bake>
#   KIOSK_CONFIG_PUBKEY_B64=<PUBLIC pinned key>
```

## 2. Build the kiosk with the pinned key (Windows host)
```
set KIOSK_CONFIG_PUBKEY_B64=<pubkey>   # baked via option_env! (signature::pinned_key)
cargo build -p kiosk-main --release
```
Without this env the build fails-closed (rejects every fetched config) — that is why
today's smoke used only the unsigned bootstrap path.

## 3. Author + sign a per-device config
`device_id` MUST equal the device's **effective** id (`[kiosk] device_id` in kiosk.ini,
or the auto machine-id) — device binding (§8/SEC-11) rejects a mismatch. `revision` must
exceed the last applied.
```jsonc
// cfg.json
{ "revision": 2, "device_id": "lobby-01",
  "content": { "url": "https://<reachable-site>/", "fallback": "error_page",
               "allowlist": ["https://<reachable-site>/*"] } }
```
```
KIOSK_SIGNING_KEY_B64=<seed> \
  cargo run -p kiosk-core --example kioskctl -- sign cfg.json > lobby-01.json
```

## 4. Serve it at the device's `config_url`
Any static host. Local smoke:
```
python3 -m http.server 8000    # serves lobby-01.json at http://<host>:8000/lobby-01.json
```
Point `[kiosk] config_url` in kiosk.ini at that URL. (Prod: the GCS bucket object,
per-device object read-scoped, SEC-04.)

## 5. Run the steps
- **4 config-change nav:** boot on rev 1 (site A). Sign rev 2 with a new `content.url`
  (site B), replace the served object. Within one `config_poll_s` the webview navigates to
  B (at-level `ConfigApplied`). Confirm `config.applied{revision:2}` in Cloud Logging.
- **3b error_page retry:** sign a config with `fallback:"error_page"` and a `content.url`
  that fails to load (DNS/TLS/404) while the network is up. Confirm the bundled error page
  shows and retries on the countdown up to `error_max_retries`, then falls to the offline
  video — the C1 fix (retry ladder alive) validated on hardware.
- **Bonus (D2b):** with a real allowlist, confirm an off-allowlist nav is blocked
  (`nav.blocked`), an off-allowlist subresource is blocked (egress), a `mailto:` no-ops,
  a PDF link's handling (PDF runtime block is a known P1 gap — see ledger).

## Security
The private seed and any `google-credential.json` are secrets — keep out of the repo and
off shared dirs; delete local copies after the run (a prior smoke left a GCP key in
`C:\Users\p\kiosk-smoke\`, since removed).
