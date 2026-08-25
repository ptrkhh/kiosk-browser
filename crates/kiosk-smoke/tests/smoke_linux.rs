//! Ignored Linux functional scenarios.
//!
//! The scenario assertions remain in the owning P2-A..D registers and the
//! release runner selected by KIOSK_SMOKE_DRIVER. Keeping the test bodies here
//! makes every ID visible to Cargo while refusing to turn missing compositor,
//! artifact, or fixture setup into a false pass.

use std::process::Command;

use kiosk_smoke::preflight::preflight_from_env;

fn run_scenario(id: &str, compositor: &str) -> Result<(), String> {
    let _binaries = preflight_from_env().map_err(|error| format!("environment: {error:?}"))?;
    let driver = std::env::var_os("KIOSK_SMOKE_DRIVER")
        .ok_or_else(|| "environment: KIOSK_SMOKE_DRIVER is required".to_string())?;
    let status = Command::new(driver)
        .env("KIOSK_SCENARIO", id)
        .env("KIOSK_COMPOSITOR", compositor)
        .status()
        .map_err(|error| format!("scenario {id} runner: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("scenario {id} failed with {status}"))
    }
}

macro_rules! scenario {
    ($name:ident, $id:literal, $compositor:literal) => {
        #[test]
        #[ignore = "requires release artifacts and a real Linux compositor"]
        fn $name() -> Result<(), String> {
            run_scenario($id, $compositor)
        }
    };
}

scenario!(scenario_1_boot_and_fullscreen, "1", "weston");
scenario!(scenario_2_navigation_block, "2", "weston");
scenario!(scenario_3_offline_fallback, "3", "weston");
scenario!(scenario_4_renderer_recovery, "4", "weston");
scenario!(scenario_5_iframe_scope, "5", "weston");
scenario!(scenario_6_profile_clear, "6", "weston");
scenario!(scenario_7_safe_boot, "7", "weston");
scenario!(scenario_8_linux_hardening, "8", "weston");
scenario!(scenario_9_egress_filter, "9", "weston");
scenario!(scenario_10_keyboard_and_print, "10", "weston");
scenario!(scenario_11_permissions, "11", "weston");
scenario!(scenario_12_systemd_inhibit_degrade, "12", "weston");
scenario!(scenario_13_cage_chain, "13", "cage");
scenario!(scenario_14_cage_input, "14", "cage");
scenario!(scenario_15_cage_reap, "15", "cage");
scenario!(scenario_16_idle_clear, "16", "weston");
scenario!(scenario_17_xwayland_input, "17", "weston");
