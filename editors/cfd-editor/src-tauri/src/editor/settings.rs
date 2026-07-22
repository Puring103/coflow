use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde::{Deserialize, Serialize};

use super::{EditorError, EditorProjectSettings, EditorRecordGroup, ViewConfig, ViewKind};

const SETTINGS_DIR: &str = "editor-setting";
const VIEWS_FILE: &str = "views.json";
const RECORD_GROUPS_FILE: &str = "record-groups.json";
const MIN_COLUMN_WIDTH: f64 = 48.0;
const MAX_VIEW_NAME_LEN: usize = 80;
const MAX_FIELD_LEN: usize = 160;
/// Reserved id prefix for implicit default views. User views cannot use it.
pub(super) const RESERVED_VIEW_ID_PREFIX: &str = "__";
const RECORD_GROUP_COLORS: &[&str] = &[
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "gray",
];

/// On-disk shape of `editor-setting/views.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ViewsFile {
    #[serde(default)]
    views: BTreeMap<String, BTreeMap<String, Vec<ViewConfig>>>,
    #[serde(default)]
    default_table_column_widths: BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
}

/// On-disk shape of `editor-setting/record-groups.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RecordGroupsFile {
    #[serde(default)]
    record_groups: BTreeMap<String, BTreeMap<String, Vec<EditorRecordGroup>>>,
}

fn views_path(project_root: &Path) -> PathBuf {
    project_root.join(SETTINGS_DIR).join(VIEWS_FILE)
}

fn record_groups_path(project_root: &Path) -> PathBuf {
    project_root.join(SETTINGS_DIR).join(RECORD_GROUPS_FILE)
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> Result<T, EditorError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes = fs::read(path).map_err(|error| {
        EditorError::other(format!("failed to read {}: {error}", path.display()))
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| EditorError::other(format!("failed to parse {}: {error}", path.display())))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), EditorError> {
    let parent = path
        .parent()
        .ok_or_else(|| EditorError::other("editor settings path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| {
        EditorError::other(format!("failed to create {}: {error}", parent.display()))
    })?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        EditorError::other(format!("failed to encode editor settings: {error}"))
    })?;
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(&bytes))
        .map_err(|error| EditorError::other(format!("failed to write {}: {error}", path.display())))
}

/// Read the project settings files and merge them into one in-memory struct.
pub(super) fn read_project_settings(
    project_root: &Path,
) -> Result<EditorProjectSettings, EditorError> {
    let views: ViewsFile = read_json(&views_path(project_root))?;
    let groups: RecordGroupsFile = read_json(&record_groups_path(project_root))?;
    Ok(EditorProjectSettings {
        views: views.views,
        default_table_column_widths: views.default_table_column_widths,
        record_groups: groups.record_groups,
    })
}

/// Persist settings by splitting them back into the two on-disk files.
pub(super) fn write_project_settings(
    project_root: &Path,
    settings: &EditorProjectSettings,
) -> Result<(), EditorError> {
    let views = ViewsFile {
        views: settings.views.clone(),
        default_table_column_widths: settings.default_table_column_widths.clone(),
    };
    write_json(&views_path(project_root), &views)?;
    let groups = RecordGroupsFile {
        record_groups: settings.record_groups.clone(),
    };
    write_json(&record_groups_path(project_root), &groups)
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
            let name = group.name.trim().chars().take(MAX_VIEW_NAME_LEN).collect::<String>();
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

/// Trim/dedupe an ordered list of field-like strings, preserving first-seen order.
fn sanitized_field_list(fields: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    fields
        .into_iter()
        .map(|field| field.trim().chars().take(MAX_FIELD_LEN).collect::<String>())
        .filter(|field| !field.is_empty() && seen.insert(field.clone()))
        .collect()
}

/// Sanitize a (filePath, actualType)'s custom view list:
/// - drop empty / duplicate ids, and ids using the reserved `__` prefix
/// - trim + truncate names (fallback to a default)
/// - clear `group_filter` when it does not point at a valid group id
/// - trim/dedupe columns/relations/fields; clamp column widths
/// - clear fields unused by the view's `kind`
pub(super) fn sanitized_views(
    views: Vec<ViewConfig>,
    valid_group_ids: &BTreeSet<String>,
) -> Vec<ViewConfig> {
    let mut ids = BTreeSet::new();
    views
        .into_iter()
        .filter_map(|view| {
            let id = view.id.trim().to_string();
            if id.is_empty()
                || id.starts_with(RESERVED_VIEW_ID_PREFIX)
                || !ids.insert(id.clone())
            {
                return None;
            }
            let name = view.name.trim().chars().take(MAX_VIEW_NAME_LEN).collect::<String>();
            let group_filter = view
                .group_filter
                .filter(|group_id| valid_group_ids.contains(group_id));
            let sanitized = match view.kind {
                ViewKind::Table => ViewConfig {
                    id,
                    name: if name.is_empty() { "未命名视图".to_string() } else { name },
                    kind: ViewKind::Table,
                    group_filter,
                    columns: sanitized_field_list(view.columns),
                    column_widths: sanitized_column_widths(view.column_widths),
                    relations: Vec::new(),
                    fields: Vec::new(),
                },
                ViewKind::Graph => ViewConfig {
                    id,
                    name: if name.is_empty() { "未命名视图".to_string() } else { name },
                    kind: ViewKind::Graph,
                    group_filter,
                    columns: Vec::new(),
                    column_widths: BTreeMap::new(),
                    relations: sanitized_field_list(view.relations),
                    fields: sanitized_field_list(view.fields),
                },
            };
            Some(sanitized)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use coflow_cft::{RecordKey, TypeName};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn coordinate(key: &str) -> coflow_runtime::RecordCoordinate {
        coflow_runtime::RecordCoordinate::new(
            TypeName::new("Item").expect("type"),
            RecordKey::new(key).expect("key"),
        )
    }

    #[test]
    fn settings_round_trip_under_editor_setting_directory() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("coflow-editor-settings-{nonce}"));
        let mut settings = EditorProjectSettings::default();
        settings
            .default_table_column_widths
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
                    records: vec![coordinate("a"), coordinate("b")],
                }],
            );
        settings
            .views
            .entry("data/items.cfd".to_string())
            .or_default()
            .insert(
                "Item".to_string(),
                vec![ViewConfig {
                    id: "view-1".to_string(),
                    name: "Cheap".to_string(),
                    kind: ViewKind::Table,
                    group_filter: None,
                    columns: vec!["name".to_string(), "price".to_string()],
                    column_widths: BTreeMap::from([("name".to_string(), 120.0)]),
                    relations: Vec::new(),
                    fields: Vec::new(),
                }],
            );

        write_project_settings(&root, &settings).expect("write settings");
        let loaded = read_project_settings(&root).expect("read settings");

        assert_eq!(loaded.views, settings.views);
        assert_eq!(
            loaded.default_table_column_widths,
            settings.default_table_column_widths
        );
        assert_eq!(loaded.record_groups, settings.record_groups);
        assert!(root.join(SETTINGS_DIR).join(VIEWS_FILE).is_file());
        assert!(root.join(SETTINGS_DIR).join(RECORD_GROUPS_FILE).is_file());
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
    fn views_drop_reserved_prefix_and_duplicate_ids() {
        let valid_groups = BTreeSet::new();
        let views = sanitized_views(
            vec![
                ViewConfig {
                    id: "__default_table".to_string(),
                    name: "Sneaky".to_string(),
                    kind: ViewKind::Table,
                    group_filter: None,
                    columns: vec![],
                    column_widths: BTreeMap::new(),
                    relations: vec![],
                    fields: vec![],
                },
                ViewConfig {
                    id: " view-a ".to_string(),
                    name: "  ".to_string(),
                    kind: ViewKind::Table,
                    group_filter: None,
                    columns: vec![" name ".to_string(), "name".to_string(), String::new()],
                    column_widths: BTreeMap::new(),
                    relations: vec!["ignored".to_string()],
                    fields: vec!["ignored".to_string()],
                },
                ViewConfig {
                    id: "view-a".to_string(),
                    name: "Dup".to_string(),
                    kind: ViewKind::Table,
                    group_filter: None,
                    columns: vec![],
                    column_widths: BTreeMap::new(),
                    relations: vec![],
                    fields: vec![],
                },
            ],
            &valid_groups,
        );

        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "view-a");
        assert_eq!(views[0].name, "未命名视图");
        assert_eq!(views[0].columns, vec!["name".to_string()]);
        // Table view drops graph-only fields.
        assert!(views[0].relations.is_empty());
        assert!(views[0].fields.is_empty());
    }

    #[test]
    fn views_clear_dangling_group_filter_and_graph_fields() {
        let valid_groups = BTreeSet::from(["potions".to_string()]);
        let views = sanitized_views(
            vec![
                ViewConfig {
                    id: "keep".to_string(),
                    name: "Keep".to_string(),
                    kind: ViewKind::Graph,
                    group_filter: Some("potions".to_string()),
                    columns: vec!["ignored".to_string()],
                    column_widths: BTreeMap::from([("ignored".to_string(), 100.0)]),
                    relations: vec!["owner".to_string()],
                    fields: vec!["name".to_string()],
                },
                ViewConfig {
                    id: "drop-filter".to_string(),
                    name: "Drop".to_string(),
                    kind: ViewKind::Graph,
                    group_filter: Some("missing".to_string()),
                    columns: vec![],
                    column_widths: BTreeMap::new(),
                    relations: vec![],
                    fields: vec![],
                },
            ],
            &valid_groups,
        );

        assert_eq!(views.len(), 2);
        assert_eq!(views[0].group_filter.as_deref(), Some("potions"));
        assert_eq!(views[0].relations, vec!["owner".to_string()]);
        assert_eq!(views[0].fields, vec!["name".to_string()]);
        // Graph view drops table-only fields.
        assert!(views[0].columns.is_empty());
        assert!(views[0].column_widths.is_empty());
        assert_eq!(views[1].group_filter, None);
    }
}
