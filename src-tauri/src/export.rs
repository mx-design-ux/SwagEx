use crate::protocol::validate_profile;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct ExportedProfile {
    pub path: PathBuf,
    pub display_name: String,
}

pub fn write_profile(profile: &Value, output_directory: &Path) -> anyhow::Result<ExportedProfile> {
    validate_profile(profile)?;
    fs::create_dir_all(output_directory)?;

    let wizard_info = profile.get("wizard_info");
    let wizard_name = wizard_info
        .and_then(|info| info.get("wizard_name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Compte");
    let wizard_id = wizard_info
        .and_then(|info| info.get("wizard_id"))
        .and_then(Value::as_i64)
        .or_else(|| profile.get("wizard_id").and_then(Value::as_i64))
        .map(|id| id.to_string())
        .unwrap_or_else(|| "profil".to_string());

    let display_name = format!("{wizard_name}-{wizard_id}");
    let filename = format!("{}.json", sanitize_filename(&display_name));
    let final_path = output_directory.join(filename);
    let temporary_path = output_directory.join(format!(".swagex-{wizard_id}.tmp"));
    let pretty_json = serde_json::to_vec_pretty(profile)?;

    fs::write(&temporary_path, pretty_json)?;
    fs::rename(&temporary_path, &final_path)?;

    Ok(ExportedProfile {
        path: final_path,
        display_name,
    })
}

fn sanitize_filename(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '-',
            other if other.is_control() => '-',
            other => other,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        "Compte-profil".to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_similar_profile_json_without_mutating_it() {
        let directory = tempfile::tempdir().unwrap();
        let profile = serde_json::json!({
            "command": "HubUserLogin",
            "wizard_info": { "wizard_id": 42, "wizard_name": "A/B" },
            "building_list": [],
            "unit_list": [],
            "runes": [],
            "unknown_future_field": { "kept": true }
        });

        let exported = write_profile(&profile, directory.path()).unwrap();
        assert_eq!(exported.path.file_name().unwrap(), "A-B-42.json");
        let reread: Value = serde_json::from_slice(&fs::read(exported.path).unwrap()).unwrap();
        assert_eq!(reread, profile);
    }
}
