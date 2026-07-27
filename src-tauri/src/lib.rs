mod certificate;
mod export;
mod protocol;
mod proxy;
mod setup;
mod steam;

use std::path::PathBuf;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::process::Command;

use proxy::{
    AppState, cancel_export, cancel_steam_export, complete_certificate_setup, complete_proxy_setup,
    complete_windows_certificate_setup, export_status, open_windows_certificate, reset_setup,
    start_certificate_setup, start_proxy_setup, start_steam_capture,
    start_windows_certificate_setup, stop_steam_capture,
};

/// Reveals an application-private export in the platform file manager without
/// requiring access to the user's Downloads folder.
#[tauri::command]
fn reveal_export_in_file_manager(path: String) -> Result<(), String> {
    let export_path = PathBuf::from(path);
    if export_path.as_os_str().is_empty() {
        return Err("Le chemin de l’export est introuvable".to_string());
    }

    reveal_in_platform_file_manager(&export_path)
}

#[cfg(target_os = "macos")]
fn reveal_in_platform_file_manager(export_path: &PathBuf) -> Result<(), String> {
    let status = Command::new("open")
        .arg("-R")
        .arg(export_path)
        .status()
        .map_err(|error| format!("Impossible d’ouvrir le Finder : {error}"))?;

    status
        .success()
        .then_some(())
        .ok_or_else(|| "Finder n’a pas pu ouvrir le dossier de l’export".to_string())
}

#[cfg(target_os = "windows")]
fn reveal_in_platform_file_manager(export_path: &PathBuf) -> Result<(), String> {
    let status = Command::new("explorer.exe")
        .arg("/select,")
        .arg(export_path)
        .status()
        .map_err(|error| format!("Impossible d’ouvrir l’Explorateur de fichiers : {error}"))?;

    status.success().then_some(()).ok_or_else(|| {
        "L’Explorateur de fichiers n’a pas pu ouvrir le dossier de l’export".to_string()
    })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn reveal_in_platform_file_manager(_export_path: &PathBuf) -> Result<(), String> {
    Err("L’ouverture du dossier de l’export n’est pas disponible sur ce système".to_string())
}

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
            start_windows_certificate_setup,
            open_windows_certificate,
            complete_windows_certificate_setup,
            start_steam_capture,
            cancel_steam_export,
            stop_steam_capture,
            reset_setup,
            cancel_export,
            reveal_export_in_file_manager,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::reveal_export_in_file_manager;

    #[test]
    fn rejects_an_empty_export_path_without_launching_the_file_manager() {
        assert!(reveal_export_in_file_manager(String::new()).is_err());
    }
}
