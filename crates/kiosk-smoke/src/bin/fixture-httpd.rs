//! Small release artifact used by the Linux smoke runner.
//!
//! Keeping the fixture server in the smoke crate avoids depending on Python in the
//! Debian 12 runtime container. The parent process owns the listener and exits on
//! SIGTERM, which is all the shell driver needs for its stop/restart windows.

use kiosk_smoke::httpd::FixtureServer;
use std::path::PathBuf;
use std::time::Duration;

fn main() {
    let root = std::env::var_os("KIOSK_FIXTURE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("KIOSK_FIXTURE_ROOT is required");
            std::process::exit(2);
        });
    let port = std::env::var("KIOSK_FIXTURE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(0);
    let server = FixtureServer::start_on(&root, port).unwrap_or_else(|error| {
        eprintln!("fixture-httpd: cannot bind: {error}");
        std::process::exit(1);
    });
    println!("KIOSK_FIXTURE_PORT={}", server.port());
    eprintln!(
        "fixture-httpd: serving {} on {}",
        root.display(),
        server.port()
    );
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
