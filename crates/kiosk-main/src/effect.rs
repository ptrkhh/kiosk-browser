//! Pure `Effect` → page mapping (P1-D2a Task 6, spec §Architecture actor-spine). This is
//! the only part of the Tauri assembly that stays host-testable: which bundled/remote page
//! an effect shows is a plain data decision, so it lives here rather than inline in
//! `TauriSink`, where it would only be exercisable on the Windows host.
//!
//! `RefetchConfig`/`ClearProfile` are not page transitions — `TauriSink` handles them
//! directly (refetch ping / D2c no-op) without ever consulting this mapping.

use kiosk_core::app::state::Effect;

/// Which bundled or remote page an effect shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageTarget {
    /// Navigate to this remote URL (the site itself, or an ErrorPage retry).
    Remote(String),
    /// The bundled offline-video page.
    Offline,
    /// The bundled boot splash.
    Splash,
    /// The bundled error page, armed to retry after N seconds.
    Error { retry_after_seconds: u64 },
}

/// Pure: which page an effect shows, or `None` for an effect that is not a page
/// transition at all (`RefetchConfig` pings the fetch task; `ClearProfile` is D2c).
pub fn page_for(effect: &Effect) -> Option<PageTarget> {
    match effect {
        Effect::Navigate(u) => Some(PageTarget::Remote(u.clone())),
        Effect::ShowVideo => Some(PageTarget::Offline),
        Effect::ShowSplash => Some(PageTarget::Splash),
        Effect::ShowErrorPage {
            retry_after_seconds,
        } => Some(PageTarget::Error {
            retry_after_seconds: *retry_after_seconds,
        }),
        Effect::RefetchConfig | Effect::ClearProfile { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_maps_to_remote_with_the_same_url() {
        assert_eq!(
            page_for(&Effect::Navigate("https://home.test/".into())),
            Some(PageTarget::Remote("https://home.test/".into()))
        );
    }

    #[test]
    fn show_video_maps_to_offline() {
        assert_eq!(page_for(&Effect::ShowVideo), Some(PageTarget::Offline));
    }

    #[test]
    fn show_splash_maps_to_splash() {
        assert_eq!(page_for(&Effect::ShowSplash), Some(PageTarget::Splash));
    }

    #[test]
    fn show_error_page_maps_to_error_with_the_same_countdown() {
        assert_eq!(
            page_for(&Effect::ShowErrorPage {
                retry_after_seconds: 15
            }),
            Some(PageTarget::Error {
                retry_after_seconds: 15
            })
        );
    }

    #[test]
    fn refetch_config_is_not_a_page_transition() {
        assert_eq!(page_for(&Effect::RefetchConfig), None);
    }

    #[test]
    fn clear_profile_is_not_a_page_transition() {
        assert_eq!(page_for(&Effect::ClearProfile { full: true }), None);
        assert_eq!(page_for(&Effect::ClearProfile { full: false }), None);
    }
}
