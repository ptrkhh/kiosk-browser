//! Full-profile clear execution (P1-D2c, spec §3.5 / M5 — makes the P1-D1 `Clearing`
//! privacy gate live).
//!
//! P1-D1's FSM parks in `Clearing` on idle reset with `clear_data_on_reset` set,
//! emitting exactly one `Effect::ClearProfile{full: true}` and holding the screen
//! there until an `Event::ProfileCleared` releases it (see
//! `kiosk_core::app::state`, rule 9). Before this module, `TauriSink` treated that
//! effect as a no-op, so the gate never actually cleared anything — this module is
//! the real clear.
//!
//! **Invariant: `ProfileCleared` is sent EXACTLY ONCE per call to [`clear`], on every
//! path** — cast/call failure or success — because a kiosk stranded on the
//! `Clearing` gate is worse than a best-effort (or entirely failed) clear. Every
//! branch below sends it and then returns; none of them can fall through to a
//! second send.

use kiosk_core::app::state::Event as AppEvent;
use tokio::sync::mpsc;

use crate::telemetry::Telemetry;

#[cfg(windows)]
pub fn clear(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
    windows_impl::clear(window, tx, telem);
}

#[cfg(not(windows))]
pub fn clear(_window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, _telem: Telemetry) {
    eprintln!("clear: only implemented on Windows; profile data will never be cleared");
    // Never strand the kiosk on the Clearing gate, even on a platform with no real
    // clear implementation.
    let _ = tx.try_send(AppEvent::ProfileCleared);
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::Interface;

    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;

    use crate::telemetry::Telemetry;

    /// No dedicated `profile.clear.*` entry exists in the spec §6 taxonomy, and D2c's
    /// brief is explicit that adding one is out of scope for a single failure signal.
    /// `Telemetry::nav_error` is reused instead: it is already the generic "a WebView2
    /// operation failed, here's a short diagnostic reason" WARNING (see its doc
    /// comment in `telemetry.rs`), which is exactly this shape — reused deliberately,
    /// not confused with real navigation.
    fn report_failure(telem: &Telemetry, reason: &str) {
        eprintln!("clear: {reason}");
        telem.nav_error(reason);
    }

    pub fn clear(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
        // Cloned so the outer `with_webview`-failure branch below still has its own
        // handle after the inner closure (which needs its own `move`d copies) runs.
        let tx_outer = tx.clone();
        let telem_outer = telem.clone();
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2Profile2, ICoreWebView2_13,
                COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE,
            };
            use webview2_com::ClearBrowsingDataCompletedHandler;

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    report_failure(&telem, &format!("clear_profile: CoreWebView2() unavailable, profile not cleared: {e}"));
                    let _ = tx.try_send(AppEvent::ProfileCleared);
                    return;
                }
            };

            // `Profile()` lives on `ICoreWebView2_13` (webview2-com-sys 0.38.2's
            // bindings.rs:39882, inside `impl ICoreWebView2_13`), not the base
            // `ICoreWebView2` — same cast-up idiom as `scheme_guard`'s `ICoreWebView2_18`
            // and `egress`'s `ICoreWebView2_2`.
            let webview2_13 = match webview2.cast::<ICoreWebView2_13>() {
                Ok(w) => w,
                Err(e) => {
                    report_failure(&telem, &format!("clear_profile: CoreWebView2 does not implement ICoreWebView2_13, profile not cleared: {e}"));
                    let _ = tx.try_send(AppEvent::ProfileCleared);
                    return;
                }
            };
            let profile = match webview2_13.Profile() {
                Ok(p) => p,
                Err(e) => {
                    report_failure(&telem, &format!("clear_profile: Profile() failed, profile not cleared: {e}"));
                    let _ = tx.try_send(AppEvent::ProfileCleared);
                    return;
                }
            };

            // `ClearBrowsingData` (NOT `...Async` — confirmed against webview2-com-sys
            // 0.38.2's bindings.rs:31372; the "Async" naming in the task brief was a
            // guess, the real method is synchronous-signature-but-callback-completed,
            // like every other WebView2 COM method here) lives on `ICoreWebView2Profile2`
            // (bindings.rs:31356), not the base `ICoreWebView2Profile`.
            let profile2 = match profile.cast::<ICoreWebView2Profile2>() {
                Ok(p) => p,
                Err(e) => {
                    report_failure(&telem, &format!("clear_profile: ICoreWebView2Profile does not implement ICoreWebView2Profile2, profile not cleared: {e}"));
                    let _ = tx.try_send(AppEvent::ProfileCleared);
                    return;
                }
            };

            // `COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE` (bindings.rs:141, value
            // 16384) is the WebView2 "clear everything profile-scoped" bit — cookies,
            // DOM storage (local/session/indexedDB/webSQL/cacheStorage), autofill
            // (general + password), history, downloads history, settings and service
            // workers. That is the full clear P1-D1's `Clearing{full: true}` calls for.
            //
            // Built INLINE, per call, exactly here — never stored or subscribed once at
            // setup (as `scheme_guard`/`egress`'s handlers are for their long-lived
            // events) — because a fresh `Effect::ClearProfile` dispatch is a fresh call
            // to `clear`, and each one must send exactly one `ProfileCleared`. Storing a
            // handler across calls would either fire stale closures or require
            // unsubscribe bookkeeping this one-shot operation doesn't need.
            let telem_done = telem.clone();
            let tx_done = tx.clone();
            let handler = ClearBrowsingDataCompletedHandler::create(Box::new(
                move |result: windows::core::Result<()>| -> windows::core::Result<()> {
                    if let Err(e) = result {
                        report_failure(&telem_done, &format!("clear_profile: ClearBrowsingData completed with an error, best-effort clear only: {e}"));
                    }
                    // Success OR failure: the gate must release either way.
                    let _ = tx_done.try_send(AppEvent::ProfileCleared);
                    Ok(())
                },
            ));

            if let Err(e) = profile2.ClearBrowsingData(COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_PROFILE, &handler) {
                report_failure(&telem, &format!("clear_profile: ClearBrowsingData call failed, profile not cleared: {e}"));
                let _ = tx.try_send(AppEvent::ProfileCleared);
            }
            // else: the completion handler above owns the (exactly one) send from here.
        });
        if let Err(e) = result {
            report_failure(
                &telem_outer,
                &format!("clear_profile: with_webview failed, profile not cleared: {e}"),
            );
            let _ = tx_outer.try_send(AppEvent::ProfileCleared);
        }
    }
}
