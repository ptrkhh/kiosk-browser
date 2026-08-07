# COVERAGE VERIFIER — P2 requirement → owner traceability matrix

Role: Coverage Verifier. No argument, no proposal. Sources read in full: parent spec of
record `docs/superpowers/specs/2026-07-05-kiosk-browser-design.md` (rev 2, 933 lines);
all seven P2 sub-specs (`2026-08-06-p2{a..g}-*.md`); P1 sub-specs D2a/D2b/D2c/D2e, E1, E2,
F1, F2, G; and `crates/` (`kiosk-core`, `kiosk-main`, `kiosk-launcher`).

**Method note.** Every UNOWNED verdict below was produced by grepping all seven P2 specs
for the mechanism term, the requirement ID, and at least one paraphrase, before declaring.
Naïve substring greps (`ping`, `hang`) match `mapping`/`changes` in all seven specs and
were discarded; the anchored greps used are recorded per row.

**Result: 8 UNOWNED, 6 PARTIAL.** Summary list at the end.

---

## 1. Master matrix

Obligations are merged where the parent states the same requirement in several sections
(all § refs cited on the merged row), so the counts are of *distinct* obligations, not of
parent sentences.

### 1a. §9 roadmap P2 row — decomposed, comma-separated item by item

Verbatim row (§9, line 839): *"WebKitGTK parity (incl. pinch-gesture intercept, keep-awake
at compositor), .deb + systemd + cage docs + §7.2 Linux hardening, idle reset (native),
memory cap restart + health-sampled RSS, cross-platform webview-hang detection (JS ping),
config-driven `inject_css`/`inject_js` knobs (behind signed config), remote log level,
restart_app"*.

| # | Obligation (verbatim fragment + parent §) | ID(s) | Owner | Evidence | Status |
|---|---|---|---|---|---|
| R1 | "WebKitGTK parity" (§9 P2 row) — roll-up of the §7 Linux column | §7 preamble, PF-* | A + B | A: "gives the existing `#[cfg(not(windows))]` stubs in `crates/kiosk-main/src/{nav,recovery,clear}.rs` real bodies"; B: "The three Windows-only control groups get Linux bodies with honest parity: `hardening.rs` …, `egress.rs` …, `scheme_guard.rs` …, plus `display.keep_awake`." | **PARTIAL** — roll-up. Parity is incomplete by R2, R11, R12, R13 and R6 below. A+B cover context menu, devtools, base zoom, script dialogs, permissions, autofill, downloads, popups, egress, crash recovery, nav guard. |
| R2 | "incl. pinch-gesture intercept" (§9 P2 row; §7 zoom row "interactive pinch is GTK-owned and needs a gesture-controller intercept in the platform layer / a wry patch, validated on touch hardware in P2"; §11 "P2 intercepts the GTK zoom gesture in the platform layer / upstreams a wry hook") | PF-04, wry #544 | **none** | Grep `-iE "pinch\|gesture.controller\|wry #544\|544"` over all seven specs: **zero hits for pinch**. Only zoom hits are B's base-zoom rows: "`WebViewExt::set_zoom_level` (`web_view.rs:1980`); whether full-content zoom needs `zoom-text-only=false` asserted explicitly — plan-time". Parent explicitly says `zoom-level` "fixes only base zoom". P2-D owns GDK event interception and never mentions zoom/pinch gestures. | **UNOWNED** |
| R3 | "keep-awake at compositor" (§9); §7 keep-awake row "PRIMARY is configuring cage/wlroots not to blank (idle-inhibit is secondary…)"; §11 "P2 disables blanking at the compositor (cage/wlroots) as **primary**" | PF-07, M8, H5 | B (+G) | B: "spawn `systemd-inhibit --what=idle:sleep … --mode=block cat` … The *suspenders* are P2-G's image contract: no idle daemon, `IdleAction=ignore` in `logind.conf`". G §2: "Sleep/idle: mask `sleep.target suspend.target hibernate.target hybrid-sleep.target`; `IdleAction=ignore` (B's keep-awake suspenders — the `systemd-inhibit` child is the belt); no screensaver/idle daemon installed" + "kernel `consoleblank=0`". G H3: "Keep-awake positive: `systemd-inhibit --list` shows the hold; display never blanks over 24 h". | **PARTIAL** — the parent's **PRIMARY** (configure cage/wlroots not to blank) is never stated as such by any spec: B calls `systemd-inhibit` the **belt** and the compositor/image config the **suspenders**, exactly inverting §7/§11. No spec names a cage or wlroots blanking setting; the compositor half is discharged only indirectly (absence of an idle daemon + `IdleAction=ignore` + `consoleblank=0`). The parent's own caveat — "`systemd-inhibit` blocks *suspend* only, display blanking is compositor-owned" — is not addressed. |
| R4 | ".deb" (§9) | §4 paths | G | G §1: "`.deb` — `packaging/linux/` … **Payload:** `kiosk-main` + `kiosk-launcher` → `/usr/libexec/kiosk/` …"; assembly executed by F §3 ("`.deb` assembly from P2-G's `packaging/linux/` (dpkg-deb; the package *content* is G's spec, F only executes it)"). | **COVERED** |
| R5 | "systemd" (§9); §3.1 "systemd `Restart=always` with `RestartPreventExitStatus=86` and `SuccessExitStatus=86`" | arch-05 | C (shape) + G (values/install) | C: "`ExecStart=cage -- /usr/libexec/kiosk/kiosk-launcher` / `Restart=always` / `RestartPreventExitStatus=86` / `RuntimeDirectory=kiosk`"; G §1: "`kiosk.service` (C's contract shape + G's values: `StartLimitIntervalSec`/`StartLimitBurst` chosen here …)" and "**Autostart:** `systemctl enable kiosk.service` in postinst". | **COVERED** — note: parent §3.1 also names `SuccessExitStatus=86`; C's unit block lists only `RestartPreventExitStatus=86`. Cosmetic (LOW), the behaviour is discharged. |
| R6 | "cage docs" (§9); §7.2 "cage (Wayland) locked session as the supported secure config (X11/openbox is documented but NOT app-enforced — demo only)" | §7.2 Linux | G | G §2: "cage locked session as the supported secure config; **X11/openbox stays demo-only, documented as NOT app-enforced** (parent §7.2 verbatim — one appendix paragraph, no more)." | **COVERED** |
| R7 | "§7.2 Linux hardening" (§9) — see §1c below for the five sub-items | RT-11/12/15, H5, PF-07, M8 | G | G §2 lockdown runbook, itemized. | **COVERED** (sub-items in §1c) |
| R8 | "idle reset (native)" (§9); §3.5 "native input-idle timer (platform last-input timestamp)" | §3.5, SEC-02/06 | D | D: "`idle_reset_seconds` fires `IdleExpired` (completing the app-path `ClearProfile` → `ProfileCleared` chain that P2-A could only reach via a harness binary)" + "`idle.rs` — Linux body … idle seconds = now − `ActivityClock` … The stub at `idle.rs:57-64` is replaced". Profile-clear body from A: "`clear.rs` — profile clear with real completion". | **COVERED** |
| R9 | "memory cap restart" (§9); §5.2 `"max_webview_mem_mb": 1500 // 0 = off; {0} ∪ [256, 8192] (P2)`; §11 "P2 adds memory-cap restart" | cfg (max_webview_mem_mb), M-leak risk row | E | E: "**memory-cap restart + health-sampled RSS** — the parent roadmap assigns these to P2 verbatim … and E is their natural owner" + "The cap decision is a pure, host-tested latch: N consecutive samples over the cap → one `health.memory_cap` event → clean process exit with a dedicated restart exit code — **never 86**". | **PARTIAL** — the mechanism is owned, the **schema field is renamed without a stated migration**: parent §5.2 and `crates/kiosk-core/src/config/validate.rs:19` both name `maintenance.max_webview_mem_mb` (with the RT-08 capability warning already wired for that exact key); E says "New config key `memory_max_mb` (schema section placed at plan time beside its consumers; default **0 = off**)". Parent's range `{0} ∪ [256, 8192]` and default `1500` are also not carried over (E says default 0). What is missing: a statement that `memory_max_mb` *is* `maintenance.max_webview_mem_mb`, or that the parent's key is superseded. |
| R10 | "health-sampled RSS" (§9); §6 `health.sample` "CPU %, mem, disk free, uptime, **webview RSS**, `spool.dropped_expired` (P2)"; §11 "P2 adds … health-sampled RSS" | TEL, D2e deferral | E | E: "`health.rs` gains an RSS sample per existing poll tick (`/proc/self/status` `VmRSS` on Linux; `GetProcessMemoryInfo` on Windows), surfaced through the existing metrics pipeline (D2e)." Deferral confirmed at P1: D2e "**health.sample is BASIC in P1.** Roadmap §9 puts webview-process **RSS** and the **memory-cap restart** in P2". `crates/kiosk-main/src/health.rs:3` — "Webview-process RSS / `max_webview_mem_mb` enforcement" listed as absent. | **COVERED** |
| R11 | "cross-platform webview-hang detection (JS ping)" (§9); §3.1 "The **cross-platform webview round-trip / JS-ping** liveness (main emits each 5 s heartbeat only after round-tripping a no-op through the webview — evaluate a trivial script, await its echo, 3 s cap; a wedged renderer withholds the heartbeat → 3-missed rule restarts main) lands in **P2**"; §3.1 arch-15 case (c) "child alive + healthy channel + no round-trip ping → genuine hang → restart"; §6 `watchdog.hang` "(Win native P1; **JS-ping P2**)" | **arch-04, RT-02, OD-1** | **none** | Grep `-iE "js.?ping\|round.?trip\|liveness\|unresponsive\|webview.hang\|watchdog\\.hang\|responsive\|evaluate.*script"` over all seven specs → **one hit**, and it is a negative: A `recovery.rs` section — "Windows reserves `Reload` for `RENDER_PROCESS_UNRESPONSIVE`, which has no WebKitGTK analogue". That sentence *names* the gap and hands it to nobody. C ports the heartbeat client verbatim as process liveness — "same `RECONNECT_BACKOFF` … and the same Ready-then-Ping frame discipline over shared `kiosk_core::ipc`" — with no round-trip gate added; C's smoke 15 (`SIGSTOP` main) exercises a *whole-process* stop, not a renderer-only wedge. `arch-04`/`RT-02` appear in zero P2 specs. | **UNOWNED** |
| R12 | "config-driven `inject_css`/`inject_js` knobs (behind signed config)" (§9); §7 preamble "**The document-start injection engine ships in P1** … P2 only exposes the operator-supplied `inject_css`/`inject_js` knobs on top of it (RT-16)"; §5.2 `"inject_css": "" // REJECTED unless config carries a valid signature (SEC-01)`; §12/OD-3 "keep, gated behind signed config (applied)" | **RT-16, SEC-01, OD-3** | **none** | Grep `-iE "inject_css\|inject_js\|operator.*knob\|SEC-01\|RT-16"` over all seven specs → **zero hits**. B mentions injection only for its own CSP belt ("injected as a document-start user script"); D mentions "input injection" (smoke wording). The engine exists and is platform-agnostic (`crates/kiosk-main/src/inject.rs`, `build_injection`, called unconditionally at `main.rs:1046`) — but no spec exposes the two operator knobs on it, and `crates/kiosk-core/src/config/validate.rs:16-17` still lists both fields in `UNIMPLEMENTED` with phase `"P2"`. | **UNOWNED** |
| R13 | "remote log level" (§9); §5.2 `"logging": { "level": "info" }` | cfg / TEL-06 | **none** | Grep `-iE "log.?level\|logging\\.level\|remote log"` over all seven specs → **zero hits**. Grep over the whole repo for `LogLevel\|log_level\|severity_filter\|min_level` → **zero hits**: no severity filter exists anywhere in `kiosk-core/logging`. `VALID_LOG_LEVELS` (`validate.rs:11`) validates the string and nothing consumes it, and the field is **not** in the RT-08 `UNIMPLEMENTED` table, so setting it produces neither behaviour nor a `config.warn` — a silent no-op today. | **UNOWNED** |
| R14 | "restart_app" (§9); §5.2 `"restart_app": null, // "04:30" = daily full restart` | cfg | **none** | Grep `-iE "restart_app\|maintenance"` over all seven specs → one hit, E quoting §10 about memory-cap, not `restart_app`. In code: `crates/kiosk-core/src/config/schema.rs:230` declares the field; `validate.rs:20` lists `("maintenance.restart_app", "P2")` in `UNIMPLEMENTED`; `crates/kiosk-main/src/maintenance.rs` implements the **nightly reload only** ("Nightly-reload timer (spec `maintenance.nightly_reload`)"). No P2 spec picks it up. | **UNOWNED** |

### 1b. Every other parent sentence that says "P2" / "in P2" / "lands in P2" / "validated in P2" / "at P2"

Grep `P2` over the parent yields 14 lines; those already discharged above are cross-referenced.

| # | Obligation (verbatim fragment + parent §) | ID(s) | Owner | Evidence | Status |
|---|---|---|---|---|---|
| P1 | §3.1: "lands in **P2** (WebKitGTK/Android, where no native unresponsive signal exists)" | arch-04, RT-02, OD-1 | — | see R11 | **UNOWNED** (= R11) |
| P2 | §5.2 line 538: `"max_webview_mem_mb": 1500 … (P2)` | cfg | E | see R9 | **PARTIAL** (= R9) |
| P3 | §6 line 660: `webview.crash` "renderer process died — auto-reload (P1 Win via ProcessFailed; **WebKitGTK P2**; Android P3)" | — | A | A `recovery.rs`: "`web-process-terminated(reason)` → `webview.crash` telemetry + `WebviewWindow::navigate(home.parse()?)`" + "Add a separate `#[cfg(not(windows))] fn termination_label(…)`". Smoke 4 pins it. | **COVERED** |
| P4 | §6 line 663: `watchdog.hang` "(Win native P1; **JS-ping P2**)" | arch-04, RT-02 | — | see R11 — the event has no Linux producer under any P2 spec | **UNOWNED** (= R11) |
| P5 | §6 line 671: `health.sample` "… webview RSS, `spool.dropped_expired` **(P2)**" | TEL | E / P1 | RSS half → E (see R10). `spool.dropped_expired` half → already delivered in P1: D2e "a periodic `health.sample` … `spool.dropped_expired`" is in D2e's shipped set (D2e defers only "webview-process RSS + `max_webview_mem_mb`"). | **COVERED** (RSS) / **ALREADY-P1** (`spool.dropped_expired`) |
| P6 | §7 preamble line 677: "Linux and Android columns land with their platform phases (**P2**, P3)" | §7 whole table | A/B/D/G | Per-row breakdown in §1d. | **PARTIAL** (see §1d) |
| P7 | §7 preamble line 678: "P2 only exposes the operator-supplied `inject_css`/`inject_js` knobs on top of it (RT-16)" | RT-16 | — | see R12 | **UNOWNED** (= R12) |
| P8 | §7 zoom row line 685: "needs a gesture-controller intercept in the platform layer / a wry patch, validated on touch hardware **in P2** (wry #544, PF-04)" | PF-04 | — | see R2 | **UNOWNED** (= R2) |
| P9 | §7 keep-awake row line 695: "PRIMARY is configuring cage/wlroots not to blank (idle-inhibit is secondary … **validated in P2**, PF-07)" | PF-07, M8 | B/G | see R3 | **PARTIAL** (= R3) |
| P10 | §10 line 883: "Linux compile check (**P0 → functional at P2**)" | §10 CI | F | F: "Every PR gets the Linux functional gate §10 promised at P2 — the real app under a real compositor with real signed config, in minutes" + "### 1. Per-PR job: `smoke-linux`". | **COVERED** |
| P11 | §11 line 894: "**P2** intercepts the GTK zoom gesture in the platform layer / upstreams a wry hook; validate on touch hardware" | PF-04 | — | see R2 | **UNOWNED** (= R2) |
| P12 | §11 line 895: "**P2** disables blanking at the compositor (cage/wlroots) as primary; confirm cage honours idle-inhibit before relying on it" | PF-07, M8, H5 | B/G | see R3. Note the second clause — "confirm cage honours idle-inhibit **before relying on it**" — is a precondition B relies on without confirming; G H3 asserts `systemd-inhibit --list` shows the hold and "display never blanks over 24 h", which tests the outcome but is scheduled *after* the design commits to it. | **PARTIAL** (= R3) |
| P13 | §11 line 900: "P1 nightly reload; **P2** adds memory-cap restart + health-sampled RSS; validated by scheduled soak (§10)" | §10 soak | E (feature) / **none** (the §10 Windows soak job) | E owns the feature (R9/R10). The **validating job** named by §10 is unowned — see O8. | **PARTIAL** (feature COVERED, validation UNOWNED — split into R9/R10 + O8) |
| P14 | §12/OD-1 line 923: "(b) relabel P1 'attended pilot' and defer some to P2" | OD-1 | n/a | Parent selected "**(a)**" — everything stays in P1. No P2 obligation is created. | **DEFERRED-BY-PARENT** (inverse: pulled *into* P1) |

### 1c. §7.2 Linux — the deployment-gate sub-items

Parent §7.2 Linux, verbatim: *"cage (Wayland) locked session as the supported secure config
(X11/openbox is documented but NOT app-enforced — demo only). Disable VT switching and zap:
logind `NAutoVTs=0`/`ReserveVT=0` (or X11 `DontVTSwitch`/`DontZap`); run on a dedicated seat
with no other TTYs; disable DPMS/screensaver in the cage session; mask sleep/suspend targets
(H5/PF-07/M8)."*

| # | Sub-item | Owner | Evidence | Status |
|---|---|---|---|---|
| L1 | cage locked session as supported secure config; X11/openbox demo-only, not app-enforced | G | G §2 bullet 1 (quoted at R6) | **COVERED** |
| L2 | Disable VT switching and zap: `NAutoVTs=0`/`ReserveVT=0` | G | G §2: "VT/console: `NAutoVTs=0`, `ReserveVT=0`, no getty on the kiosk seat, kernel `consoleblank=0`; the D spec's note lands here — chord *swallowing* is unnecessary under cage, VT switching is what actually needs killing, and it dies in logind, not in app code." Cross-referenced from D: "`should_swallow` … is deliberately **not** ported … VT-switch (`Ctrl-Alt-Fn`) is a kernel/console concern the P2-G image handles". | **COVERED** |
| L3 | "run on a dedicated seat with no other TTYs" | G | G §2: "no getty on the kiosk seat"; G §2 "Seat/session: the **service-user and seat-access decision is the runbook's one open fork** (below); both candidate recipes are written up, one gets promoted after hardware validation" + Open decisions: "root-service … vs dedicated `kiosk` user with logind seat semantics vs `seatd`. Both non-root recipes drafted in the runbook; hardware validation (H1) promotes one." | **COVERED** — an owned deferral with a named gate (H1), which the frame permits. |
| L4 | "disable DPMS/screensaver in the cage session" | G | G §2: "no screensaver/idle daemon installed"; "kernel `consoleblank=0`"; "`IdleAction=ignore`". | **COVERED** — but see R3: the *compositor-side* blanking config the parent calls PRIMARY is discharged only by absence-of-daemon, never by a named cage/wlroots setting. |
| L5 | "mask sleep/suspend targets" | G | G §2: "mask `sleep.target suspend.target hibernate.target hybrid-sleep.target`". | **COVERED** |
| L6 | §7.2 escape-vector sweep as a deployment gate (§10 "manual smoke checklist … incl. the escape vectors in §7.2") | G | G H6: "§7.2 escape-vector sweep under the locked session (the §10 hardening list: chords, edges, dialogs, VT attempts)". | **COVERED** |

### 1d. §7 hardening table — the **Linux column** of every row (a P2 obligation by §7's preamble)

| # | §7 row (Linux mechanism, verbatim) | ID | Owner | Evidence | Status |
|---|---|---|---|---|---|
| H-a | Context menu off — "WebKitGTK `context-menu` signal" | — | B | B table: "`connect_context_menu` → return `true` (suppress; `web_view.rs:2074`)" | **COVERED** |
| H-b | DevTools off — "per-webview settings; release builds compile without devtools feature" | — | B | B: "`set_enable_developer_extras(false)` explicitly (`settings.rs:1475`)" | **COVERED** |
| H-c | Zoom lock — "WebKitGTK fixed `zoom-level`" (base zoom) | PF-04 | B | B: "`WebViewExt::set_zoom_level` (`web_view.rs:1980`)" | **COVERED** |
| H-d | Zoom lock — "interactive pinch is GTK-owned and needs a gesture-controller intercept" | **PF-04** | **none** | see R2 | **UNOWNED** (= R2) |
| H-e | Text selection off — "injected `* { user-select: none }` with `input, textarea { user-select: text }` (config flag)" | — | P1 | `crates/kiosk-main/src/inject.rs:33-39` emits exactly that CSS; `build_injection` is pure and **platform-agnostic** (no `cfg` in the file; the only `#[cfg]` is `#[cfg(test)]`), and `main.rs:1046` calls `builder.initialization_script(inject::build_injection(…))` with no `cfg` gate — so it lands on wry/WebKitGTK for free. D2b: "**Ships in P1**". | **ALREADY-P1** — needs no Linux work. |
| H-f | Drag/drop off — "injected `dragstart`/`drop` preventDefault **+ platform drop-target disable**" | — | P1 (injected half) / **none** (native half) | Injected half: `inject.rs:42-44`, platform-agnostic (as H-e). The "**platform drop-target disable**" half has no implementation on either platform (grep `drop_target` over `crates/` → zero hits) and no P2 spec mentions it (grep `-iE "drag\|drop.?target"` over the seven specs → zero hits). | **PARTIAL** — injected half ALREADY-P1 and platform-agnostic; the native GTK drop-target disable named by the parent is unowned on Linux (and undelivered on Windows). |
| H-g | Cursor auto-hide — "injected JS: `cursor: none` after `cursor_autohide_seconds` idle" | — | P1 | `inject.rs:50-58` emits the timer; same platform-agnostic path as H-e. | **ALREADY-P1** — needs no Linux work. |
| H-h | Printing off (H1) — "inject at document-start `Object.defineProperty(window,"print",…)`" | H1 | P1 | `inject.rs:46-48` emits that exact line, platform-agnostic. | **ALREADY-P1** |
| H-i | Printing off (H1) — "**WebKitGTK `print` signal returns TRUE**" | **H1** | **none** | Grep `-iE "connect_print\|print signal\|\"print\""` over all seven specs → the only `print` hits in B are `eprintln` (lines 165, 194). B's `hardening.rs` control-mapping table has no printing row at all. Ctrl+P is in the §7.2 *Windows* swallow list only, and D explicitly declines to port swallowing ("`should_swallow` … is deliberately **not** ported"). So on Linux the only remaining print barrier is the P1 JS override; the native mechanism the parent names is unowned. | **UNOWNED** |
| H-j | Script dialogs (M3) — "WebKitGTK `script-dialog` returns TRUE with the same policy" | M3 | B | B: "`connect_script_dialog` → return `true` always (no dialog chrome exists to paint); `BeforeUnloadConfirm` → `confirm_set_confirmed(true)` (leave the page, matching Windows); same budget semantics mirrored" | **COVERED** |
| H-k | Autofill / saved data off (M5) — "WebKitGTK disable form persistence" | M5 | B | B: "autofill/password-save off | documented no-op — WebKitGTK ships no password manager/autofill store" | **COVERED** — a declared no-op with a stated reason; the divergence is documented per C3. |
| H-l | Web permissions default-deny (M9) — "WebKitGTK `permission-request` deny" | M9 | B | B: "`connect_permission_request` (`web_view.rs:2428`) → classify by the request's **runtime type** … everything else … → `Other` → deny. `request.allow()`/`deny()` + return `true`." Divergence stated: "clipboard read is always denied". | **COVERED** |
| H-m | Media autoplay — "WebView2/WebKitGTK: muted autoplay allowed by default" | arch-10 | E | E proves the path end-to-end: "The offline video loops for hours on WebKitGTK with zero silent failure modes"; no configuration is required by the parent on this engine. | **COVERED** |
| H-n | Shortcut blocking (Linux) — "compositor owns keys — cage session has none; VT switching … and Ctrl+Alt+Backspace are kernel/logind-level, closed via §7.2" | PF-03 | D + G | D: "`should_swallow` (`shortcuts.rs:66`) is deliberately **not** ported: under cage there is no desktop shell and no OS chord to swallow … VT-switch (`Ctrl-Alt-Fn`) is a kernel/console concern the P2-G image handles (documented divergence …)". G L2 above. | **COVERED** |
| H-o | Keep-awake (Linux/Wayland) | PF-07, M8 | B/G | see R3 | **PARTIAL** (= R3) |
| H-p | Text selection ActionMode (Android, M7) | M7 | — | Android row; parent §9 places Android at P3. | **DEFERRED-BY-PARENT (P3)** |
| H-q | Touch keyboard — "**Linux: squeekboard/onboard deployment docs**" | PF-02 (Windows twin) | G (partially) | D explicitly hands it off: "**Out:** … on-screen keyboard deployment (parent §7 table — P2-G)" and again in Scope/defer: "on-screen keyboard (squeekboard/onboard per parent §7) … → P2-G". G's only mention is a *validation* row, H4: "Touch: corner-tap gesture on real touch hardware; `GDK_TOUCH_CANCEL` behavior; **on-screen keyboard decision (squeekboard/onboard per §7 table) exercised and chosen**". | **PARTIAL** — G validates and *chooses*, but the parent's deliverable is **deployment docs**, and G's runbook component list (§2: cage, VT/console, sleep/idle, seat/session, boot cosmetics, updates, SSH) contains **no on-screen-keyboard section**. There is no packaged dependency either — G §1's `Dependencies:` line lists `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `cage`, and the four GStreamer packages; neither `squeekboard` nor `onboard` appears. What is missing: the documented deployment recipe (and, if the chosen answer is a package, its dependency line). |
| H-r | Downloads / popups / file pickers — "blocked; new windows navigate in place" | — | A + B | A: "`on_new_window` hands the URL back into the main webview and *then* denies … navigate-in-place is a requirement of the parent spec of record". B: "`on_download` denies every `DownloadEvent::Requested` → `false`, emitting `nav.blocked{download}` once". | **COVERED** |
| H-s | PDF (M4) — "default: navigations returning `application/pdf` are **blocked** (`nav.blocked`) … Confirm interceptors wired per platform (… WebKitGTK `download-started` …)" | **M4, OD-8** | **none** | B declines it explicitly: "PDF blocking is **not** wired on Windows (`scheme_guard::pdf_decision` is `#[allow(dead_code)]`, `scheme_guard.rs:36-40` — descoped ponytail with its reason on record), so B does not wire it on Linux either; wiring it on *both* platforms is a recorded future work item, not smuggled in here." Verified in code: `crates/kiosk-main/src/scheme_guard.rs` — `pdf_decision` and `REASON_PDF` both carry `#[allow(dead_code)]` and "not called from any COM callsite yet". | **UNOWNED** — see the resolution note in §2.10 below. |
| H-t | Egress containment (SEC-10) — "WebKitGTK `resource-load-started` — plus an injected restrictive CSP. … Residual gaps … documented" | **SEC-10** | B | B: "**Layer 1 — WebKit content filter (the enforcement boundary).** A pure, host-tested compiler turns the live allowlist into WebKit's declarative content-rules JSON …" + "**Layer 2 — CSP belt (the observability layer)** … `telem.nav_blocked("egress", blocked_uri)` — the same `REASON_EGRESS` label Windows emits". Residual gaps documented ("Path-scoped blocks … are enforced by Layer 1 only and therefore silent"; SW coverage "**pinned by smoke, not assumed**", scenario 8c). Closes the A-spec residual: "after B, a Linux device no longer has the 'do not field before P2-B' egress hole." | **COVERED** — note the parent names `resource-load-started`; B substitutes a content-filter + CSP pair and justifies it ("WebKitGTK has no request-level host API"). Mechanism divergence is stated, so the requirement is discharged. |

### 1e. Requirement IDs implicating the Linux platform, not already rowed above

| # | ID + parent text | Owner | Evidence | Status |
|---|---|---|---|---|
| I1 | **SEC-09** (§8, §4) — "Linux kernel keyring or `root:root 0600`"; "at boot AND on every config reload, if the credential lacks its required restrictive mode the device refuses to load it" | A (kiosk-main) + C (kiosk-launcher) + G (package) | A C12: "The stub returns `Ok(true)` unconditionally, so **both** SEC-09 gates — boot (`boot.rs:165`) and every config fetch (`fetch.rs:100`) — fail open on Linux … **P2-A is the commit that falsifies that sentence**, so P2-A owns the fix." C: "the launcher's own `credential_acl.rs` Unix implementation (the same fail-open stub kiosk-main's C12 fixed — one crate was fixed in A, this one is C's)". G: "postinst pre-creates the credential path `0600 root:root` empty so the mode exists before the secret does". | **COVERED** — all three crates/edges accounted for. |
| I2 | **arch-15** liveness disambiguation on Linux (channel fault vs hang vs exit) | C | C `pipe.rs`: "EOF/reset while the child lives → `Event::ChannelFault{at}`, re-accept + first frame → `Event::ChannelReconnected` — the mapping table is byte-for-byte the Windows one". Transport chosen expressly to preserve it: "there is no listener, so a faulted channel can never re-accept: `Event::ChannelReconnected` becomes unreachable". | **COVERED** — except sub-case (c) "no round-trip ping → genuine hang", which is R11. |
| I3 | **arch-05** exit-code-86 exemption end-to-end on Linux | C + G | C: "Technician exit end-to-end: pinpad exit → kiosk-main exits 86 → launcher FSM `ExitLauncher{86}` (E2, unchanged) → cage exits 86 → systemd stops restarting", with an empirical pin ("cage propagates its child's exit code exactly"). G H2: "`RestartPreventExitStatus=86` end-to-end via systemctl (technician exit stays exited)". | **COVERED** |
| I4 | **arch-01** spool partitioning / orphaned-spool scoop on Linux; **TEL-10** | C | C: "spawn, watch a heartbeat channel, restart per the FSM, **drain a dead main's spool**, exit 86"; "the E1 FSM, the actor loop, the sink, the spool drain, and the safe-mode chain are already portable". | **COVERED** |
| I5 | **arch-09 / media.error** — "wires the `<video>` element's `error`/`stalled`/`emptied` events … and emit a `media.error` log" | E | E: "the `media.error` IPC bridge (closing `offline.html:44-47`'s recorded gap)" + "A Tauri command … registered cross-platform". Declared as one of E's "**Two deliberate cross-platform changes**". | **COVERED** |
| I6 | **PF-05 / RT-05** — offline-video loop soak on the pinned Debian 12 image; ≥72 h hardware soak as pre-release gate | E + F + G | E soak protocol (scenario 18): "**in-session** ~2 h minimum … **scheduled CI** 8 h+ (wired by F, run in a `debian:12` container for target fidelity); **hardware** ≥72 h (G checklist, RT-05)". F §2(b): "**soak** — E's protocol at 8 h+, same container, RSS series retained as an artifact even on pass". G H5: "≥72 h offline-video soak, RSS trend, loop count; visual black-frame check". Contingency designed up front per §3.4/§11: "**seamless double-buffered loop**". | **COVERED** — chain of custody complete across three specs. |
| I7 | **RT-13** — "end-to-end watchdog test with a real (headless) kiosk-main" | C + F | C: "`tests/rt13.rs` + the `rt13-mock-main` bin are `#[cfg(windows)]`-gated today … C makes the transport-name construction a platform seam … and un-gates the test. Result: the real launcher, real sink, real transport, scriptable child — on every PR". F: "After C lands, `cargo test` already includes RT-13 — the supervise loop is a per-PR gate with zero F work." | **COVERED** — and upgraded from Windows-only to per-PR cross-platform. |
| I8 | **§10** "Soak/endurance (scheduled CI, not per-PR): **a Windows-runner job** drives looped navigation + a deliberately leaking page with accelerated thresholds; asserts bounded RSS, that a `max_webview_mem_mb` breach **fires a restart**, and that **nightly reload resets RSS**" | **none** | Grep `-iE "leak\|windows.runner"` over all seven specs → **zero hits** for either. F's `endurance` workflow is Linux-only by construction: "(a) **full matrix** … run in a **`debian:12` container**"; "(b) **soak** — E's protocol at 8 h+, **same container**". E's soak protocol is the offline-video loop, whose pass criterion is "zero `media.error` … RSS delta over the window under a declared bound" — it is not a leaking-page test and asserts no `max_webview_mem_mb` breach→restart. E's own host tests cover the "memory-cap latch (consecutive-sample semantics, 0-disables, never-86 code)" as a **pure function**, not end-to-end. F excludes Windows from `endurance` entirely (only §3 `release` touches Windows, for MSI assembly). | **UNOWNED** |
| I9 | **§10** "per-platform manual smoke checklist in `docs/testing.md`" | G (substance) | G §4's H1–H8 hardware checklist is the substantive Linux equivalent ("Checklist results append to the runbook per device class"). Verified: `docs/testing/` contains one file, `p1d2-signed-config-smoke.md`; there is no `docs/testing.md` and no Linux checklist. | **PARTIAL** — the content exists and is owned (G H1–H8), but it lives in `packaging/linux/lockdown.md` per G §2, not in the `docs/testing.md` the parent names, and no spec states the relocation. LOW severity, listed for completeness. |
| I10 | **§10** CI matrix "Android build (P3)" | — | Parent §9 places Android at P3. F: "Android CI rows (§10) → P3." | **DEFERRED-BY-PARENT (P3)** |
| I11 | **§9 P4** — auto-update, one-shot remote commands (reload/clear-cache/screenshot), staged/canary rollout, playlist rotation, display on/off schedule, Cloud Monitoring dashboard | — | Parent §9 P4 row. F confirms the boundary rather than claiming it: "Windows P1/P2 ships no auto-updater: update = install the new MSI. Linux matches … Recorded ponytails, deliberately not P2 scope: an apt repository, `unattended-upgrades` policy, delta/A-B updates." | **DEFERRED-BY-PARENT (P4)** |
| I12 | **§3.2 / OD-4 / PF-08 / H3 / arch-11 / M7 / RT-15** — Lock Task Mode, device-owner fail-closed, foreground service + `foregroundServiceType`, `BOOT_COMPLETED`, `FLAG_KEEP_SCREEN_ON`, `setMediaPlaybackRequiresUserGesture(false)`, `with_webview` JNI, ActionMode override, in-process watchdog + in-app crash-loop, APK + provisioning docs | — | Parent §9: "**P3** | Android | Tauri android target, Kotlin plugin (…)". G: "Android packaging (P3)". | **DEFERRED-BY-PARENT (P3)** — all Android rows excluded from the matrix per instruction 11. |
| I13 | **OD-9** native non-webview last-resort safe screen | — | Parent §12/OD-9: "**(a) now; (b) deferred**". | **DEFERRED-BY-PARENT** |
| I14 | **OD-2 / SEC-03** token-proxy credential architecture | — | Parent §8: "**Target architecture:** a token-proxy … **Interim (pilot/small fleets only):** a per-device service account". Not phased to P2. | not a P2 obligation |

---

## 2. The eleven items requiring a definitive resolution

**2.1 — Cross-platform webview-hang detection (JS ping), arch-04/RT-02, §3.1 "lands in P2": UNOWNED.**
Confirmed by anchored grep over all seven specs for `js.?ping`, `round.?trip`, `liveness`,
`unresponsive`, `webview.hang`, `watchdog\.hang`, `responsive`, `evaluate.*script` — one hit,
and it is A stating the gap without owning it: *"Windows reserves `Reload` for
`RENDER_PROCESS_UNRESPONSIVE`, which has no WebKitGTK analogue."* C ports the heartbeat client
as pure process liveness and says so; nothing gates the 5 s heartbeat on a webview echo. The
consequence is concrete: on Linux, `watchdog.hang` (parent §6) has **no producer**, and
arch-15's case (c) — *"child alive + healthy channel + no round-trip ping → genuine hang →
restart"* — is unreachable, so a wedged WebKitGTK renderer with a live process leaves the kiosk
frozen indefinitely with a green heartbeat. C's smoke 15 does not cover this: `SIGSTOP`
stops the whole process (heartbeats stop too), which is the *process* path, not the renderer path.

**2.2 — Pinch-gesture intercept on WebKitGTK, PF-04 / wry #544: UNOWNED.**
Zero hits for `pinch` in any of the seven specs. B's zoom coverage is `set_zoom_level`, which
parent §7 pre-emptively rules insufficient: *"WebKitGTK fixed `zoom-level` — note this fixes
only base zoom, interactive pinch is GTK-owned and needs a gesture-controller intercept in the
platform layer / a wry patch."* P2-D is the spec that owns GDK event interception
(`gdk::event::set_handler`) and would be its natural home — it is the one place in P2 that
already sees every GDK event before dispatch — but D's scope is explicitly *"Observation only —
this handler never swallows"*, and D never mentions zoom or pinch.

**2.3 — Keep-awake "at compositor", PF-07/M8: PARTIAL, with an inversion of the parent's
primary/secondary.** Parent §7: *"PRIMARY is configuring cage/wlroots not to blank (idle-inhibit
is secondary and only if wry exposes an inhibitor surface)"*; §11: *"P2 disables blanking at the
compositor (cage/wlroots) as primary."* B makes `systemd-inhibit --what=idle:sleep` the
mechanism and calls the image config *"the **suspenders**"* while the inhibitor is the belt —
the reverse assignment. G supplies `IdleAction=ignore`, masked sleep targets, `consoleblank=0`,
and "no screensaver/idle daemon installed", which is a *de facto* non-blanking configuration,
but no spec names a cage or wlroots setting, and neither spec addresses the parent's stated
limitation that `systemd-inhibit` covers suspend only. §11's precondition — *"confirm cage
honours idle-inhibit **before** relying on it"* — is satisfied only after the fact, at G's H3.

**2.4 — `inject_css`/`inject_js` operator knobs behind signed config, RT-16: UNOWNED.**
Zero hits for `inject_css`, `inject_js`, `RT-16`, or `SEC-01` across all seven specs. The
underlying engine is present and platform-agnostic (`crates/kiosk-main/src/inject.rs`;
`main.rs:1046` calls it with no `cfg`), exactly as §7's preamble describes, so the remaining
work is only the knob exposure the parent assigns to P2 — and nobody has it.
`crates/kiosk-core/src/config/validate.rs:16-17` still marks both fields `"P2"`-unimplemented,
so today a signed config setting them emits an RT-08 `config.warn` and does nothing.

**2.5 — Remote log level: UNOWNED.**
The phrase appears exactly once in the entire spec corpus — parent §9's P2 row. Zero hits in the
seven specs. Zero implementation: repo-wide grep for `LogLevel|log_level|severity_filter|
min_level` returns nothing, so `logging.level` is parsed and validated
(`validate.rs:11` `VALID_LOG_LEVELS`) and then discarded. Worse than the other unowned rows in
one respect: it is *not* in the RT-08 `UNIMPLEMENTED` table, so unlike `inject_*` and
`restart_app` it fails silently rather than warning — the exact "silent no-op" class RT-08 exists
to eliminate.

**2.6 — `restart_app` remote command: UNOWNED.**
Zero hits in the seven specs. `crates/kiosk-core/src/config/schema.rs:230` declares
`pub restart_app: Option<String>`; `validate.rs:20` carries `("maintenance.restart_app", "P2")`;
`crates/kiosk-main/src/maintenance.rs` implements the nightly reload only and says so in its
module doc. P2-E is adjacent (it owns a self-restart exit code for the memory cap and even
coordinates the code value with C's signal-mapping table) but never claims `restart_app`.

**2.7 — Linux on-screen keyboard (squeekboard/onboard): PARTIAL.**
D hands it to G twice, explicitly. G receives it only as hardware-validation row H4
("*on-screen keyboard decision (squeekboard/onboard per §7 table) exercised and chosen*"), which
covers *choosing* but not the parent's deliverable — *"Linux: squeekboard/onboard **deployment
docs**"*. G's runbook component list has no keyboard section and G's `.deb` `Dependencies:` line
does not include either package. The gap is the recipe, not the decision.

**2.8 — Printing off on WebKitGTK (`print` signal returns TRUE): UNOWNED.**
The JS half of the parent's H1 row is ALREADY-P1 and lands on Linux for free
(`inject.rs:46-48`, no `cfg`). The native half — the parent's *"WebKitGTK `print` signal returns
TRUE"* — appears in no P2 spec; B's `hardening.rs` mapping table has no printing row. Note the
parent's stated rationale for having two entry points: *"both entry points are removed"*. On
Linux the second entry point is the WebKit signal, and D has separately declined to port
accelerator swallowing, so nothing covers a print reaching the engine by a non-`window.print()`
route. Residual risk is low in a cage session with no shell, but the named mechanism is unowned.

**2.9 — Text selection off / drag-drop off / cursor auto-hide: mostly ALREADY-P1 and
platform-agnostic; one half unowned.**
Verified in code, not inferred: `crates/kiosk-main/src/inject.rs` contains no `#[cfg]` other than
`#[cfg(test)]`, and `crates/kiosk-main/src/main.rs:1046` calls
`builder.initialization_script(inject::build_injection(…))` unconditionally — wry implements
`initialization_script` on WebKitGTK, so all three controls ship on Linux with **no P2 work**:
- text selection off (`inject.rs:33-39`) — **ALREADY-P1**
- cursor auto-hide (`inject.rs:50-58`) — **ALREADY-P1**
- drag/drop: the injected `dragstart`/`drop` preventDefault (`inject.rs:42-44`) — **ALREADY-P1**;
  the parent's second clause, *"+ platform drop-target disable"*, has no implementation on either
  platform (`grep drop_target crates/` → nothing) and no P2 spec mentions it → **PARTIAL**
  (counted once, at H-f).
One caveat that no spec states: `inject.rs`'s module doc records that `initialization_script`
"may be called only ONCE per webview". P2-B independently adopts `UserContentManager::add_script`
for its CSP belt *because* of that contract (B: *"`initialization_script` (single-caller
contract, `nav_policy.rs:146-150`)"*), so the two do not collide. No coverage defect; recorded
because a reader could reasonably suspect one.

**2.10 — PDF policy (M4/OD-8): the parent's requirement is undischarged fleet-wide, and P2-B's
parity argument is internally honest but leaves the row unowned.**
The parent is unambiguous that blocking is the shipped default: §7 *"default: navigations
returning `application/pdf` are **blocked** (`nav.blocked`)"*; §12/OD-8 *"**block by default,
`content.pdf_view=true` opt-in** (applied)"*; §9's P1 row lists the *"hardening set (§7)"*
as P1 content. Code confirms it was descoped, not delivered: `scheme_guard::pdf_decision` and
`REASON_PDF` both carry `#[allow(dead_code)]` with "not called from any COM callsite yet". So:
- B's factual claim is **correct** — PDF is unwired on Windows, and B's refusal to wire Linux
  only is consistent with C3 (parity with what Windows *actually* enforces).
- But the parent's requirement is **not** satisfied by symmetric non-delivery. §7's Linux column
  is a P2 obligation, and the parent nowhere defers PDF past P2. B's disposal — *"wiring it on
  *both* platforms is a recorded future work item"* — names no owner and no phase, which is
  precisely the unowned-deferral shape.
- Compounding it: `validate.rs:18` marks `content.pdf_view` unimplemented at phase `"P1"`, and
  the RT-08 warning fires only when the field is set **away from default** — i.e. an operator who
  leaves the default (`false` = "block") gets no warning and no blocking. Silent, not loud.
**Verdict: UNOWNED** (recorded at H-s). Not "consistent with the parent" — the parent requires
blocking by default and no P2 spec delivers it on either platform.

**2.11 — Android-only items: DEFERRED-BY-PARENT (P3), excluded from the defect count.**
Parent §9 P3 row assigns the whole Android leg. Rows I10 and I12 above carry the quotes; §7's
Android column cells (`setSupportZoom(false)`, `setOnLongClickListener`, `setSaveFormData(false)`,
`onPermissionRequest`, `setMediaPlaybackRequiresUserGesture(false)`, ActionMode override M7,
`setDownloadListener`, `shouldInterceptRequest`, Lock Task) and §7.2's Android row are all
excluded on the same basis.

---

## 3. Scenario-number registry

Numeric smoke-scenario namespace (A–E) is contiguous 1–18 with **no collisions and no gaps**.
G uses a disjoint `H1`–`H8` hardware namespace. F allocates no numbers of its own — it only
selects from the existing set.

| # | Owner | One-line description |
|---|---|---|
| 1 | A | Boot → splash → remote home `nav.committed` (local httpd allowlisted) |
| 2 | A | Off-list navigation → exactly one `nav.blocked`; `target=_blank` navigates in place |
| 3 | A | Config/network down → offline fallback page loads from app origin |
| 4 | A | Kill `WebKitWebProcess` → `webview.crash` spooled + recovery navigate-home |
| 5 | A | iframe: in-allowlist loads, off-allowlist blocked once, no `NavigationFailed` (pins main-frame scope of `load-changed`/`load-failed`) |
| 6 | A | Profile clear via harness binary → cookie gone + `ProfileCleared` (superseded as app-path proof by 16) |
| 7 | A | Safe boot from malformed `kiosk.ini` → `safe.html` from app origin, `device_id` from `/etc/machine-id` |
| 8 | B | Egress: off-list `<img>`/`fetch` blocked + `nav.blocked{egress}`; path-scoped silent block; service-worker fetch (pins SW coverage) |
| 9 | B | Downloads: `Content-Disposition: attachment` → no file, exactly one `nav.blocked{download}` |
| 10 | B | Dialog/chrome: `alert()` loop doesn't wedge; no context menu; `beforeunload` doesn't prompt |
| 11 | B | Permissions: geolocation + `getUserMedia` denied; allowed when fixture flips `permissions.camera` |
| 12 | B | Keep-awake **degrade path only** (no systemd in container); positive assertion deferred to G H3 |
| 13 | C | Full chain: `cage -- kiosk-launcher` headless → `kill -9` main → launcher restarts within FSM window |
| 14 | C | Technician exit: pinpad → launcher process exits 86 (systemd half at G H2) |
| 15 | C | Hang path: `SIGSTOP` main past heartbeat-miss window → kill/restart, corpse reaped, no zombie |
| 16 | D | Idle → clear: `IdleExpired` → profile clear → `ProfileCleared` → cookie gone; latch asserted |
| 17 | D | Gesture + chord + activity-reset (conditional on cage-headless virtual input; else G H4) |
| 18 | E | Offline-video soak: 2 h in-session / 8 h+ CI / ≥72 h hardware; zero `media.error`, bounded RSS delta |

**Collisions:** none. **Numeric gaps:** none (1–18 contiguous).

**Registry observations (not collisions):**
- **F's per-PR fast subset** (F §1) names A 1–3, 5, 7; B 8–11; D 16. It explicitly excludes A 4,
  C 13–15, E 18. **Scenarios 12 and 17 are named in neither F job list explicitly** — they are
  swept up only by F §2(a)'s blanket *"all A–D scenarios"*. Both are the conditional/degrade-only
  scenarios (12 needs systemd, 17 needs virtual input), so both may be unrunnable in CI; neither
  spec says which side of the line they land on. Minor registry ambiguity, no collision.
- **F §2(a) says "all A–D scenarios"**, which by construction excludes E's 18; F §2(b) covers 18
  separately. Correct, but the phrasing means no single F sentence covers the full 1–18 set.
- **Two scenarios are conditionally demotable by their own specs** — 17 ("if it does not,
  scenario 17 moves to the deferred hardware list … recorded, not silently dropped") and 12
  (degrade-only by construction). Both have named G checklist heirs (H4, H3), so the deferral is
  owned in both cases.
- **G's H1–H8** is a separate namespace and every row cites its origin spec; no numeric overlap
  with 1–18.

---

## 4. Counts

- **UNOWNED: 8** — R2/H-d, R11, R12, R13, R14, H-i, H-s, I8.
- **PARTIAL: 6** — R1 (roll-up), R3/H-o, R9, H-f, H-q, I9.
- COVERED: 30. ALREADY-P1: 4. DEFERRED-BY-PARENT: 5.
