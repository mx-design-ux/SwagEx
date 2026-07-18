mod certificate;
mod export;
mod protocol;
mod proxy;
mod setup;

use proxy::{
    AppState, cancel_export, complete_setup, export_status, reset_setup, start_export, start_setup,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            export_status,
            start_export,
            start_setup,
            complete_setup,
            reset_setup,
            cancel_export,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
