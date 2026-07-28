use anyhow::{Context, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

const APP_DATA_DIRECTORY: &str = "SwagEx";

/// The first public builds used a personal bundle identifier as their data
/// directory. Keep that historical value private to this one-time migration;
/// all current storage uses the neutral, product-named directory above.
const LEGACY_APP_DATA_DIRECTORY: &str = "com.jmlebret.swagex";

pub fn app_data_directory(app: &AppHandle) -> anyhow::Result<PathBuf> {
    Ok(app.path().data_dir()?.join(APP_DATA_DIRECTORY))
}

pub fn migrate_legacy_app_data(app: &AppHandle) -> anyhow::Result<()> {
    let data_root = app.path().data_dir()?;
    migrate_directory(
        &data_root.join(LEGACY_APP_DATA_DIRECTORY),
        &data_root.join(APP_DATA_DIRECTORY),
    )
}

fn migrate_directory(legacy: &Path, current: &Path) -> anyhow::Result<()> {
    if !legacy.exists() {
        return Ok(());
    }

    if !current.exists() {
        fs::rename(legacy, current).with_context(|| {
            format!(
                "migration des données SwagEx de {} vers {}",
                legacy.display(),
                current.display()
            )
        })?;
        return Ok(());
    }

    merge_without_overwrite(legacy, current)?;
    remove_empty_directory(legacy)?;
    Ok(())
}

fn merge_without_overwrite(source: &Path, destination: &Path) -> anyhow::Result<()> {
    if source.is_dir() {
        if destination.exists() && !destination.is_dir() {
            bail!(
                "La migration SwagEx a trouvé un conflit entre {} et {}.",
                source.display(),
                destination.display()
            );
        }
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            merge_without_overwrite(&entry.path(), &destination.join(entry.file_name()))?;
        }
        remove_empty_directory(source)?;
        return Ok(());
    }

    if destination.exists() {
        if destination.is_file() && fs::read(source)? == fs::read(destination)? {
            fs::remove_file(source)?;
            return Ok(());
        }
        bail!(
            "La migration SwagEx refuse d’écraser le fichier existant {}.",
            destination.display()
        );
    }

    fs::rename(source, destination)?;
    Ok(())
}

fn remove_empty_directory(path: &Path) -> anyhow::Result<()> {
    if path.is_dir() && fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_the_complete_legacy_directory() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let current = root.path().join("SwagEx");
        fs::create_dir_all(legacy.join("certificate")).unwrap();
        fs::write(legacy.join("settings.json"), b"settings").unwrap();
        fs::write(legacy.join("certificate").join("SwagEx-CA.cer"), b"ca").unwrap();

        migrate_directory(&legacy, &current).unwrap();

        assert!(!legacy.exists());
        assert_eq!(
            fs::read(current.join("settings.json")).unwrap(),
            b"settings"
        );
        assert_eq!(
            fs::read(current.join("certificate").join("SwagEx-CA.cer")).unwrap(),
            b"ca"
        );
    }

    #[test]
    fn merges_identical_files_without_overwriting_current_data() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let current = root.path().join("SwagEx");
        fs::create_dir_all(legacy.join("exports")).unwrap();
        fs::create_dir_all(current.join("exports")).unwrap();
        fs::write(legacy.join("settings.json"), b"same").unwrap();
        fs::write(current.join("settings.json"), b"same").unwrap();
        fs::write(legacy.join("exports").join("legacy.json"), b"legacy").unwrap();
        fs::write(current.join("exports").join("current.json"), b"current").unwrap();

        migrate_directory(&legacy, &current).unwrap();

        assert!(!legacy.exists());
        assert_eq!(fs::read(current.join("settings.json")).unwrap(), b"same");
        assert_eq!(
            fs::read(current.join("exports").join("legacy.json")).unwrap(),
            b"legacy"
        );
        assert_eq!(
            fs::read(current.join("exports").join("current.json")).unwrap(),
            b"current"
        );
    }

    #[test]
    fn refuses_to_overwrite_conflicting_data() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let current = root.path().join("SwagEx");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("settings.json"), b"legacy").unwrap();
        fs::write(current.join("settings.json"), b"current").unwrap();

        let error = migrate_directory(&legacy, &current).unwrap_err();

        assert!(error.to_string().contains("refuse d’écraser"));
        assert_eq!(fs::read(legacy.join("settings.json")).unwrap(), b"legacy");
        assert_eq!(fs::read(current.join("settings.json")).unwrap(), b"current");
    }
}
