#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod boot;
mod cli;
mod driver;
mod telemetry;

const HARDCODED_URL: &str = "https://example.com/";

fn main() {
    let args = cli::Args::parse(std::env::args());
    let url: tauri::Url = args
        .url
        .as_deref()
        .unwrap_or(HARDCODED_URL)
        .parse()
        .expect("target URL must be a valid absolute URL");

    tauri::Builder::default()
        .setup(move |app| {
            let mut builder = tauri::WebviewWindowBuilder::new(
                app,
                "kiosk",
                tauri::WebviewUrl::External(url.clone()),
            );
            builder = if args.windowed {
                builder.inner_size(1280.0, 800.0).decorations(true)
            } else {
                builder
                    .fullscreen(true)
                    .decorations(false)
                    .always_on_top(true)
                    .focused(true)
            };
            builder.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to start kiosk-main");
}
