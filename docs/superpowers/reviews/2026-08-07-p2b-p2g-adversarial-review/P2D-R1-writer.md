# P2-D — WRITER, Round 1 (opening)

No frame dispute.

**Headline:** the verification record kills D's central mechanism. `gdk::Event::set_handler`
is **withdrawn** (D1). It is replaced by GTK signal observation on the two objects Tauri
already hands us. Every other change is rebuilt on top of that, so read D1 first.

---

## Change register

| ID | Change | Requirement discharged | Depends on |
|---|---|---|---|
| **D1** | **WITHDRAWN + REPLACED.** Observation via `WidgetExt` signal handlers on `gtk::ApplicationWindow` (keys) and `webkit2gtk::WebView` (button/touch/motion/scroll), always returning `Propagation::Proceed`. Not `gdk::Event::set_handler`. | parent §3.5 (idle reset, exit gesture); C2 shipped-API-first | P2-A (`with_webview` route + GTK-main-thread rule, A:60-77); D10 |
| **D2** | Recorded rejections, revised: evdev **upheld**; `ext-idle-notify-v1` **upheld as parked fallback**; **new**: `gdk_event_handler_set` rejected on evidence (§5b objdump, reproduced below). | FRAME §4.4 evidenced; Q4 blast radius | D1 |
| **D3** | **Two independent exit legs, not one fan-out.** Taps ride the WebView widget; chord rides the GtkWindow. Separate objects, separate signals, separate install sites, separate failure. Plus a named leg-3 owner. | parent §3.5:318-320 ("never unexitable") — the rule D cited wrong | D1, D7, D11; **P2-G** (leg 3) |
| **D4** | Activity clock: module-local `static LAST_INPUT_MS: AtomicU64` + `OnceLock<Instant>` base in `idle.rs`, stamped by D1's handlers. `idle::run` signature and `main.rs:917` **untouched**. `idle_secs_from_ticks` **untouched** (no cfg added). | parent §3.5 idle reset; C8 (Windows green) | D1 |
| **D5** | Gesture routing: `EventButton::position()`/`EventTouch::position()` + `allocated_width/height()` → existing `in_region`/`TapCounter`, with the Windows `inside_window` bounds check replicated. `TAP_WINDOW_MS` loses `#[cfg(windows)]` (one-line diff, no second constant). | parent §5.2; C1 reuse; C3 parity | D1 |
| **D6** | keyval→VK: a two-arm match (`gdk::keys::constants::{K,k}` → `VK_K`), modifiers from `ModifierType`. `is_technician_chord` gains `&& !mods.win` (no-op on Windows, fixes the Super-held divergence at the shared root). | parent §3.5 chord; C1; C3 | D1 |
| **D7** | `gesture.rs:194` / `shortcuts.rs:113` stubs → documented delegations to `input_watch`. | C1 layering; A's `scheme_guard` precedent | D1 |
| **D8** | `should_swallow` still not ported — justification rewritten to cover the webview-chrome half of its list and to state the divergence in both directions. | C3 honest parity | D7; P2-B (context menu) |
| **D9** | Clear-gate chain completes app-path (`Online + idle_clear → ClearProfile → ProfileCleared`). Unchanged from draft. | parent §3.5; A smoke 6 successor | D4 |
| **D10** | **New C6 decision (was undeclared):** `[target.'cfg(target_os = "linux")'.dependencies] gtk = "0.18"`. **One** crate — `gdk` arrives via `gtk::gdk` (`gtk-0.18.2/src/lib.rs:18`). | C6 | — |
| **D11** | Real error model: `gtk_window()` and `with_webview` are `Result`; degrade per C4 with the repo's existing eprintln+telemetry shape. Panic containment is structural (no borrow held across an outward call). | C4; FRAME Q3 | D1, D3 |
| **D12** | Smoke 16–17, plus one added assertion in 17: the page still receives clicks/keys (regression guard on the Proceed rule). | C9 | D1 |
| **D13** | **Coverage flag, not a change:** PF-04 pinch intercept. D's revised mechanism is its natural host (`gtk::GestureZoom` + sequence-claim on the WebView widget). Named, unresolved, owner proposed. | parent §7 zoom-lock row / PF-04 | D1 |

---

## D1 — Observation mechanism: GTK widget signals, not GDK dispatch replacement

**Proposal.** Install, inside the Tauri setup closure:

- On `WebviewWindow::gtk_window()? : gtk::ApplicationWindow` —
  `connect_key_press_event` / `connect_key_release_event`
  (`gtk-0.18.2/src/auto/widget.rs:3035,3068`, `F: Fn(&Self, &gdk::EventKey) -> glib::Propagation`).
- Inside `with_webview(|pw| …)` on `pw.inner() : webkit2gtk::WebView` —
  `add_events(BUTTON_PRESS_MASK|TOUCH_MASK|POINTER_MOTION_MASK|SCROLL_MASK)` then
  `connect_button_press_event` (`widget.rs:2015`, `&gdk::EventButton`),
  `connect_touch_event` (`:3899`, `&gdk::Event` → `downcast_ref::<EventTouch>()`),
  `connect_motion_notify_event` (`:3224`), `connect_scroll_event` (`:3549`).
- **Every handler returns `glib::Propagation::Proceed`, unconditionally.** Observation only.

**Requirement discharged.** parent §3.5 (idle reset source; exit-gesture tap capture; chord).

**Evidence.**

- *Tier 3/4, the withdrawal trigger — reproduced by me.* GTK owns the GDK handler slot:
  ```
  $ objdump -R /usr/lib/x86_64-linux-gnu/libgtk-3.so.0 | grep event_handler_set
  00000000007c4788 R_X86_64_JUMP_SLOT  gdk_event_handler_set@Base
  $ nm -D … | grep event_handler_set →  U gdk_event_handler_set
  ```
  Exactly **one** call site in the whole library (`grep -c gdk_event_handler_set@plt` over the
  full `objdump -d` → 1 call, at `1fdbd8`), in a static function in GTK's init path (nearest
  preceding exported symbol `gtk_true@@Base`; surrounding calls `g_module_close`,
  `gdk__private__`, then `g_getenv` — `do_post_parse_initialization` in `gtk/gtkmain.c`):
  ```
  1fdbcd: lea 0x591c(%rip),%rdi   # 2034f0 <gtk_main_do_event@@Base>
  1fdbd4: xor %edx,%edx           # destroy = NULL
  1fdbd6: xor %esi,%esi           # data    = NULL
  1fdbd8: call 91df0 <gdk_event_handler_set@plt>
  ```
  Environment is Ubuntu 24.04 / libgtk-3.so.0.2409.32, **not** the C7 floor — see Residual.
- *Tier 4, precedent for the replacement.* wry already does exactly the pointer half:
  `wry-0.55.1/src/webkitgtk/synthetic_mouse_events.rs:10` `webview.add_events(…)`,
  `:15` `webview.connect_button_press_event(…)` — on the **WebView widget**, and it works with
  WebKit consuming events, because `g_signal_connect` user handlers run before the widget
  class's default handler. tao already does the key half:
  `tao-0.35.3/src/platform_impl/linux/event_loop.rs:865,872`
  `window.connect_key_press_event/connect_key_release_event` on the **GtkWindow**, returning
  `glib::Propagation::Proceed` — that is tao's entire Linux keyboard input, with a focused
  WebKitWebView present. Two proven in-tree patterns, one per event family.
- *Tier 4, the API surface.* `WebviewWindow::gtk_window() -> crate::Result<gtk::ApplicationWindow>`
  (`tauri-2.11.5/src/webview/webview_window.rs:1861`, linux-cfg'd);
  `PlatformWebview::inner() -> webkit2gtk::WebView` (`tauri-2.11.5/src/webview/mod.rs:173`);
  `WebView` is `@extends gtk::Container, gtk::Widget` (`webkit2gtk-2.0.2/src/auto/web_view.rs:58`),
  so `WidgetExt` applies.

**Why this and not the GDK handler** (the four things the record forces me to answer):

1. *What GTK's handler does that I would have to replicate.* Nothing beyond a single call:
   the installed handler **is** `gtk_main_do_event`, with `data = NULL` and `destroy = NULL`
   (registers above). So D's "forward exactly once, unconditionally, before classification"
   was, on its own terms, a faithful replication — the draft was right about that and the
   verifier's §5b does not show otherwise. The rule preserves GTK's semantics **if and only
   if the event object is identical**, and it is not: `gdk-0.18.2/src/event.rs:65`
   `from_glib_none(event)` → `glib-0.18.5/src/boxed.rs:482-485` → `gdk_event_copy`. GTK
   dispatches a copy. Whether GTK3's `GdkEventPrivate` state survives `gdk_event_copy` for
   `gtk_main_do_event`'s purposes is a GTK3 C-internals question with **no GTK3 sources in
   this environment** — genuinely UNVERIFIABLE, and it sits on the only path by which the
   product receives input.
2. *What breaks if I forward naively.* Nothing forwards naively — but the **failure mode** is
   the disqualifier, not the mechanism. `Event::set_handler` REPLACES dispatch. Any defect in
   our handler — a panic across the `extern "C"` trampoline (`event.rs:59`, UB/abort in
   Rust 2021), a `RefCell` reentry, a missed forward — takes **all input to the webview** with
   it. The kiosk becomes a black, unclickable, un-exitable pane. That is precisely the state
   parent §3.5:319-320 forbids, produced by our own code. Under D1-revised the same defect
   costs at most our own feature: we never sit on the dispatch path, we always return
   `Proceed`, and the webview's input is untouched by construction. FRAME Q4 (blast radius)
   and the §3.5 safety rule point the same direction, hard.
3. *Cost paid.* Five `connect_*` calls instead of one `set_handler`. In exchange: zero
   `unsafe`, zero per-event `gdk_event_copy`/`gdk_event_free` on the main thread (including
   every `MotionNotify`), zero process-global slot, zero reentrancy analysis, zero GTK-dispatch
   re-implementation, one new dep instead of two, and one UNVERIFIABLE retired. On FRAME Q2
   ("fewest moving parts that meets the requirement") the count of `connect_` calls is the
   wrong unit; total mechanism is smaller here.
4. *Typed accessors arrive for free*, which retires a separate FALSE (see §Response, item 12):
   `EventButton::position() -> (f64,f64)` (`gdk-0.18.2/src/event_button.rs:19`),
   `EventTouch::position()` (`event_touch.rs:21`), `EventKey::keyval() -> keys::Key`
   (`event_key.rs:23`), `state() -> ModifierType` (`:18`). All non-`Option`, no downcast except
   touch.

**Dependencies.** P2-A's `with_webview` route and its GTK-main-thread rule (A:60-77) —
binding precedent, reused verbatim. D10 (`gtk` dep). D3/D11 build on the install shape.

**Residual risk / pinning.** (a) The objdump is Ubuntu 24.04, not the Debian 12 floor — but
the finding it produced is *why I withdrew*, so a floor mismatch cannot resurrect the
withdrawn design; the replacement does not depend on it. (b) Signal-handler ordering vs
tao's own handlers is irrelevant (both `Proceed`). (c) Panics in glib closures still cross
FFI — contained structurally, see D11.

---

## D3 — Two independent exit legs, and the named leg 3

**Proposal.**

- Leg 1 (taps) is installed on the `webkit2gtk::WebView`, inside `with_webview`.
- Leg 2 (chord) is installed on the `gtk::ApplicationWindow`, outside it.
- Neither install can fail the other: separate `Result`s, separate log lines, separate
  telemetry events. If one fails to install, that is logged loudly and the other still exits
  the device. This is the property parent §3.5:318-320 actually asks for, and D-as-drafted
  did not have it (one handler, one failure, both legs gone).
- **Delete** from §Error handling: *"cfg-12's no-fail-open semantics: nothing exits, nothing
  unlocks."* That sentence asserts the forbidden state. Replaced by D11.
- **Leg 3** (the §7.2-OS-lockdown equivalent) is **not** in-process and is **not** D's to
  build. Parent §7 (`:693`) already fixes its Linux shape: *"Linux: compositor owns keys —
  cage session has none; VT switching (Ctrl+Alt+F1–F7) and Ctrl+Alt+Backspace are
  kernel/logind-level, closed via §7.2."* D therefore states a **constraint on P2-G**: the
  hardened image must retain exactly one administrative route to `systemctl stop` the kiosk
  unit (a reserved maintenance path), and that route is a P2-G hardware-checklist row. D
  declares the dependency; P2-G owns the deliverable. If P2-G refuses it, §3.5 is unsatisfied
  on Linux and that is a HIGH integration defect, recorded here rather than discovered in the
  field.

**Evidence.** parent `2026-07-05-kiosk-browser-design.md:318-320` (verbatim in the verifier's
§9.1, re-read by me); `gesture.rs:12-15` = the §3.5 fallback rule; `gesture.rs:17-23` = cfg-12,
a different rule. Both re-read at source.

**Dependencies.** P2-G (leg 3), P2-C (launcher is the process that observes exit 86).

---

## D4 — `idle.rs` Linux body, with no signature change and no ordering constraint

**Proposal.** In `idle.rs`, under `#[cfg(not(windows))]`:

```rust
static BASE: OnceLock<Instant> = OnceLock::new();
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);
pub fn note_activity() { LAST_INPUT_MS.store(ms_since_base(), Relaxed); }
```

`run`'s Linux arm keeps the identical 1 s poll + `should_fire` latch, computing
`idle_secs = (now_ms − max(loop_start_ms, LAST_INPUT_MS)) / 1000`.

- `idle::run`'s signature is **unchanged**. `main.rs:917` — a single, non-cfg-gated
  `tokio::spawn(idle::run(idle_reset_seconds, tx.clone(), cancel.clone()))` — is **untouched**.
- The ordering hazard the verifier named (§11.8: `:917` spawns before the setup closure at
  `:1105`) disappears: there is no handle to plumb, and `max(loop_start_ms, …)` means an
  unstamped clock reads as "idle since the loop started", not "idle forever".
- Cross-thread correctness: the writer is the GTK main thread, the reader is a tokio worker.
  `AtomicU64`/`Relaxed`, **not** `Arc<RefCell<…>>` as the draft's prose mixed.
- `idle_secs_from_ticks` (`idle.rs:44`) is **not touched, not cfg-gated, not moved**. Its
  host test (`idle.rs:129-139`) keeps running on Linux CI. Zero diff.
- SEC-09 never-cancelled property (`idle.rs:16-24`): unchanged, same `cancel` argument.

**Requirement.** parent §3.5 idle reset; C8.
**Dependencies.** D1 (the stamping callers).
**Declared divergence (C3, looser than Windows).** Windows measures *system-wide* last input
(`GetLastInputInfo`); Linux measures *our-window* last input. See §Response item 6 for the
assumption and its pin.

---

## D5 — Gesture routing

**Proposal.** In the button/touch handlers:

```rust
let (x, y) = ev.position();                       // widget-relative
let (w, h) = (wv.allocated_width() as f64, wv.allocated_height() as f64);
let inside = x >= 0.0 && y >= 0.0 && x < w && y < h;   // parity with gesture.rs:254
if inside && in_region(x, y, w, h, g.region) { … counter.tap(now_ms()) … }
```

- `TAP_WINDOW_MS` **loses** its `#[cfg(windows)]` (`gesture.rs:181-182`). One-line deletion.
  Same 3000 ms on both platforms — no second constant, no silent parity divergence (C3).
- The `inside_window` guard is replicated deliberately: `in_region` performs **no** bounds
  check (`gesture.rs:34-43`; `TopLeft` is true for negative coordinates), and the Windows
  caller supplies its own at `gesture.rs:254`. Verified at source.
- Borrow discipline: the `RefCell<TapCounter>` borrow is scoped to produce a `fired: bool`,
  and **dropped before** `open_pin_pad` is called. Reentry through a nested GTK main loop
  therefore cannot hit a live borrow. Structural, not a rule to remember.

**Evidence.** `gdk-0.18.2/src/event_button.rs:19`, `event_touch.rs:21`,
`gtk-0.18.2/src/auto/widget.rs:473,494`, `crates/kiosk-main/src/gesture.rs:249-256,181-182,34-43`.
All read by me.
**Requirement.** parent §5.2. **Dependencies.** D1.

---

## D6 — keyval→VK and the chord

**Proposal.**

```rust
use gtk::gdk::keys::constants as k;
let vk = match ev.keyval() { k::K | k::k => VK_K, _ => return Proceed };
let m = ev.state();
let mods = Modifiers {
    ctrl:  m.contains(CONTROL_MASK),
    alt:   m.contains(MOD1_MASK),
    shift: m.contains(SHIFT_MASK),
    win:   m.intersects(MOD4_MASK | SUPER_MASK),
};
if is_technician_chord(vk, mods) { open_pin_pad(&app, gesture.as_ref()); }
```

Plus a **one-line change to the shared pure function**: `is_technician_chord` gains
`&& !mods.win`.

**Why at the root.** Verifier §9.4 is correct: `is_technician_chord` (`shortcuts.rs:99-101`)
does not require `!mods.win`; on Windows `should_swallow` returns `true` for any `mods.win`
(`shortcuts.rs:71-73`) and Vector A checks swallow first (`:184`), so Ctrl+Alt+Shift+**Win**+K
never reaches the chord. Adding the guard to the shared function is a **no-op on Windows** and
restores the stated invariant on Linux, in one line, where all callers route through — rather
than a Linux-side special case. Cost: D2c's host tests for `is_technician_chord` gain one case;
declared, not hidden.

**Also settled.** The draft's open decision *"the exact keyval set … enumerate, don't wildcard"*
is a **one-key** set: `shortcuts.rs:58` `const VK_K: u32 = 0x4B;` is the only chord constant,
and `gdk::keys::constants::{K, k}` exist at `gdk-0.18.2/src/keys.rs:886,952` with
`Key: Deref<Target = u32>` (`:9,11`). Both cases matched because Shift is held. Removed from
open decisions.

**Requirement.** parent §3.5 chord; C1; C3. **Dependencies.** D1, D7.

---

## D7 / D8 / D9 / D12 — unchanged in substance

- **D7** stubs → delegations. Citations corrected: `gesture.rs:193-194`, `shortcuts.rs:112-113`
  (`#[cfg(not(windows))]` on `:193`/`:112`, `pub fn install(` on `:194`/`:113`).
- **D8** `should_swallow` still not ported. Justification **rewritten** — see §Response item 9.
- **D9** unchanged; verifier confirmed the FSM chain (`kiosk-core/src/app/state.rs:296-304`)
  and A's own hand-forward (A:303-305 names P2-D as successor).
- **D12** smoke 16 unchanged. Smoke 17 gains one assertion: **the page still receives a click
  and a keystroke** after install. That is the one way D1-revised can hurt the product, and it
  costs one line in the scenario.

---

## D13 — Coverage flag: PF-04 pinch intercept

Not discharged by this revision, and I am naming it rather than letting it fall between B and D.
Parent §7 zoom-lock row / frame §2 assign P2 the GTK zoom-gesture intercept. `gdk::EventType`
carries `TouchpadPinch` (`gdk-0.18.2/src/auto/enums.rs:1621`), and `gtk::GestureZoom` is bound
(`gtk-0.18.2/src/auto/gesture_zoom.rs`) with sequence-claiming, which is the intercept
mechanism — on the same WebView widget D1 already attaches to. **It is the one place D1's
"always `Proceed`" rule would have to be relaxed**, so it must be designed, not assumed.
Proposed owner: D at plan time, with P2-B's `set_zoom_level` (`web_view.rs:1980`) as the
level-lock half. Flagged for the Moderator as a coverage question, not asserted as covered.

---

## Response to the verification record

### FALSE (7 flagged)

| # | Finding | Disposition |
|---|---|---|
| 1 | `gesture.rs:17-23` cited as "never unexitable"; it is cfg-12, and the §3.5 rule is `:12-15` | **CONCEDE.** Re-read both. Corrected in D3. The inversion in §Error handling is deleted, not softened. |
| 2 | `idle_secs_from_ticks` "stays `#[cfg(windows)]`" — it has no cfg (`idle.rs:44`) and its test (`:129-139`) is unconditional | **CONCEDE**, fully. Verified at source. Adding the cfg would break Linux CI (C8/C9). Revision: the function is **not touched at all** (D4). Zero diff is the correct diff. |
| 3 | `gdk::event::set_handler` — wrong path; `mod event` is private, it is `gdk::Event::set_handler` | **CONCEDE.** Moot: mechanism withdrawn (D1). |
| 4 | "`set_handler` cannot fail" — `event.rs:58` `assert_initialized_main_thread!` → panic (`rt.rs:16-25`), and `gtk::main_do_event` re-asserts per event | **CONCEDE.** Reproduced both at source. The draft's whole Error-handling section rested on it. Real error model in D11. |
| 5 | "The slot is free" | **CONCEDE**, and it is the reason for the withdrawal. Reproduced the objdump myself (D1 evidence). Honest statement: *no first-party Rust crate competes for the slot — GTK owns it, and taking it means owning GTK's dispatch.* |
| 6 | Usable "via tao/wry" without a direct dep — false; no re-export exists | **CONCEDE.** Declared as D10. Correction to the verifier's remedy: **one** crate, not two — `gtk-0.18.2/src/lib.rs:18` is `pub use gdk;`, so `gtk::gdk` covers `EventButton`/`ModifierType`/`keys`. `glib` likewise (`:21`), and `webkit2gtk` is already a direct dep per A:63. |
| 7 | Windows is "two global hooks" — it is three vectors, and the chord rides `AcceleratorKeyPressed` (`shortcuts.rs:186`), not `WH_KEYBOARD_LL` | **CONCEDE on the fact; PARTIAL REBUT on the conclusion.** The mis-attribution is real and the Architecture opener is rewritten. But the verifier's inference — that D's parity claim collapses — does not follow: under D1-revised the Linux chord rides an **in-widget signal on the focused webview**, which is the structural analogue of Vector A, the one Windows path #13919 does *not* starve. Parity is closer after the correction, not looser. Tier-3 evidence: `shortcuts.rs:186` (sole `is_technician_chord` call site) vs `shortcuts.rs:224-232` (`kb_hook` calls only `should_swallow`). |

### DRIFT (4) — all conceded, LOW

`idle.rs:66-75` → `:85` (call) / `:95-110` (loop). `gesture.rs:184-291` → `:204-327`.
`shortcuts.rs:103-208` → `:208-256` (the cited range is Vector A). `should_swallow` "stays
Windows-only" → true of its *callers*, false of the function. All four corrected verbatim.

### UNVERIFIABLE (4)

| Item | Disposition |
|---|---|
| `gdk_event_copy` fidelity through `gtk_main_do_event` | **ELIMINATED**, not pinned — D1-revised never copies an event and never dispatches one. |
| `GDK_TOUCH_CANCEL` distinct emission on Wayland | **DECLARED**, stays an open decision. Pin: smoke 17 / hardware checklist. Residual: a cancelled touch counts as a tap → a false tap toward N; bounded by `TAP_WINDOW_MS`, and it can only make the *exit* easier, never the lock weaker. |
| `cage` / `wlrctl` not installed | **DECLARED** with the existing pinning mechanism (plan-time check → smoke 17 or hardware list). P2-G `:94` row H4 already owns the fallback — verifier confirmed. Unchanged. |
| Environment ≠ C7 floor (Ubuntu 24.04 vs Debian 12) | **DECLARED.** The floor mismatch only affects evidence for the *withdrawn* design (D1 evidence note (a)); the replacement rests on tier-4 vendored-source citations, which are floor-independent. |

### Undeclared assumptions (9)

| # | Assumption | Disposition |
|---|---|---|
| 1 | "it cannot fail" | CONCEDE — FALSE #4. |
| 2 | "the slot is free" | CONCEDE — FALSE #5, drives the withdrawal. |
| 3 | "crates already in our tree" ⇒ no dep change | CONCEDE → D10, one crate, C6-justified below. |
| 4 | "window-relative coords straight off the event"; no `w`/`h`; no bounds check | CONCEDE → D5. `w`/`h` come from `allocated_width/height()` on the same widget the coords are relative to (so they cannot desync), and the `inside_window` guard from `gesture.rs:254` is replicated explicitly. |
| 5 | Alt = `MOD1_MASK` | **DECLARE AS ASSUMPTION.** Verified there is no `ALT_MASK` in `ModifierType` (`gdk-0.18.2/src/auto/flags.rs:563-620`); Mod1 is an XKB convention. **Pin:** smoke 17 (chord opens the pad). **Residual:** an exotic layout mapping Alt off Mod1 kills leg 2 only — leg 1 (taps) still exits the device, so §3.5 holds. |
| 6 | "every user input event enters our process through GDK" ⇒ per-process clock ≡ system-wide clock | **DECLARE AS ASSUMPTION + C3 divergence, both directions.** Stricter than Windows: nothing. Looser: input to a *second* Wayland client (an OSK, parent §7 `:697`, deferred to P2-G) is invisible to us, whereas `GetLastInputInfo` would see it. **Pin:** smoke 17's activity-reset assertion, plus a hardware-checklist row on the P2-G OSK deployment. **Residual:** idle reset can fire while a technician is at the pad but not typing. I decline to argue the "OSK injects into the focused surface, so its keystrokes do reach us" defence — that is tier-5 and I cannot verify it here, so it stays an assumption rather than a rebuttal. |
| 7 | "the handler never panics" — asserted, not enforced; panic across `extern "C"` is UB/abort | CONCEDE. Under D1-revised the *consequence* changes class: a panic aborts loudly (Q3) instead of silently killing all input. Contained structurally: no `unwrap`/index in the classifier, and the `RefCell` borrow is dropped before any outward call (D5). No `catch_unwind` — it would only convert a loud abort into a silent dead feature, which Q3 rates worse. |
| 8 | "the Windows call site is untouched"; plus the `:917`-before-`:1105` ordering hazard | CONCEDE both. **Revision removes them**: no signature change, no handle to plumb, `max(loop_start_ms, …)` covers the boot window (D4). Verified `main.rs:917` is a single non-cfg call. |
| 9 | `should_swallow` "meaningless where no shell exists" | CONCEDE — the justification covered only the shell half. `should_swallow` (`shortcuts.rs:77-86`) also swallows Ctrl+P, F5, F11 and the Menu/Apps key: webview-chrome chords. **Rewritten justification, both directions (C3):** *stricter on Windows* — Windows swallows those four in-app; *looser on Linux* — they are not swallowed, and the covering mechanisms are WebKitGTK settings (P2-B: `set_enable_developer_extras`, `connect_context_menu` for the Menu key — P2-B `:48`) plus the cage session for shell chords, plus §7.2/P2-G for VT switching. Where P2-B does not cover one (F5 reload, F11 fullscreen under an already-fullscreen cage surface), that is stated as an accepted looser divergence, not hidden. **PARTIAL REBUT of verifier §9.1's use of this:** not porting `should_swallow` does **not** remove a leg of the escape chain, because it was never one — parent `:693` says the LL hook *"is NOT a security boundary; OS-level Assigned Access / Shell Launcher is the covering boundary (§7.2, §12/OD-5)"*, and OD-5 (`:927`) repeats it. `should_swallow` is defense-in-depth on the *lockdown* side, not the *exit* side. Leg 3 is addressed on its own terms in D3. |

### Additionally raised, addressed head-on

- **§9.3 `TAP_WINDOW_MS` is `#[cfg(windows)]`** — CONCEDE, verified at `gesture.rs:181-182`.
  Fix in D5: delete the attribute. One line, one shared constant, no parity divergence. The
  draft's silence here was a real hole.
- **gdk/gtk must become direct target-gated deps (C6)** — D10. **Justification, in A's own
  template:** no new `Cargo.lock` entries (`gdk 0.18.2` / `gtk 0.18.2` already there), no new
  compile units (both already built for wry/tauri/webkit2gtk), version unification guaranteed
  by the lock so our types cannot diverge from Tauri's, and **the dependency is forced by
  Tauri's own public signature** — `gtk_window()` returns `gtk::ApplicationWindow`, a type we
  cannot name without the crate. Strictly one crate: `gdk` and `glib` arrive as
  `gtk::gdk` / `gtk::glib` (`gtk-0.18.2/src/lib.rs:18,21`).
- **The forwarded event is a `gdk_event_copy`** — CONCEDE the mechanism
  (`gdk-0.18.2/src/event.rs:65` → `glib-0.18.5/src/boxed.rs:482-485`), including the
  alloc/free per `MotionNotify` and the loss of identity. It is one of the two reasons the
  mechanism is withdrawn (the other is blast radius).
- **`position()` is not on `gdk::Event`** — CONCEDE for the generic type; moot under
  D1-revised, which receives `&EventButton` / `&EventTouch` / `&EventKey` typed from the
  signal, where `position()` does exist (`event_button.rs:19`, `event_touch.rs:21`), non-`Option`.

---

## Withdrawals / restructuring

1. **WITHDRAWN: `gdk::Event::set_handler` as D's observation mechanism.** Not on the citation
   error, and not on the copy alone — on blast radius. It puts our code on the critical path of
   *all* input to the product, where our own bug produces exactly the un-exitable device
   parent §3.5 forbids. Replaced by D1, which cannot do that by construction.
2. **WITHDRAWN: the "one handler, three consumers" architecture**, and with it the claim that
   one interception point is the simpler design. Replaced by two independent legs (D3) —
   because §3.5's fallback chain is a *requirement for independence*, and a fan-out from a
   single handler is its opposite.
3. **WITHDRAWN: the entire §Error handling paragraph**, including "nothing exits, nothing
   unlocks". Replaced by D11 + D3.
4. **Re-opened alternatives, re-closed:**
   - **evdev — rejection UPHELD.** The reasons stand and strengthen: D1-revised gets
     widget-relative coordinates *and* the widget's own `w`/`h` from the same object, so the
     screen→window conversion evdev would force (`gesture.rs:249-253` on Windows) is skipped
     entirely. Privilege/hotplug/transform costs unchanged.
   - **`ext-idle-notify-v1` — rejection UPHELD, still parked.** It solves idle only; the
     gesture and chord still need the GTK path, so it cannot replace D1, only D4's clock. It
     remains the fallback if D4's per-window clock fails smoke 16/17 or if the OSK divergence
     (assumption 6) proves load-bearing in the field. Dependency implication if taken:
     `wayland-client` as a new direct dep with no lockfile precedent — a real C6 cost, which
     is why it stays parked rather than pre-emptively adopted.
   - **NEW rejected-and-recorded: `gdk_event_handler_set`**, with the objdump evidence, so the
     next reader does not re-derive it.
5. **Restructured, not withdrawn:** D4 (no signature change, no ordering constraint, atomics
   not `RefCell`), D5 (`TAP_WINDOW_MS` un-cfg + bounds parity), D6 (`!mods.win` at the shared
   root), D8 (justification rewritten to cover the webview-chrome half).
6. **Added:** D10 (the C6 dependency decision the Scope omitted), D13 (PF-04 coverage flag).
7. **Open decisions, revised.** Removed: GDK-handler reentrancy (mechanism gone); the chord
   keyval set (settled — one key, `gdk::keys::constants::{K,k}`). Retained: `GDK_TOUCH_CANCEL`
   emission; cage-headless virtual input. Added: PF-04 intercept design (D13); the P2-G
   maintenance-route constraint (D3, leg 3).
