use crate::protocol::validate_profile;
use serde_json::Value;
use std::{
    cmp::Ordering,
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
    let account_name = wizard_info
        .and_then(|info| info.get("wizard_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Compte");
    let wizard_id = wizard_info
        .and_then(|info| info.get("wizard_id"))
        .and_then(Value::as_i64)
        .or_else(|| profile.get("wizard_id").and_then(Value::as_i64))
        .map(|id| id.to_string())
        .unwrap_or_else(|| "profil".to_string());

    // SWEX uses the account name from the profile verbatim and appends the id.
    // A trailing `~` is part of some account names; it must not be stripped or
    // manufactured by the exporter.
    let display_name = format!("{account_name}-{wizard_id}");
    let filename = format!("{}.json", sanitize_filename(&display_name));
    let final_path = output_directory.join(filename);
    let temporary_path = output_directory.join(format!(".swagex-{wizard_id}.tmp"));
    let mut export_profile = profile.clone();
    canonicalize_profile(&mut export_profile);
    let pretty_json = serde_json::to_vec_pretty(&export_profile)?;

    fs::write(&temporary_path, pretty_json)?;
    fs::rename(&temporary_path, &final_path)?;

    Ok(ExportedProfile {
        path: final_path,
        display_name,
    })
}

fn canonicalize_profile(profile: &mut Value) {
    let storage_id = storage_building_id(profile);

    if let Some(units) = profile.get_mut("unit_list").and_then(Value::as_array_mut) {
        for unit in &mut *units {
            object_values_to_array(unit, "runes");
            sort_array_by_fields(
                unit.get_mut("runes"),
                &[("slot_no", false), ("rune_id", false)],
            );
            sort_array_by_fields(
                unit.get_mut("artifacts"),
                &[("slot", false), ("rid", false)],
            );
            sort_array_by_fields(unit.get_mut("relics"), &[("rid", false)]);
        }

        units.sort_by(|left, right| compare_units(left, right, storage_id));
    }

    object_values_to_array(profile, "runes");
    sort_array_by_fields(
        profile.get_mut("runes"),
        &[("set_id", false), ("slot_no", false), ("rune_id", false)],
    );
    sort_array_by_fields(
        profile.get_mut("rune_craft_item_list"),
        &[("craft_type", false), ("craft_item_id", false)],
    );
    sort_array_by_fields(profile.get_mut("artifacts"), &[("rid", false)]);
    sort_array_by_fields(profile.get_mut("relics"), &[("rid", false)]);
}

fn storage_building_id(profile: &Value) -> Option<i64> {
    profile
        .get("building_list")
        .and_then(Value::as_array)
        .and_then(|buildings| {
            buildings.iter().rev().find_map(|building| {
                (number_field(building, "building_master_id") == Some(25))
                    .then(|| number_field(building, "building_id"))
                    .flatten()
            })
        })
}

fn compare_units(left: &Value, right: &Value, storage_id: Option<i64>) -> Ordering {
    let left_in_storage =
        storage_id.is_some_and(|id| number_field(left, "building_id") == Some(id));
    let right_in_storage =
        storage_id.is_some_and(|id| number_field(right, "building_id") == Some(id));

    left_in_storage
        .cmp(&right_in_storage)
        .then_with(|| compare_field(left, right, "class", true))
        .then_with(|| compare_field(left, right, "unit_level", true))
        .then_with(|| compare_field(left, right, "attribute", false))
        .then_with(|| compare_field(left, right, "unit_id", false))
}

fn object_values_to_array(value: &mut Value, field: &str) {
    let Some(object) = value.get_mut(field).and_then(Value::as_object_mut) else {
        return;
    };

    let values = object.values().cloned().collect();
    value[field] = Value::Array(values);
}

fn sort_array_by_fields(value: Option<&mut Value>, fields: &[(&str, bool)]) {
    let Some(array) = value.and_then(Value::as_array_mut) else {
        return;
    };

    array.sort_by(|left, right| {
        fields
            .iter()
            .fold(Ordering::Equal, |ordering, (field, descending)| {
                ordering.then_with(|| compare_field(left, right, field, *descending))
            })
    });
}

fn compare_field(left: &Value, right: &Value, field: &str, descending: bool) -> Ordering {
    match (number_field(left, field), number_field(right, field)) {
        (Some(left), Some(right)) => {
            if descending {
                right.cmp(&left)
            } else {
                left.cmp(&right)
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
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
        "Compte-profil".to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn profile_with_resources(reverse: bool) -> Value {
        let mut units = vec![
            serde_json::json!({
                "unit_id": 30,
                "class": 5,
                "unit_level": 40,
                "attribute": 2,
                "building_id": 1,
                "runes": [
                    { "rune_id": 302, "slot_no": 2 },
                    { "rune_id": 301, "slot_no": 1 }
                ],
                "artifacts": [
                    { "rid": 302, "slot": 1 },
                    { "rid": 301, "slot": 0 }
                ],
                "relics": [
                    { "rid": 402 },
                    { "rid": 401 }
                ],
                "skills": [[30, 2], [30, 1]]
            }),
            serde_json::json!({
                "unit_id": 10,
                "class": 6,
                "unit_level": 40,
                "attribute": 1,
                "building_id": 900,
                "runes": [
                    { "rune_id": 102, "slot_no": 2 },
                    { "rune_id": 101, "slot_no": 1 }
                ],
                "artifacts": [],
                "relics": []
            }),
            serde_json::json!({
                "unit_id": 20,
                "class": 6,
                "unit_level": 35,
                "attribute": 1,
                "building_id": 1,
                "runes": [],
                "artifacts": [],
                "relics": []
            }),
            serde_json::json!({
                "unit_id": 11,
                "class": 6,
                "unit_level": 40,
                "attribute": 1,
                "building_id": 1,
                "runes": [],
                "artifacts": [],
                "relics": []
            }),
        ];
        let mut runes = vec![
            serde_json::json!({ "rune_id": 3, "set_id": 2, "slot_no": 1 }),
            serde_json::json!({ "rune_id": 1, "set_id": 1, "slot_no": 2 }),
            serde_json::json!({ "rune_id": 2, "set_id": 1, "slot_no": 1 }),
        ];
        let mut crafts = vec![
            serde_json::json!({ "craft_item_id": 2, "craft_type": 1 }),
            serde_json::json!({ "craft_item_id": 1, "craft_type": 1 }),
        ];
        let mut artifacts = vec![
            serde_json::json!({ "rid": 5 }),
            serde_json::json!({ "rid": 3 }),
        ];
        let mut relics = vec![
            serde_json::json!({ "rid": 8 }),
            serde_json::json!({ "rid": 6 }),
        ];

        if reverse {
            units.reverse();
            runes.reverse();
            crafts.reverse();
            artifacts.reverse();
            relics.reverse();
        }

        serde_json::json!({
            "command": "HubUserLogin",
            "wizard_info": { "wizard_id": 42, "wizard_name": "Test" },
            "building_list": [{ "building_master_id": 25, "building_id": 900 }],
            "unit_list": units,
            "runes": runes,
            "rune_craft_item_list": crafts,
            "artifacts": artifacts,
            "relics": relics,
            "deck_list": [{ "unit_id": 2 }, { "unit_id": 1 }],
            "guildsiege_defense_deck_list": [{ "unit_id": 4 }, { "unit_id": 3 }],
            "sec_eff": [[2, 20], [1, 10]],
            "sec_effects": [{ "id": 2 }, { "id": 1 }]
        })
    }

    fn ids(value: &Value, key: &str) -> BTreeSet<i64> {
        value
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| number_field(entry, key))
            .collect()
    }

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

    #[test]
    fn preserves_the_account_name_when_it_ends_with_a_tilde() {
        let directory = tempfile::tempdir().unwrap();
        let profile = serde_json::json!({
            "command": "HubUserLogin",
            "wizard_info": { "wizard_id": 65581, "wizard_name": "Berserk~" },
            "building_list": [],
            "unit_list": [],
            "runes": []
        });

        let exported = write_profile(&profile, directory.path()).unwrap();
        assert_eq!(exported.path.file_name().unwrap(), "Berserk~-65581.json");
    }

    #[test]
    fn does_not_add_a_tilde_to_an_account_name_that_has_none() {
        let directory = tempfile::tempdir().unwrap();
        let profile = serde_json::json!({
            "command": "HubUserLogin",
            "wizard_info": { "wizard_id": 42, "wizard_name": "Berserk" },
            "building_list": [],
            "unit_list": [],
            "runes": []
        });

        let exported = write_profile(&profile, directory.path()).unwrap();
        assert_eq!(exported.path.file_name().unwrap(), "Berserk-42.json");
    }

    #[test]
    fn canonicalizes_a_copy_without_mutating_the_input() {
        let directory = tempfile::tempdir().unwrap();
        let profile = profile_with_resources(false);
        let original = profile.clone();

        let exported = write_profile(&profile, directory.path()).unwrap();
        assert_eq!(profile, original);

        let reread: Value = serde_json::from_slice(&fs::read(exported.path).unwrap()).unwrap();
        let mut expected = original.clone();
        canonicalize_profile(&mut expected);
        assert_eq!(reread, expected);
    }

    #[test]
    fn preserves_resource_sets_and_applies_all_deterministic_orders() {
        let profile = profile_with_resources(false);
        let mut canonical = profile.clone();
        canonicalize_profile(&mut canonical);

        assert_eq!(
            ids(&profile["runes"], "rune_id"),
            ids(&canonical["runes"], "rune_id")
        );
        assert_eq!(
            ids(&profile["artifacts"], "rid"),
            ids(&canonical["artifacts"], "rid")
        );
        assert_eq!(
            ids(&profile["relics"], "rid"),
            ids(&canonical["relics"], "rid")
        );
        assert_eq!(
            ids(&profile["rune_craft_item_list"], "craft_item_id"),
            ids(&canonical["rune_craft_item_list"], "craft_item_id")
        );
        assert_eq!(
            canonical["unit_list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| unit["unit_id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![11, 20, 30, 10]
        );
        assert_eq!(
            canonical["runes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|rune| rune["rune_id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
        assert_eq!(
            canonical["rune_craft_item_list"]
                .as_array()
                .unwrap()
                .iter()
                .map(|craft| craft["craft_item_id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            canonical["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|artifact| artifact["rid"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![3, 5]
        );
        assert_eq!(
            canonical["relics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|relic| relic["rid"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![6, 8]
        );
        let canonical_unit_30 = canonical["unit_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|unit| unit["unit_id"] == 30)
            .unwrap();
        assert_eq!(
            canonical_unit_30["runes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|rune| rune["rune_id"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![301, 302]
        );
        assert_eq!(
            canonical_unit_30["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|artifact| artifact["rid"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![301, 302]
        );
        assert_eq!(
            canonical_unit_30["relics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|relic| relic["rid"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![401, 402]
        );
        assert_eq!(
            ids(&profile["unit_list"][0]["runes"], "rune_id"),
            ids(&canonical_unit_30["runes"], "rune_id")
        );
        let canonical_skill_unit = canonical["unit_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|unit| unit["unit_id"] == 30)
            .unwrap();
        let original_skill_unit = profile["unit_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|unit| unit["unit_id"] == 30)
            .unwrap();
        assert_eq!(
            canonical_skill_unit["skills"],
            original_skill_unit["skills"]
        );
    }

    #[test]
    fn equivalent_resource_permutations_have_identical_exports() {
        let mut first = profile_with_resources(false);
        let mut second = profile_with_resources(true);
        canonicalize_profile(&mut first);
        canonicalize_profile(&mut second);
        assert_eq!(first, second);
    }

    #[test]
    fn canonicalization_is_idempotent_and_keeps_business_ordered_lists() {
        let mut once = profile_with_resources(false);
        canonicalize_profile(&mut once);
        let expected_decks = once["deck_list"].clone();
        let expected_guild_decks = once["guildsiege_defense_deck_list"].clone();
        let expected_sec_eff = once["sec_eff"].clone();
        let expected_sec_effects = once["sec_effects"].clone();
        let original = profile_with_resources(false);

        let mut twice = once.clone();
        canonicalize_profile(&mut twice);
        assert_eq!(once, twice);
        assert_eq!(twice["deck_list"], expected_decks);
        assert_eq!(twice["guildsiege_defense_deck_list"], expected_guild_decks);
        assert_eq!(twice["sec_eff"], expected_sec_eff);
        assert_eq!(twice["sec_effects"], expected_sec_effects);
        assert_eq!(once["deck_list"], original["deck_list"]);
        assert_eq!(
            once["guildsiege_defense_deck_list"],
            original["guildsiege_defense_deck_list"]
        );
        assert_eq!(once["sec_eff"], original["sec_eff"]);
        assert_eq!(once["sec_effects"], original["sec_effects"]);
    }

    #[test]
    fn converts_object_form_runes_to_arrays_on_the_export_copy() {
        let mut profile = profile_with_resources(false);
        profile["runes"] = serde_json::json!({
            "first": { "rune_id": 2, "set_id": 1, "slot_no": 1 },
            "second": { "rune_id": 1, "set_id": 1, "slot_no": 2 }
        });
        profile["unit_list"][0]["runes"] = serde_json::json!({
            "first": { "rune_id": 302, "slot_no": 2 },
            "second": { "rune_id": 301, "slot_no": 1 }
        });
        let original = profile.clone();
        let mut export_copy = profile.clone();

        canonicalize_profile(&mut export_copy);

        assert!(original["runes"].is_object());
        assert_eq!(profile, original);
        assert!(export_copy["runes"].is_array());
        let unit_with_object_runes = export_copy["unit_list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|unit| unit["unit_id"] == 30)
            .unwrap();
        assert!(unit_with_object_runes["runes"].is_array());
        assert_eq!(export_copy["runes"][0]["rune_id"], 2);
        assert_eq!(unit_with_object_runes["runes"][0]["rune_id"], 301);
    }
}
