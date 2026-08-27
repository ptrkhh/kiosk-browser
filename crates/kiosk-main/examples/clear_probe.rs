//! P2-A scenario 6 probe.
//!
//! This is intentionally a tiny standalone Tauri host. It includes the
//! production clear implementation so the smoke test exercises the same
//! WebsiteDataManager completion callback and waits for the real
//! ProfileCleared event instead of sleeping for an arbitrary duration.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("clear_probe requires the Linux/WebKitGTK compositor harness");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod telemetry {
    #[derive(Clone)]
    pub struct Telemetry;

    impl Telemetry {
        pub fn nav_error(&self, _reason: &str) {}
    }
}

#[cfg(target_os = "linux")]
#[path = "../src/clear.rs"]
mod clear;

#[cfg(target_os = "linux")]
fn main() {
    use kiosk_core::app::state::Event as AppEvent;
    use std::time::Duration;
    use tokio::sync::mpsc;

    let (tx, mut rx) = mpsc::channel(4);
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    tauri::Builder::default()
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "clear-probe",
                tauri::WebviewUrl::External(
                    "http://127.0.0.1:9/clear-probe"
                        .parse()
                        .expect("probe URL must parse"),
                ),
            )
            .visible(false)
            .build()?;

            clear::clear(&window, tx, telemetry::Telemetry);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let outcome = runtime.block_on(async {
                    tokio::time::timeout(Duration::from_secs(10), rx.recv()).await
                });
                match outcome {
                    Ok(Some(AppEvent::ProfileCleared)) => handle.exit(0),
                    Ok(Some(other)) => {
                        eprintln!("clear_probe: unexpected event {other:?}");
                        handle.exit(1);
                    }
                    Ok(None) => {
                        eprintln!("clear_probe: event channel closed");
                        handle.exit(1);
                    }
                    Err(_) => {
                        eprintln!("clear_probe: ProfileCleared timeout");
                        handle.exit(1);
                    }
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("clear_probe: build failed")
        .run(|_, _| {});
}
