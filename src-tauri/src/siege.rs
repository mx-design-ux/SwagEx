use chrono::{Local, NaiveDate};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const HEADQUARTER_BASE_NUMBERS: [i64; 3] = [1, 14, 27];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiegeProgress {
    pub matchup_captured: bool,
    pub attack_log_captured: bool,
    pub defense_log_captured: bool,
    pub defense_list_captured: bool,
}

impl SiegeProgress {
    pub fn is_complete(self) -> bool {
        self.matchup_captured
            && self.attack_log_captured
            && self.defense_log_captured
            && self.defense_list_captured
    }
}

#[derive(Debug, Clone)]
pub struct ExportedSiege {
    pub path: PathBuf,
    pub display_name: String,
}

#[derive(Debug, Default)]
pub struct SiegeCapture {
    wizard_id: Option<Value>,
    matchup_info: Option<Value>,
    attack_log: Option<Value>,
    defense_log: Option<Value>,
    defense_list: Option<Value>,
    exported: bool,
}

impl SiegeCapture {
    pub fn progress(&self) -> SiegeProgress {
        SiegeProgress {
            matchup_captured: self.matchup_info.is_some(),
            attack_log_captured: self.attack_log.is_some(),
            defense_log_captured: self.defense_log.is_some(),
            defense_list_captured: self.defense_list.is_some(),
        }
    }

    pub fn observe(&mut self, request: &Value, response: &Value) -> SiegeProgress {
        if let Some(wizard_id) = request.get("wizard_id") {
            self.wizard_id = Some(wizard_id.clone());
        }

        let command = response
            .get("command")
            .and_then(Value::as_str)
            .or_else(|| request.get("command").and_then(Value::as_str));

        match command {
            Some("GetGuildSiegeMatchupInfo") if response_ret_code_is_successful(response) => {
                self.matchup_info = Some(response.clone());
            }
            Some("GetGuildSiegeBattleLog") => {
                match number_field(request, "log_type")
                    .or_else(|| number_field(response, "log_type"))
                {
                    Some(1) => self.attack_log = Some(response.clone()),
                    Some(2) => self.defense_log = Some(response.clone()),
                    _ => {}
                }
            }
            Some("GetGuildSiegeBaseDefenseUnitList" | "GetGuildSiegeBaseDefenseUnitListPreset") => {
                let base_number = number_field(request, "base_number")
                    .or_else(|| number_field(response, "base_number"));
                if base_number.is_some_and(|number| HEADQUARTER_BASE_NUMBERS.contains(&number)) {
                    let mut defense_list = response.clone();
                    if let (Some(object), Some(base_number)) =
                        (defense_list.as_object_mut(), base_number)
                    {
                        object.insert("hq_base_number".into(), Value::from(base_number));
                    }
                    self.defense_list = Some(defense_list);
                }
            }
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
        if let Some(wizard_id) = &self.wizard_id {
            document.insert("wizard_id".into(), wizard_id.clone());
        }
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
        document.insert(
            "defense_list".into(),
            self.defense_list.clone().unwrap_or(Value::Null),
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
    fn captures_the_four_historical_siege_steps_and_writes_a_dedicated_json() {
        let mut capture = SiegeCapture::default();
        let common_request = serde_json::json!({ "wizard_id": 42 });

        capture.observe(
            &common_request,
            &serde_json::json!({
                "command": "GetGuildSiegeMatchupInfo",
                "ret_code": 0,
                "match_info": { "match_id": 123456 }
            }),
        );
        capture.observe(
            &serde_json::json!({ "wizard_id": 42, "log_type": 1 }),
            &serde_json::json!({ "command": "GetGuildSiegeBattleLog", "log_list": [] }),
        );
        capture.observe(
            &serde_json::json!({ "wizard_id": 42, "log_type": 2 }),
            &serde_json::json!({ "command": "GetGuildSiegeBattleLog", "log_list": [] }),
        );
        let progress = capture.observe(
            &serde_json::json!({ "wizard_id": 42, "base_number": 14 }),
            &serde_json::json!({
                "command": "GetGuildSiegeBaseDefenseUnitList",
                "defense_unit_list": []
            }),
        );
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
        assert_eq!(document["wizard_id"], 42);
        assert_eq!(document["defense_list"]["hq_base_number"], 14);
        assert_eq!(document["matchup_info"]["match_info"]["match_id"], 123456);
        assert!(
            capture
                .write_if_complete(directory.path())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn ignores_a_non_headquarters_defense_list() {
        let mut capture = SiegeCapture::default();
        let progress = capture.observe(
            &serde_json::json!({ "base_number": 5 }),
            &serde_json::json!({
                "command": "GetGuildSiegeBaseDefenseUnitListPreset"
            }),
        );

        assert!(!progress.defense_list_captured);
    }

    #[test]
    fn formats_the_siege_filename_with_the_local_capture_date() {
        let date = NaiveDate::from_ymd_opt(2026, 8, 6).unwrap();

        assert_eq!(siege_display_name(date), "siege-060826");
    }
}
