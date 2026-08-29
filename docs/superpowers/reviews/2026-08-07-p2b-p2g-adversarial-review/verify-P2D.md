# VERIFIER REPORT — P2-D (Linux Native Input: Idle Reset + Exit Gesture)

Target: `docs/superpowers/specs/2026-08-06-p2d-linux-native-input-design.md`
Evidence tiers per FRAME §1. Every claim below was checked mechanically. No opinions, no
proposals.

Registry root used throughout: `/root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`

---

## Verdict counts

| Verdict | Count |
|---|---|
| VERIFIED | 31 |
| FALSE | 6 |
| DRIFT | 4 |
| UNVERIFIABLE | 4 |
| Undeclared assumptions stated as fact (listed separately, §11) | 9 |

---

## 1. `file.rs:NNN` citations into `crates/kiosk-main/src/`

### `idle.rs` (140 lines)

| Citation | Spec's use | Actual text | Verdict |
|---|---|---|---|
| `idle.rs:1-14` | "the Windows shape is a 1 s poll loop over a system-wide last-input source with the `should_fire` latch" | Module doc. `:3-4` "Windows has no per-window 'idle' event, so this polls system-wide last-input time (`GetLastInputInfo`) once a second". `:10-12` "The one thing this module owns is the LATCH ([`should_fire`]): fire once when idle crosses the threshold, then stay quiet until activity resumes". | **VERIFIED** |
| `idle.rs:16-24` | "the SEC-09 never-cancelled property … carries over as-is (same `cancel` token wiring)" | `:16-17` "SEC-09 final review: this task is never cancelled by a credential-DACL violation (its `cancel` is the top-level shutdown token, not `main::fetch_probe_cancel`)". | **VERIFIED** |
| `idle.rs:32-34` | `should_fire` latch | `:32` `pub fn should_fire(idle_secs: u64, threshold: u64, already_fired: bool) -> bool {` `:33` `threshold != 0 && idle_secs >= threshold && !already_fired`. | **VERIFIED** |
| `idle.rs:57-64` | "The stub at `idle.rs:57-64` is replaced" | `:57` `#[cfg(not(windows))]` … `:63` `eprintln!("idle: only implemented on Windows; idle reset will never fire");` `:64` `}`. Exact. | **VERIFIED** |
| `idle.rs:66-75` | "a system poll (`GetLastInputInfo`, `idle.rs:66-75`)" | `:66-67` `#[cfg(windows)] mod windows_impl {`; `:71` `use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};` (import only); `:75` is the opening line of a doc comment. The **call** `GetLastInputInfo(&mut info)` is at `:85`; the **poll loop** is `:95-110`. | **DRIFT** — range names the module header + import, not the poll. Correct file/module; off by 10 (call) / 20-35 (loop). |

### `gesture.rs` (559 lines)

| Citation | Spec's use | Actual text | Verdict |
|---|---|---|---|
| `gesture.rs:10-11` | Tauri #13919 hook starvation | `:10-11` "for `WH_KEYBOARD_LL`, with the identical Tauri #13919 caveat: Windows can / silently stop delivering this hook's callbacks while WebView2 holds focus." | **VERIFIED** |
| `gesture.rs:17-23` | 'The "never unexitable" rule (cfg-12, `gesture.rs:17-23`)' | `:17-23` is the **`No fail-open (cfg-12)`** paragraph: "[`effective_gesture`] returns `None` when neither remote `input.exit_gesture` nor bootstrap `[exit_gesture]` is configured — the gesture is DISABLED in that case". The phrase **"never unexitable" is at `:15`**, in bullet 2 (the chord fallback), and is a *different* rule. | **FALSE (conflation)** — see §9.1. Location is right for cfg-12, wrong for "never unexitable", and the two rules point in opposite directions. |
| `gesture.rs:107` | "`effective_gesture`'s region/config" | `:107` `pub fn effective_gesture(`. | **VERIFIED** |
| `gesture.rs:153` | "fire → `open_pin_pad`" | `:153` `pub fn open_pin_pad(app: &tauri::AppHandle, gesture: Option<&EffectiveGesture>) {`. | **VERIFIED** |
| `gesture.rs:184-291` | "`WH_MOUSE_LL` in `gesture.rs:184-291`" | `:184` `#[cfg(windows)]` (over the safe `install` wrapper, `:185-191`). `:291` is `app: tauri::AppHandle,` — a parameter line inside `windows_impl::install`'s signature. `WH_MOUSE_LL` itself appears at `:212` (import) and `:312` (`SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0)`). The hook callback is `:244-283`. **The cited range excludes the actual hook installation.** | **DRIFT** — right module, end boundary cuts mid-signature and omits `:312`. Honest range is `:204-327`. |
| `gesture.rs:193` | "the Linux stub" | `:193` `#[cfg(not(windows))]`, `:194` `pub fn install(`. | **VERIFIED** |
| `gesture.rs:239-241` | "on Windows the hook must convert screen→window itself" | `:239-241` "instead). Converts the hook's screen-coordinate `pt` to window-relative / coordinates via the live window position/size, then routes through the same / pure `in_region`/`TapCounter` this module host-tests." (doc comment; the code is `:249-253`). | **VERIFIED** (doc says exactly what is claimed) |

### `shortcuts.rs` (431 lines)

| Citation | Spec's use | Actual text | Verdict |
|---|---|---|---|
| `shortcuts.rs:18` | Tauri #13919 | `:18` "only.** Per Tauri issue #13919, `WH_KEYBOARD_LL` is dropped by Windows while". | **VERIFIED** |
| `shortcuts.rs:66` | "`should_swallow` is deliberately not ported" | `:66` `pub fn should_swallow(vk: u32, mods: Modifiers) -> bool {`. | **VERIFIED** |
| `shortcuts.rs:99` | "the existing `is_technician_chord(vk, mods)`" | `:99` `pub fn is_technician_chord(vk: u32, mods: Modifiers) -> bool {`. | **VERIFIED** |
| `shortcuts.rs:103-208` | "`WH_KEYBOARD_LL` in `shortcuts.rs:103-208`" | `:103` `#[cfg(windows)]` over the safe `install` wrapper. `:104-119` the two `install` arms. `:121-206` is **Vector A — `install_accelerator_handler`** (`AcceleratorKeyPressed` on the WebView2 controller). `:208` is the comment header `// ---- Vector B: WH_KEYBOARD_LL …`. The `WH_KEYBOARD_LL` import is `:221`, `kb_hook` is `:224-232`, `install_ll_hook` is `:238-256`. **The cited range contains essentially none of the WH_KEYBOARD_LL implementation.** | **DRIFT (severe)** — the range points at Vector A, not the low-level keyboard hook. Honest range is `:208-256`. |
| `shortcuts.rs:112` | "the Linux stub" | `:112` `#[cfg(not(windows))]`, `:113` `pub fn install(`. | **VERIFIED** |

---

## 2. Named items and signatures

| Item | Claimed | Actual | Verdict |
|---|---|---|---|
| `idle::should_fire` | pure latch, reused | `pub fn should_fire(idle_secs: u64, threshold: u64, already_fired: bool) -> bool` (`idle.rs:32`), not cfg-gated, host-tested `:117-127`. | **VERIFIED** |
| `idle_secs_from_ticks` | "is a Windows-tick artifact and **stays** `#[cfg(windows)]` with its doc" | `idle.rs:44` `pub fn idle_secs_from_ticks(now_tick_ms32: u32, last_input_tick_ms32: u32) -> u64` — **has NO `cfg` attribute at all**, and is exercised by the host test `idle_secs_is_wrap_safe_across_the_32bit_tick_boundary` (`:129-139`), which is likewise not cfg-gated and therefore runs on the Linux CI job. | **FALSE** — "stays `#[cfg(windows)]`" describes a state that does not exist; making it true would break `cargo test` on Linux (the test at `:129-139` calls it unconditionally). See §9.2. |
| `gesture::in_region` | "window-relative", "the exact form `in_region` already takes" | `gesture.rs:34` `pub fn in_region(x: f64, y: f64, w: f64, h: f64, region: GestureRegion) -> bool`. Doc `:30` "given a point already in **window-relative** coordinates (spec §5.2)". | **VERIFIED** (coordinates *are* window-relative) — but see §11.4: `in_region` performs **no bounds check**; the Windows caller supplies its own (`gesture.rs:254`, `inside_window`), and `in_region` also needs `w`/`h`, which a GDK event does not carry. |
| `gesture::TapCounter` | reused verbatim | `gesture.rs:51-76`. `new(taps_needed: u8, window_ms: i64) -> Self`, `tap(&mut self, now_ms: i64) -> bool`. | **VERIFIED** |
| `gesture::effective_gesture` | supplies "region/config" | `gesture.rs:107-110` `pub fn effective_gesture(remote: Option<&ExitGesture>, bootstrap: Option<&BootstrapExitGesture>) -> Option<EffectiveGesture>`; `EffectiveGesture` fields `{taps, region, pin_hash, min_len, alphanumeric}` (`:81-87`). | **VERIFIED** — note it supplies `taps` + `region` but **not** the tap window; see §9.3. |
| `gesture::open_pin_pad` | "unchanged" | `gesture.rs:153` `pub fn open_pin_pad(app: &tauri::AppHandle, gesture: Option<&EffectiveGesture>)`. Not cfg-gated. | **VERIFIED** |
| `shortcuts::is_technician_chord(vk, mods)` | "one chord *definition*, two key-code domains" | `shortcuts.rs:99-101`: `pub fn is_technician_chord(vk: u32, mods: Modifiers) -> bool { vk == VK_K && mods.ctrl && mods.alt && mods.shift }`. **VK domain:** raw Win32 virtual-key `u32`, pinned locally, *not* the `windows` crate's `VIRTUAL_KEY` — `shortcuts.rs:41-44` "pinned as raw `u32`s … so this module's decision table stays pure and host-testable on every target". **Chord constant:** exactly one — `shortcuts.rs:58` `const VK_K: u32 = 0x4B;` ("`K`, for the technician exit-gesture chord below"). `Modifiers` = `{ctrl, alt, shift, win}` bools (`:33-39`). | **VERIFIED**. The chord is Ctrl+Alt+Shift+K; the keyval→VK map therefore needs **one** key (`K` → `0x4B`) plus four modifier bits. The spec's open decision "the exact keyval set for the chord map … enumerate, don't wildcard" overstates a one-element set. |
| `shortcuts::should_swallow` | "stays Windows-only" | `shortcuts.rs:66` — **not cfg-gated today**; it is a plain `pub fn` with 18 host tests (`:287-390`) that run on Linux. "Stays Windows-only" is true of its *callers*, false of the function. | **DRIFT** (imprecise, harmless) |
| cfg-12 "never unexitable" rule | one rule at `gesture.rs:17-23` | Two distinct rules. cfg-12 (parent `:442-443`, "if absent here and remote, exit gesture is **DISABLED**") = `gesture.rs:17-23`. "never unexitable" (parent §3.5 `:319-320`, "the exit gesture falls back to a reserved `AcceleratorKeyPressed` technician chord **and/or the §7.2 OS-lockdown escape**, so a locked device is never unexitable") = `gesture.rs:12-15`. | **FALSE** — see §9.1 |
| SEC-09 never-cancelled property in `idle.rs` | "carries over as-is (same `cancel` token wiring)" | `idle.rs:16-24` documents it; mechanism is that `run` receives the top-level shutdown `CancellationToken` from `main.rs:917` (`cancel.clone()`), not `fetch_probe_cancel`. Nothing in the Linux swap touches that argument. | **VERIFIED** |
| FSM chain `Online + idle_clear → Effect::ClearProfile → ProfileCleared` | claimed | `kiosk-core/src/app/state.rs:296-304`: `(Online { .. }, IdleExpired) if self.cfg.idle_clear => … vec![Effect::ClearProfile { full: true }]`, state → `Clearing { next: Online{url} }`; `ProfileCleared` arm at `:313+`. | **VERIFIED** |

---

## 3. `gdk-0.18.2/src/event.rs:56-57` — `set_handler`

**Location VERIFIED, path name FALSE.**

```
52	    /// Set the event handler.
53	    ///
54	    /// The callback `handler` is called for each event. If `None`, event
55	    /// handling is disabled.
56	    #[doc(alias = "gdk_event_handler_set")]
57	    pub fn set_handler<F: Fn(&mut Event) + 'static>(handler: Option<F>) {
58	        assert_initialized_main_thread!();
```

- **Exists at `:56-57`** — doc alias line 56, signature line 57. **VERIFIED**
- **Safe** — `pub fn`, not `unsafe fn`. **VERIFIED**
- **Wraps `gdk_event_handler_set`** — `:83-87` `ffi::gdk_event_handler_set(Some(event_handler_trampoline::<F>), ptr, Some(event_handler_destroy::<F>))`; `:90` the removal arm. **VERIFIED**
- **Exact signature:** `pub fn set_handler<F: Fn(&mut Event) + 'static>(handler: Option<F>)`
- **`'static`: YES, required.** **VERIFIED**
- **`Fn`, not `FnMut`** — mutable state must go behind `Cell`/`RefCell`. The spec's `RefCell` choice is consistent. **VERIFIED**
- **Ownership:** the closure is double-boxed (`:80-81`, `Box::into_raw`) and freed by the destroy-notify `event_handler_destroy::<F>` (`:69-76`). **Replaceable** by calling again (GDK invokes the old destroy notify); **removable** by passing `None`. **VERIFIED**
- **Path: `gdk::event::set_handler` does not exist.** `event.rs:33` `impl Event {` — it is an **associated function on `gdk::Event`**, and `gdk-0.18.2/src/lib.rs:19` declares `mod event;` (private, not `pub mod`). The callable path is `gdk::Event::set_handler(...)`. **FALSE (path)** — cosmetic but it is a mechanically checkable citation.

### 3a. "It cannot fail" — **FALSE**

Spec §Error handling: *"If `set_handler` installation itself fails (**it cannot** — it is a process-global function-pointer store, not a fallible call …)"*.

`event.rs:58` is `assert_initialized_main_thread!();`, expanded from `gdk-0.18.2/src/rt.rs:16-26`:

```
18        if !crate::rt::is_initialized_main_thread() {
19            if crate::rt::is_initialized() {
20                panic!("GDK may only be used from the main thread.");
21            } else {
22                panic!("GDK has not been initialized. Call `gdk::init` or `gtk::init` first.");
23            }
24        }
```

`set_handler` **panics** if called before GTK/GDK init or from any thread other than the GDK main thread. `gtk::main_do_event` carries the identical assertion (`gtk-0.18.2/src/auto/functions.rs:377`) and therefore panics on the same two conditions — inside the handler, on every event. The spec's error-handling section is built on a premise the binding contradicts. **FALSE.**

---

## 4. `gtk-0.18.2/src/auto/functions.rs:376` — `main_do_event`

**VERIFIED, exact.**

```
375	#[doc(alias = "gtk_main_do_event")]
376	pub fn main_do_event(event: &mut gdk::Event) {
377	    assert_initialized_main_thread!();
378	    unsafe {
379	        ffi::gtk_main_do_event(event.to_glib_none_mut().0);
380	    }
381	}
```

Safe `pub fn`, at the cited line. Signature takes `&mut gdk::Event`.

---

## 5. "The slot is free" — **FALSE**

### 5a. First-party crates: VERIFIED (and stronger than the spec claims)

```
grep -rn "event_handler_set|set_handler|main_do_event" tao-0.35.3/src/   → 0 hits (exit 1)
grep -rn "event_handler_set|set_handler|main_do_event" wry-0.55.1/src/  → 0 hits (exit 1)
```

Widened to the **entire vendored registry**:

```
grep -rln "gdk_event_handler_set|event_handler_set" ~/.cargo/registry/src/*/
  → gdk-sys-0.18.2/src/lib.rs   (the FFI declaration)
  → gdk-0.18.2/src/event.rs     (the binding itself)
```

Zero hits in tao, wry, tauri, webkit2gtk, webkit2gtk-sys, glib, gio. No Rust crate in the tree competes for the slot. **VERIFIED**, and D under-claims here (it only checked `tao/src/platform_impl/linux/` and wry).

### 5b. GTK itself: the slot is **occupied**. **FALSE as stated.**

`/usr/lib/x86_64-linux-gnu/libgtk-3.so.0` (GTK 3.24.32):

```
$ objdump -R libgtk-3.so.0 | grep event_handler_set
00000000007c4788 R_X86_64_JUMP_SLOT  gdk_event_handler_set@Base
$ nm -D libgtk-3.so.0 | grep event_handler_set
                 U gdk_event_handler_set
```

Disassembly of the single call site (GTK's init path):

```
  1fdbcd:	lea    0x591c(%rip),%rdi        # 2034f0 <gtk_main_do_event@@Base>
  1fdbd4:	xor    %edx,%edx
  1fdbd6:	xor    %esi,%esi
  1fdbd8:	call   91df0 <gdk_event_handler_set@plt>
```

GTK installs `gtk_main_do_event` as **the** GDK event handler (data `NULL`, destroy `NULL`) during init. `Event::set_handler` therefore **replaces GTK's own dispatch**, it does not add to it. The design's forwarding to `gtk::main_do_event` is not a courtesy — it is the sole thing preventing total input death for the webview. **The claim "the slot is free" is FALSE**; what is free is "no first-party Rust crate competes for it".

### 5c. Reentrancy (the spec's deferred plan-time question) — partially resolvable NOW

`gdk_event_handler_set` stores a **single** function pointer, consulted by libgdk at dispatch. Our handler calls `gtk_main_do_event` **directly** (a plain exported GTK symbol, `nm -D → 00000000002034f0 T gtk_main_do_event`), not through the stored pointer. There is no path by which forwarding re-enters the handler through the handler slot: `gdk_event_handler_set` is called exactly once in libgtk (the init site above), and there is no `gdk_event_put` call in `gtk_main_do_event`'s text range.

Residual, genuinely unverifiable here: nested main-loop reentry (a GTK modal/`gtk_main_iteration` spun from inside `open_pin_pad` → `window.navigate` → GTK re-dispatch) would re-enter the handler on the same thread while its `RefCell` is borrowed → `RefCell` panic. The spec's "the handler never panics" is asserted, not enforced. See §11.7.

---

## 6. `gdk` / `gtk` as dependencies — **VERIFIED transitive, FALSE as "usable"**

`Cargo.lock`:
```
1101	name = "gdk"
1102	version = "0.18.2"
1345	name = "gtk"
1346	version = "0.18.2"
```
Both present at the claimed 0.18. **VERIFIED.**

`crates/kiosk-main/Cargo.toml`: `[dependencies]` = kiosk-core, tauri, tokio, tokio-util, chrono, reqwest, serde, serde_json, arc-swap, sysinfo. `[target.'cfg(windows)'.dependencies]` = webview2-com, windows. **There is no `[target.'cfg(not(windows))'.dependencies]` section and no `gdk`/`gtk` entry anywhere**, and the workspace `Cargo.toml` declares neither.

No re-export path exists either: `grep "pub use gtk|pub use gdk"` returns zero hits in tao-0.35.3, wry-0.55.1 and tauri-2.11.5. (tao *uses* `gtk::` internally, e.g. `platform/unix.rs:79 fn gtk_window(&self) -> &gtk::ApplicationWindow`, but does not re-export the crate.)

The spec says the bindings are "in crates already in our tree (gtk/gdk 0.18 via tao/wry)". True of the **lockfile**; **false of usability** — `use gdk::Event;` will not compile without adding `gdk` and `gtk` as **direct**, target-gated dependencies of `kiosk-main`. The spec's Scope section lists no dependency change, and FRAME C6 ("no new dependencies without justification") makes a direct-dep declaration a decision that must be stated. **FALSE / undeclared.** (Mitigating: no new lockfile entries, no new compile units.)

---

## 7. GDK event types and accessors

### EventType variants — all 10 exist (`gdk-0.18.2/src/auto/enums.rs`)

| Variant | Line | GDK alias |
|---|---|---|
| `MotionNotify` | 1545 | `GDK_MOTION_NOTIFY` |
| `ButtonPress` | 1547 | `GDK_BUTTON_PRESS` |
| `ButtonRelease` | 1553 | `GDK_BUTTON_RELEASE` |
| `KeyPress` | 1555 | `GDK_KEY_PRESS` |
| `KeyRelease` | 1557 | `GDK_KEY_RELEASE` |
| `Scroll` | 1599 | `GDK_SCROLL` |
| `TouchBegin` | 1611 | `GDK_TOUCH_BEGIN` |
| `TouchUpdate` | 1613 | `GDK_TOUCH_UPDATE` |
| `TouchEnd` | 1615 | `GDK_TOUCH_END` |
| `TouchCancel` | 1617 | `GDK_TOUCH_CANCEL` |

**VERIFIED.** `TouchCancel` is a distinct binding variant (**the binding question is settled; runtime emission on Wayland is not — see §10**).

**Not in the spec's activity set but present in the enum:** `DoubleButtonPress` (`:1549`), `TripleButtonPress` (`:1551`), `TouchpadSwipe` (`:1619`), `TouchpadPinch` (`:1621`), `PadButtonPress/Release/Ring/Strip` (`:1622-1629`), `ProximityIn/Out` (`:1579-1581`), plus `__Unknown(i32)` (`:1633`). The spec promises a host test over "the full event-type table"; the enum has **~45 variants incl. a non-exhaustive `__Unknown`**, and the spec's prose enumerates 8 activity + 4 non-activity. `TouchpadPinch` is directly relevant to PF-04 (P2-A/parent pinch-intercept obligation) and is not mentioned.

### Accessors on the generic `gdk::Event` (`gdk-0.18.2/src/event.rs`)

| Accessor | Line | Signature | Notes |
|---|---|---|---|
| `coords()` | 145 | `pub fn coords(&self) -> Option<(f64, f64)>` | wraps `gdk_event_get_coords`; locals are named `x_win`/`y_win` — **window-relative**. **This is the accessor the design needs.** |
| `root_coords()` | 195 | `pub fn root_coords(&self) -> Option<(f64, f64)>` | screen-relative; not what the design wants |
| `state()` | 252 | `pub fn state(&self) -> Option<ModifierType>` | wraps `gdk_event_get_state` |
| `keyval()` | 179 | `pub fn keyval(&self) -> Option<u32>` | wraps `gdk_event_get_keyval` |
| `keycode()` | 163 | `pub fn keycode(&self) -> Option<u16>` | hardware keycode |
| `event_type()` | 362 | `pub fn event_type(&self) -> EventType` | **not** an `Option` |
| `button()` | 113 | `pub fn button(&self) -> Option<u32>` | |
| `event_sequence()` | 281 | `pub fn event_sequence(&self) -> Option<EventSequence>` | needed for touch tracking |
| `is_pointer_emulated()` | 304 | `pub fn is_pointer_emulated(&self) -> bool` | relevant to the spec's "Wayland delivers real touch as touch events, not synthesized buttons" claim |
| **`position()`** | — | **DOES NOT EXIST on `gdk::Event`** | it exists only on the concrete `EventButton` (`event_button.rs:19`) and `EventTouch` (`event_touch.rs:21`), each `pub fn position(&self) -> (f64, f64)` (non-`Option`), reachable via `Event::downcast_ref::<EventButton>()` (`event.rs:384`). |

**VERIFIED:** window-relative coordinates (`coords()`) and modifier state (`state()`) *are* readable from a generic `gdk::Event`. **`position()` is NOT** — that name is a per-type accessor.

`ModifierType` bits available (`auto/flags.rs:562-620`): `SHIFT_MASK` (564), `CONTROL_MASK` (568), `MOD1_MASK` (570), `MOD4_MASK` (576), `SUPER_MASK` (616), `META_MASK` (620). There is **no `ALT_MASK`** — Alt must be read as `MOD1_MASK` (an XKB convention, not a guarantee). See §11.5.

### 7a. The forwarded event is a **copy**, not the original — undocumented and load-bearing

`event.rs:59-68` (the trampoline):
```
65                let mut event = from_glib_none(event);
66                f(&mut event)
```
`gdk::Event` is `glib::wrapper!{ pub struct Event(Boxed<ffi::GdkEvent>) }` with `copy => |ptr| ffi::gdk_event_copy(ptr)` / `free => |ptr| ffi::gdk_event_free(ptr)` (`event.rs:20-31`), and `glib-0.18/src/boxed.rs:480-486`:
```
482    unsafe fn from_glib_none(ptr: *mut T) -> Self {
484        let ptr = MM::copy(ptr);
485        from_glib_full(ptr)
486    }
```

So every event handed to the closure is a **`gdk_event_copy`**, and `gtk::main_do_event(&mut event)` dispatches **the copy**, with `gdk_event_free` on drop. Consequences the spec does not state:

1. "does O(1) work per event" is true asymptotically but each event now costs a `gdk_event_copy` + `gdk_event_free` (heap alloc/free per event, on the GTK main thread, including every `MotionNotify`).
2. GTK dispatches a copied `GdkEvent`, not the one GDK produced. Whether GTK3's private-flag / double-click-emulation / `is_send_event` state survives `gdk_event_copy` intact for `gtk_main_do_event`'s purposes is a GTK3 C-internals question — **UNVERIFIABLE here**, pinned only by running the smoke.
3. The spec's "no ordering change" is correct; **identity** is not preserved, and the spec's phrase "sees every `gdk::Event` before dispatch and forwards each" implies pass-through of the same object.

---

## 8. Windows-side claims — **partly FALSE**

Spec: *"Windows needs **two** global hooks (`WH_MOUSE_LL` in `gesture.rs:184-291`, `WH_KEYBOARD_LL` in `shortcuts.rs:103-208`) plus a system poll (`GetLastInputInfo`, `idle.rs:66-75`)."*

- `WH_MOUSE_LL`: exists. Import `gesture.rs:212`, install `gesture.rs:312` `SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0)`, dedicated thread `:309-322`. **VERIFIED** (line range DRIFT, §1).
- `WH_KEYBOARD_LL`: exists. Import `shortcuts.rs:221`, install `shortcuts.rs:242` `SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), None, 0)`, dedicated thread `:239-252`. **VERIFIED** (line range DRIFT, §1).
- `GetLastInputInfo`: exists. `idle.rs:71` (import), `idle.rs:85` (call), poll loop `idle.rs:95-110`. **VERIFIED** (line range DRIFT, §1).
- **"two" is wrong — Windows has three observation vectors.** The third is `AcceleratorKeyPressed` on the `ICoreWebView2Controller` (`shortcuts.rs:156-206`, subscribed at `:199`). It is not a hook and is not counted.
- **Materially: the technician chord on Windows rides Vector A, not `WH_KEYBOARD_LL`.** `kb_hook` (`shortcuts.rs:224-232`) calls **only** `should_swallow`; the one and only `is_technician_chord` call site is `shortcuts.rs:186`, inside the `AcceleratorKeyPressed` handler. The spec's "this *removes* the Windows caveat class — Tauri #13919 … has no GDK analogue" therefore mis-attributes the Windows chord path: #13919 starves the LL hooks, which never carried the chord.

**Verdict: FALSE** on "two global hooks" and on the implied #13919→chord linkage.

---

## 9. Headline FALSE findings, expanded

### 9.1 "never unexitable" ≠ cfg-12 — and D's error handling inverts the parent's rule

Parent §3.5, `2026-07-05-kiosk-browser-design.md:318-320`:
> "If native tap capture proves unreliable, the exit gesture falls back to a reserved `AcceleratorKeyPressed` technician chord **and/or the §7.2 OS-lockdown escape**, so a locked device is never unexitable."

cfg-12, parent `:442-443`:
> "`pin_hash` … ; if absent here and remote, exit gesture is DISABLED"

These are opposite-direction rules: cfg-12 makes the gesture *disappear* when unconfigured; §3.5 requires an *always-available* escape. D cites `gesture.rs:17-23` (cfg-12) as the source of "never unexitable", and then in §Error handling writes:

> "the kiosk runs with gesture/idle dead and the chord's absence is covered by **cfg-12's no-fail-open semantics: nothing exits, nothing unlocks**."

"Nothing exits" is precisely the state §3.5 forbids. On Windows the parent's escape chain has three independent legs (accelerator handler, LL keyboard hook, §7.2 Assigned Access). D collapses Linux to **one** GDK handler carrying **both** exit vectors, and separately declines to port `should_swallow` on the grounds that "under cage there is no desktop shell" — i.e. the §7.2-equivalent leg is also gone. If the single handler dies (panic per §3a, `RefCell` reentry per §5c, or `set_handler` never reached), there is no remaining exit path at all. This is a mechanically-established contradiction with parent §3.5, not a preference.

### 9.2 `idle_secs_from_ticks` "stays `#[cfg(windows)]`"

`idle.rs:44` has no `cfg`. The host test at `idle.rs:129-139` calls it unconditionally and runs on the existing per-PR ubuntu CI job (P2-A `:320` "Host tests (existing per-PR ubuntu CI job)"). Adding `#[cfg(windows)]` as the spec states would fail Linux CI compilation. The spec describes a change it does not name as a change, and the change as described breaks a green gate (FRAME C8/C9).

### 9.3 `TAP_WINDOW_MS` is `#[cfg(windows)]` and the spec does not account for it

`gesture.rs:181-182`:
```
181	#[cfg(windows)]
182	const TAP_WINDOW_MS: i64 = 3000;
```
`TapCounter::new` requires `window_ms`. `effective_gesture`/`EffectiveGesture` supply `taps` and `region` only — **there is no window value on the Linux side**. The spec says taps "feed the existing `TapCounter` + `in_region` against `effective_gesture`'s region/config (`gesture.rs:107`)", which cannot be done without either un-cfg-ing `TAP_WINDOW_MS` or introducing a second constant (a silent parity divergence in the tap-window value — FRAME C3). Unowned.

### 9.4 Also worth recording (not FALSE, but unstated)

`is_technician_chord` (`shortcuts.rs:100`) does **not** require `!mods.win`. On Windows, `should_swallow` returns `true` for *any* `mods.win` (`:71-73`) and Vector A checks swallow **first** (`:184`), so Ctrl+Alt+Shift+**Win**+K is swallowed, not chorded. On Linux with `should_swallow` absent, the same physical keys **would** open the pad. The spec's stated invariant — "the same physical chord opens the pad on both platforms" — is therefore violated for the Super-held case. Mechanically checkable, small blast radius.

---

## 10. Environment / UNVERIFIABLE items

| Item | Status | Why | Pinning mechanism |
|---|---|---|---|
| `wlrctl` (smoke 17) | **UNVERIFIABLE — not installed** | `which wlrctl` → not found. `apt-cache policy wlrctl` → `Installed: (none)`, `Candidate: 0.2.2-1` (ubuntu noble/universe). | The spec's own plan-time check + smoke 17 fallback to the hardware list. This one *is* declared with a pinning mechanism. |
| `cage` (smokes 13/17) | **UNVERIFIABLE — not installed** | `which cage` → not found. `apt-cache policy cage` → `Installed: (none)`, `Candidate: 0.1.5+20240127-2build1`. | Same; also P2-C smoke 13 depends on it. |
| Any wlr virtual-input tooling | **UNVERIFIABLE — none present** | `which weston wtype ydotool sway` → all not found. | — |
| Platform floor mismatch | **Noted** | This environment is `Ubuntu 24.04.4 LTS`, not the FRAME C7 floor (Debian 12 / Ubuntu 22.04). `libgtk-3.so.0.2409.32` here vs whatever Debian 12 ships. The libgtk disassembly evidence in §5b is therefore *indicative* of GTK3's design, not a measurement of the floor image. | Re-run the `objdump -R libgtk-3.so.0 \| grep event_handler_set` check on the P2-G pinned image. |
| `GDK_TOUCH_CANCEL` distinct emission on Wayland | **UNVERIFIABLE at runtime** (binding VERIFIED, §7) | Requires a Wayland compositor + touch. | The spec already lists it as an open decision. Declared. |
| `gdk_event_copy` fidelity through `gtk_main_do_event` (§7a) | **UNVERIFIABLE** | GTK3 C internals; no gtk3 sources in this environment. | Smoke 16/17 under weston would surface gross breakage (clicks/keys not reaching the page); subtle breakage (double-click emulation, pointer-emulated touch) would not. **Not currently pinned by any declared scenario.** |

---

## 11. Claims stated as fact that are undeclared assumptions

Ordered by load-bearing weight.

1. **"If `set_handler` installation itself fails (it cannot …)"** — refuted in §3a: `assert_initialized_main_thread!` panics on two conditions, and `gtk::main_do_event` re-asserts on **every event**. The whole error-handling section rests on this. Not an assumption at all — a wrong fact.
2. **"The slot is free"** — refuted in §5b. The true statement is "no first-party Rust crate competes for it; GTK owns it and we must re-do GTK's job by hand". Stated as fact, with binary evidence to the contrary.
3. **"crates already in our tree"** implying no dependency change — §6. A direct, target-gated `gdk`+`gtk` dep must be added to `crates/kiosk-main/Cargo.toml`; the spec's Scope section does not list it and FRAME C6 requires justification.
4. **"window-relative coordinates straight off the event"** — `Event::coords()` gives window-relative `(x, y)`, but `in_region` also needs `w`/`h`, which no `gdk::Event` carries (must come from the GTK/Tauri window), and `in_region` performs **no bounds check** (`gesture.rs:34-43`; `TopLeft` is `x < mid_x && y < mid_y`, true for negative coordinates). The Windows path supplies that check itself at `gesture.rs:254` (`inside_window`). Dropping it on Linux, unremarked, is a silent behavioral divergence.
5. **Alt = `MOD1_MASK`** — no `ALT_MASK` exists in `ModifierType` (§7). The keyval→VK map's modifier half rests on the XKB convention that Alt is Mod1. Unstated.
6. **"every user input event enters our process through GDK" / "one fullscreen GTK window in a compositor with no other client"** — this is the load-bearing premise for replacing the *system-wide* `GetLastInputInfo` with a *per-process* `ActivityClock`. It is contradicted by the spec's own scope: parent §7 (`:697`) requires "Linux: squeekboard/onboard deployment docs", and D defers the on-screen keyboard to P2-G. An OSK is a **second Wayland client**; keystrokes into it produce **no GDK events in our process**, so the idle timer keeps counting while a technician types the PIN. Stated as fact, not as an assumption with a pinning mechanism.
7. **"the handler never panics"** — asserted as discipline, not enforced. `RefCell` reentry (§5c), `assert_initialized_main_thread!` (§3a), and any `unwrap` in the classifier all panic. A panic unwinding across the `extern "C"` trampoline (`event.rs:59`) is UB/abort in Rust 2021. No `catch_unwind` is specified.
8. **"the Windows call site is untouched"** (idle signature) — `main.rs:917` is a **single, non-cfg-gated** call: `tokio::spawn(idle::run(idle_reset_seconds, tx.clone(), cancel.clone()));`. `idle::run`'s two arms currently share one arity. Growing the Linux arm forces either a cfg-gated call site at `:917` or the parameter on both arms. Also an ordering constraint the spec does not name: `idle::run` is spawned at `:917`, while `input_watch` installs "from setup, after the window exists" — the Tauri setup closure runs at `main.rs:1105-1106`, i.e. **after**. The `ActivityClock` must therefore be constructed before `:917` and shared, and the idle loop will run against an unstamped clock for the boot window.
9. **`should_swallow` "is meaningless where no shell exists"** — covers only part of the swallow list. `should_swallow` (`shortcuts.rs:77-86`) also swallows `Ctrl+P` (print), `F5` (reload), `F11` (fullscreen) and the standalone Menu/Apps key (`VK_APPS`) — **webview-chrome** chords, not shell chords. (`VK_APPS` → context menu is separately covered by `hardening.rs:152` `SetAreDefaultContextMenusEnabled(false)` on Windows / P2-B on Linux, which D does not cite.) The divergence is real but the justification given does not cover it — FRAME C3 requires the divergence be stated in both directions.

---

## 12. Cross-spec checks

| Check | Result |
|---|---|
| Smoke numbering A 1-7, B 8-12, C 13-15, D 16-17 | **VERIFIED, no collision.** A `:312` "Gate: scenarios 1–7"; B `:31` "smoke scenarios 8–12"; C `:21` "smoke scenarios 13–15" (bodies at `:152-159`); D `:25` "smoke scenarios 16–17"; E `:94` "Soak protocol (scenario 18)". Contiguous, disjoint. |
| Does A actually say what D claims about scenario 6? | **VERIFIED.** A `:303-305`: *"profile clear: no app-path producer for `ClearProfile` **until P2-D**, so a dedicated harness binary (cargo example) creates a webview under the compositor, drives `clear::clear` directly, and asserts cookie-gone + `ProfileCleared` received"*. D's *"A's harness-binary scenario (A smoke 6) stays as the completion unit check; D's smoke 16 supersedes it as the app-path proof"* is an accurate reading — A itself names P2-D as the successor. |
| D's claim that C's scenario 14 "gains its app-path driver from D's chord" | **VERIFIED (consistent).** C `:155-156`: *"14. **technician exit:** drive the pinpad exit → assert the *launcher process* exits 86"*. C does not say how the pad is reached; D supplies it. No contradiction. |
| G's hardware checklist reference to D | **VERIFIED.** P2-G `:94` row H4 names *"D (smoke 17 if headless virtual input was unavailable; §7 keyboard row)"* — D's deferral is owned downstream, as FRAME §4.5 requires. |

---

## 13. Summary table

| # | Claim | Verdict |
|---|---|---|
| 1 | `idle.rs:1-14,16-24,32-34,57-64` | VERIFIED ×4 |
| 2 | `idle.rs:66-75` = `GetLastInputInfo` poll | DRIFT (call at :85, loop at :95-110) |
| 3 | `gesture.rs:10-11,107,153,193,239-241` | VERIFIED ×5 |
| 4 | `gesture.rs:17-23` = "never unexitable" | **FALSE** (that is cfg-12; §3.5 rule is at :12-15) |
| 5 | `gesture.rs:184-291` = `WH_MOUSE_LL` | DRIFT (install at :312, outside range) |
| 6 | `shortcuts.rs:18,66,99,112` | VERIFIED ×4 |
| 7 | `shortcuts.rs:103-208` = `WH_KEYBOARD_LL` | DRIFT (that range is Vector A; hook is :208-256) |
| 8 | `in_region` coords are window-relative | VERIFIED |
| 9 | `is_technician_chord(vk: u32, mods: Modifiers)`, VK domain = raw Win32 u32, chord = `VK_K 0x4B` + ctrl/alt/shift | VERIFIED |
| 10 | `idle_secs_from_ticks` "stays `#[cfg(windows)]`" | **FALSE** (never was; gating breaks Linux CI) |
| 11 | `gdk::event::set_handler` at `event.rs:56-57`, safe, wraps `gdk_event_handler_set` | VERIFIED (location/safety/wrapping); **FALSE** on path (`gdk::Event::set_handler`; `mod event` is private) |
| 12 | `set_handler` "cannot fail" | **FALSE** (`assert_initialized_main_thread!` panics) |
| 13 | `gtk::main_do_event` at `functions.rs:376`, safe | VERIFIED |
| 14 | tao/wry install no GDK handler | VERIFIED (zero hits repo-wide) |
| 15 | "The slot is free" | **FALSE** (GTK3 init installs `gtk_main_do_event` into it — objdump evidence) |
| 16 | gdk/gtk 0.18 in `Cargo.lock` | VERIFIED |
| 17 | usable without a new direct dep | **FALSE** (no dep, no re-export; must add 2 target-gated direct deps) |
| 18 | 10 `EventType` variants incl. `TouchCancel` | VERIFIED (binding); runtime emission UNVERIFIABLE |
| 19 | `coords()`/`state()`/`keyval()` on generic `Event` | VERIFIED; `position()` NOT on `Event` (only `EventButton`/`EventTouch`) |
| 20 | Windows = "two global hooks" + `GetLastInputInfo` | **FALSE** (three vectors; the chord rides `AcceleratorKeyPressed`, not `WH_KEYBOARD_LL`) |
| 21 | Smoke numbering 1-7 / 8-12 / 13-15 / 16-17 / 18 | VERIFIED, no collision |
| 22 | A smoke 6 says what D claims | VERIFIED |
| 23 | `wlrctl` / `cage` availability | UNVERIFIABLE (neither installed; both in noble/universe) |
| 24 | Forwarded event is the original | **Undocumented: it is a `gdk_event_copy`** (`boxed.rs:480-486`) |
