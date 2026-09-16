//! Editor settings: `editor-setting/editor.json` read/write + sanitize.
//!
//! 按职责拆分（仅结构拆分，不改行为）：
//! - `model`：版本化磁盘结构与常量；
//! - `io`：原子读写与版本校验；
//! - `sanitize`：workspace/列宽/分组/视图归一化。

#[path = "settings/io.rs"]
pub(crate) mod io;
#[path = "settings/model.rs"]
pub(crate) mod model;
#[path = "settings/sanitize.rs"]
pub(crate) mod sanitize;

pub(crate) use io::{read_project_settings, write_project_settings};
pub(crate) use sanitize::{
    sanitized_column_widths, sanitized_record_groups, sanitized_views, sanitized_workspace,
};

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use coflow_core::schema::{RecordKey, TypeName};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::model::MIN_COLUMN_WIDTH;
    use super::model::SETTINGS_FILE;
    use super::{read_project_settings, write_project_settings};
    use super::{sanitized_column_widths, sanitized_record_groups, sanitized_views};
    use crate::editor::types::{
        EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState, EditorWorkspaceTab,
        ViewConfig, ViewKind,
    };
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;

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
        settings.graph_positions.insert(
            r#"["data/items.cfd","view-1","Item"]"#.to_string(),
            BTreeMap::from([("Item::a".to_string(), [-240.5, 360.25])]),
        );
        settings
            .graph_compact_modes
            .insert(r#"["data/items.cfd","view-1","Item"]"#.to_string(), false);
        settings.view_order.insert(
            "data/items.cfd".to_string(),
            BTreeMap::from([(
                "Item".to_string(),
                vec!["view-1".to_string(), "__default_record".to_string()],
            )]),
        );
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
        let tab_id = "data/items.cfd\u{1f}Item".to_string();
        settings.workspace = EditorWorkspaceState {
            tabs: vec![EditorWorkspaceTab {
                file_path: "data/items.cfd".to_string(),
                type_name: "Item".to_string(),
                view_id: "view-1".to_string(),
                view_kind: crate::editor::WorkspaceViewKind::Table,
                coordinate: Some(coordinate("a")),
            }],
            active_tab_id: Some(tab_id),
        };

        write_project_settings(&root, &settings).expect("write settings");
        let loaded = read_project_settings(&root).expect("read settings");

        assert_eq!(loaded.views, settings.views);
        assert_eq!(loaded.graph_positions, settings.graph_positions);
        assert_eq!(loaded.graph_compact_modes, settings.graph_compact_modes);
        assert_eq!(loaded.view_order, settings.view_order);
        assert_eq!(
            loaded.default_table_column_widths,
            settings.default_table_column_widths
        );
        assert_eq!(loaded.record_groups, settings.record_groups);
        assert_eq!(loaded.workspace, settings.workspace);
        assert!(root.join("editor-setting").join(SETTINGS_FILE).is_file());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn settings_store_project_and_external_files_as_relative_paths() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("coflow-editor-relative-settings-{nonce}"));
        let root = base.join("project");
        let external = base.join("shared").join("items.cfd");
        fs::create_dir_all(&root).expect("create project root");

        let mut settings = EditorProjectSettings::default();
        let external_runtime = coflow_runtime::path_to_slash(&external);
        settings
            .view_order
            .insert(external_runtime.clone(), BTreeMap::new());
        settings.workspace = EditorWorkspaceState {
            tabs: vec![EditorWorkspaceTab {
                file_path: external_runtime.clone(),
                type_name: "Item".to_string(),
                view_id: "__default_table".to_string(),
                view_kind: crate::editor::WorkspaceViewKind::Table,
                coordinate: None,
            }],
            active_tab_id: Some(format!("{external_runtime}\u{1f}Item")),
        };

        write_project_settings(&root, &settings).expect("write relative settings");
        let raw = fs::read_to_string(root.join("editor-setting").join(SETTINGS_FILE))
            .expect("read settings json");
        assert!(raw.contains("../shared/items.cfd"));
        assert!(!raw.contains(&external_runtime));

        let loaded = read_project_settings(&root).expect("read relative settings");
        assert!(loaded.view_order.contains_key(&external_runtime));
        assert_eq!(loaded.workspace.tabs[0].file_path, external_runtime);
        fs::remove_dir_all(base).expect("remove fixture");
    }

    #[test]
    fn settings_reject_unknown_file_version() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("coflow-editor-settings-{nonce}"));
        let path = root.join("editor-setting").join(SETTINGS_FILE);
        fs::create_dir_all(path.parent().expect("settings parent")).expect("create settings dir");
        fs::write(&path, "{\"version\":2}").expect("write unsupported settings");

        let error = read_project_settings(&root).expect_err("reject unknown settings version");
        assert!(error
            .to_string()
            .contains("unsupported editor settings version"));
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
