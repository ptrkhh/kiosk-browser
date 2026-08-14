//! External-scheme, download, and PDF blocking (P1-D2b Task 3, spec §3.6 H2 / §7 / M4).
//!
//! `NavigationStarting` (guarded by `crate::nav`) never fires for external URI
//! schemes (`mailto:`, `tel:`, `ms-settings:`, ...) — WebView2 raises
//! `LaunchingExternalUriScheme` instead, a wholly separate event on a wholly separate
//! args type with its own `Cancel`. Downloads and (best-effort) PDF main-frame
//! responses are the other two P1 block classes that live outside the
//! `NavigationStarting` guard's reach, so they are handled here rather than bolted
//! onto `nav.rs`, which is already large.
//!
//! Every pure decision below is a LOCAL policy helper (membership / a bool), never a
//! reimplementation of `kiosk_core::nav::decide` — that matcher owns main-frame
//! navigation verdicts exclusively; this module's business is the events `decide`
//! never sees.

use crate::nav_policy::SharedNavPolicy;
use crate::telemetry::Telemetry;

/// Bare scheme (no trailing colon — the caller strips it, matching the P1-C
/// carry-forward convention `kiosk_core::nav::scheme` already uses), ASCII-case-
/// insensitive membership in the operator's `scheme_allowlist`. `allow` defaults to
/// empty, so every external scheme is blocked unless explicitly listed.
pub fn scheme_allowed(scheme: &str, allow: &[String]) -> bool {
    allow.iter().any(|entry| entry.eq_ignore_ascii_case(scheme))
}

/// Should a main-frame response with this `Content-Type` be blocked as a PDF (spec
/// M4)? Matches case-insensitively and tolerates a `; charset=...`-style parameter
/// suffix — only the media type before the first `;` is compared. `pdf_view=false`
/// (P1's default) blocks; `pdf_view=true` allows (the bundled pdf.js viewer is a
/// later phase — `true` here only means "don't block", it does not build the
/// viewer route).
///
/// ponytail: not called from any COM callsite yet — see the module doc comment's
/// PDF section for why. Host-tested below regardless, per task brief.
#[allow(dead_code)]
pub fn pdf_decision(content_type: &str, pdf_view: bool) -> bool {
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    media_type.eq_ignore_ascii_case("application/pdf") && !pdf_view
}

/// Stable, greppable `nav.blocked` reason labels for the two block classes this
/// module owns that have no `kiosk_core::nav::BlockReason` variant (out of scope to
/// add one — see task brief). `scheme_not_allowed` (the external-scheme block) DOES
/// have a variant (`BlockReason::SchemeNotAllowed`) and is reused directly instead.
const REASON_DOWNLOAD: &str = "download";
// ponytail: not read from any COM callsite yet — see the module doc comment's PDF
// section. Pinned by a test below so a future wiring can't drift from the plan's
// `"pdf"` literal.
#[allow(dead_code)]
const REASON_PDF: &str = "pdf";

#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
    windows_impl::install(window, telem, nav_policy);
}

/// No-op on Linux: scheme-allowlist enforcement rides the nav guard here — the
/// `on_navigation` handler installed in `main.rs` already calls `NavPolicy::decision_for`,
/// and `kiosk_core::nav::decide` already covers schemes, so there is nothing left for this
/// module to enforce on that front. Downloads and PDF blocking are P2-B.
#[cfg(not(windows))]
pub fn install(_window: &tauri::WebviewWindow, _telem: Telemetry, _nav_policy: SharedNavPolicy) {}

#[cfg(windows)]
mod windows_impl {
    use kiosk_core::nav::BlockReason;
    use windows::core::Interface;

    use crate::nav_policy::SharedNavPolicy;
    use crate::telemetry::Telemetry;

    pub fn install(window: &tauri::WebviewWindow, telem: Telemetry, nav_policy: SharedNavPolicy) {
        let result = window.with_webview(move |platform_webview| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2DownloadStartingEventArgs, ICoreWebView2LaunchingExternalUriSchemeEventArgs,
                ICoreWebView2_18, ICoreWebView2_4,
            };
            use webview2_com::{DownloadStartingEventHandler, LaunchingExternalUriSchemeEventHandler};

            let controller = platform_webview.controller();
            let webview2 = match controller.CoreWebView2() {
                Ok(w) => w,
                Err(e) => {
                    eprintln!(
                        "scheme_guard: CoreWebView2() unavailable, external schemes/downloads will never be blocked: {e}"
                    );
                    return;
                }
            };

            // `LaunchingExternalUriScheme` lives on `ICoreWebView2_18` (webview2-com-sys
            // 0.38.2's bindings.rs: `add_LaunchingExternalUriScheme` is declared in
            // `impl ICoreWebView2_18`, not the base `ICoreWebView2`), so the base
            // interface obtained from the controller must be cast up first.
            match webview2.cast::<ICoreWebView2_18>() {
                Ok(webview2_18) => {
                    let policy = nav_policy.clone();
                    let telem_scheme = telem.clone();
                    let handler = LaunchingExternalUriSchemeEventHandler::create(Box::new(
                        move |_sender, args: Option<ICoreWebView2LaunchingExternalUriSchemeEventArgs>| -> windows::core::Result<()> {
                            let Some(args) = args else { return Ok(()) };
                            let mut uri_pw = windows::core::PWSTR::null();
                            args.Uri(&mut uri_pw)?;
                            let uri = webview2_com::take_pwstr(uri_pw);
                            // The scheme is everything before the first `:` — bare, no
                            // colon, matching `scheme_allowed`'s own contract.
                            let scheme = uri.split(':').next().unwrap_or("");
                            if !super::scheme_allowed(scheme, policy.load().scheme_allowlist()) {
                                args.SetCancel(true)?;
                                telem_scheme.nav_blocked(BlockReason::SchemeNotAllowed.as_str(), &uri);
                            }
                            Ok(())
                        },
                    ));
                    let mut token: i64 = 0;
                    if let Err(e) = webview2_18.add_LaunchingExternalUriScheme(&handler, &mut token) {
                        eprintln!("scheme_guard: add_LaunchingExternalUriScheme failed, external schemes will never be blocked: {e}");
                    }
                }
                Err(e) => eprintln!(
                    "scheme_guard: CoreWebView2 does not implement ICoreWebView2_18, external schemes will never be blocked: {e}"
                ),
            }

            // `DownloadStarting` lives on `ICoreWebView2_4`.
            match webview2.cast::<ICoreWebView2_4>() {
                Ok(webview2_4) => {
                    let telem_dl = telem.clone();
                    let handler = DownloadStartingEventHandler::create(Box::new(
                        move |_sender, args: Option<ICoreWebView2DownloadStartingEventArgs>| -> windows::core::Result<()> {
                            let Some(args) = args else { return Ok(()) };
                            // Downloads are blocked outright in P1 — no allowlist, no
                            // per-type distinction. Best-effort source URI for the
                            // (already-redacting) telemetry call; if the download
                            // operation can't be read, still cancel and report with an
                            // empty URL rather than skip the cancel.
                            let uri = args
                                .DownloadOperation()
                                .and_then(|op| {
                                    let mut uri_pw = windows::core::PWSTR::null();
                                    op.Uri(&mut uri_pw)?;
                                    Ok(webview2_com::take_pwstr(uri_pw))
                                })
                                .unwrap_or_default();
                            args.SetCancel(true)?;
                            telem_dl.nav_blocked(super::REASON_DOWNLOAD, &uri);
                            Ok(())
                        },
                    ));
                    let mut token: i64 = 0;
                    if let Err(e) = webview2_4.add_DownloadStarting(&handler, &mut token) {
                        eprintln!("scheme_guard: add_DownloadStarting failed, downloads will never be blocked: {e}");
                    }
                }
                Err(e) => eprintln!(
                    "scheme_guard: CoreWebView2 does not implement ICoreWebView2_4, downloads will never be blocked: {e}"
                ),
            }

            // PDF (M4): NOT wired. webview2-com-sys 0.38.2 exposes no pre-render,
            // cancel-capable, main-frame content-type signal to hang this on:
            // `ContentLoading`'s args (`ICoreWebView2ContentLoadingEventArgs`) carry only
            // `IsErrorPage`/`NavigationId` — no content-type, no `Cancel`/`SetCancel`.
            // `WebResourceResponseReceived`'s args (`ICoreWebView2WebResourceResponse-
            // ReceivedEventArgs`) DO expose the response (`Response()` ->
            // `ICoreWebView2WebResourceResponseView`, which has `Headers()`), but the
            // args type has no `Cancel`/`SetCancel` member either — by the time this
            // event fires, WebView2 has already committed to rendering the response, so
            // there is nothing to cancel. Neither event can drive `pdf_decision` without
            // guessing an API this crate version doesn't generate. `pdf_decision` itself
            // is implemented and host-tested below; wiring it up needs either a newer
            // webview2-com-sys with a cancel-capable signal, or a `WebResourceRequested`
            // filter-and-substitute approach (a materially bigger change than this task's
            // scope) — left for a follow-up once one of those is confirmed available.
        });
        if let Err(e) = result {
            eprintln!("scheme_guard: with_webview failed, external schemes/downloads will never be blocked: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{pdf_decision, scheme_allowed};

    #[test]
    fn unlisted_scheme_is_blocked() {
        assert!(!scheme_allowed("mailto", &[]));
    }

    #[test]
    fn allowlisted_scheme_is_allowed() {
        assert!(scheme_allowed("tel", &["tel".to_string()]));
    }

    #[test]
    fn scheme_allowlist_membership_is_ascii_case_insensitive() {
        assert!(scheme_allowed("TEL", &["tel".to_string()]));
        assert!(scheme_allowed("tel", &["TEL".to_string()]));
    }

    #[test]
    fn pdf_content_type_blocks_when_pdf_view_is_off() {
        assert!(pdf_decision("application/pdf", false));
    }

    #[test]
    fn pdf_content_type_does_not_block_when_pdf_view_is_on() {
        assert!(!pdf_decision("application/pdf", true));
    }

    #[test]
    fn non_pdf_content_type_never_blocks() {
        assert!(!pdf_decision("text/html", false));
    }

    #[test]
    fn pdf_content_type_with_parameters_still_blocks() {
        assert!(pdf_decision("application/pdf; charset=binary", false));
    }

    #[test]
    fn pdf_content_type_match_is_ascii_case_insensitive() {
        assert!(pdf_decision("Application/PDF", false));
    }

    /// `REASON_PDF` is not wired to any COM callsite yet (see the module-level doc
    /// comment: no cancel-capable pre-render content-type signal exists in
    /// webview2-com-sys 0.38.2) but its stable label is pinned here now so a future
    /// wiring can't silently drift from the plan's `"pdf"` literal.
    #[test]
    fn the_stable_pdf_block_reason_label_is_pinned() {
        assert_eq!(super::REASON_PDF, "pdf");
    }
}
