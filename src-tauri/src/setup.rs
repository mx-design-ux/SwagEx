use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
pub struct SetupSettings {
    pub setup_completed: bool,
}

fn settings_path(app_data_directory: &Path) -> PathBuf {
    app_data_directory.join(SETTINGS_FILE)
}

pub fn read(app_data_directory: &Path) -> anyhow::Result<SetupSettings> {
    let path = settings_path(app_data_directory);
    if !path.exists() {
        return Ok(SetupSettings::default());
    }

    let contents = fs::read(&path).with_context(|| format!("lecture de {}", path.display()))?;
    Ok(serde_json::from_slice(&contents).unwrap_or_default())
}

pub fn write(app_data_directory: &Path, settings: SetupSettings) -> anyhow::Result<()> {
    fs::create_dir_all(app_data_directory)?;
    let path = settings_path(app_data_directory);
    let temporary_path = app_data_directory.join(".swagex-settings.tmp");
    let contents = serde_json::to_vec_pretty(&settings)?;
    fs::write(&temporary_path, contents)?;
    fs::rename(&temporary_path, &path)?;
    Ok(())
}

pub fn reset(app_data_directory: &Path) -> anyhow::Result<()> {
    write(app_data_directory, SetupSettings::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_persisted_and_reset() {
        let directory = tempfile::tempdir().unwrap();
        write(
            directory.path(),
            SetupSettings {
                setup_completed: true,
            },
        )
        .unwrap();
        assert!(read(directory.path()).unwrap().setup_completed);

        reset(directory.path()).unwrap();
        assert!(!read(directory.path()).unwrap().setup_completed);
    }
}
