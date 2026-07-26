//! WebView2 settings flags + script-dialog and permission policy (P1-D2b Task 5,
//! spec §7 M3/M9, Windows).
//!
//! Three independent things live here, all reached the same `with_webview` way as
//! `nav`/`scheme_guard`/`egress`:
//!
//! 1. **Settings flags** (`apply_settings`): a batch of `ICoreWebView2Settings*`/
//!    `ICoreWebView2Controller*` setters that turn off browser chrome the kiosk never
//!    wants (context menu, devtools, zoom, autofill/password-save, default script
//!    dialogs). Best-effort like every other guard in this crate: a failed cast or
//!    setter logs and moves on rather than blocking boot.
//! 2. **Script dialogs** (`ScriptDialogOpening`, spec M3): `beforeunload` is always
//!    auto-accepted (never surfaced); `alert`/`confirm`/`prompt` are rate-limited by a
//!    coarse per-window counter. **This is defense-in-depth, not a security
//!    boundary** — a compromised page cannot exfiltrate anything through a script
//!    dialog; the goal is only to stop a hostile/broken page from wedging the kiosk
//!    behind an unbounded stack of native dialogs.
//! 3. **Permissions** (`PermissionRequested`, spec M9): every request is mapped to a
//!    local [`crate::nav_policy::PermissionKind`] and decided by
//!    [`crate::nav_policy::permission_allowed`] against the LIVE `NavPolicy` — default-
//!    deny, same store Task 1 already built (no second `ArcSwap` cell).
//!
//! ## The `SetAreDefaultScriptDialogsEnabled(false)` ↔ `ScriptDialogOpening` interaction
//!
//! `ICoreWebView2Settings::AreDefaultScriptDialogsEnabled` controls only whether
//! WebView2 shows its OWN built-in dialog chrome when a script dialog resolves with no
//! host intervention; per the WebView2 API contract, `ScriptDialogOpening` is raised
//! for every `alert`/`confirm`/`prompt`/`beforeunload` regardless of that flag — it is
//! the mechanism BY WHICH a host app replaces or suppresses the built-in UI, not
//! something the flag turns off. So disabling default dialogs here does not stop this
//! module's handler from running; it removes the fallback UI that would otherwise
//! appear for any dialog our handler declines to accept/take a deferral on. Net effect
//! with both in force: no native dialog chrome ever paints on this kiosk. This module's
//! `ScriptDialogOpening` handler takes no deferral on any path, so every dialog resolves
//! synchronously as an implicit dismiss (`confirm`->false, `prompt`->null,
//! `beforeunload`->leave-the-page) — belt (flag off) and suspenders (handler always
//! resolves synchronously) agree. The parts of the brief's Step 3 that assumed a visible
//! dialog to suppress or be lenient about are moot while the flag stays false; see the
//! per-branch comment and task-5-report.md "Descoped (with reason)".

use crate::nav_policy::{PermissionKind, SharedNavPolicy};

#[cfg(windows)]
pub fn apply(window: &tauri::WebviewWindow, nav_policy: SharedNavPolicy) {
    windows_impl::apply(window, nav_policy);
}

#[cfg(not(windows))]
pub fn apply(_window: &tauri::WebviewWindow, _nav_policy: SharedNavPolicy) {
    eprintln!(
        "hardening: only implemented on Windows; settings flags/script-dialog/permission policy will never apply"
    );
}

/// Maps a WebView2 `COREWEBVIEW2_PERMISSION_KIND` (via its raw `i32`, so this stays
/// callable from both the pure test below and the real cfg(windows) binding without
/// duplicating the match) onto the local, host-testable [`PermissionKind`] — every
/// value this crate does not explicitly recognize (autoplay, file-read-write,
/// local-fonts, MIDI-sysex, window-management, or a genuinely unknown future value)
/// falls into `Other`, which `permission_allowed` always denies.
fn classify_permission_kind(raw: i32) -> PermissionKind {
    // Values pinned against webview2-com-sys 0.38.2's `COREWEBVIEW2_PERMISSION_KIND_*`
    // constants (bindings.rs:571-597) rather than depending on the `windows`-generated
    // type outside `cfg(windows)`, so this classifier — and its test below — compiles
    // and runs on every host, not just Windows.
    match raw {
        1 => PermissionKind::Microphone,
        2 => PermissionKind::Camera,
        3 => PermissionKind::Geolocation,
        4 => PermissionKind::Notifications,
        6 => PermissionKind::ClipboardRead,
        _ => PermissionKind::Other,
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::cell::Cell;
    use std::rc::Rc;

    use windows::core::Interface;

    use crate::nav_policy::{permission_allowed, SharedNavPolicy};

    /// Coarse per-window budget for `alert`/`confirm`/`prompt` before this module starts
    /// auto-dismissing them instead of letting WebView2's (already-disabled, see the
    /// module doc) default chrome resolve them. Not a security control (spec M3 is
    /// belt-and-suspenders here, not a boundary) — just a backstop against a
    /// broken/hostile page trying to wedge the kiosk behind an unbounded dialog stack.
    const SCRIPT_DIALOG_BUDGET: u32 = 20;

    pub fn apply(window: &tauri::WebviewWindow, nav_policy: SharedNavPolicy) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2PermissionRequestedEventArgs, ICoreWebView2ScriptDialogOpeningEventArgs,
                ICoreWebView2Settings4, ICoreWebView2Settings5, COREWEBVIEW2_PERMISSION_STATE_ALLOW,
                COREWEBVIEW2_PERMISSION_STATE_DENY, COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD,
            };
            use webview2_com::{PermissionRequestedEventHandler, ScriptDialogOpeningEventHandler};

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "hardening: CoreWebView2() unavailable, settings/script-dialog/permission policy will never apply: {e}"
                    );
                    return;
                }
            };

            // ---- Step 2: settings flags -------------------------------------------
            //
            // Every setter below is confirmed against webview2-com-sys 0.38.2's
            // bindings.rs; see the module doc for the interaction with
            // `ScriptDialogOpening`.
            match webview2.Settings() {
                Ok(settings) => {
                    // Base `ICoreWebView2Settings` (bindings.rs:34885-35223, all four
                    // setters declared directly in that `impl` block).
                    if let Err(e) = settings.SetAreDefaultContextMenusEnabled(false) {
                        eprintln!("hardening: SetAreDefaultContextMenusEnabled failed: {e}");
                    }
                    if let Err(e) = settings.SetAreDevToolsEnabled(false) {
                        eprintln!("hardening: SetAreDevToolsEnabled failed: {e}");
                    }
                    if let Err(e) = settings.SetIsZoomControlEnabled(false) {
                        eprintln!("hardening: SetIsZoomControlEnabled failed: {e}");
                    }
                    if let Err(e) = settings.SetAreDefaultScriptDialogsEnabled(false) {
                        eprintln!("hardening: SetAreDefaultScriptDialogsEnabled failed: {e}");
                    }

                    // `ICoreWebView2Settings4` (bindings.rs:35792-35864): password
                    // autosave + general (form-field) autofill. 0.38.2 exposes no
                    // separate "password autofill" setter distinct from autosave —
                    // `SetIsPasswordAutofillEnabled` does not exist anywhere in this
                    // bindings.rs (grepped: zero matches); only
                    // `SetIsPasswordAutosaveEnabled` (here) and the identically-named
                    // pair on `ICoreWebView2Profile6` (a profile-scoped, not
                    // per-webview, surface this module does not reach). Reported as a
                    // brief-requested-but-absent flag rather than guessed.
                    match webview2.cast::<ICoreWebView2Settings4>() {
                        Ok(settings4) => {
                            if let Err(e) = settings4.SetIsPasswordAutosaveEnabled(false) {
                                eprintln!("hardening: SetIsPasswordAutosaveEnabled failed: {e}");
                            }
                            if let Err(e) = settings4.SetIsGeneralAutofillEnabled(false) {
                                eprintln!("hardening: SetIsGeneralAutofillEnabled failed: {e}");
                            }
                        }
                        Err(e) => eprintln!(
                            "hardening: CoreWebView2Settings does not implement Settings4, autofill/password-autosave will stay on: {e}"
                        ),
                    }

                    // `ICoreWebView2Settings5` (bindings.rs:35969-36009): pinch zoom.
                    // The brief cites WebView2Feedback #459 for "pinch zoom lives on
                    // the CONTROLLER, not Settings" — that described an OLDER SDK
                    // surface. In webview2-com-sys 0.38.2, `SetIsPinchZoomEnabled` is
                    // declared only in `impl ICoreWebView2Settings5` (no such setter
                    // exists anywhere on `ICoreWebView2Controller*` — grepped: zero
                    // matches). Verified against bindings.rs rather than the brief's
                    // (superseded) note; used here as actually generated.
                    match webview2.cast::<ICoreWebView2Settings5>() {
                        Ok(settings5) => {
                            if let Err(e) = settings5.SetIsPinchZoomEnabled(false) {
                                eprintln!("hardening: SetIsPinchZoomEnabled failed: {e}");
                            }
                        }
                        Err(e) => eprintln!(
                            "hardening: CoreWebView2Settings does not implement Settings5, pinch zoom will stay on: {e}"
                        ),
                    }
                }
                Err(e) => eprintln!(
                    "hardening: Settings() unavailable, no settings flags applied: {e}"
                ),
            }

            // ---- Step 3: script dialogs (M3) --------------------------------------
            //
            // Counts `alert`/`confirm`/`prompt` for THIS webview only; reset on every
            // `NavigationStarting` (a fresh top-level navigation is a fresh JS
            // execution context, so it gets a fresh budget). No wall clock — see the
            // ponytail note below on the ceiling this simplification accepts.
            let dialog_count: Rc<Cell<u32>> = Rc::new(Cell::new(0));
            let dialog_count_reset = dialog_count.clone();
            // Reused from `nav.rs`'s own `NavigationStarting` idiom: WebView2 allows
            // multiple independent subscribers to the same event, so adding a second,
            // narrowly-scoped one here (reset the counter only) does not interfere
            // with `nav::install`'s own cancel-capable subscription.
            let reset_handler = webview2_com::NavigationStartingEventHandler::create(Box::new(
                move |_sender, _args| -> windows::core::Result<()> {
                    dialog_count_reset.set(0);
                    Ok(())
                },
            ));
            let mut reset_token: i64 = 0;
            if let Err(e) = webview2.add_NavigationStarting(&reset_handler, &mut reset_token) {
                eprintln!(
                    "hardening: add_NavigationStarting (dialog-budget reset) failed, budget will never reset: {e}"
                );
            }

            let dialog_handler = ScriptDialogOpeningEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2ScriptDialogOpeningEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD;
                    args.Kind(&mut kind)?;

                    if kind == COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD {
                        // Never surfaced, ever. Returning without a `GetDeferral`
                        // resolves the request synchronously as an implicit dismiss —
                        // for `beforeunload` that means "leave the page" (don't block
                        // navigation on a page-authored confirmation), which is the
                        // kiosk-correct outcome.
                        return Ok(());
                    }

                    // Every `alert`/`confirm`/`prompt` also just returns here: no
                    // `Accept()`, no `GetDeferral`, so it resolves synchronously as an
                    // implicit cancel/dismiss (`confirm`->false, `prompt`->null), and
                    // Step 2's `AreDefaultScriptDialogsEnabled(false)` guarantees no
                    // native chrome ever paints. Descoped from the brief's Step 3 (all
                    // MOOT while default dialogs stay disabled — there is nothing to
                    // surface and nothing to be lenient about; re-implementing either
                    // would require re-enabling the very chrome this task disables):
                    //   * "never surface a dialog on a blocked navigation" — no dialog
                    //     is ever surfaced, blocked-nav or not.
                    //   * app-origin-vs-remote (`is_remote_origin`) leniency — there is
                    //     no leniency to grant when nothing is shown. `args.Uri()` is
                    //     therefore deliberately not read.
                    // See task-5-report.md "Descoped (with reason)".
                    //
                    // ponytail: this counter is a NO-OP today (both branches dismiss
                    // identically; it has no JS-observable effect). Kept — not deleted —
                    // as the one piece of forward-compat wiring for a possible future
                    // "default dialogs re-enabled" mode, where the over-budget branch
                    // would auto-dismiss to stop an unbounded native-dialog stack. It's
                    // a coarse budget (per-webview, reset on `NavigationStarting`, no
                    // wall clock); if that mode ever ships, upgrade to a wall-clock
                    // sliding window and add the actual dismiss/allow calls then.
                    let count = dialog_count.get() + 1;
                    dialog_count.set(count);
                    // `_over_budget` is where the future re-enabled-dialogs mode would
                    // force-dismiss; today both paths dismiss alike, so it stays unread.
                    let _over_budget = count > SCRIPT_DIALOG_BUDGET;
                    Ok(())
                },
            ));
            let mut dialog_token: i64 = 0;
            if let Err(e) = webview2.add_ScriptDialogOpening(&dialog_handler, &mut dialog_token) {
                eprintln!(
                    "hardening: add_ScriptDialogOpening failed, script dialogs will never be throttled: {e}"
                );
            }

            // ---- Step 4: permissions (M9) -----------------------------------------
            let permission_handler = PermissionRequestedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2PermissionRequestedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut raw_kind = webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PERMISSION_KIND_UNKNOWN_PERMISSION;
                    args.PermissionKind(&mut raw_kind)?;
                    let kind = super::classify_permission_kind(raw_kind.0);
                    let allowed = permission_allowed(kind, nav_policy.load().permissions());
                    args.SetState(if allowed {
                        COREWEBVIEW2_PERMISSION_STATE_ALLOW
                    } else {
                        COREWEBVIEW2_PERMISSION_STATE_DENY
                    })?;
                    Ok(())
                },
            ));
            let mut permission_token: i64 = 0;
            if let Err(e) = webview2.add_PermissionRequested(&permission_handler, &mut permission_token) {
                eprintln!(
                    "hardening: add_PermissionRequested failed, every permission request will use WebView2's own default prompt: {e}"
                );
            }
        });
        if let Err(e) = result {
            eprintln!(
                "hardening: with_webview failed, settings/script-dialog/permission policy will never apply: {e}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiosk_core::config::schema::Permissions;

    use crate::nav_policy::permission_allowed;

    #[test]
    fn known_permission_kinds_classify_correctly() {
        assert_eq!(classify_permission_kind(2), PermissionKind::Camera);
        assert_eq!(classify_permission_kind(1), PermissionKind::Microphone);
        assert_eq!(classify_permission_kind(3), PermissionKind::Geolocation);
        assert_eq!(classify_permission_kind(4), PermissionKind::Notifications);
        assert_eq!(classify_permission_kind(6), PermissionKind::ClipboardRead);
    }

    #[test]
    fn unmapped_kinds_classify_as_other_and_are_denied() {
        // 9 == COREWEBVIEW2_PERMISSION_KIND_AUTOPLAY, 0 == _UNKNOWN_PERMISSION: neither
        // has a `Permissions` field, so both must fall into `Other` and be denied.
        assert_eq!(classify_permission_kind(9), PermissionKind::Other);
        assert_eq!(classify_permission_kind(0), PermissionKind::Other);
        assert!(!permission_allowed(
            classify_permission_kind(9),
            &Permissions {
                camera: true,
                microphone: true,
                geolocation: true,
                notifications: true,
                clipboard_read: true,
                ..Permissions::default()
            }
        ));
    }
}
