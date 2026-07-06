//! Watchdog stub. The real launcher (spawn/supervise/heartbeat/safe-mode,
//! spec §3.1) is implemented in a later plan; this exists so the workspace,
//! packaging, and CI shapes are final from P0.

fn main() {
    println!("kiosk-launcher {}", kiosk_core::app_version());
}
