use chrono::{Local, NaiveDate};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiegeProgress {
    pub matchup_captured: bool,
    pub attack_log_captured: bool,
    pub defense_log_captured: bool,
}

impl SiegeProgress {
    pub fn is_complete(self) -> bool {
        self.matchup_captured && self.attack_log_captured && self.defense_log_captured
    }
}

#[derive(Debug, Clone)]
pub struct ExportedSiege {
    pub path: PathBuf,
    pub display_name: String,
}

#[derive(Debug, Default)]
pub struct SiegeCapture {
    matchup_info: Option<Value>,
    attack_log: Option<Value>,
    defense_log: Option<Value>,
    exported: bool,
}

impl SiegeCapture {
    pub fn progress(&self) -> SiegeProgress {
        SiegeProgress {
            matchup_captured: self.matchup_info.is_some(),
            attack_log_captured: self.attack_log.is_some(),
            defense_log_captured: self.defense_log.is_some(),
        }
    }

    pub fn observe(&mut self, response: &Value) -> SiegeProgress {
        let command = response.get("command").and_then(Value::as_str);

        match command {
            Some("GetGuildSiegeMatchupInfo") if response_ret_code_is_successful(response) => {
                self.matchup_info = Some(response.clone());
            }
            Some("GetGuildSiegeBattleLog") => match number_field(response, "log_type") {
                Some(1) => self.attack_log = Some(response.clone()),
                Some(2) => self.defense_log = Some(response.clone()),
                _ => {}
            },
            _ => {}
        }

        self.progress()
    }

    pub fn write_if_complete(
        &mut self,
        output_directory: &Path,
    ) -> anyhow::Result<Option<ExportedSiege>> {
        if self.exported || !self.progress().is_complete() {
            return Ok(None);
        }

        let display_name = siege_display_name(Local::now().date_naive());
        let safe_name = sanitize_filename(&display_name);
        fs::create_dir_all(output_directory)?;
        let final_path = output_directory.join(format!("{safe_name}.json"));
        let temporary_path = output_directory.join(format!(".swagex-{safe_name}.tmp"));

        let mut document = Map::new();
        document.insert(
            "matchup_info".into(),
            self.matchup_info.clone().unwrap_or(Value::Null),
        );
        document.insert(
            "attack_log".into(),
            self.attack_log.clone().unwrap_or(Value::Null),
        );
        document.insert(
            "defense_log".into(),
            self.defense_log.clone().unwrap_or(Value::Null),
        );
        fs::write(
            &temporary_path,
            serde_json::to_vec_pretty(&Value::Object(document))?,
        )?;
        fs::rename(&temporary_path, &final_path)?;
        self.exported = true;

        Ok(Some(ExportedSiege {
            path: final_path,
            display_name,
        }))
    }
}

fn response_ret_code_is_successful(response: &Value) -> bool {
    number_field(response, "ret_code").is_none_or(|code| code == 0)
}

fn siege_display_name(date: NaiveDate) -> String {
    format!("siege-{}", date.format("%d%m%y"))
}

fn number_field(value: &Value, field: &str) -> Option<i64> {
    value.get(field).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
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
        "SiegeMatch".to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_the_three_siege_steps_and_writes_a_dedicated_json() {
        let mut capture = SiegeCapture::default();

        capture.observe(&serde_json::json!({
            "command": "GetGuildSiegeMatchupInfo",
            "ret_code": 0,
            "match_info": { "match_id": 123456 }
        }));
        capture.observe(&serde_json::json!({
            "command": "GetGuildSiegeBattleLog",
            "log_type": 1,
            "log_list": []
        }));
        let progress = capture.observe(&serde_json::json!({
            "command": "GetGuildSiegeBattleLog",
            "log_type": 2,
            "log_list": []
        }));
        assert!(progress.is_complete());

        let directory = tempfile::tempdir().unwrap();
        let exported = capture
            .write_if_complete(directory.path())
            .unwrap()
            .unwrap();
        let filename = exported.path.file_name().unwrap().to_string_lossy();
        assert!(filename.starts_with("siege-"));
        assert!(filename.ends_with(".json"));
        let document: Value = serde_json::from_slice(&fs::read(exported.path).unwrap()).unwrap();
        assert_eq!(document["matchup_info"]["match_info"]["match_id"], 123456);
        assert!(document["attack_log"].is_object());
        assert!(document["defense_log"].is_object());
        assert!(document.get("defense_list").is_none());
        assert!(
            capture
                .write_if_complete(directory.path())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ignores_a_defense_list_that_is_not_part_of_the_three_step_export() {
        let mut capture = SiegeCapture::default();
        let progress = capture.observe(&serde_json::json!({
            "command": "GetGuildSiegeBaseDefenseUnitListPreset"
        }));

        assert_eq!(progress, SiegeProgress::default());
    }

    #[test]
    fn formats_the_siege_filename_with_the_local_capture_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();

        assert_eq!(siege_display_name(date), "siege-060826");
    }
}
