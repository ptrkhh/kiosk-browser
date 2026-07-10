# P0 Input-Capture Gate Report (spec §9 P0 gate)

Run: `cargo run -p kiosk-main -- --spike-input --url https://example.com 2> spike.log`
on physical Windows hardware (WebView2 evergreen), window fullscreen, webview focused
(click the page first). Date/hardware/Windows build: **2026-07-11 — ARM64 (aarch64)
Windows 11 Home Single Language, build 10.0.26200.8655; WebView2 evergreen (x64 runtime,
rendered under x64 emulation — the ARM64 machine is the **dev host**; the x64 build it runs is the actual fleet arch, since every kiosk is Intel/AMD x64).** Actual run used the built exe directly
(`target/debug/kiosk-main.exe --spike-input --url https://example.com 2> spike.log`).

> **Tester note — tap timing (row 7/8).** The `SPIKE[C]` tap window is anchored to the
> *first* tap of a burst and is 3000 ms wide, so all 7 taps must land inside 3 s of the
> first. Tap **fast** (≈5 taps/sec). Slow taps (>~430 ms apart) can miss and reset the
> counter — that is a spike-instrumentation simplification, not a real pointer-capture
> failure. Only conclude POINTER FAIL if fast taps produce no `SPIKE[C] tap at (...)`
> lines at all (taps never reach the hook), not merely no `EXIT-GESTURE DETECTED`.

> **This run also clears two other deferred [WINDOWS HOST] checks** (record inline):
> - **Task 4 Step 5 — Windows compile.** Before running, `cargo check -p kiosk-main` on
>   the Windows host must succeed. The spike's Vector-A COM signatures were fixed against
>   crate source but never compiled on MSVC. If it fails, capture the error and STOP —
>   that is a Task 4 fix, not a gate result. **Compile clean? YES** — Vector A compiles
>   clean against `webview2-com 0.38.2` / `windows 0.61.3` (no COM change needed; commit
>   7cfb784 was already correct). Two host-environment fixes *were* required to reach a
>   build — see the "Windows compile" line below.
> - **Task 2 Step 7 — fullscreen behavior** = row 0 below.
>
> **ARM64 note:** an aarch64 Windows device is a valid host — WebView2 and the `windows`/
> `webview2-com` crates support `aarch64-pc-windows-msvc`. No x64 machine is required; the
> gate result is the same. Record the arch actually used.

| # | Probe (webview focused) | Expected SPIKE line | Observed? | Swallowed (nothing escaped to OS/page)? |
|---|---|---|---|---|
| 0 | environment sanity: fullscreen, borderless, always-on-top, covers taskbar; `--windowed` gives 1280×800 decorated (Task 2 Step 7) | n/a | **YES** — fullscreen, borderless, always-on-top, taskbar covered; example.com rendered in WebView2 (confirmed via right-click context menu). `--windowed` decorated mode not re-exercised this session. | n/a |
| 1 | type letters into the page | `SPIKE[B] key vk=...` per keypress | **NO webview keys reached the hook** — Ctrl/W/F5/F11 were physically pressed (Vector A saw them) yet the LL hook logged none of them | n/a (arrival test — **Tauri #13919 confirmed**: LL hook is blind to keys routed into the focused WebView2) |
| 2 | Ctrl+W | `SPIKE[A] ... vk=0x57 swallowed=true` | **YES** `SPIKE[A] accelerator vk=0x57 swallowed=true` | window stayed open ✓ |
| 3 | F5 / F11 | `SPIKE[A]` swallowed=true | **YES** `vk=0x74` (F5) & `vk=0x7A` (F11) `swallowed=true` | no reload / no fullscreen toggle ✓ |
| 4 | Alt+F4 | `SPIKE[B] ... alt=true swallowed=true` | **NO (Vector B blind)** — B never saw F4; Vector A saw `vk=0x73 swallowed=false` | **window closed** via Alt+F4 ✗ — not contained in-app |
| 5 | Alt+Tab | `SPIKE[B]` swallowed=true | **YES** `SPIKE[B] key vk=0x09 alt=true swallowed=true` (24×) | task switch **blocked**, **but the Alt+Tab switcher overlay still appears** (partial) |
| 6 | Win key | `SPIKE[B]` swallowed=true | **YES** `SPIKE[B] key vk=0x5B swallowed=true` (10×) | **LEAK — Start menu still opens** (hook swallows key*down*; Start fires on key*up*, which passes) ✗ |
| 7 | 7 fast clicks in top-left corner | `SPIKE[C] EXIT-GESTURE DETECTED` | **YES** `SPIKE[C] tap at (0,0) in_region=true` ×7 → `SPIKE[C] EXIT-GESTURE DETECTED (7 taps < 3000 ms)` — fired **twice** | n/a |
| 8 | 7 fast touch taps top-left (touch hardware) | `SPIKE[C] EXIT-GESTURE DETECTED` | corner-tap capture worked (mouse/touch source not isolated); **but touch edge-swipes leaked to the shell**: right-edge→Action Center, bottom-edge→Start menu (not seen by `WH_MOUSE_LL`) | n/a |

`SPIKE[hb] kb_events_total` after the session: **38 — but every one was an OS hotkey
(Alt+Tab / Win / Alt); ZERO webview-delivered keys.** So the hook is *not* globally blind
(it catches shell-intercepted hotkeys) yet *is* blind to keys the focused WebView2
consumes — Ctrl/W/F5/F11 and any typed letters never reach it (Tauri #13919).

Windows compile (`cargo check -p kiosk-main`, arch used): **CLEAN — built for x64, the sole Windows deployment target (every kiosk is Intel/AMD x64).** This
host has no native ARM64 MSVC compiler/linker (`Hostarm64/arm64/` ships no `cl.exe`/
`link.exe`), and VS 2026 ("18") + a non-functional bundled `vswhere` means rustc cannot
auto-discover MSVC (it fell back to a bare `link.exe`, which Git Bash's coreutils `link`
shadowed). Resolved by building with the **x86_64 Rust toolchain (rustc 1.97.0) under x64
emulation**, inside the `arm64_x64` VS dev environment (`vcvarsarm64_amd64.bat`). Build
also required adding `crates/kiosk-main/icons/icon.ico` — `tauri-build` needs it for the
Windows executable resource and the P0 skeleton never created one (committed separately as
a P0 fix). WebView2 and the Win32 hooks behaved normally; because x64 is the fleet arch,
this gate exercised the **production architecture** — the ARM64 dev box merely runs the
x64 build under emulation.

## Verdict (fills spec §12/OD-5 and shapes the P1 hardening plan)

- [ ] KEYBOARD PASS — rows 1–6 observed+swallowed ⇒ in-app blocking is real defense-in-depth; OS lockdown remains the boundary per spec §7.2.
- [x] KEYBOARD PARTIAL — vector A works, B blind ⇒ accelerators in-app; Alt+F4/Alt+Tab/Win closed ONLY by Assigned Access / Shell Launcher, which becomes a hard P1 deployment requirement.
- [ ] KEYBOARD FAIL — neither arrives ⇒ drop in-app blocking from P1 code scope; §7.2 OS lockdown is the only mechanism.
- [x] POINTER PASS — rows 7 (and 8 on touch) fire ⇒ native exit gesture per spec §3.5 is viable.
- [ ] POINTER FAIL — no SPIKE[C] over the webview ⇒ P1 exit gesture uses the reserved AcceleratorKeyPressed technician chord fallback (spec §3.5).

Notes / anomalies:

- **Vector A (WebView2 `AcceleratorKeyPressed`) is the only reliable in-app keyboard
  defense**, and only for accelerators the webview receives: Ctrl+W (`0x57`), F5 (`0x74`),
  F11 (`0x7A`) were seen and swallowed with `SetHandled(true)`, and nothing leaked
  (window stayed open, no reload/toggle).
- **Vector B is not simply "blind" — it is worse than the checkbox wording:** it *sees*
  shell hotkeys (Alt+Tab, Win) and returns non-zero, but that does **not contain** them —
  the Start menu still opens (keydown-only swallow; Start triggers on keyup), the Alt+Tab
  switcher overlay still flashes (the switch itself is blocked), and Alt+F4 is invisible to
  it (window closed). It is simultaneously blind to every webview-delivered key. Net effect
  for the P1 plan is exactly the PARTIAL consequence: **Alt+F4 / Alt+Tab / Win containment
  must come from OS lockdown (Assigned Access / Shell Launcher), not in-app hooks.**
- **Pointer capture works** for the native exit gesture: 7 fast top-left taps over the
  focused webview reliably produced `EXIT-GESTURE DETECTED` via `WH_MOUSE_LL` (fired twice).
  §3.5 native gesture is viable — no need for the `AcceleratorKeyPressed` technician-chord
  fallback.
- **Additional lockdown items surfaced (feed P1 §7 / §7.2):** (a) WebView2 right-click
  context menu (Back / Refresh / Print / …) is available and must be disabled in webview
  hardening; (b) touch **edge-swipes** bypass `WH_MOUSE_LL` entirely — right-edge opens the
  Action Center, bottom-edge opens Start — so OS-level edge-gesture lockdown is required in
  addition to the shortcut lockdown.
- **Host note:** the gate exercised **x64 — the fleet's production arch**
  (`x86_64-pc-windows-msvc`); it ran under emulation only because the dev *box* is ARM64
  (no native arm64 MSVC tooling — see Windows compile line). aarch64 Windows is **not** a
  deployment target.
