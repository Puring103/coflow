use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;

use atomicwrites::{AllowOverwrite, AtomicFile};

use super::{EditorError, EditorProjectSettings, EditorRecordGroup};

const SETTINGS_PATH: &str = ".coflow/editor.json";
const MIN_COLUMN_WIDTH: f64 = 48.0;
const RECORD_GROUP_COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "gray",
];

pub(super) fn read_project_settings(
    project_root: &Path,
) -> Result<EditorProjectSettings, EditorError> {
    let path = project_root.join(SETTINGS_PATH);
    if !path.exists() {
        return Ok(EditorProjectSettings::default());
    }
    let bytes = fs::read(&path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| EditorError::other(format!("failed to parse {}: {error}", path.display())))
}

pub(super) fn write_project_settings(
    project_root: &Path,
    settings: &EditorProjectSettings,
) -> Result<(), EditorError> {
    let path = project_root.join(SETTINGS_PATH);
    let parent = path
        .parent()
        .ok_or_else(|| EditorError::other("editor settings path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        EditorError::other(format!("failed to create {}: {error}", parent.display()))
    })?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| file.write_all(&bytes))
        .map_err(|error| EditorError::other(format!("failed to write {}: {error}", path.display())))
}

pub(super) fn sanitized_column_widths(widths: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    widths
        .into_iter()
        .filter_map(|(column, width)| {
            width
                .is_finite()
                .then(|| (column, width.max(MIN_COLUMN_WIDTH)))
        })
        .collect()
}

pub(super) fn sanitized_record_groups(groups: Vec<EditorRecordGroup>) -> Vec<EditorRecordGroup> {
    let mut ids = BTreeSet::new();
    let mut assigned_records = BTreeSet::new();
    groups
        .into_iter()
        .filter_map(|group| {
            let id = group.id.trim().to_string();
            if id.is_empty() || !ids.insert(id.clone()) {
                return None;
            }
            let name = group.name.trim().chars().take(80).collect::<String>();
            let mut group_records = BTreeSet::new();
            let records = group
                .records
                .into_iter()
                .filter(|coordinate| {
                    !assigned_records.contains(coordinate)
                        && group_records.insert(coordinate.clone())
                })
                .collect::<Vec<_>>();
            if records.len() < 2 {
                return None;
            }
            assigned_records.extend(records.iter().cloned());
            Some(EditorRecordGroup {
                id,
                name: if name.is_empty() {
                    "未命名分组".to_string()
                } else {
                    name
                },
                color: group
                    .color
                    .filter(|color| RECORD_GROUP_COLORS.contains(&color.as_str())),
                records,
            })
        })
        .collect()
}

pub(super) fn sanitized_graph_fields(fields: Vec<String>) -> Vec<String> {
    fields
        .into_iter()
        .map(|field| field.trim().chars().take(160).collect::<String>())
        .filter(|field| !field.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn settings_round_trip_under_project_coflow_directory() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("coflow-editor-settings-{nonce}"));
        let mut settings = EditorProjectSettings::default();
        settings
            .table_column_widths
            .entry("data/items.cfd".to_string())
            .or_default()
            .entry("Item".to_string())
            .or_default()
            .insert("name".to_string(), 240.0);
        settings
            .record_groups
            .entry("data/items.cfd".to_string())
            .or_default()
            .insert(
                "Item".to_string(),
                vec![EditorRecordGroup {
                    id: "potions".to_string(),
                    name: "Potions".to_string(),
                    color: Some("blue".to_string()),
                    records: vec![
                        coflow_runtime::RecordCoordinate::new("Item", "a"),
                        coflow_runtime::RecordCoordinate::new("Item", "b"),
                    ],
                }],
            );
        settings
            .graph_enabled_fields
            .entry("data/items.cfd".to_string())
            .or_default()
            .insert(
                "Item".to_string(),
                vec!["name".to_string(), "price".to_string()],
            );

        write_project_settings(&root, &settings).expect("write settings");
        let loaded = read_project_settings(&root).expect("read settings");

        assert_eq!(loaded.table_column_widths, settings.table_column_widths);
        assert_eq!(loaded.record_groups, settings.record_groups);
        assert_eq!(loaded.graph_enabled_fields, settings.graph_enabled_fields);
        assert!(root.join(SETTINGS_PATH).is_file());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn column_widths_preserve_finite_values_above_the_minimum() {
        let widths = BTreeMap::from([
            ("zero".to_string(), 0.0),
            ("small".to_string(), 1.0),
            ("large".to_string(), 9_999.0),
            ("negative".to_string(), -1.0),
            ("invalid".to_string(), f64::NAN),
        ]);

        assert_eq!(
            sanitized_column_widths(widths),
            BTreeMap::from([
                ("large".to_string(), 9_999.0),
                ("negative".to_string(), MIN_COLUMN_WIDTH),
                ("small".to_string(), MIN_COLUMN_WIDTH),
                ("zero".to_string(), MIN_COLUMN_WIDTH),
            ])
        );
    }

    #[test]
    fn record_groups_remove_duplicate_members_and_invalid_groups() {
        let coordinate = |key: &str| coflow_runtime::RecordCoordinate::new("Item", key);
        let groups = sanitized_record_groups(vec![
            EditorRecordGroup {
                id: " group-1 ".to_string(),
                name: " Potions ".to_string(),
                color: Some("blue".to_string()),
                records: vec![coordinate("a"), coordinate("a"), coordinate("b")],
            },
            EditorRecordGroup {
                id: "group-2".to_string(),
                name: String::new(),
                color: Some("not-a-color".to_string()),
                records: vec![coordinate("b"), coordinate("c")],
            },
            EditorRecordGroup {
                id: "group-3".to_string(),
                name: "Later".to_string(),
                color: None,
                records: vec![coordinate("c"), coordinate("d")],
            },
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].id, "group-1");
        assert_eq!(groups[0].name, "Potions");
        assert_eq!(groups[0].color.as_deref(), Some("blue"));
        assert_eq!(groups[0].records, vec![coordinate("a"), coordinate("b")]);
        assert_eq!(groups[1].records, vec![coordinate("c"), coordinate("d")]);
        assert_eq!(groups[1].color, None);
    }

    #[test]
    fn graph_fields_are_trimmed_deduplicated_and_sorted() {
        assert_eq!(
            sanitized_graph_fields(vec![
                " price ".to_string(),
                "name".to_string(),
                "price".to_string(),
                String::new(),
            ]),
            vec!["name".to_string(), "price".to_string()],
        );
    }
}
