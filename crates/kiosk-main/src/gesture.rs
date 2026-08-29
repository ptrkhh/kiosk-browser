//! Exit-gesture triggers (P1-D2c Task 4, spec §3.5/§5.2): opens the technician PIN
//! pad (Task 5 builds `pinpad.html` itself — navigating there is correct even
//! before that page exists) via TWO independent paths:
//!
//! 1. **N taps in a configured screen corner** ([`install`]'s native `WH_MOUSE_LL`
//!    hook, Windows-only) — see that module's doc comment for the P0-UNCONFIRMED
//!    caveat: webview2-com-sys 0.38.2 exposes no pointer-observation event at all
//!    (only `SendMouseInput`, for *injecting* into a composition controller), so
//!    this reuses the same low-level-hook idiom `shortcuts.rs` already established
//!    for `WH_KEYBOARD_LL`, with the identical Tauri #13919 caveat: Windows can
//!    silently stop delivering this hook's callbacks while WebView2 holds focus.
//! 2. **A reserved technician keyboard chord** (`shortcuts.rs`'s
//!    `is_technician_chord`, matched but deliberately NOT swallowed by
//!    `should_swallow`) — the guaranteed fallback for path 1's P0-unconfirmed
//!    reliability, so a locked-down device is never unexitable (spec §3.5).
//!
//! **No fail-open (cfg-12).** [`effective_gesture`] returns `None` when neither
//! remote `input.exit_gesture` nor bootstrap `[exit_gesture]` is configured — the
//! gesture is DISABLED in that case: [`open_pin_pad`] and the tap-capture hook both
//! no-op. A missing/empty `pin_hash` never reaches this module without a
//! `BootstrapExitGesture`/`ExitGesture` value to carry it, so it can never grant a
//! no-PIN exit here; PIN verification itself is Task 5's job (this module only
//! opens the pad, it never verifies anything).

use kiosk_core::config::bootstrap::BootstrapExitGesture;
use kiosk_core::config::schema::{ExitGesture, GestureRegion};
use tauri::Manager;

/// Quadrant/centre split of a `w`×`h` window, given a point already in
/// window-relative coordinates (spec §5.2). `Center` is the middle half of each
/// axis (the outer quarter on every edge is quadrant territory, not centre) — the
/// spec names five regions but does not pin exact centre-box proportions, so this
/// is a reasonable, documented choice rather than a guess left silent.
pub fn in_region(x: f64, y: f64, w: f64, h: f64, region: GestureRegion) -> bool {
    let (mid_x, mid_y) = (w / 2.0, h / 2.0);
    match region {
        GestureRegion::TopLeft => x < mid_x && y < mid_y,
        GestureRegion::TopRight => x >= mid_x && y < mid_y,
        GestureRegion::BottomLeft => x < mid_x && y >= mid_y,
        GestureRegion::BottomRight => x >= mid_x && y >= mid_y,
        GestureRegion::Center => x >= w * 0.25 && x < w * 0.75 && y >= h * 0.25 && y < h * 0.75,
    }
}

/// Rolling-window tap counter (pure, host-tested). `tap` pushes `now_ms`, retains
/// only hits within `window_ms` of `now_ms` (NOT anchored to the first tap — a slow
/// first tap must fall out of the window on its own rather than permanently
/// anchoring every later tap's countdown to it, the exact P0 bug this is pinned
/// against), and fires (returning `true`, then clearing) once enough hits remain.
#[derive(Debug, Clone)]
pub struct TapCounter {
    taps_needed: usize,
    window_ms: i64,
    hits: Vec<i64>,
}

impl TapCounter {
    pub fn new(taps_needed: u8, window_ms: i64) -> Self {
        Self {
            taps_needed: taps_needed as usize,
            window_ms,
            hits: Vec::new(),
        }
    }

    pub fn tap(&mut self, now_ms: i64) -> bool {
        self.hits.push(now_ms);
        self.hits.retain(|&t| now_ms - t <= self.window_ms);
        if self.hits.len() >= self.taps_needed {
            self.hits.clear();
            true
        } else {
            false
        }
    }
}

/// The exit gesture actually in force right now: remote `input.exit_gesture` wins,
/// else the bootstrap `[exit_gesture]`, else disabled entirely (cfg-12).
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveGesture {
    pub taps: u8,
    pub region: GestureRegion,
    pub pin_hash: String,
    pub min_len: Option<u8>,
    pub alphanumeric: bool,
}

/// Bootstrap's `[exit_gesture] region` is a free-form ini string (validated only at
/// the point of use), not the typed `GestureRegion` remote config gets — parsed
/// here with the same default (`top-left`) `bootstrap.rs`'s own parser already
/// falls back to when the key is absent, so an unrecognized value degrades to that
/// default rather than panicking or silently disabling the whole gesture.
fn parse_bootstrap_region(s: &str) -> GestureRegion {
    match s {
        "top-right" => GestureRegion::TopRight,
        "bottom-left" => GestureRegion::BottomLeft,
        "bottom-right" => GestureRegion::BottomRight,
        "center" => GestureRegion::Center,
        _ => GestureRegion::TopLeft,
    }
}

/// Remote wins, else bootstrap, else `None` (gesture disabled — cfg-12, no
/// fail-open). Never called with both `None` treated as "enabled with defaults":
/// absence of configuration IS the disabled state.
pub fn effective_gesture(
    remote: Option<&ExitGesture>,
    bootstrap: Option<&BootstrapExitGesture>,
) -> Option<EffectiveGesture> {
    if let Some(g) = remote {
        // An empty/whitespace remote pin_hash is a misconfiguration, not a reason to
        // fall through to bootstrap: cfg-12 says a source with no usable pin_hash
        // DISABLES the gesture (→ None), logged — the pad must never open with an
        // unverifiable hash. Remote-present still "wins" over bootstrap; it just wins
        // by disabling.
        if g.pin_hash.trim().is_empty() {
            eprintln!(
                "gesture: remote input.exit_gesture has an empty pin_hash (cfg-12: disabled)"
            );
            return None;
        }
        return Some(EffectiveGesture {
            taps: g.taps,
            region: g.region,
            pin_hash: g.pin_hash.clone(),
            min_len: g.min_len,
            alphanumeric: g.alphanumeric,
        });
    }
    let g = bootstrap?;
    // Bootstrap's own parser already drops the section when pin_hash is absent, but an
    // explicitly-empty value would still reach here — same cfg-12 rule applies.
    if g.pin_hash.trim().is_empty() {
        eprintln!("gesture: bootstrap [exit_gesture] has an empty pin_hash (cfg-12: disabled)");
        return None;
    }
    Some(EffectiveGesture {
        taps: g.taps,
        region: parse_bootstrap_region(&g.region),
        pin_hash: g.pin_hash.clone(),
        // Bootstrap ini has no min_len/alphanumeric knobs (spec §5.1) — Task 5's
        // verify step must treat these as "no extra PIN-shape constraint", not as
        // an accidental "reject everything".
        min_len: None,
        alphanumeric: false,
    })
}

/// Navigates the webview to the bundled PIN pad (Task 5 builds `pinpad.html`;
/// navigating there now is correct even before that page exists). Guards the
/// no-fail-open rule (cfg-12): if `gesture` is `None` the whole exit gesture is
/// disabled, so this does nothing — called from EITHER trigger path, so both are
/// covered by the one guard here rather than each needing their own copy.
pub fn open_pin_pad(app: &tauri::AppHandle, gesture: Option<&EffectiveGesture>) {
    if gesture.is_none() {
        eprintln!("gesture: exit gesture not configured (cfg-12: disabled); open_pin_pad no-op");
        return;
    }
    let Some(window) = app.get_webview_window(crate::WINDOW_LABEL) else {
        eprintln!(
            "gesture: window {:?} missing, cannot open pin pad",
            crate::WINDOW_LABEL
        );
        return;
    };
    let url = crate::bundled_url("pinpad.html");
    match url.parse() {
        Ok(parsed) => {
            if let Err(e) = window.navigate(parsed) {
                eprintln!("gesture: navigate({url}) failed: {e}");
            }
        }
        Err(e) => eprintln!("gesture: {url:?} is not a valid URL: {e}"),
    }
}

#[cfg(not(windows))]
pub(crate) fn observe<W, E>(f: impl Fn(&E) + 'static) -> impl Fn(&W, &E) -> gtk::glib::Propagation {
    move |_widget, event| {
        f(event);
        gtk::glib::Propagation::Proceed
    }
}

/// A tap must land within this many ms of the *N-th-most-recent* tap to count
/// toward the gesture (see `TapCounter`'s rolling, not first-tap-anchored, window).
/// No spec value is pinned for this; 3s is a human-tappable window for the
/// bootstrap default of 7 taps. ponytail: revisit if field feedback says operators
/// need more/less time.
const TAP_WINDOW_MS: i64 = 3000;

#[cfg(windows)]
pub fn install(
    window: &tauri::WebviewWindow,
    app: tauri::AppHandle,
    gesture: Option<EffectiveGesture>,
) {
    windows_impl::install(window, app, gesture);
}

#[cfg(not(windows))]
pub fn install(
    window: &tauri::WebviewWindow,
    app: tauri::AppHandle,
    gesture: Option<EffectiveGesture>,
) {
    linux_impl::install(window, app, gesture);
}

#[cfg(not(windows))]
mod linux_impl {
    use gtk::{gdk, prelude::*};
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{in_region, now_ms, EffectiveGesture, TapCounter, TAP_WINDOW_MS};

    fn fire_if_tap(
        state: &Rc<RefCell<Option<(TapCounter, EffectiveGesture)>>>,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Option<EffectiveGesture> {
        let fired = {
            let mut state = state.borrow_mut();
            let (counter, gesture) = state.as_mut()?;
            in_region(x, y, width, height, gesture.region) && counter.tap(now_ms())
        };
        fired.then(|| {
            state
                .borrow()
                .as_ref()
                .expect("gesture state exists")
                .1
                .clone()
        })
    }

    pub fn install(
        window: &tauri::WebviewWindow,
        app: tauri::AppHandle,
        gesture: Option<EffectiveGesture>,
    ) {
        let result = window.with_webview(move |platform_webview| {
            let webview = platform_webview.inner();
            let state =
                Rc::new(RefCell::new(gesture.map(|gesture| {
                    (TapCounter::new(gesture.taps, TAP_WINDOW_MS), gesture)
                })));
            webview.add_events(
                gdk::EventMask::BUTTON_PRESS_MASK
                    | gdk::EventMask::TOUCH_MASK
                    | gdk::EventMask::POINTER_MOTION_MASK
                    | gdk::EventMask::SCROLL_MASK,
            );

            let state_button = state.clone();
            let app_button = app.clone();
            let webview_button = webview.clone();
            webview.connect_button_press_event(super::observe(move |event: &gdk::EventButton| {
                crate::idle::note_activity();
                if event.is_pointer_emulated() || event.button() != 1 {
                    return;
                }
                let (x, y) = event.position();
                let width = webview_button.allocated_width() as f64;
                let height = webview_button.allocated_height() as f64;
                if let Some(gesture) = fire_if_tap(&state_button, x, y, width, height) {
                    super::open_pin_pad(&app_button, Some(&gesture));
                }
            }));

            let state_touch = state.clone();
            let app_touch = app.clone();
            let webview_touch = webview.clone();
            webview.connect_touch_event(super::observe(move |event: &gdk::Event| {
                crate::idle::note_activity();
                if event.is_pointer_emulated() || event.event_type() != gdk::EventType::TouchBegin {
                    return;
                }
                let Some(touch) = event.downcast_ref::<gdk::EventTouch>() else {
                    return;
                };
                let (x, y) = touch.position();
                let width = webview_touch.allocated_width() as f64;
                let height = webview_touch.allocated_height() as f64;
                if let Some(gesture) = fire_if_tap(&state_touch, x, y, width, height) {
                    super::open_pin_pad(&app_touch, Some(&gesture));
                }
            }));

            webview.connect_motion_notify_event(super::observe(|_event: &gdk::EventMotion| {
                crate::idle::note_activity();
            }));
            webview.connect_scroll_event(super::observe(|_event: &gdk::EventScroll| {
                crate::idle::note_activity();
            }));

            // PF-04: claim pinch sequences at capture phase so WebKit never turns a
            // two-finger gesture into browser zoom/navigation.
            let zoom = gtk::GestureZoom::new(&webview);
            zoom.set_propagation_phase(gtk::PropagationPhase::Capture);
            zoom.connect_scale_changed(|gesture, _scale| {
                let _ = gesture.set_state(gtk::EventSequenceState::Claimed);
            });
            std::mem::forget(zoom);
        });
        if let Err(error) = result {
            eprintln!("gesture: Linux GTK input observation install failed: {error}");
        }
    }
}

#[cfg(not(windows))]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, MSG,
        MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN,
    };

    use super::{in_region, EffectiveGesture, TapCounter, TAP_WINDOW_MS};

    struct HookState {
        app: tauri::AppHandle,
        window: tauri::WebviewWindow,
        gesture: EffectiveGesture,
        counter: TapCounter,
    }

    // `SetWindowsHookExW`'s callback must be a plain `extern "system" fn` (no
    // captures — same constraint `shortcuts.rs`'s `kb_hook` already documents), so
    // the per-tap state that DOES need to persist across calls lives in this static
    // instead of a closure. Set once by `install`, read by every hook invocation.
    static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

    fn now_ms() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    /// Vector: `WH_MOUSE_LL` (P0-UNCONFIRMED substitute — see module doc comment;
    /// webview2-com-sys 0.38.2 has no pointer-observation event to hang this on
    /// instead). Converts the hook's screen-coordinate `pt` to window-relative
    /// coordinates via the live window position/size, then routes through the same
    /// pure `in_region`/`TapCounter` this module host-tests. Same Tauri #13919
    /// caveat as `shortcuts.rs`'s `WH_KEYBOARD_LL` vector: NOT a security boundary,
    /// best-effort only, and can silently stop firing while WebView2 holds focus.
    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && wparam.0 as u32 == WM_LBUTTONDOWN {
            if let Some(state) = HOOK_STATE.get() {
                let fired = if let Ok(mut s) = state.lock() {
                    let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                    match (s.window.outer_position(), s.window.inner_size()) {
                        (Ok(pos), Ok(size)) => {
                            let x = (ms.pt.x - pos.x) as f64;
                            let y = (ms.pt.y - pos.y) as f64;
                            let (w, h) = (size.width as f64, size.height as f64);
                            let inside_window = x >= 0.0 && y >= 0.0 && x < w && y < h;
                            if inside_window && in_region(x, y, w, h, s.gesture.region) {
                                s.counter.tap(now_ms())
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                } else {
                    false
                };
                if fired {
                    // Lock already released (the `if let Ok(mut s) = ...` guard's scope
                    // ended above) before this reaches into `open_pin_pad`, which itself
                    // calls into the Tauri runtime — never call that while still holding
                    // the mutex.
                    if let Some(state) = HOOK_STATE.get() {
                        if let Ok(s) = state.lock() {
                            let app = s.app.clone();
                            let gesture = s.gesture.clone();
                            drop(s);
                            super::open_pin_pad(&app, Some(&gesture));
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Runs `WH_MOUSE_LL` on its own dedicated OS thread with its own message pump —
    /// identical idiom to `shortcuts.rs`'s `install_ll_hook`, for the identical
    /// reason (a low-level hook's callbacks are delivered by pumping messages on the
    /// installing thread, which must never be the Tauri/WebView2 UI thread).
    pub fn install(
        window: &tauri::WebviewWindow,
        app: tauri::AppHandle,
        gesture: Option<EffectiveGesture>,
    ) {
        let Some(gesture) = gesture else {
            eprintln!("gesture: exit gesture not configured (cfg-12); tap capture disabled");
            return;
        };
        let taps = gesture.taps;
        let state = HookState {
            app,
            window: window.clone(),
            gesture,
            counter: TapCounter::new(taps, TAP_WINDOW_MS),
        };
        if HOOK_STATE.set(Mutex::new(state)).is_err() {
            eprintln!("gesture: install called more than once; ignoring the second call");
            return;
        }
        let spawned = std::thread::Builder::new()
            .name("gesture-mouse-hook".into())
            .spawn(|| unsafe {
                let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0);
                if hook.is_err() {
                    eprintln!("gesture: SetWindowsHookExW(WH_MOUSE_LL) failed; tap capture will never fire (the technician chord fallback still works)");
                    return;
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            });
        if let Err(e) = spawned {
            eprintln!("gesture: failed to spawn the WH_MOUSE_LL thread: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn observe_always_proceeds_after_observing() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen_by_handler = seen.clone();
        let handler = observe(move |value: &u8| {
            seen_by_handler.set(*value == 7);
        });
        assert_eq!(handler(&(), &7), gtk::glib::Propagation::Proceed);
        assert!(seen.get());
    }

    // ---- in_region ------------------------------------------------------------

    #[test]
    fn top_left_quadrant() {
        assert!(in_region(10.0, 10.0, 1000.0, 800.0, GestureRegion::TopLeft));
        assert!(!in_region(
            900.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::TopLeft
        ));
    }

    #[test]
    fn top_right_quadrant() {
        assert!(in_region(
            900.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::TopRight
        ));
        assert!(!in_region(
            10.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::TopRight
        ));
    }

    #[test]
    fn bottom_left_quadrant() {
        assert!(in_region(
            10.0,
            700.0,
            1000.0,
            800.0,
            GestureRegion::BottomLeft
        ));
        assert!(!in_region(
            10.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::BottomLeft
        ));
    }

    #[test]
    fn bottom_right_quadrant() {
        assert!(in_region(
            900.0,
            700.0,
            1000.0,
            800.0,
            GestureRegion::BottomRight
        ));
        assert!(!in_region(
            10.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::BottomRight
        ));
    }

    #[test]
    fn center_region_is_the_middle_half_of_each_axis() {
        assert!(in_region(
            500.0,
            400.0,
            1000.0,
            800.0,
            GestureRegion::Center
        ));
        assert!(!in_region(10.0, 10.0, 1000.0, 800.0, GestureRegion::Center));
        assert!(!in_region(
            900.0,
            10.0,
            1000.0,
            800.0,
            GestureRegion::Center
        ));
    }

    // ---- TapCounter -------------------------------------------------------------

    #[test]
    fn fewer_than_needed_taps_never_fire() {
        let mut c = TapCounter::new(3, 1000);
        assert!(!c.tap(0));
        assert!(!c.tap(100));
    }

    #[test]
    fn n_taps_within_the_window_fire() {
        let mut c = TapCounter::new(3, 1000);
        assert!(!c.tap(0));
        assert!(!c.tap(100));
        assert!(c.tap(200));
    }

    #[test]
    fn firing_clears_the_counter_for_the_next_gesture() {
        let mut c = TapCounter::new(2, 1000);
        assert!(!c.tap(0));
        assert!(c.tap(1), "2nd tap within the window fires");
        assert!(
            !c.tap(2),
            "one tap right after firing must not immediately re-fire"
        );
        assert!(c.tap(3), "2nd tap of the NEXT gesture fires again");
    }

    #[test]
    fn rolling_window_not_anchored_to_first_tap() {
        // the P0 first-tap-anchor bug
        let mut c = TapCounter::new(3, 1000);
        assert!(!c.tap(0));
        assert!(!c.tap(2000)); // first tap fell out of the window
        assert!(!c.tap(2200));
        assert!(c.tap(2400), "3 taps within the rolling 1000ms window fire");
    }

    // ---- effective_gesture --------------------------------------------------------

    fn remote_gesture() -> ExitGesture {
        ExitGesture {
            taps: 5,
            region: GestureRegion::BottomRight,
            min_len: Some(4),
            alphanumeric: true,
            pin_hash: "$remote$".to_string(),
            unknown: Default::default(),
        }
    }

    fn bootstrap_gesture() -> BootstrapExitGesture {
        BootstrapExitGesture {
            pin_hash: "$bootstrap$".to_string(),
            taps: 7,
            region: "top-left".to_string(),
        }
    }

    #[test]
    fn remote_present_wins_over_bootstrap() {
        let g = effective_gesture(Some(&remote_gesture()), Some(&bootstrap_gesture())).unwrap();
        assert_eq!(g.taps, 5);
        assert_eq!(g.region, GestureRegion::BottomRight);
        assert_eq!(g.pin_hash, "$remote$");
        assert_eq!(g.min_len, Some(4));
        assert!(g.alphanumeric);
    }

    #[test]
    fn only_bootstrap_present_is_used() {
        let g = effective_gesture(None, Some(&bootstrap_gesture())).unwrap();
        assert_eq!(g.taps, 7);
        assert_eq!(g.region, GestureRegion::TopLeft);
        assert_eq!(g.pin_hash, "$bootstrap$");
        assert_eq!(g.min_len, None);
        assert!(!g.alphanumeric);
    }

    #[test]
    fn neither_present_disables_the_gesture() {
        assert!(effective_gesture(None, None).is_none());
    }

    #[test]
    fn empty_remote_pin_hash_disables_the_gesture_and_does_not_fall_through() {
        // cfg-12: a present-but-unusable pin_hash DISABLES the gesture (the pad must
        // never open with an unverifiable hash), and remote-present must not fall
        // through to bootstrap.
        let empty = ExitGesture {
            pin_hash: "  ".to_string(),
            ..remote_gesture()
        };
        assert!(effective_gesture(Some(&empty), Some(&bootstrap_gesture())).is_none());
    }

    #[test]
    fn empty_bootstrap_pin_hash_disables_the_gesture() {
        let empty = BootstrapExitGesture {
            pin_hash: "".to_string(),
            ..bootstrap_gesture()
        };
        assert!(effective_gesture(None, Some(&empty)).is_none());
    }

    // ---- open_pin_pad's no-fail-open guard ----------------------------------------
    // (the AppHandle/window-navigate path itself needs a live Tauri app, so only the
    // None-guard's early-return is host-testable here; see the report for the
    // Windows smoke steps covering the rest.)

    #[test]
    fn bootstrap_region_parses_the_kebab_case_ini_string() {
        assert_eq!(
            super::parse_bootstrap_region("top-left"),
            GestureRegion::TopLeft
        );
        assert_eq!(
            super::parse_bootstrap_region("top-right"),
            GestureRegion::TopRight
        );
        assert_eq!(
            super::parse_bootstrap_region("bottom-left"),
            GestureRegion::BottomLeft
        );
        assert_eq!(
            super::parse_bootstrap_region("bottom-right"),
            GestureRegion::BottomRight
        );
        assert_eq!(
            super::parse_bootstrap_region("center"),
            GestureRegion::Center
        );
        assert_eq!(
            super::parse_bootstrap_region("garbage"),
            GestureRegion::TopLeft,
            "unrecognized value degrades to the same default bootstrap.rs's own parser uses"
        );
    }
}
