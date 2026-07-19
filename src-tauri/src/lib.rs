mod certificate;
mod export;
mod protocol;
mod proxy;
mod setup;

use proxy::{
    AppState, cancel_export, complete_certificate_setup, complete_proxy_setup, export_status,
    reset_setup, start_certificate_setup, start_proxy_setup,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            export_status,
            start_certificate_setup,
            start_proxy_setup,
            complete_certificate_setup,
            complete_proxy_setup,
            reset_setup,
            cancel_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
