# P2-D — CRITIC, Round 1

No frame dispute.

**Headline:** the withdrawal was **correct, not an over-correction** — I verified the
replacement's coverage question end to end and D1's core mechanism survives. Eight
objections remain, none against D1's central claim. The two that matter are a HIGH
coverage gap (PF-04) and a MED that reverses a documented Windows failure-direction
choice (D4/D11).

## Objection index

| ID | Change | Objection (one line) | Sev | Evidence tier |
|---|---|---|---|---|
| OB-1 | D13 | PF-04 pinch intercept is owned by no spec in A–G; *proposing* an owner discharges nothing, and D1's own mechanism makes D the natural owner | HIGH | 1 (parent :685,:894) + 2 (A/B/G grep) |
| OB-2 | D4 + D11 | Degraded install ⇒ unstamped clock ⇒ one spurious `IdleExpired` → `ClearProfile` wipes a live session; inverts Windows' explicit "fall back to *not idle*" choice, undeclared | MED | 3 (`idle.rs:78-90`, `state.rs:296-304`) |
| OB-3 | D6 | `&& !mods.win` at the shared root violates `is_technician_chord`'s own documented invariant; a zero-change alternative already exists in-tree | MED | 3 (`shortcuts.rs:88-101,184-190`) |
| OB-4 | D3 | "Two independent legs" overclaims: both ride one GTK main loop, where Windows' two legs ride two dedicated threads — undeclared C3 divergence; and leg 3 contradicts P2-G's default recipe as written | MED | 3 (`gesture.rs:309-322`) + 2 (P2-G §2) |
| OB-5 | D1/D5 | `touch-event` and `button-press-event` both feed one `TapCounter` with no pointer-emulation guard → one physical tap can count twice | MED | 4 (`event_touch.rs:32`, `event.rs:304`) |
| OB-6 | D12 | The only gate for D1's load-bearing "always `Proceed`" rule is smoke 17, which D itself says may not run — C9 feasibility | MED | 2 (D §17, P2-G H4) + 5 |
| OB-7 | D7 | New `input_watch` install site in `main.rs` is an unlisted scope change; the two existing stubs already take exactly the right arguments and map 1:1 onto D3's two legs | LOW | 3 (`main.rs:1105-1106`) |
| OB-8 | D5 | `position()` is GdkWindow-relative, not allocation-relative; "cannot desync" is an assumption, not an identity | LOW | 4 (`event_button.rs:19`) |

---

## D1 — the coverage question, resolved (and why the withdrawal was right)

I verified the whole chain myself before attacking anything. Recording the result because
it is the load-bearing question of the round and it comes out **in the Writer's favour**.

**API surface — all four citations exact.**
`tauri-2.11.5/src/webview/webview_window.rs:1861` `pub fn gtk_window(&self) -> crate::Result<gtk::ApplicationWindow>` (linux-cfg'd, read at source).
`tauri-2.11.5/src/webview/mod.rs:173` `pub fn inner(&self) -> webkit2gtk::WebView`.
`webkit2gtk-2.0.2/src/auto/web_view.rs:58` `@extends WebViewBase, gtk::Container, gtk::Widget`.
`gtk-0.18.2/src/auto/widget.rs:62` `pub trait WidgetExt: IsA<Widget>` + `:4840` `impl<O: IsA<Widget>> WidgetExt for O {}` — so `WidgetExt` reaches `webkit2gtk::WebView` with no `webkit2gtk` import needed.
Signal lines all correct: `:2015` button-press (`&gdk::EventButton`), `:3035`/`:3068` key press/release (`&gdk::EventKey`), `:3224` motion, `:3549` scroll, `:3899` touch (`&gdk::Event`), `:473` `allocated_height`, `:494` `allocated_width`. All `-> glib::Propagation`.

**Do window key handlers fire when the webview holds focus? Yes.** Two independent
in-tree proofs, and the Writer's citation is accurate:

1. `tao-0.35.3/src/platform_impl/linux/event_loop.rs:865,872` — `window.connect_key_press_event` / `connect_key_release_event` on the `gtk::ApplicationWindow`, both returning `glib::Propagation::Proceed`, feeding tao's keyboard events *and* `IMContextSimple::filter_keypress`. That is tao's entire Linux keyboard input, in every wry/Tauri app, with a focused WebKitWebView present. If the webview starved it, Tauri Linux would have no keyboard events at all.
2. Ordering: gtk-rs connects via `connect_raw(..., b"key-press-event\0", ...)` (`widget.rs:3054-3062`) — plain `g_signal_connect_data`, **no** `G_CONNECT_AFTER — so user handlers precede the class closure on these `RUN_LAST` signals. Independently confirmed behaviourally: `wry-0.55.1/src/webkitgtk/synthetic_mouse_events.rs:44` returns `Propagation::Stop` *to prevent WebKit from handling* buttons 8/9. Returning Stop can only suppress WebKit if the user handler runs first. That is a stronger proof than the Writer offered.

**On the wry citation specifically** (the Moderator's suspicion): `synthetic_mouse_events`
is about **back/forward mouse buttons 8/9**, synthesised into JS `mouse{down,up}` events —
not touch, not general pointer observation. The Writer's phrase "wry already does exactly
the pointer half" is loose about *purpose*. It is nonetheless sound about *mechanism*, which
is all D leans on: `:10` `webview.add_events(BUTTON1_MOTION_MASK | BUTTON_PRESS_MASK)` then
`:15` `webview.connect_button_press_event(...)` on the **`webkit2gtk::WebView`**, live in
every build (`wry-0.55.1/src/webkitgtk/mod.rs:463` calls `setup(webview)` unconditionally).
Not an objection — the inference holds and is under-claimed.

**The keys-to-window / pointer-to-webview split is correct and non-obvious.** tao *also*
connects `connect_button_press_event`/`connect_touch_event`/`connect_motion_notify_event`/
`connect_scroll_event` on the **window** (`event_loop.rs:521,556,496,785`) with
`window.add_events(POINTER_MOTION|BUTTON1_MOTION|BUTTON_PRESS|TOUCH|STRUCTURE|FOCUS_CHANGE|SCROLL)`
at `:471-478`. Those are starved when the webview consumes — GTK3 pointer events go to the
widget owning the GdkWindow and propagate *up* only if unhandled — which is precisely why
wry attaches its own to the webview. D attaching pointer/touch to the WebView and keys to
the Window matches GTK3's actual asymmetry. `add_events` on the webview is therefore
required and D1 says so; on the window it is unnecessary (tao already set the masks) and D1
correctly does not ask for it.

**Was withdrawing the GDK handler an over-correction? No.** The "narrower fix (chain rather
than replace)" the Moderator floats does not exist: `gdk_event_handler_set` stores a single
function pointer with no chaining API, so "chaining" *is* calling `gtk::main_do_event`
yourself — which is exactly the withdrawn design. The withdrawal therefore has no cheaper
alternative, and the replacement is strictly better on the record: two in-tree precedents
instead of zero, no `unsafe`, no per-event `gdk_event_copy`/`gdk_event_free` on the main
thread, no process-global slot, and one UNVERIFIABLE (copy fidelity through
`gtk_main_do_event`) retired rather than pinned. **Clean pass on D1's mechanism.**

---

## OB-1 — PF-04 pinch intercept is uncovered, and D is its owner (vs D13, HIGH)

**What breaks.** Frame §2 lists the P2 row verbatim, "WebKitGTK parity (**incl. pinch-gesture
intercept** …)", and makes an uncovered P2-row item HIGH against its natural owner. D13 names
the gap but explicitly declines it: *"Flagged for the Moderator as a coverage question, not
asserted as covered."* Proposing an owner discharges nothing under frame §4.5, which requires
a **named owner** (a later sub-project, a `ponytail:` record, or a hardware-checklist row) —
"D at plan time" is none of those; it is the same spec deferring to itself.

**When.** At merge: after A–G land, no spec owns the intercept, and PF-04 ships unclosed.

**Why it matters.** I checked every sibling. `grep -in 'zoom|pinch|PF-04'` over P2-A, P2-B and
P2-G returns exactly three hits, all in B, all `set_zoom_level` (`p2b:48`, `:116`, `:231`) —
the *base-zoom* half. Parent `:685` says in terms that this is not enough: *"WebKitGTK fixed
`zoom-level` — note this fixes only base zoom, **interactive pinch is GTK-owned and needs a
gesture-controller intercept in the platform layer** / a wry patch, validated on touch hardware
in P2 (wry #544, PF-04)"*, and `:894` repeats *"P2 intercepts the GTK zoom gesture in the
platform layer"*. No spec owns the gesture-controller half. G's checklist does not pin it
either: H4 is corner-tap / `GDK_TOUCH_CANCEL` / OSK, H6 is the §7.2 escape-vector sweep
(chords, edges, dialogs, VT) — neither is pinch-zoom.

D is the natural owner on the parent's own words ("in the platform layer"), and **D1 makes it
easier, not harder** — which strengthens the case rather than weakening it. The withdrawn GDK
handler could only have intercepted pinch by *selectively not forwarding* an event, i.e. by
re-implementing dispatch policy. The replacement attaches a `gtk::GestureZoom`
(`gtk-0.18.2/src/auto/gesture_zoom.rs`) to the **same widget D1 already holds**, and claims the
sequence — which is verbatim the mechanism the parent names. D13 concedes this ("D's revised
mechanism is its natural host").

Finally, frame Q5: *"'Resolve at plan time' is legitimate for values and shims, not for whether
the mechanism works at all — the latter must be pinned by a gate."* Whether a `GestureZoom`
sequence-claim actually beats WebKit's internal pinch handling is a *does-the-mechanism-work*
question with no gate, no smoke scenario and no checklist row anywhere in A–G.

**Remedy that would close it.** Either (a) D owns it: one paragraph naming `GestureZoom` +
`set_sequence_state(Claimed)` on the webview widget, the one documented exception to D1's
`Proceed` rule, gated by a new G checklist row on touch hardware; or (b) it becomes a G
checklist row with a named implementing spec. Naming it without either is the current state
and is the defect.

**Evidence tier.** 1 (parent `:685`, `:894`, §9 P2 row) + 2 (grep over P2-A/B/G, run by me) +
4 (`gesture_zoom.rs` exists in gtk 0.18.2).

---

## OB-2 — Degraded install silently wipes a live session (vs D4 + D11, MED)

**What breaks.** D4 computes `idle_secs = (now_ms − max(loop_start_ms, LAST_INPUT_MS)) / 1000`
and justifies the `max` as covering the boot window: *"an unstamped clock reads as 'idle since
the loop started', not 'idle forever'."* The unstated other case is **permanent** unstamping.
D11 makes both installs fallible and degrades per C4 (log + continue). If both handler installs
degrade, nothing ever stamps `LAST_INPUT_MS`, so `idle_secs` grows monotonically from loop
start, `should_fire` returns true exactly once at the threshold (`idle.rs:32-34`), the FSM takes
`(Online, IdleExpired) if idle_clear → Effect::ClearProfile { full: true }`
(`kiosk-core/src/app/state.rs:296-304`), and the profile is wiped **while a user is mid-session**.
The latch then never re-arms (`secs` never drops below `threshold`), so it happens once and stays
invisible thereafter.

**When.** Any deployment where `gtk_window()` or `with_webview` returns `Err` — exactly the
condition D11 exists to handle — with `idle_clear` on and `idle_reset_seconds` set.

**Why it matters.** Windows made the opposite choice **explicitly and documented it**:
`idle.rs:78-79` — *"Falls back to 'not idle' (0) if the Win32 call fails, **rather than risking
a false idle-fire off garbage data**."* D silently inverts that convention on the same
requirement (parent §3.5 idle reset) without declaring it, which is a C3 divergence in the
direction that destroys user state, and a Q3 failure mode (the wipe itself is silent; only the
unrelated install error is logged). D4's declared C3 divergence covers *system-wide vs
per-window* scope — not this.

**Fix, one line either way.** Take the Windows convention (unstamped ⇒ not idle: seed
`LAST_INPUT_MS` at loop start *and* gate the loop on "at least one handler installed"), or keep
the current behaviour and declare it as a C3 divergence with its blast radius. The current
text does neither.

**Evidence tier.** 3 — `crates/kiosk-main/src/idle.rs:32-34,78-90`,
`crates/kiosk-core/src/app/state.rs:296-304` (verifier §2, re-checked by me).

---

## OB-3 — `!mods.win` at the shared root breaks that function's own stated invariant (vs D6, MED)

**What breaks.** `is_technician_chord`'s doc comment (`shortcuts.rs:88-98`), reviewed P1-D2c
code, states the rule the edit violates, verbatim:

> *"Deliberately checked INDEPENDENTLY of [`should_swallow`] (never folded into that table):
> matching here must not swallow the key … and `should_swallow` swallowing it would prevent
> this from ever being observed at all — **the two decisions must never be layered on the same
> key**."*

`&& !mods.win` folds a `should_swallow` rule — `shortcuts.rs:71-73`, `if mods.win { return true }`
— into `is_technician_chord`. That is the layering the function forbids, in the function that
forbids it. There is also a pinned test whose whole point is the independence:
`technician_chord_is_matched_but_never_swallowed` (`shortcuts.rs:395-402`), *"The two decisions
are deliberately independent (see `is_technician_chord`'s doc comment)."*

**Not a test-breakage claim.** I checked: every existing `is_technician_chord` test passes
`win: false` (`:388-431`), so nothing goes red. C8 is not violated — the Writer's no-op-on-Windows
analysis is correct (Vector A checks `should_swallow` first at `shortcuts.rs:184-190`; any
`mods.win` is swallowed at `:71-73` and never reaches the chord branch). This objection is C1 +
Q2 + the function's documented contract, not regression.

**Why it matters — the alternative is strictly smaller and strictly more correct.** D8 declines
to port `should_swallow` *as a swallow mechanism*; nothing stops using it as a **guard**, which
is exactly Vector A's own shape and would be a verbatim port of the Windows ordering:

```rust
if should_swallow(vk, mods) { return Proceed; }        // shortcuts.rs:184 order, verbatim
else if is_technician_chord(vk, mods) { open_pin_pad(...) }
```

Diff: zero lines in `kiosk-core`-adjacent shared pure code, zero new D2c test cases, zero
cross-platform blast radius (Q4/C8), and it restores parity for **every** swallow-listed
combination on Linux — Ctrl+P, F5, F11, Alt+F4/Tab/Esc, Menu — not just the Super case §9.4
happened to name. The Writer's version fixes one of them by editing shared reviewed code; this
one fixes all of them by editing nothing shared. On Q2 ("fewest moving parts", "existing pattern
before new code") and C1 ("decision logic stays put") the guard wins outright.

**Evidence tier.** 3 — `crates/kiosk-main/src/shortcuts.rs:66-101,184-190,388-431`, all read at
source.

---

## OB-4 — "Two independent legs" overclaims, and leg 3 conflicts with P2-G as written (vs D3, MED)

### (a) Independence is narrower than claimed — undeclared C3 divergence

D3: *"Neither install can fail the other: separate `Result`s … separate failure."* True for
**install** failure and for WebKit consuming input. False for liveness: both handlers are
dispatched by the same GTK main loop, on the same thread, in the same process. One wedged main
iteration — a long handler, a nested main loop spun out of `open_pin_pad` → `navigate` — takes
both legs at once.

Windows is structurally different, and I verified it rather than assuming: the tap leg runs on a
**dedicated OS thread with its own message pump**, `gesture.rs:309-322`
(`std::thread::Builder::new().name("gesture-mouse-hook")` + `GetMessageW` loop, with the doc at
`:286-289`: *"must never be the Tauri/WebView2 UI thread"*), and the LL keyboard hook does the
same at `shortcuts.rs:239-252`. So on Windows a wedged UI thread leaves a leg alive; on Linux it
does not.

C3 requires divergence stated in both directions. D3 states the *stricter* direction (Linux is
free of the #13919 starvation class) and omits this *looser* one. Note this is a documentation
defect, not a design defect — I am not asking for a second thread; GTK signal handlers cannot
run off the main thread and P2-A's GTK-main-thread rule (A:76-77) forbids it. One sentence
closes it.

### (b) Leg 3 is not a mechanism, and G's default recipe excludes it

D3 states a constraint on P2-G: *"the hardened image must retain exactly one administrative
route to `systemctl stop` the kiosk unit."* I checked whether G, as written, can accept it. It
currently does the opposite, in three places (`p2g:60-63,75`):

- `NAutoVTs=0`, `ReserveVT=0`, **no getty on the kiosk seat**, kernel `consoleblank=0`
- *"SSH: keyed-only if enabled; **default recipe leaves it absent**."*
- H6 is an escape-vector *sweep* — an item that hunts and closes routes, the opposite polarity.

So D's constraint is a change request that G's default recipe contradicts, with no named route,
no checklist row, and no gate. Under frame §4.5 that is an unowned deferral, not a named owner.

Additionally — and this cuts against D's own framing — parent §3.5:318-320 says the fallback is
*"a reserved `AcceleratorKeyPressed` technician chord **and/or** the §7.2 OS-lockdown escape"*.
"and/or": the chord alone satisfies "never unexitable". Leg 3 is therefore **D's own addition**,
not a parent requirement, and D has invented a cross-spec obligation that its named owner
excludes by default. Either drop it (legs 1+2 already discharge §3.5) or land it concretely as
P2-G row H9 with a named route an operator can execute and validate. As written it is neither.

**Evidence tier.** 3 (`gesture.rs:286-322`, `shortcuts.rs:239-252`) + 2 (P2-G `:60-63,75,86-99`)
+ 1 (parent `:318-320`). All read at source.

---

## OB-5 — Touch double-count: no pointer-emulation guard (vs D1/D5, MED)

**What breaks.** D1 routes **both** `connect_button_press_event` and `connect_touch_event` into
the gesture path, and D5's snippet is written once, "in the button/touch handlers", feeding one
`TapCounter`. Nothing discriminates a real button press from a button event GTK3 emulated out of
an unhandled pointer-emulating touch sequence. If WebKit does not consume the touch, one physical
corner tap increments the counter twice.

**When.** Touch hardware under cage — i.e. the target deployment — for any touch sequence WebKit's
class handler leaves unhandled.

**Why it matters.** The exit gesture then fires at ⌈N/2⌉ taps on Linux against N on Windows
(where `mouse_hook` counts one `WM_LBUTTONDOWN` per tap, `gesture.rs:244-245`). That is an
undeclared C3 divergence in the **looser** direction on a security-adjacent control, and it is
the same class of finding the Writer already accepted for `GDK_TOUCH_CANCEL` — but that one he
bounded and declared, and this one is unmentioned. Blast radius is limited by the PIN pad still
requiring a PIN, hence MED not HIGH.

**The discriminator is already in the binding, and D uses neither.** `gdk-0.18.2/src/event_touch.rs:32`
`pub fn is_emulating_pointer(&self) -> bool` and `gdk-0.18.2/src/event.rs:304`
`pub fn is_pointer_emulated(&self) -> bool` (the latter is in the verifier's own accessor table,
§7, and neither the draft nor the revision cites it). One `if` closes it.

**Falsifiable.** Show that WebKitWebViewBase's `touch-event` class handler returns `TRUE`
unconditionally under the C7 floor and emulation can never occur — then this is moot and I
withdraw it. The draft's line *"Wayland delivers real touch as touch events, not synthesized
buttons"* does not do that: it addresses **GDK-level** synthesis, not **GTK-level** emulation of
unhandled touch, and it is tier-5 in either reading.

**Evidence tier.** 4 (`event_touch.rs:32`, `event.rs:304`) + 3 (`gesture.rs:244-245`).

---

## OB-6 — D1's load-bearing invariant has no gate that is known to run (vs D12, MED)

**What breaks.** D1's entire safety argument is *"we always return `Proceed`, so the webview's
input is untouched by construction"*, and the Writer correctly identifies the one way it can
hurt the product. D12's answer is a single added assertion inside **smoke 17**. But D's own
scenario 17 is conditional — *"blocking under cage-headless **IF virtual input is available**,
else hardware-checklist"* — and its fallback owner is P2-G H4 (*"D (smoke 17 if headless virtual
input was unavailable …)"*). The verifier confirms neither `cage` nor `wlrctl` nor any wlr
virtual-input tooling is installed in this environment.

**When.** If cage-headless does not expose `zwlr_virtual_pointer`/`virtual_keyboard`, the
regression guard for the change's one product-breaking failure mode moves to a manual hardware
checklist — i.e. it does not gate the merge at all. Frame C9: *"a gate that cannot actually run
in the stated environment is a feasibility defect."*

**Why it matters.** This is the asymmetry: the tap/chord assertions *deserve* a hardware fallback
(they need real input). The `Proceed` assertion does not need input injection at all to be
pinned — it can be pinned **structurally, at compile time**, for less code than the scenario line:
route every handler through one wrapper that supplies the return value itself, so no handler
*can* return `Stop`:

```rust
fn observe<W: IsA<gtk::Widget>, E>(f: impl Fn(&E) + 'static)
    -> impl Fn(&W, &E) -> glib::Propagation { move |_, e| { f(e); glib::Propagation::Proceed } }
```

Then the invariant is enforced by the type, not by a scenario that may not run — and PF-04
(OB-1) becomes the single named, reviewed exception rather than an ad-hoc relaxation. On Q5
("pinned by a gate") and Q2 this beats the added assertion, and it costs one function.

**Not an objection to scenario 17's ownership**, which is clean — see Clean passes.

**Evidence tier.** 2 (D spec `:122-129`, P2-G `:95`) + admitted verifier §10.

---

## OB-7 — The install site is an unlisted scope change, and it discards a free fit (vs D7, LOW)

D7 turns `gesture.rs:193-194` and `shortcuts.rs:112-113` into documented no-ops, and D1 installs
"inside the Tauri setup closure" — i.e. a **new** `input_watch::install(...)` call site in
`main.rs`. D's Scope section lists Linux bodies/delegations for three modules and a new module;
it does not list a `main.rs` edit.

The two stubs already receive exactly the inputs D1 needs — `&tauri::WebviewWindow`,
`tauri::AppHandle`, `Option<EffectiveGesture>` — and are already called at
`main.rs:1105-1106`, inside the setup closure, in that order. They also map **one-to-one onto
D3's two legs**: `gesture::install` = leg 1 (taps, WebView widget), `shortcuts::install` = leg 2
(chord, GtkWindow). Using them gives D3's "separate install sites, separate `Result`s, separate
log lines" for free, with zero `main.rs` diff and zero new module. Q2 (existing pattern before
new code). The `scheme_guard`-covered-by-nav precedent D7 cites is a *genuine* no-op — a control
subsumed by another control — which is not this case: here the stub's own job is being done, just
elsewhere.

**Evidence tier.** 3 — `crates/kiosk-main/src/main.rs:1105-1106`, `gesture.rs:193-199`,
`shortcuts.rs:112-118`.

---

## OB-8 — `position()`'s frame is an assumption, stated as an identity (vs D5, LOW)

D5: *"`w`/`h` come from `allocated_width/height()` on the same widget the coords are relative
to (so they cannot desync)."* `EventButton::position()` (`gdk-0.18.2/src/event_button.rs:19`)
and `EventTouch::position()` (`event_touch.rs:21`) return the raw `event->x/y` fields, which are
relative to **the `GdkWindow` the event was delivered to**; `allocated_width/height()` are the
widget's *allocation*. Those coincide only when the widget owns a `GdkWindow` coextensive with
its allocation. Under Tauri the webview sits inside `default_vbox` inside the `ApplicationWindow`
(`tauri-2.11.5/src/webview/webview_window.rs:1874` `default_vbox()`), so with one fullscreen
child the offset is 0 and this is benign **today** — but it is an assumption about layout, not the
identity D5 asserts, and D5's bounds check (correctly replicated from `gesture.rs:254`) would
silently pass a shifted frame. One declarative line, or `allocation()`'s `x`/`y`, closes it.

**Evidence tier.** 4 (`event_button.rs:19`, `event_touch.rs:21`, `widget.rs:473,494`) + 3
(`gesture.rs:249-256`).

---

## Clean passes

**D1 — core mechanism.** See the section above. Every citation verified exact; the coverage
question resolves in the Writer's favour on two independent in-tree proofs; the keys-to-window /
pointer-to-webview split is not merely defensible but *required* by GTK3's event asymmetry, which
tao's own starved window-level pointer handlers (`event_loop.rs:496,521,556,785`) demonstrate.
**The withdrawal was necessary, not an over-correction** — no narrower fix exists, because
`gdk_event_handler_set` has no chaining API and "chain to GTK's handler" *is* the withdrawn design.

**D2 — rejections.** evdev upheld; the coordinate argument is now stronger under D1 and I agree.
`ext-idle-notify-v1` upheld as parked: I verified the C6 cost the Writer claims —
`grep '^name = "wayland' Cargo.lock` returns **zero** rows, so `wayland-client` really would be
a first-of-its-kind direct dep with no lockfile precedent. `gdk_event_handler_set` recorded as
rejected-with-evidence is the right disposition.

**D4 — mechanics.** The Moderator's suspicion does not materialise. A module-local
`static LAST_INPUT_MS: AtomicU64` reached from `input_watch` via a free `pub fn note_activity()`
has no borrow or lifetime problem: `AtomicU64` is `Sync`, the GTK main thread stores and the tokio
worker loads, `Relaxed` is correct for a lone monotonic timestamp with nothing ordered by it, and
no handle needs plumbing. `idle::run`'s signature is genuinely unchanged and `main.rs:917` is
genuinely a single non-cfg-gated call — verified at source. `idle_secs_from_ticks` (`idle.rs:44`)
has **no** cfg and its test `idle_secs_is_wrap_safe_across_the_32bit_tick_boundary`
(`idle.rs:129-139`) is unconditional; "zero diff is the correct diff" is right, and the ubuntu
`cargo test --workspace` job (`.github/workflows/ci.yml:24`) keeps compiling it. The full
concession on verifier FALSE #2 is correct. My only quarrel with D4 is OB-2, which is about the
*default value*, not the mechanism.

**D5 — geometry.** `EventButton::position()`/`EventTouch::position()` exist, non-`Option`, at the
cited lines; `allocated_width`/`allocated_height` at `widget.rs:494`/`:473`. `in_region`
(`gesture.rs:34-43`) takes exactly `(x, y, w, h, region)` in window-relative coordinates and does
**no** bounds check — so replicating `gesture.rs:254`'s `inside_window` guard is right, and
catching that hole was the correct read of verifier §11.4. Deleting `#[cfg(windows)]` from
`TAP_WINDOW_MS` (`gesture.rs:181-182`) rather than minting a second constant is the correct
C3 call, and it is a one-line diff. Dropping the `RefCell` borrow before `open_pin_pad` mirrors
`mouse_hook`'s own documented lock discipline (`gesture.rs:270-273`).

**D6 — keyval mapping** (the mapping, not the `!mods.win` edit). `gdk::keys::constants` is public
(`gdk-0.18.2/src/keys.rs:119` `pub mod constants`), `K` is at `:886` and `k` at `:952`, `Key` is
`pub struct Key(u32)` with `Deref<Target = u32>` at `:9-16` and `#[derive(…PartialEq, Eq…)]` at
`:8` — so it is structurally matchable and `match ev.keyval() { k::K | k::k => … }` compiles.
`EventKey::keyval() -> keys::Key` (`event_key.rs:22-24`) and `state() -> ModifierType`
(`:15-18`) are non-`Option`. One-key set confirmed: `shortcuts.rs:58` `const VK_K: u32 = 0x4B;`
is the sole chord constant. Closing the "enumerate the keyval set" open decision is correct.

**D8 — `should_swallow` rebuttal: it holds, and I confirm it.** I re-read the parent text rather
than taking it on the Writer's word. Parent `:693`, Windows half of the shortcut-blocking row:
*"It is **NOT a security boundary**; OS-level Assigned Access / Shell Launcher is the covering
boundary (§7.2, §12/OD-5)."* OD-5 at `:927` repeats it: *"the in-app hook is documented-unreliable
on focused WebView2 and cannot block OS-reserved chords; procurement must target these SKUs."*
The module doc restates it at `shortcuts.rs:20-27`. So `should_swallow` is defence-in-depth on
the **lockdown** side, and not porting it removes no leg of the **exit** chain. The Writer's
PARTIAL REBUT of verifier §9.1's use of it is **correct** and I concede it without reservation.
The rewritten both-directions divergence (stricter on Windows for Ctrl+P/F5/F11/Menu; looser on
Linux, with P2-B `:48` covering the Menu key and developer extras, cage covering shell chords,
§7.2/P2-G covering VT) satisfies C3. Note OB-3 offers to close the F5/F11 residue for free.

**D9 — clear gate.** Unchanged and confirmed: `state.rs:296-304` has the
`(Online, IdleExpired) if idle_clear → Effect::ClearProfile { full: true }` arm, and A `:303-305`
names P2-D as smoke 6's successor in its own words.

**D10 — dependency, and no duplicate GTK.** Verified in full:
`gtk-0.18.2/src/lib.rs:18` `pub use gdk;` and `:21` `pub use glib;`, so one crate really does
cover `EventButton`/`ModifierType`/`keys`/`Propagation`. `Cargo.lock:1101-1102` `gdk 0.18.2` and
`:1345-1346` `gtk 0.18.2` — `gtk = "0.18"` resolves to the same 0.18.2 the graph already has,
so **no second GTK binding over one C library**; no new lockfile entries, no new compile units.
`webkit2gtk` is **not** additionally required for D's use: `WidgetExt`'s blanket impl
(`widget.rs:4840`) reaches `webkit2gtk::WebView` through `IsA<gtk::Widget>` (`web_view.rs:58`)
with no `webkit2gtk` import — and P2-A declares it as a direct dep anyway (A `:70-72`,
`[target.'cfg(target_os = "linux")'.dependencies] webkit2gtk = { version = "2.0.2", features = ["v2_16"] }`).
Feasible on CI: both ubuntu jobs already `apt-get install libgtk-3-dev`
(`.github/workflows/ci.yml:16-18,49-52`), and there is no macOS job, so the cosmetic mismatch
between D10's `cfg(target_os = "linux")` dep gate and the modules' `#[cfg(not(windows))]` code
gate is inert — and it is A's established precedent besides. C6 justification meets A's own
template. **No objection.**

**D11 — error model.** Correct in kind: `gtk_window()` is `crate::Result<gtk::ApplicationWindow>`
(`webview_window.rs:1861`), `with_webview` is `Result`, degrade-per-C4 with the repo's
eprintln+telemetry shape matches `shortcuts.rs:200-206`/`gesture.rs:315-320`. Declining
`catch_unwind` on Q3 grounds (a loud abort beats a silent dead feature) is the right call and
matches the project's stated failure taxonomy. Deleting the "nothing exits, nothing unlocks"
sentence is mandatory and done. The one gap is OB-2, which is about what the *idle clock* does
when D11's degrade path fires — not about D11's shape.

**D12 — scenario 17 has a real answer, no deferral loop.** I checked the chain the Moderator
flagged: D `:127-129` says the plan-time check either lands scenario 17 in CI or moves it to the
hardware list, and P2-G `:95` row H4 names *"D (smoke 17 if headless virtual input was
unavailable; §7 keyboard row)"* — a named downstream owner, exactly as frame §4.5 requires. That
is not a loop and I do not press it. My objection (OB-6) is narrower and different: the one
assertion in 17 that does **not** need input injection is the one riding on the conditional gate.

**Verifier dispositions.** All seven FALSE, four DRIFT, four UNVERIFIABLE and nine undeclared
assumptions are answered, and the concessions are real rather than cosmetic — including the two
that cost the most (withdrawing the mechanism on FALSE #5, and "zero diff is the correct diff"
on FALSE #2). Assumption 6's refusal to run the tier-5 OSK defence, and assumption 5's Mod1
declaration with leg-1 as its residual bound, are both the right call under frame §1.6.
