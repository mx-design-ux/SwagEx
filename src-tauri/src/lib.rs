mod certificate;
mod export;
mod protocol;
mod proxy;
mod setup;

use std::{path::PathBuf, process::Command};

use proxy::{
    AppState, cancel_export, complete_certificate_setup, complete_proxy_setup, export_status,
    reset_setup, start_certificate_setup, start_proxy_setup,
};

/// Reveals an application-private export in Finder through macOS Launch
/// Services, without requiring access to the user's Downloads folder.
#[tauri::command]
fn reveal_export_in_finder(path: String) -> Result<(), String> {
    let export_path = PathBuf::from(path);
    if export_path.as_os_str().is_empty() {
        return Err("Le chemin de l’export est introuvable".to_string());
    }

    let status = Command::new("open")
        .arg("-R")
        .arg(export_path)
        .status()
        .map_err(|error| format!("Impossible d’ouvrir le Finder : {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Finder n’a pas pu ouvrir le dossier de l’export".to_string())
    }
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
            reset_setup,
            cancel_export,
            reveal_export_in_finder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::reveal_export_in_finder;

    #[test]
    fn rejects_an_empty_export_path_without_launching_finder() {
        assert!(reveal_export_in_finder(String::new()).is_err());
    }
}
