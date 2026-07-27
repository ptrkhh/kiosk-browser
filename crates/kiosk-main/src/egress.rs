//! Subresource egress containment (P1-D2b Task 4, spec §7 / SEC-10, Windows).
//!
//! `crate::nav`'s `NavigationStarting` guard covers top-level (main-frame) navigations
//! only. A loaded page can still exfiltrate data without ever navigating: an `<img
//! src=https://evil/a>`, a CSS `url(https://evil/a)`, a `fetch()`, or a beacon are all
//! subresource loads, not navigations, and none of them ever reaches `NavigationStarting`
//! (confirmed against `nav.rs`'s own doc comment on that event). This module closes that
//! hole by subscribing WebView2's `WebResourceRequested` with an ALL-contexts filter and
//! substituting a synthetic 403 for anything `NavPolicy::resource_allowed` denies.
//!
//! `WebResourceRequested`'s args have **no `Cancel`/`SetCancel`** (unlike every other
//! guard in this crate) — the only way to stop the load is to set `args.Response` to a
//! response object built via the owning `ICoreWebView2Environment::CreateWebResourceResponse`.
//! See `windows_impl::install` for the exact bindings.rs call sites.

use crate::nav_policy::SharedNavPolicy;
use crate::telemetry::Telemetry;

/// Stable, greppable `nav.blocked` reason label for this module's block class (plan-
/// defined, same convention as `scheme_guard::REASON_DOWNLOAD` — no
/// `kiosk_core::nav::BlockReason` variant exists for it, and adding one is out of scope).
const REASON_EGRESS: &str = "egress";

#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
    windows_impl::install(window, telem, nav_policy);
}

#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow, _telem: Telemetry, _nav_policy: SharedNavPolicy) {
    eprintln!("egress: only implemented on Windows; subresource egress will never be blocked");
}

#[cfg(windows)]
mod windows_impl {
    use windows::core::{Interface, HSTRING};

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2WebResourceRequestedEventArgs, ICoreWebView2_2,
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
            };
            use webview2_com::WebResourceRequestedEventHandler;

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "egress: CoreWebView2() unavailable, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };

            // `Environment()` — needed to synthesize the blocked `Response` below — lives
            // on `ICoreWebView2_2` (webview2-com-sys 0.38.2's bindings.rs:41053, inside
            // `impl ICoreWebView2_2` starting at bindings.rs:40975), not the base
            // `ICoreWebView2`, so the base interface obtained from the controller must be
            // cast up first (same idiom as `scheme_guard`'s `ICoreWebView2_18`/`_4` casts).
            let webview2_2 = match webview2.cast::<ICoreWebView2_2>() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "egress: CoreWebView2 does not implement ICoreWebView2_2, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };
            let environment = match webview2_2.Environment() {
                Ok(env) => env,
                Err(e) => {
                    eprintln!(
                        "egress: Environment() failed, subresource egress will never be blocked: {e}"
                    );
                    return;
                }
            };

            // `AddWebResourceRequestedFilter("*", COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)`
            // (bindings.rs:1708) is required before `WebResourceRequested` fires for
            // anything — an unfiltered subscription sees no events at all.
            if let Err(e) = webview2
                .AddWebResourceRequestedFilter(&HSTRING::from("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)
            {
                eprintln!(
                    "egress: AddWebResourceRequestedFilter failed, subresource egress will never be blocked: {e}"
                );
                return;
            }

            let handler = WebResourceRequestedEventHandler::create(Box::new(
                move |_sender, args: Option<ICoreWebView2WebResourceRequestedEventArgs>| -> windows::core::Result<()> {
                    let Some(args) = args else { return Ok(()) };
                    let request = args.Request()?;
                    let mut uri_pw = windows::core::PWSTR::null();
                    request.Uri(&mut uri_pw)?;
                    let uri = webview2_com::take_pwstr(uri_pw);

                    if nav_policy.load().resource_allowed(&uri) {
                        return Ok(());
                    }

                    // No `Cancel` on this args type (bindings.rs:37812-37866): the load is
                    // stopped by substituting a synthetic 403 `Response` instead of letting
                    // the real (possibly data-carrying) request go out.
                    //
                    // Telemetry-flood note: a single hostile page can issue 100+ off-list
                    // requests (every pixel of a tracking pixel farm, say). We deliberately
                    // do NOT add a second limiter here — `Telemetry::nav_blocked` already
                    // feeds the Logger's `nav.blocked` bucket, which coalesces at 20/burst
                    // then emits a suppressed-summary (shared with `nav.rs`/`scheme_guard`'s
                    // own blocked events); a second bucket would just double-count the same
                    // burst under a different key.
                    // Emit `nav.blocked{egress}` ONLY when the substitution actually
                    // took — a failed 403 substitution is fail-open (the data egressed),
                    // so logging "blocked" there would make the dashboard lie.
                    match environment.CreateWebResourceResponse(
                        None::<&windows::Win32::System::Com::IStream>,
                        403,
                        &HSTRING::from("Forbidden"),
                        &HSTRING::from(""),
                    ) {
                        Ok(response) => match args.SetResponse(&response) {
                            Ok(()) => telem.nav_blocked(super::REASON_EGRESS, &uri),
                            Err(e) => eprintln!(
                                "egress: SetResponse failed, request left un-substituted (fail-open): {e}"
                            ),
                        },
                        Err(e) => eprintln!(
                            "egress: CreateWebResourceResponse failed, request left un-substituted (fail-open): {e}"
                        ),
                    }
                    Ok(())
                },
            ));
            let mut token: i64 = 0;
            if let Err(e) = webview2.add_WebResourceRequested(&handler, &mut token) {
                eprintln!(
                    "egress: add_WebResourceRequested failed, subresource egress will never be blocked: {e}"
                );
            }
        });
        if let Err(e) = result {
            eprintln!("egress: with_webview failed, subresource egress will never be blocked: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    /// `REASON_EGRESS`'s stable label is pinned here so a future refactor cannot silently
    /// drift from the plan's literal `"egress"`.
    #[test]
    fn the_stable_egress_block_reason_label_is_pinned() {
        assert_eq!(super::REASON_EGRESS, "egress");
    }
}
