use coflow_api::ProviderRegistry;

#[must_use]
pub fn builtin_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::default();
    register_builtin_providers(&mut registry);
    registry
}

pub fn register_builtin_providers(registry: &mut ProviderRegistry) {
    registry.register_loader(coflow_loader_excel::ExcelLoader);
    registry.register_loader(coflow_loader_lark::LarkSheetLoader::default());
    registry.register_loader(coflow_loader_cfd::CfdLoader);
    registry.register_exporter(coflow_exporter_json::JsonExporter);
    registry.register_exporter(coflow_exporter_messagepack::MessagePackExporter);
    registry.register_codegen(coflow_codegen_csharp::CsharpCodeGenerator);
}
