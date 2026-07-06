# P0 Input-Capture Gate Report (spec §9 P0 gate)

Run: `cargo run -p kiosk-main -- --spike-input --url https://example.com 2> spike.log`
on physical Windows hardware (WebView2 evergreen), window fullscreen, webview focused
(click the page first). Date/hardware/Windows build: ___

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
>   that is a Task 4 fix, not a gate result. Compile clean? ___
> - **Task 2 Step 7 — fullscreen behavior** = row 0 below.
>
> **ARM64 note:** an aarch64 Windows device is a valid host — WebView2 and the `windows`/
> `webview2-com` crates support `aarch64-pc-windows-msvc`. No x64 machine is required; the
> gate result is the same. Record the arch actually used.

| # | Probe (webview focused) | Expected SPIKE line | Observed? | Swallowed (nothing escaped to OS/page)? |
|---|---|---|---|---|
| 0 | environment sanity: fullscreen, borderless, always-on-top, covers taskbar; `--windowed` gives 1280×800 decorated (Task 2 Step 7) | n/a | ___ | n/a |
| 1 | type letters into the page | `SPIKE[B] key vk=...` per keypress | ___ | n/a (arrival test — Tauri #13919) |
| 2 | Ctrl+W | `SPIKE[A] ... vk=0x57 swallowed=true` | ___ | window stayed open? ___ |
| 3 | F5 / F11 | `SPIKE[A]` swallowed=true | ___ | no reload / no fullscreen toggle? ___ |
| 4 | Alt+F4 | `SPIKE[B] ... alt=true swallowed=true` | ___ | window stayed open? ___ |
| 5 | Alt+Tab | `SPIKE[B]` swallowed=true | ___ | no task switch? ___ |
| 6 | Win key | `SPIKE[B]` swallowed=true | ___ | Start menu did not open? ___ |
| 7 | 7 fast clicks in top-left corner | `SPIKE[C] EXIT-GESTURE DETECTED` | ___ | n/a |
| 8 | 7 fast touch taps top-left (touch hardware) | `SPIKE[C] EXIT-GESTURE DETECTED` | ___ | n/a |

`SPIKE[hb] kb_events_total` after the session: ___ (0 while typing ⇒ hook is blind, Tauri #13919 confirmed)

Windows compile (`cargo check -p kiosk-main`, arch used): ___

## Verdict (fills spec §12/OD-5 and shapes the P1 hardening plan)

- [ ] KEYBOARD PASS — rows 1–6 observed+swallowed ⇒ in-app blocking is real defense-in-depth; OS lockdown remains the boundary per spec §7.2.
- [ ] KEYBOARD PARTIAL — vector A works, B blind ⇒ accelerators in-app; Alt+F4/Alt+Tab/Win closed ONLY by Assigned Access / Shell Launcher, which becomes a hard P1 deployment requirement.
- [ ] KEYBOARD FAIL — neither arrives ⇒ drop in-app blocking from P1 code scope; §7.2 OS lockdown is the only mechanism.
- [ ] POINTER PASS — rows 7 (and 8 on touch) fire ⇒ native exit gesture per spec §3.5 is viable.
- [ ] POINTER FAIL — no SPIKE[C] over the webview ⇒ P1 exit gesture uses the reserved AcceleratorKeyPressed technician chord fallback (spec §3.5).

Notes / anomalies: ___
