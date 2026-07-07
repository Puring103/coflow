use serde_json::{json, Value};

use crate::{byte_range, field_location, LspBuild};

/// Find the LSP location (uri + range) of a CFT type definition by name.
pub(crate) fn cft_type_definition_location(build: &LspBuild, type_name: &str) -> Option<Value> {
    use coflow_cft::parser::parse_module;
    use coflow_cft::ModuleId;

    for (module_id, document) in &build.documents {
        let Some(ast) = document
            .ast
            .clone()
            .or_else(|| parse_module(&ModuleId::new(module_id.clone()), &document.source).ok())
        else {
            continue;
        };

        for item in &ast.items {
            use coflow_cft::ast::Item;
            let (name, name_span) = match item {
                Item::Type(t) => (t.name.as_str(), t.name_span),
                Item::Enum(e) => (e.name.as_str(), e.name_span),
                Item::Const(_) => continue,
            };
            if name == type_name {
                let range = byte_range(&document.source, name_span.start, name_span.end);
                return Some(json!({
                    "uri": document.uri,
                    "range": range,
                }));
            }
        }
    }
    None
}

/// Find the LSP location of a CFT field definition by owning type and field name.
pub(crate) fn cft_schema_field_definition_location(
    build: &LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<Value> {
    field_location(build, type_name, field_name)
}
