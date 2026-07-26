//! Shortcut blocking (P1-D2b Task 7, spec §7): swallows the OS/browser chrome
//! shortcuts a kiosk must never expose (close tab, new window, dev tools reload,
//! task-switching), through TWO independent vectors recovered/adapted from the
//! (now-removed) P0 spike (`git show v0.1.0-p0:crates/kiosk-main/src/spike.rs`,
//! also present at `753ef41^`):
//!
//! 1. **`AcceleratorKeyPressed`** ([`install`]/`windows_impl::install_accelerator_handler`):
//!    subscribed on the `ICoreWebView2Controller`, exactly like the spike's "Vector A".
//!    Covers the chords WebView2 itself receives as keyboard input while focused
//!    (Ctrl+W/N/T/P, F5, F11, the Menu/App key).
//! 2. **`WH_KEYBOARD_LL`** (`windows_impl::install_ll_hook`): the spike's "Vector B",
//!    on its own message-pump-isolated thread. Covers the global chords the webview
//!    never sees as ordinary key events (Alt+F4, Alt+Tab, Win combos — Windows
//!    intercepts these at the shell level before they ever reach an app's window
//!    procedure, let alone WebView2's accelerator pipeline).
//!
//! **Neither vector is a security boundary — this is best-effort defense-in-depth
//! only.** Per Tauri issue #13919, `WH_KEYBOARD_LL` is dropped by Windows while
//! WebView2 has focus and the hook's callback exceeds `LowLevelHooksTimeout` (a
//! system-wide timeout, not configurable per-hook) — a slow/loaded system can
//! silently stop delivering hook callbacks to this process entirely, with no error
//! surfaced here. The covering boundary for shortcut/task-switch escape is spec
//! §7.2 (OS lockdown: Assigned Access / Shell Launcher), not this module. Never
//! claim otherwise in a comment, telemetry field, or operator-facing doc.
//!
//! Both vectors decide through the one pure, host-tested [`should_swallow`] — so
//! the accelerator handler and the low-level hook can never disagree on what counts
//! as a shortcut to swallow.

/// Modifier-key state at the moment a candidate key was pressed. A small local
/// struct (not a bitflags crate): four bools, no combinators needed beyond
/// equality, so a dependency would be pure overhead here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

// Virtual-key codes for the swallow list (spec §7's explicit list), pinned as raw
// `u32`s (not the `windows` crate's `VIRTUAL_KEY`) so this module's decision table
// stays pure and host-testable on every target, matching `hardening::classify_permission_kind`'s
// same "pin the raw constant, verify against the crate at the one real callsite" convention.
const VK_W: u32 = 0x57;
const VK_N: u32 = 0x4E;
const VK_T: u32 = 0x54;
const VK_P: u32 = 0x50;
const VK_F4: u32 = 0x73;
const VK_F5: u32 = 0x74;
const VK_F11: u32 = 0x7A;
const VK_TAB: u32 = 0x09;
const VK_ESCAPE: u32 = 0x1B;
/// The "Menu"/"Apps" key (the one between right-Alt and right-Ctrl on a full
/// keyboard) — `windows` crate's `VK_APPS`.
const VK_APPS: u32 = 0x5D;

/// The pure classification behind both swallow vectors (spec §7's explicit list):
/// Ctrl+W/N/T/P, F5, F11, the standalone Menu/App key, Alt+F4/Tab/Esc, Ctrl+Esc,
/// and ANY Win-key combo (Win alone is already handled by the OS as a chord
/// trigger, so any other key held alongside it is swallowed here too). `false`
/// for ordinary in-page typing (a plain letter, arrow keys, or any key with no
/// swallow-listed modifier combination).
pub fn should_swallow(vk: u32, mods: Modifiers) -> bool {
    // Win+anything: the OS-level "open Start/switch desktop/…" chords. Swallowing
    // every Win combo here is coarser than naming each one individually, but the
    // brief's list is explicitly "Win combos" (plural, unenumerated) — matching
    // that intent beats guessing which subset the operator meant.
    if mods.win {
        return true;
    }
    let only = |m: Modifiers, ctrl: bool, alt: bool, shift: bool| {
        m.ctrl == ctrl && m.alt == alt && m.shift == shift
    };
    match vk {
        VK_W | VK_N | VK_T | VK_P if only(mods, true, false, false) => true, // Ctrl+W/N/T/P
        VK_F5 | VK_F11 if only(mods, false, false, false) => true,           // F5, F11
        VK_APPS => true, // Menu/App key: standalone, no modifier required
        VK_F4 if only(mods, false, true, false) => true, // Alt+F4
        VK_TAB if only(mods, false, true, false) => true, // Alt+Tab
        VK_ESCAPE if only(mods, false, true, false) => true, // Alt+Esc
        VK_ESCAPE if only(mods, true, false, false) => true, // Ctrl+Esc
        _ => false,
    }
}

#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, telem: crate::telemetry::Telemetry) {
    windows_impl::install(window, telem);
}

#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow, _telem: crate::telemetry::Telemetry) {
    eprintln!("shortcuts: only implemented on Windows; nothing will be swallowed");
}

#[cfg(windows)]
mod windows_impl {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    use super::Modifiers;
    use crate::telemetry::Telemetry;

    /// High bit of `GetKeyState` set = key currently down. A synchronous, per-call
    /// Win32 query (not `GetAsyncKeyState`'s "since last call" semantics) — correct
    /// here because both call sites (the accelerator handler and the LL hook) query
    /// it fresh at the moment they observe an interesting key, not across polls.
    fn is_down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
        (unsafe { GetKeyState(vk.0 as i32) } as u16 & 0x8000) != 0
    }

    fn current_modifiers() -> Modifiers {
        Modifiers {
            ctrl: is_down(VK_CONTROL),
            alt: is_down(VK_MENU),
            shift: is_down(VK_SHIFT),
            win: is_down(VK_LWIN) || is_down(VK_RWIN),
        }
    }

    /// Vector A (P0 spike's `install_accelerator_handler`, adapted): subscribes
    /// `AcceleratorKeyPressed` on the controller and swallows anything
    /// [`super::should_swallow`] names. Unlike the spike (which checked the raw vk
    /// only, e.g. "W" without actually confirming Ctrl was held), modifier state is
    /// read via `GetKeyState` — `ICoreWebView2AcceleratorKeyPressedEventArgs` carries
    /// no modifier flags of its own (confirmed against webview2-com-sys 0.38.2
    /// bindings.rs: `KeyEventKind`/`VirtualKey`/`KeyEventLParam`/`PhysicalKeyStatus`/
    /// `Handled` are its only members — `PhysicalKeyStatus.IsMenuKeyDown` reports Alt,
    /// but nothing reports Ctrl/Shift/Win), so those three still need the same Win32
    /// query the LL hook (below) already needs.
    fn install_accelerator_handler(window: &tauri::WebviewWindow) {
        let result = window.with_webview(|platform_webview| unsafe {
            use webview2_com::AcceleratorKeyPressedEventHandler;
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2AcceleratorKeyPressedEventArgs, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
                COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN,
            };

            let controller = platform_webview.controller();
            let handler = AcceleratorKeyPressedEventHandler::create(Box::new(
                move |_controller,
                      args: Option<ICoreWebView2AcceleratorKeyPressedEventArgs>|
                      -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                    args.KeyEventKind(&mut kind)?;
                    if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                    {
                        return Ok(());
                    }
                    let mut vk: u32 = 0;
                    args.VirtualKey(&mut vk)?;
                    if super::should_swallow(vk, current_modifiers()) {
                        args.SetHandled(true)?;
                    }
                    Ok(())
                },
            ));
            // webview2-com-sys 0.38.2: `add_AcceleratorKeyPressed`'s token out-param
            // is a raw `*mut i64` — same Win32-interop shape as every other
            // `add_*`/i64-token pairing in this crate (nav.rs, hardening.rs).
            let mut token: i64 = 0;
            if let Err(e) = controller.add_AcceleratorKeyPressed(&handler, &mut token) {
                eprintln!("shortcuts: add_AcceleratorKeyPressed failed, accelerator swallow will never run: {e}");
            }
        });
        if let Err(e) = result {
            eprintln!("shortcuts: with_webview failed, accelerator swallow will never run: {e}");
        }
    }

    // ---- Vector B: WH_KEYBOARD_LL (P0 spike's `install_ll_hooks`, adapted) --------
    //
    // NOT A SECURITY BOUNDARY (see module doc): Tauri #13919 — Windows silently
    // drops delivery of this hook's callbacks while WebView2 holds keyboard focus
    // and the callback exceeds the system `LowLevelHooksTimeout`. Kept as
    // defense-in-depth for the chords the accelerator handler above never sees
    // (Alt+F4/Tab, Win combos are intercepted by the shell before WebView2's
    // accelerator pipeline runs at all). §7.2 OS lockdown (Assigned Access / Shell
    // Launcher) is the real, covering boundary.

    use std::sync::OnceLock;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
        KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    /// The telemetry handle the hook callback reaches into. Set once, before the
    /// hook is installed, by the dedicated hook thread itself — never mutated again,
    /// so a plain `OnceLock` (no `Mutex`) is enough; the `extern "system"` callback
    /// cannot capture state directly (`SetWindowsHookExW` takes a bare fn pointer,
    /// not a closure).
    static HOOK_TELEM: OnceLock<Telemetry> = OnceLock::new();

    unsafe extern "system" fn kb_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if super::should_swallow(k.vkCode, current_modifiers()) {
                return LRESULT(1); // non-zero = swallow (do not pass on)
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    /// Runs `WH_KEYBOARD_LL` on its own OS thread with its own message pump — a
    /// low-level hook's callbacks are delivered by pumping messages on the
    /// installing thread, so it must never share the Tauri/WebView2 UI thread's
    /// pump (spec §3.1 M2 / the P0 spike's own doc comment on this exact point).
    fn install_ll_hook(telem: Telemetry) {
        let _ = HOOK_TELEM.set(telem);
        let spawned = std::thread::Builder::new()
            .name("shortcuts-ll-hook".into())
            .spawn(|| unsafe {
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_hook), None, 0);
                if hook.is_err() {
                    eprintln!("shortcuts: SetWindowsHookExW(WH_KEYBOARD_LL) failed; global chords (Alt+F4/Tab, Win combos) will not be swallowed by this vector");
                    return;
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            });
        if let Err(e) = spawned {
            eprintln!("shortcuts: failed to spawn the WH_KEYBOARD_LL thread: {e}");
        }
    }

    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry) {
        install_accelerator_handler(window);
        install_ll_hook(telem);
    }
}

#[cfg(test)]
mod tests {
    use super::{should_swallow, Modifiers};

    fn mods(ctrl: bool, alt: bool, shift: bool, win: bool) -> Modifiers {
        Modifiers {
            ctrl,
            alt,
            shift,
            win,
        }
    }

    fn none() -> Modifiers {
        Modifiers::default()
    }

    // ---- swallow-list table (spec §7's explicit list) -----------------------------

    #[test]
    fn ctrl_w_is_swallowed() {
        assert!(should_swallow(0x57, mods(true, false, false, false)));
    }

    #[test]
    fn ctrl_n_is_swallowed() {
        assert!(should_swallow(0x4E, mods(true, false, false, false)));
    }

    #[test]
    fn ctrl_t_is_swallowed() {
        assert!(should_swallow(0x54, mods(true, false, false, false)));
    }

    #[test]
    fn ctrl_p_is_swallowed() {
        assert!(should_swallow(0x50, mods(true, false, false, false)));
    }

    #[test]
    fn f5_with_no_modifiers_is_swallowed() {
        assert!(should_swallow(0x74, none()));
    }

    #[test]
    fn f11_with_no_modifiers_is_swallowed() {
        assert!(should_swallow(0x7A, none()));
    }

    #[test]
    fn app_menu_key_is_swallowed_standalone() {
        assert!(should_swallow(0x5D, none()));
    }

    #[test]
    fn alt_f4_is_swallowed() {
        assert!(should_swallow(0x73, mods(false, true, false, false)));
    }

    #[test]
    fn alt_tab_is_swallowed() {
        assert!(should_swallow(0x09, mods(false, true, false, false)));
    }

    #[test]
    fn alt_esc_is_swallowed() {
        assert!(should_swallow(0x1B, mods(false, true, false, false)));
    }

    #[test]
    fn ctrl_esc_is_swallowed() {
        assert!(should_swallow(0x1B, mods(true, false, false, false)));
    }

    #[test]
    fn win_plus_anything_is_swallowed() {
        assert!(should_swallow(
            0x44, /* D */
            mods(false, false, false, true)
        ));
        assert!(should_swallow(
            0x45, /* E */
            mods(false, false, false, true)
        ));
        assert!(should_swallow(0x00, mods(false, false, false, true)));
    }

    // ---- false: ordinary keys / in-page typing -------------------------------------

    #[test]
    fn a_plain_letter_with_no_modifiers_is_never_swallowed() {
        assert!(!should_swallow(0x41 /* A */, none()));
    }

    #[test]
    fn arrow_keys_are_never_swallowed() {
        assert!(!should_swallow(0x25, none())); // VK_LEFT
        assert!(!should_swallow(0x26, none())); // VK_UP
        assert!(!should_swallow(0x27, none())); // VK_RIGHT
        assert!(!should_swallow(0x28, none())); // VK_DOWN
    }

    #[test]
    fn ctrl_w_without_ctrl_actually_held_is_not_swallowed() {
        // The exact gap the P0 spike had (checked vk only, never confirmed the
        // modifier) — pinned here so it can never regress.
        assert!(!should_swallow(0x57, none()));
    }

    #[test]
    fn f5_with_ctrl_held_is_not_the_same_shortcut() {
        assert!(!should_swallow(0x74, mods(true, false, false, false)));
    }

    #[test]
    fn plain_tab_with_no_modifiers_is_in_page_navigation_not_swallowed() {
        assert!(!should_swallow(0x09, none()));
    }

    #[test]
    fn plain_escape_with_no_modifiers_is_not_swallowed() {
        assert!(!should_swallow(0x1B, none()));
    }
}
