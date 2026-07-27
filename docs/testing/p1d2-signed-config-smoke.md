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
