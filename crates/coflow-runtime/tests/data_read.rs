#![allow(clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use coflow_api::{
    CreateTableRequest, DecodedSourceOptions, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult,
    ProjectSourceRef, ResolvedSource, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SyncHeaderRequest, TableAddressing, TableContext, TableManager,
    TableManagerDescriptor, TableOperationResult,
};
use coflow_data_model::CfdErrorCode;
use coflow_project::{path_to_slash, Project};
use coflow_runtime::{
    create_data_file, data_get, data_list, data_sources, sync_data_header, BuildProjectSession,
    DataCreateFileOptions, DataGetQuery, DataListQuery, DataSyncHeaderOptions,
    ProjectSchemaSession, RecordCoordinate, Runtime,
};

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
                price: int;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            sword: Item { name: "Sword", price: 100 }
            shield: Item { name: "Shield", price: 80 }
        "#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_large_project(root: &std::path::Path, count: usize) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
                price: int;
            }
        ",
    )
    .expect("write schema");
    let mut records = String::new();
    for index in 0..count {
        writeln!(
            records,
            "item_{index}: Item {{ name: \"Item {index}\", price: {index} }}"
        )
        .expect("write record text");
    }
    std::fs::write(root.join("data").join("items.cfd"), records).expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn registry() -> coflow_api::ProviderRegistry {
    coflow_builtins::default_provider_registry().expect("default provider registry")
}

fn build_session(
    project: Project,
    registry: &coflow_api::ProviderRegistry,
) -> Result<BuildProjectSession, DiagnosticSet> {
    Runtime::new(registry.clone()).build_project_session(project)
}

fn schema_session(project: Project) -> Result<ProjectSchemaSession, DiagnosticSet> {
    Runtime::open_schema_session(project)
}

#[test]
fn data_sources_report_provider_capabilities_and_types() {
    let root = std::env::temp_dir().join(format!("coflow-data-sources-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let report = data_sources(session.queries(), &registry);
    let source = report
        .sources
        .iter()
        .find(|source| source.file == "data/items.cfd")
        .expect("items source");
    assert_eq!(source.provider, "cfd");
    assert_eq!(source.capabilities.provider_id, "cfd");
    assert!(source.capabilities.can_edit_field);
    assert!(source.capabilities.can_insert_record);
    assert_eq!(source.types, vec!["Item"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_file_provider_inference_uses_table_manager_descriptor_capabilities() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-file-provider-descriptor-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let schema_session = schema_session(project).expect("schema session");

    let inferred = create_data_file(
        &schema_session,
        &registry,
        DataCreateFileOptions {
            file: "data/generated.xlsx".to_string(),
            actual_type: Some("Item".to_string()),
            provider: None,
            sheet: Some("Generated".to_string()),
        },
    )
    .expect("xlsx extension should infer excel table manager");
    assert_eq!(inferred.provider, "excel");

    let alias = create_data_file(
        &schema_session,
        &registry,
        DataCreateFileOptions {
            file: "data/generated-alias.xlsx".to_string(),
            actual_type: Some("Item".to_string()),
            provider: Some("xlsx".to_string()),
            sheet: Some("GeneratedAlias".to_string()),
        },
    )
    .expect("xlsx alias should resolve to excel table manager");
    assert_eq!(alias.provider, "excel");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn duplicate_record_diagnostics_keep_source_file_and_logical_record() {
    let root = std::env::temp_dir().join(format!(
        "coflow-duplicate-record-source-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            sword: Item { name: "Sword", price: 100 }
            sword: Item { name: "Duplicate Sword", price: 120 }
        "#,
    )
    .expect("write duplicate cfd");
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let duplicate_index = session
        .queries()
        .diagnostics()
        .as_set()
        .diagnostics
        .iter()
        .position(|diagnostic| diagnostic.code == CfdErrorCode::DuplicateId.to_string())
        .expect("duplicate diagnostic");
    let duplicate = &session.queries().diagnostics().as_set().diagnostics[duplicate_index];
    let primary = duplicate
        .primary
        .as_ref()
        .expect("duplicate diagnostic should keep source location");
    let coflow_api::SourceLocation::FileSpan { path, .. } = &primary.location else {
        panic!("duplicate diagnostic should point at a file span: {duplicate:?}");
    };
    assert!(
        path.to_string_lossy()
            .replace('\\', "/")
            .ends_with("data/items.cfd"),
        "duplicate diagnostic should point at data/items.cfd: {duplicate:?}"
    );
    let logical = session
        .queries()
        .diagnostics()
        .logical_location(duplicate_index)
        .expect("duplicate diagnostic should keep logical record location");
    assert_eq!(logical.actual_type.as_deref(), Some("Item"));
    assert_eq!(logical.record_key.as_deref(), Some("sword"));
    let indexed_file = path_to_slash(path.as_path());
    assert!(
        !session
            .queries()
            .diagnostics()
            .by_file(&indexed_file)
            .is_empty(),
        "duplicate diagnostic should be indexed by source file `{indexed_file}`"
    );
    assert!(
        !session
            .queries()
            .diagnostics()
            .by_record("Item", "sword")
            .is_empty(),
        "duplicate diagnostic should be indexed by logical record"
    );
    let rejected = session.queries().rejected_records();
    assert_eq!(
        rejected.len(),
        2,
        "duplicate model-build failure should keep all rejected source rows"
    );
    assert!(rejected.iter().all(|record| {
        record.coordinate.as_ref().is_some_and(|coordinate| {
            coordinate.actual_type.as_str() == "Item" && coordinate.key.as_str() == "sword"
        }) && record.display_path == "data/items.cfd"
    }));
    assert_eq!(
        session
            .queries()
            .rejected_records_in_file("data/items.cfd")
            .count(),
        2,
        "rejected source rows should be queryable by file"
    );
    assert_eq!(
        session
            .queries()
            .rejected_records_by_coordinate("Item", "sword")
            .count(),
        2,
        "rejected source rows should be queryable by logical coordinate"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_list_filters_and_paginates_record_summaries() {
    let root = std::env::temp_dir().join(format!("coflow-data-list-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let list = data_list(
        session.queries(),
        &DataListQuery {
            actual_type: Some("Item".to_string()),
            file: Some("data/items.cfd".to_string()),
            limit: Some(1),
            offset: 1,
        },
    );

    assert_eq!(list.records.len(), 1);
    assert_eq!(list.records[0].record.key.as_str(), "shield");
    assert_eq!(list.records[0].file, "data/items.cfd");
    assert_eq!(list.records[0].provider, "cfd");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_supports_selector_and_key_filters() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let selected = data_get(
        session.queries(),
        &DataGetQuery {
            selector: Some(RecordCoordinate::try_new("Item", "sword").unwrap()),
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("get selected");
    assert_eq!(selected.records.len(), 1);
    assert_eq!(selected.records[0].record.key.as_str(), "sword");
    assert_eq!(selected.records[0].file, "data/items.cfd");
    assert!(selected.records[0].fields.contains_key("price"));

    let filtered = data_get(
        session.queries(),
        &DataGetQuery {
            selector: None,
            actual_type: Some("Item".to_string()),
            file: Some("data/items.cfd".to_string()),
            keys: vec!["shield".to_string()],
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("get filtered");
    assert_eq!(filtered.records.len(), 1);
    assert_eq!(filtered.records[0].record.key.as_str(), "shield");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_applies_file_filter_to_selected_record() {
    let root = std::env::temp_dir().join(format!(
        "coflow-data-get-selector-file-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let report = data_get(
        session.queries(),
        &DataGetQuery {
            selector: Some(RecordCoordinate::try_new("Item", "sword").unwrap()),
            actual_type: None,
            file: Some("data/other.cfd".to_string()),
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect("selector excluded by filter should succeed");

    assert!(report.records.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_returns_diagnostic_for_missing_selector() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let diagnostics = data_get(
        session.queries(),
        &DataGetQuery {
            selector: Some(RecordCoordinate::try_new("Item", "missing").unwrap()),
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect_err("missing record should fail");

    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DATA-NOT-FOUND"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn data_get_requires_limit_or_all_for_large_unselected_results() {
    let root = std::env::temp_dir().join(format!("coflow-data-get-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_large_project(&root, 101);
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let registry = registry();
    let session = build_session(project, &registry).expect("session");

    let diagnostics = data_get(
        session.queries(),
        &DataGetQuery {
            selector: None,
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: None,
            offset: 0,
            all: false,
        },
    )
    .expect_err("large unselected result should require limit or all");

    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "DATA-GET-LIMIT"));
    assert!(diagnostics.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("records before pagination")
            && diagnostic.message.contains("--offset alone is not enough")
    }));

    let limited = data_get(
        session.queries(),
        &DataGetQuery {
            selector: None,
            actual_type: None,
            file: None,
            keys: Vec::new(),
            limit: Some(2),
            offset: 0,
            all: false,
        },
    )
    .expect("limited get");
    assert_eq!(limited.records.len(), 2);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_option_diagnostics_keep_the_project_key_path() {
    let root = std::env::temp_dir().join(format!(
        "coflow-provider-option-location-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(root.join("data").join("items.xlsx"), "").expect("write source");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - type: excel\n    path: data/items.xlsx\n    rogue: true\n",
    )
    .expect("write config");

    let config_path = root.join("coflow.yaml");
    let project = Project::open_schema_only(Some(&config_path)).expect("open project");
    let canonical_config_path = project.config_path.clone();
    let session = Runtime::new(registry())
        .build_project_session(project)
        .expect("project diagnostics should be retained in a session");
    let diagnostic = session
        .queries()
        .diagnostics()
        .as_set()
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "unknown excel source option `rogue`")
        .expect("provider option diagnostic");
    let Some(coflow_api::Label {
        location: coflow_api::SourceLocation::ProjectConfig { path, key_path },
        ..
    }) = &diagnostic.primary
    else {
        panic!("expected project config primary: {diagnostic:?}");
    };
    assert_eq!(path, &canonical_config_path);
    assert_eq!(key_path, &["sources", "0", "rogue"]);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn directory_source_rejects_options_unknown_to_every_selected_provider() {
    let root = std::env::temp_dir().join(format!(
        "coflow-directory-provider-option-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_project(&root);
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n    rogue: true\n",
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = build_session(project, &registry()).expect("session");
    let diagnostic = session
        .queries()
        .diagnostics()
        .as_set()
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message == "unknown directory source option `rogue`")
        .expect("directory option diagnostic");
    let Some(coflow_api::Label {
        location: coflow_api::SourceLocation::ProjectConfig { key_path, .. },
        ..
    }) = &diagnostic.primary
    else {
        panic!("expected project config primary: {diagnostic:?}");
    };
    assert_eq!(key_path, &["sources", "0", "rogue"]);

    let _ = std::fs::remove_dir_all(root);
}

#[derive(Debug)]
struct TestTableOptions {
    token: String,
}

#[derive(Debug)]
struct FakeLocalTable;

static FAKE_LOCAL_SOURCE: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "local-table",
    display_name: "Local test table",
    extensions: &["tabletest"],
    option_keys: &["token"],
};

static FAKE_LOCAL_TABLE: TableManagerDescriptor = TableManagerDescriptor {
    id: "local-table",
    display_name: "Local test table",
    file_extensions: &["tabletest"],
    aliases: &[],
    addressing: TableAddressing::Sheet,
};

impl SourceProvider for FakeLocalTable {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &FAKE_LOCAL_SOURCE
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(FAKE_LOCAL_SOURCE.id) {
            ProbeResult::certain()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        let token = options
            .get("token")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                DiagnosticSet::one(Diagnostic::error(
                    "TABLE-OPTIONS",
                    "TABLE",
                    "test table requires token",
                ))
            })?;
        Ok(DecodedSourceOptions::new(
            FAKE_LOCAL_SOURCE.id,
            TestTableOptions {
                token: token.to_string(),
            },
        ))
    }

    fn load(
        &self,
        _ctx: SourceLoadContext<'_>,
        _source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        Ok(LoadedSource {
            records: Vec::new(),
        })
    }
}

impl TableManager for FakeLocalTable {
    fn descriptor(&self) -> &'static TableManagerDescriptor {
        &FAKE_LOCAL_TABLE
    }

    fn create_table(
        &self,
        _ctx: TableContext<'_>,
        request: &CreateTableRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        validate_local_request(request.source)?;
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            ..TableOperationResult::default()
        })
    }

    fn sync_header(
        &self,
        _ctx: TableContext<'_>,
        request: &SyncHeaderRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        validate_local_request(request.source)?;
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added: vec!["name".to_string()],
            ..TableOperationResult::default()
        })
    }
}

fn validate_local_request(source: &ResolvedSource) -> Result<(), DiagnosticSet> {
    if !matches!(source.location, SourceLocationSpec::Path(_)) {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "TABLE-LOCATION",
            "TABLE",
            "unexpected local location",
        )));
    }
    let options = source.options::<TestTableOptions>(FAKE_LOCAL_SOURCE.id)?;
    if options.token != "secret" {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "TABLE-TOKEN",
            "TABLE",
            "unexpected token",
        )));
    }
    Ok(())
}

#[test]
fn table_operations_use_local_runtime_path() {
    let root = std::env::temp_dir().join(format!(
        "coflow-location-neutral-table-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create project dir");
    std::fs::write(root.join("schema.cft"), "type Item { name: string; }\n").expect("write schema");
    std::fs::write(root.join("data.tabletest"), "").expect("write table source");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - type: local-table\n    path: data.tabletest\n    token: secret\n",
    )
    .expect("write config");

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open");
    let session = schema_session(project).expect("schema session");
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_source_provider(FakeLocalTable)
        .expect("register source provider");
    registry
        .register_table_manager(FakeLocalTable)
        .expect("register table manager");

    let created = create_data_file(
        &session,
        &registry,
        DataCreateFileOptions {
            file: "data.tabletest".to_string(),
            actual_type: Some("Item".to_string()),
            provider: None,
            sheet: Some("Items".to_string()),
        },
    )
    .expect("create local table");
    assert_eq!(created.provider, "local-table");
    assert_eq!(created.headers, ["id", "name"]);

    let synced = sync_data_header(
        &session,
        &registry,
        DataSyncHeaderOptions {
            file: "data.tabletest".to_string(),
            actual_type: "Item".to_string(),
            provider: None,
            sheet: Some("Items".to_string()),
        },
    )
    .expect("sync local table header");
    assert_eq!(synced.added, ["name"]);

    let _ = std::fs::remove_dir_all(root);
}
