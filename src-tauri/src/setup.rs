use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct SetupSettings {
    pub certificate_setup_completed: bool,
    pub proxy_setup_completed: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct StoredSettings {
    #[serde(default)]
    certificate_setup_completed: bool,
    #[serde(default)]
    proxy_setup_completed: bool,
    #[serde(default)]
    setup_completed: bool,
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
    let stored: StoredSettings = serde_json::from_slice(&contents).unwrap_or_default();

    // Migrate the original single flag used by the prototype. A user who had
    // already completed the old setup should not be asked to repeat it.
    let legacy_completed = stored.setup_completed;
    Ok(SetupSettings {
        certificate_setup_completed: stored.certificate_setup_completed || legacy_completed,
        proxy_setup_completed: stored.proxy_setup_completed || legacy_completed,
    })
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
                certificate_setup_completed: true,
                proxy_setup_completed: true,
            },
        )
        .unwrap();
        assert_eq!(
            read(directory.path()).unwrap(),
            SetupSettings {
                certificate_setup_completed: true,
                proxy_setup_completed: true,
            }
        );

        reset(directory.path()).unwrap();
        assert_eq!(read(directory.path()).unwrap(), SetupSettings::default());
    }

    #[test]
    fn migrates_legacy_setup_flag() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join(SETTINGS_FILE), r#"{"setup_completed":true}"#).unwrap();
        assert_eq!(
            read(directory.path()).unwrap(),
            SetupSettings {
                certificate_setup_completed: true,
                proxy_setup_completed: true,
            }
        );
    }
}
