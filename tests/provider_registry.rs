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
        registry.source_provider("lark-sheet").is_some(),
        "missing lark-sheet source provider",
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
        registry.source_writer("lark-sheet").is_some(),
        "missing lark-sheet writer",
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
        registry.dimension_source_manager("csv").is_some(),
        "missing csv dimension source manager",
    )?;
    ensure(
        registry.dimension_source_manager("cfd").is_some(),
        "missing cfd dimension source manager",
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

#[derive(Debug, Clone, Copy)]
struct FakeTableManager;

static FAKE_TABLE_MANAGER_DESCRIPTOR: coflow_api::TableManagerDescriptor =
    coflow_api::TableManagerDescriptor {
        id: "fake-table",
        display_name: "Fake table",
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
