#[test]
fn builtin_registry_contains_all_default_providers() -> Result<(), String> {
    let registry = coflow::builtin_registry().map_err(|err| err.to_string())?;

    ensure(registry.loader("excel").is_some(), "missing excel loader")?;
    ensure(
        registry.loader("lark-sheet").is_some(),
        "missing lark-sheet loader",
    )?;
    ensure(registry.loader("cfd").is_some(), "missing cfd loader")?;
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
        .register_loader(coflow_loader_excel::ExcelLoader)
        .map_err(|err| err.to_string())?;
    let err = registry
        .register_loader(coflow_loader_excel::ExcelLoader)
        .err()
        .ok_or_else(|| "duplicate loader id should fail".to_string())?;
    ensure_eq(err.provider_kind(), "loader", "duplicate loader kind")?;
    ensure_eq(err.id(), "excel", "duplicate loader id")?;

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
    Ok(())
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
