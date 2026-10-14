use std::path::Path;
use serde_json::{Value, json};
use coflow_language::cft::CftValueType;
use crate::{Project, ProjectSchemaSession};
use super::{Result, Failure};
pub use super::schema_inspection::inspect;

pub fn location(path: &Path, source: &str, span: coflow_language::source::Span) -> Value {
    let range = crate::api::byte_range(source, span.start, span.end);
    json!({"path": crate::path_to_slash(path), "start_line": range.start.line, "start_character": range.start.character,
        "end_line": range.end.line, "end_character": range.end.character})
}

pub fn mentions(ty: &CftValueType, symbol: &str) -> bool {
    match ty {
        CftValueType::Object(n) | CftValueType::RecordRef(n) => n.as_str() == symbol,
        CftValueType::Enum(n) => n.as_str() == symbol,
        CftValueType::Array(t) | CftValueType::Option(t) => mentions(t,symbol),
        CftValueType::Dict(k,v) => mentions(k,symbol) || mentions(v,symbol), _ => false,
    }
}

pub fn references(project: &Project, session: &ProjectSchemaSession, args: &super::Arguments) -> Result<Value> {
    let symbol = args.require("symbol")?;
    let schema = session.schema().ok_or_else(|| Failure::missing("schema is unavailable"))?;
    if schema.resolve_type(symbol).is_none() && schema.resolve_enum(symbol).is_none() && schema.resolve_const(symbol).is_none() {
        return Err(Failure::missing(format!("unknown symbol `{symbol}`")));
    }
    let mut items = Vec::new();
    if let Some(declaration) = schema.resolve_enum(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(&source.path, &source.source, declaration.span))}));
    }
    if let Some(declaration) = schema.resolve_const(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(&source.path, &source.source, declaration.span))}));
    }
    for ty in schema.all_types() {
        let source = schema.source(&ty.module);
        if ty.name.as_str() == symbol { items.push(json!({"kind": "declaration", "symbol": ty.name, "location": source.map(|s| location(&s.path, &s.source, ty.span))})); }
        if ty.parent.as_ref().is_some_and(|p| p.as_str() == symbol) { items.push(json!({"kind": "type_reference", "symbol": ty.name, "location": source.map(|s| location(&s.path, &s.source, ty.span))})); }
        for field in ty.all_fields().filter(|f| f.declaring_type == ty.name) {
            if mentions(&field.value_type,symbol) { items.push(json!({"kind": "type_reference", "symbol": format!("{}.{}",ty.name,field.name), "location": source.map(|s| location(&s.path, &s.source, field.span))})); }
        }
    }
    for info in schema.all_check_statements() {
        if info.dependencies.iter().any(|d| match d { coflow_language::cft::CheckDependency::Field(f) => f.owner.as_str() == symbol,
            coflow_language::cft::CheckDependency::RecordSet(t) => t.as_str() == symbol }) {
            items.push(json!({"kind": "rule_expression", "rule": super::rules::describe(schema,info)}));
        }
    }
    let data = crate::Runtime::new().open_data_session(project.clone())?;
    for (_,record) in data.model().records() {
        if record.actual_type() == symbol { items.push(json!({"kind": "data", "record": record.coordinate(), "location": record.origin})); }
    }
    let page = super::query::paginate(items, &args.selector()?)?;
    Ok(json!({"symbol": symbol, "references": page.items, "total": page.total, "truncated": page.truncated, "next_cursor": page.next_cursor}))
}
