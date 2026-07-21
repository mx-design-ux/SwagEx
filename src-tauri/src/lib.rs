mod certificate;
mod export;
mod protocol;
mod proxy;
mod setup;

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use proxy::{
    AppState, cancel_export, complete_certificate_setup, complete_proxy_setup, export_status,
    reset_setup, start_certificate_setup, start_proxy_setup,
};

fn export_directory(path: &Path) -> Result<&Path, String> {
    path.parent()
        .filter(|directory| directory.is_dir())
        .ok_or_else(|| "Le dossier de l’export est introuvable".to_string())
}

/// Opens the export directory in Finder without asking Finder to select or
/// control an individual file. This action is only used after a JSON capture.
#[tauri::command]
fn open_export_directory(path: String) -> Result<(), String> {
    let export_path = PathBuf::from(path);
    let directory = export_directory(&export_path)?;

    let status = Command::new("open")
        .arg(directory)
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
            open_export_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::export_directory;

    #[test]
    fn finds_the_existing_parent_of_an_export() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let export_path = temporary_directory.path().join("Compte.json");

        assert_eq!(
            export_directory(&export_path).unwrap(),
            temporary_directory.path()
        );
    }

    #[test]
    fn rejects_an_export_in_a_missing_directory() {
        let export_path = std::path::PathBuf::from("/this/path/does/not/exist/Compte.json");

        assert!(export_directory(&export_path).is_err());
    }
}
