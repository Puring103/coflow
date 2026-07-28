#![allow(clippy::expect_used, clippy::too_many_lines)]

#[test]
fn builtin_registry_contains_all_default_providers() -> Result<(), String> {
    let registry = coflow_builtins::default_provider_registry().map_err(|err| err.to_string())?;

    ensure(
        registry.source_provider("excel").is_some(),
        "missing excel source provider",
    )?;
    ensure(
        registry.source_provider("csv").is_some(),
        "missing csv source provider",
    )?;
    ensure(
        registry.source_provider("cfd").is_some(),
        "missing cfd source provider",
    )?;
    ensure(
        registry.source_writer("excel").is_some(),
        "missing excel writer",
    )?;
    ensure(
        registry.source_writer("csv").is_some(),
        "missing csv writer",
    )?;
    ensure(
        registry.source_writer("cfd").is_some(),
        "missing cfd writer",
    )?;
    ensure(
        registry.table_manager("excel").is_some(),
        "missing excel table manager",
    )?;
    ensure(
        registry.table_manager("csv").is_some(),
        "missing csv table manager",
    )?;
    ensure(
        registry.table_manager("cfd").is_some(),
        "missing cfd table manager",
    )?;
    ensure(
        registry
            .table_manager("cfd")
            .expect("cfd table manager")
            .descriptor()
            .addressing
            == coflow_api::TableAddressing::Document,
        "cfd table manager addressing",
    )?;
    ensure(
        registry
            .table_manager("excel")
            .expect("excel table manager")
            .descriptor()
            .addressing
            == coflow_api::TableAddressing::Sheet,
        "excel table manager addressing",
    )?;
    ensure(
        registry.dimension_source_manager("csv").is_some(),
        "missing csv dimension source manager",
    )?;
    ensure(
        registry.dimension_source_manager("cfd").is_some(),
        "missing cfd dimension source manager",
    )?;
    let csv_options = registry
        .dimension_source_manager("csv")
        .expect("csv dimension manager")
        .source_options(&coflow_api::DimensionSourceOptionsRequest {
            sheet: "Item_name",
            actual_type: "Item_nameVariants",
        })
        .map_err(|err| format!("csv dimension options: {err:?}"))?;
    ensure_eq(
        csv_options.provider_id(),
        "csv",
        "csv dimension source provider identity",
    )?;
    let csv_source = coflow_api::ResolvedSource {
        provider_id: "csv".to_string(),
        location: coflow_api::SourceLocationSpec::new("Item_name.csv"),
        options: csv_options,
        display_name: "Item_name.csv".to_string(),
    };
    let csv_table = registry.table_manager("csv").expect("csv table manager");
    ensure_eq(
        csv_table
            .type_for_sheet(&csv_source, Some("Item_name"))
            .map_err(|err| format!("csv dimension type lookup: {err:?}"))?
            .as_deref()
            .unwrap_or_default(),
        "Item_nameVariants",
        "csv dimension source typed options",
    )?;
    ensure(registry.exporter("json").is_some(), "missing json exporter")?;
    ensure(
        registry.exporter("messagepack").is_some(),
        "missing messagepack exporter",
    )?;
    ensure(
        registry.codegen("csharp").is_some(),
        "missing csharp codegen",
    )?;
    Ok(())
}

#[test]
fn registry_rejects_duplicate_provider_ids() -> Result<(), String> {
    let mut registry = coflow_api::ProviderRegistry::default();

    registry
        .register_source_provider(coflow_loader_excel::ExcelLoader)
        .map_err(|err| err.to_string())?;
    let err = registry
        .register_source_provider(coflow_loader_excel::ExcelLoader)
        .err()
        .ok_or_else(|| "duplicate source provider id should fail".to_string())?;
    ensure_eq(
        err.provider_kind(),
        "source provider",
        "duplicate source provider kind",
    )?;
    ensure_eq(err.id(), "excel", "duplicate source provider id")?;

    registry
        .register_exporter(coflow_exporter_json::JsonExporter)
        .map_err(|err| err.to_string())?;
    let err = registry
        .register_exporter(coflow_exporter_json::JsonExporter)
        .err()
        .ok_or_else(|| "duplicate exporter id should fail".to_string())?;
    ensure_eq(err.provider_kind(), "exporter", "duplicate exporter kind")?;
    ensure_eq(err.id(), "json", "duplicate exporter id")?;

    registry
        .register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)
        .map_err(|err| err.to_string())?;
    let err = registry
        .register_codegen(coflow_codegen_csharp::CsharpCodeGenerator)
        .err()
        .ok_or_else(|| "duplicate codegen id should fail".to_string())?;
    ensure_eq(err.provider_kind(), "codegen", "duplicate codegen kind")?;
    ensure_eq(err.id(), "csharp", "duplicate codegen id")?;

    registry
        .register_table_manager(FakeTableManager)
        .map_err(|err| err.to_string())?;
    ensure(
        registry.table_manager("fake-table").is_some(),
        "missing fake table manager",
    )?;
    let err = registry
        .register_table_manager(FakeTableManager)
        .err()
        .ok_or_else(|| "duplicate table manager id should fail".to_string())?;
    ensure_eq(
        err.provider_kind(),
        "table manager",
        "duplicate table manager kind",
    )?;
    ensure_eq(err.id(), "fake-table", "duplicate table manager id")?;

    registry
        .register_dimension_source_manager(FakeDimensionSourceManager)
        .map_err(|err| err.to_string())?;
    ensure(
        registry
            .dimension_source_manager("fake-dimension")
            .is_some(),
        "missing fake dimension source manager",
    )?;
    let err = registry
        .register_dimension_source_manager(FakeDimensionSourceManager)
        .err()
        .ok_or_else(|| "duplicate dimension source manager id should fail".to_string())?;
    ensure_eq(
        err.provider_kind(),
        "dimension source manager",
        "duplicate dimension source manager kind",
    )?;
    ensure_eq(
        err.id(),
        "fake-dimension",
        "duplicate dimension source manager id",
    )?;
    Ok(())
}

#[test]
fn provider_bundle_registration_is_atomic() -> Result<(), String> {
    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_source_writer(coflow_loader_csv::CsvWriter::new())
        .map_err(|err| err.to_string())?;

    let mut bundle = coflow_api::ProviderBundle::default();
    bundle
        .add_source_provider(coflow_loader_csv::CsvLoader)
        .map_err(|err| err.to_string())?;
    bundle
        .add_source_writer(coflow_loader_csv::CsvWriter::new())
        .map_err(|err| err.to_string())?;

    let err = registry
        .register_bundle(bundle)
        .err()
        .ok_or_else(|| "bundle with a conflicting final role should fail".to_string())?;
    ensure_eq(
        err.provider_kind(),
        "source writer",
        "conflicting bundle role",
    )?;
    ensure_eq(err.id(), "csv", "conflicting bundle id")?;
    ensure(
        registry.source_provider("csv").is_none(),
        "failed bundle leaked an earlier source provider role",
    )?;
    ensure(
        registry.source_writer("csv").is_some(),
        "failed bundle changed the existing source writer",
    )?;
    Ok(())
}

#[test]
fn provider_package_bundle_merge_is_atomic() -> Result<(), String> {
    let mut bundle = coflow_api::ProviderBundle::default();
    bundle
        .add_source_writer(coflow_loader_csv::CsvWriter::new())
        .map_err(|err| err.to_string())?;

    let mut additions = coflow_api::ProviderBundle::default();
    additions
        .add_source_provider(coflow_loader_csv::CsvLoader)
        .map_err(|err| err.to_string())?;
    additions
        .add_source_writer(coflow_loader_csv::CsvWriter::new())
        .map_err(|err| err.to_string())?;

    let err = bundle
        .merge(additions)
        .err()
        .ok_or_else(|| "bundle merge with a conflicting final role should fail".to_string())?;
    ensure_eq(
        err.provider_kind(),
        "source writer",
        "conflicting merge role",
    )?;
    ensure_eq(err.id(), "csv", "conflicting merge id")?;

    let mut registry = coflow_api::ProviderRegistry::default();
    registry
        .register_bundle(bundle)
        .map_err(|err| err.to_string())?;
    ensure(
        registry.source_provider("csv").is_none(),
        "failed merge leaked an earlier source provider role",
    )?;
    ensure(
        registry.source_writer("csv").is_some(),
        "failed merge changed the existing source writer role",
    )?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct FakeTableManager;

static FAKE_TABLE_MANAGER_DESCRIPTOR: coflow_api::TableManagerDescriptor =
    coflow_api::TableManagerDescriptor {
        id: "fake-table",
        display_name: "Fake table",
        file_extensions: &["fake"],
        aliases: &[],
        addressing: coflow_api::TableAddressing::Sheet,
    };

impl coflow_api::TableManager for FakeTableManager {
    fn descriptor(&self) -> &'static coflow_api::TableManagerDescriptor {
        &FAKE_TABLE_MANAGER_DESCRIPTOR
    }
}

#[derive(Debug, Clone, Copy)]
struct FakeDimensionSourceManager;

static FAKE_DIMENSION_SOURCE_MANAGER_DESCRIPTOR: coflow_api::DimensionSourceManagerDescriptor =
    coflow_api::DimensionSourceManagerDescriptor {
        id: "fake-dimension",
        display_name: "Fake dimension",
    };

impl coflow_api::DimensionSourceManager for FakeDimensionSourceManager {
    fn descriptor(&self) -> &'static coflow_api::DimensionSourceManagerDescriptor {
        &FAKE_DIMENSION_SOURCE_MANAGER_DESCRIPTOR
    }
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.to_string())
    }
}

fn ensure_eq(actual: &str, expected: &str, context: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{context}: expected `{expected}`, got `{actual}`"))
    }
}
