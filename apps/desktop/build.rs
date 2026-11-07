/// The app's own commands (`generate_handler!` in main.rs). Tauri lets a
/// remote origin, such as the daemon's loopback page the UI comes from, call
/// an app command only through a permission: this manifest generates
/// `allow-<command>` for each, which main.rs `ui_capability` grants to the
/// daemon's origin. Without it every call from the UI was refused (Update now,
/// the folder picker).
const APP_COMMANDS: &[&str] = &[
    "pick_folder",
    "set_theme",
    "set_menu_labels",
    "save_file",
    "update_support",
    "install_update",
    "cancel_update",
    "restart_app",
    "open_link",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(APP_COMMANDS)),
    )
    .expect("tauri build");
}
