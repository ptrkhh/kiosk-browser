//! Navigation-outcome detection (P1-D2a Task 6, spec §Architecture actor-spine).
//!
//! Tauri's own `on_page_load` hook (`tauri::App::on_page_load`) only reports
//! [`tauri_runtime::webview::PageLoadEvent`], and that enum has exactly two variants —
//! `Started` and `Finished` — with **no failure variant at all** (confirmed against
//! tauri 2.11.5's `tauri-runtime` source). It cannot drive `AppEvent::NavigationFailed`.
//!
//! WebView2's native `NavigationCompleted` event *can*: its
//! `ICoreWebView2NavigationCompletedEventArgs::IsSuccess` reports exactly what
//! `on_page_load` cannot. Since it fires on every resolved navigation (success or
//! failure), one handler produces both `AppEvent::NavigationCommitted` and
//! `::NavigationFailed` — there is no separate `on_page_load` registration to keep in
//! sync with it. Reached the same way the (now-removed) P0 spike reached
//! `AcceleratorKeyPressed`: `WebviewWindow::with_webview` → the WebView2 controller →
//! `webview2-com`'s generated COM bindings. This is also exactly the mechanism the P1-D2b
//! design doc names for its own "Navigation guard" row
//! (`NavigationCompleted.IsSuccess == false -> NavigationFailed; success ->
//! NavigationCommitted`) — D2a implements only this half now, since the FSM needs it to
//! do anything at all; D2b's `NavigationStarting`/allowlist half is deliberately not
//! touched here (plan's own "do not add a nav intercept in D2a" constraint).
use kiosk_core::app::state::Event as AppEvent;
use tokio::sync::mpsc;

use crate::telemetry::Telemetry;

/// Installs the handler on `window`'s live `ICoreWebView2`, forwarding every resolved
/// navigation's outcome through `tx` and (on failure only) a `nav.error` telemetry
/// event through `telem`. Call once, right after the webview is built.
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
    windows_impl::install(window, tx, telem);
}

#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow, _tx: mpsc::Sender<AppEvent>, _telem: Telemetry) {
    eprintln!("nav: only implemented on Windows; NavigationCommitted/Failed will never fire");
}

#[cfg(windows)]
mod windows_impl {
    use kiosk_core::app::state::Event as AppEvent;
    use tokio::sync::mpsc;

    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, tx: mpsc::Sender<AppEvent>, telem: Telemetry) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2NavigationCompletedEventArgs, COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN,
            };
            use webview2_com::NavigationCompletedEventHandler;

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "nav: CoreWebView2() unavailable, navigation outcome will never be observed: {e}"
                    );
                    return;
                }
            };

            let handler = NavigationCompletedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2NavigationCompletedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let mut is_success = windows::core::BOOL(0);
                    args.IsSuccess(&mut is_success)?;
                    let event = if is_success.as_bool() {
                        AppEvent::NavigationCommitted
                    } else {
                        let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                        args.WebErrorStatus(&mut status)?;
                        // The numeric code maps to the `COREWEBVIEW2_WEB_ERROR_STATUS_*`
                        // constants in webview2-com-sys::bindings; good enough for an
                        // operator grepping Cloud Logging, not worth a friendly-name table.
                        telem.nav_error(&format!("{status:?}"));
                        AppEvent::NavigationFailed
                    };
                    // `try_send`, never a blocking/async send: this closure runs on
                    // WebView2's own COM callback thread, not a tokio worker — the
                    // driver's queue backing up must never stall the webview.
                    let _ = tx.try_send(event);
                    Ok(())
                },
            ));
            // webview2-com-sys 0.38.2: the token out-param is a raw `*mut i64` (mirrors
            // the P0 spike's `add_AcceleratorKeyPressed` — no `EventRegistrationToken`
            // type is generated for this Win32-interop signature).
            let mut token: i64 = 0;
            if let Err(e) = webview2.add_NavigationCompleted(&handler, &mut token) {
                eprintln!("nav: add_NavigationCompleted failed: {e}");
            }
        });
        if let Err(e) = result {
            eprintln!("nav: with_webview failed, navigation outcome will never be observed: {e}");
        }
    }
}
