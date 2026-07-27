fn main() {
    // The `verify_pin` command (P1-D2c Task 5) is defined in this app crate, not a
    // plugin — it needs an explicit entry here so tauri-build autogenerates the
    // `allow-verify-pin`/`deny-verify-pin` permissions the `default` capability
    // references.
    let attrs = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["verify_pin"]));
    tauri_build::try_build(attrs).expect("tauri-build failed");
}
