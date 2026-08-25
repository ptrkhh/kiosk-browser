//! Bounded media-failure telemetry from the bundled offline page.

use crate::telemetry::Telemetry;

pub fn normalize_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "error" => Some("error"),
        "stalled" => Some("stalled"),
        "emptied" => Some("emptied"),
        "play_rejected" => Some("play_rejected"),
        "no_progress" => Some("no_progress"),
        "stall" => Some("stall"),
        _ => None,
    }
}

pub fn sanitize_number(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value >= 0.0)
}

#[tauri::command]
pub fn media_error(
    kind: String,
    at: f64,
    ms_since_wrap: Option<f64>,
    telem: tauri::State<Telemetry>,
) {
    let Some(kind) = normalize_kind(&kind) else {
        return;
    };
    telem.media_error(
        kind,
        sanitize_number(Some(at)),
        sanitize_number(ms_since_wrap),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_enumerated_kinds_are_accepted() {
        for kind in [
            "error",
            "stalled",
            "emptied",
            "play_rejected",
            "no_progress",
            "stall",
        ] {
            assert!(normalize_kind(kind).is_some(), "{kind}");
        }
        assert_eq!(normalize_kind("engine detail"), None);
        assert_eq!(normalize_kind(""), None);
    }

    #[test]
    fn non_finite_or_negative_numbers_become_null() {
        assert_eq!(sanitize_number(Some(f64::NAN)), None);
        assert_eq!(sanitize_number(Some(f64::INFINITY)), None);
        assert_eq!(sanitize_number(Some(-1.0)), None);
        assert_eq!(sanitize_number(Some(12.5)), Some(12.5));
    }
}
