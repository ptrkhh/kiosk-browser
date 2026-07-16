//! Platform-agnostic core: config, telemetry, connectivity, navigation,
//! identity. This crate must never depend on Tauri or any per-OS API
//! (spec §4 layering rule).

pub mod app;
pub mod config;
pub mod error;
pub mod identity;
pub mod logging;
pub mod nav;
pub mod net;

/// The crate/product version, sourced from Cargo. Later plans extend this
/// with the git sha for telemetry labels (spec §6 TEL-04).
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_is_semver_from_cargo() {
        let v = app_version();
        assert_eq!(v, env!("CARGO_PKG_VERSION"));
        let parts: Vec<&str> = v.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {v}");
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
    }
}
