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
    let field = symbol.rsplit_once('.').and_then(|(owner, name)| schema.field(owner, name));
    let rule = schema.all_check_statements().any(|info| super::rules::selected(info, symbol));
    if schema.resolve_type(symbol).is_none() && schema.resolve_enum(symbol).is_none() && schema.resolve_const(symbol).is_none() && field.is_none() && !rule {
        return Err(Failure::missing(format!("unknown symbol `{symbol}`")));
    }
    let mut items = Vec::new();
    if let Some(field) = field {
        let source = schema.resolve_type(&field.declaring_type).and_then(|owner| schema.source(&owner.module));
        items.push(json!({"kind": "declaration", "symbol": format!("{}.{}", field.declaring_type, field.name),
            "location": source.map(|source| location(&source.path, &source.source, field.span))}));
    }
    if let Some(declaration) = schema.resolve_enum(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(&source.path, &source.source, declaration.span))}));
    }
    if let Some(declaration) = schema.resolve_const(symbol) {
        let source = schema.source(&declaration.module);
        items.push(json!({"kind": "declaration", "symbol": symbol, "location": source.map(|source| location(&source.path, &source.source, declaration.span))}));
        for (_, module) in session.modules().modules() {
            let Some(ast) = module.ast() else { continue; };
            for item in &ast.items {
                let coflow_language::cft::syntax::ast::Item::Type(ty) = item else { continue; };
                for field in &ty.fields {
                    let Some(default) = &field.default else { continue; };
                    let mut uses = Vec::new();
                    default_static_paths(default, &mut uses);
                    for (_, span) in uses.into_iter().filter(|(name, _)| name == symbol) {
                        items.push(json!({"kind": "default_value", "symbol": format!("{}.{}", ty.name, field.name),
                            "location": location(module.path(), module.source(), span)}));
                    }
                }
            }
        }
    }
    for ty in schema.all_types() {
        let source = schema.source(&ty.module);
        if ty.name.as_str() == symbol { items.push(json!({"kind": "declaration", "symbol": ty.name, "location": source.map(|s| location(&s.path, &s.source, ty.span))})); }
        if ty.parent.as_ref().is_some_and(|p| p.as_str() == symbol) { items.push(json!({"kind": "type_reference", "symbol": ty.name, "location": source.map(|s| location(&s.path, &s.source, ty.span))})); }
        for field in ty.all_fields().filter(|f| f.declaring_type == ty.name) {
            if mentions(&field.value_type,symbol) { items.push(json!({"kind": "type_reference", "symbol": format!("{}.{}",ty.name,field.name), "location": source.map(|s| location(&s.path, &s.source, field.span))})); }
            if field.default.as_ref().is_some_and(|value| default_mentions(value, symbol)) {
                items.push(json!({"kind": "default_value", "symbol": format!("{}.{}", ty.name, field.name),
                    "location": source.map(|source| location(&source.path, &source.source, field.span))}));
            }
        }
    }
    for info in schema.all_check_statements() {
        if rule && super::rules::selected(info, symbol) {
            items.push(json!({"kind": "declaration", "symbol": super::rules::identity(info), "rule": super::rules::describe(schema, info, project.root_dir())}));
        }
        if info.dependencies.iter().any(|d| match d { coflow_language::cft::CheckDependency::Field(f) => f.owner.as_str() == symbol || field.is_some_and(|target|
                schema.field(&f.owner, &f.field).is_some_and(|dependency| dependency.declaring_type == target.declaring_type && dependency.name == target.name)),
            coflow_language::cft::CheckDependency::RecordSet(t) => t.as_str() == symbol }) {
            items.push(json!({"kind": "rule_expression", "rule": super::rules::describe(schema,info,project.root_dir())}));
        }
    }
    let data = crate::Runtime::new().open_data_session(project.clone())?;
    let mut sources = std::collections::BTreeMap::new();
    for (_,record) in data.model().records() {
        if record.actual_type() == symbol { items.push(json!({"kind": "data", "record": record.coordinate(), "location": record.origin})); }
        if let Some(target) = field {
            // 继承字段归属于原始声明；只把源码中的显式赋值作为 CFD 使用位置。
            if schema.field(record.actual_type(), &target.name).is_some_and(|actual| actual.declaring_type == target.declaring_type) {
                if let Some(file) = data.queries().file_for_record(record.actual_type(), record.key()) {
                    let path = project.resolve_path(Path::new(file));
                    // 同一文件只解析一次，记录数不会放大源码读取与解析成本。
                    let (source, ast) = match sources.entry(file.to_string()) {
                        std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            let source = std::fs::read_to_string(&path)?;
                            let (ast, _) = coflow_language::cfd::parse_cfd(&source);
                            entry.insert((source, ast))
                        }
                    };
                    if let Some(assignment) = ast.records.iter().find(|entry| entry.key == record.key() && entry.type_name == record.actual_type())
                        .and_then(|entry| entry.fields.iter().find(|entry| entry.name == target.name.as_str())) {
                        items.push(json!({"kind": "data", "record": record.coordinate(), "field": target.name,
                            "location": location(&path, &source, assignment.span)}));
                    }
                }
            }
        }
    }
    let page = super::query::paginate(items, &args.selector()?)?;
    Ok(json!({"symbol": symbol, "references": page.items, "total": page.total, "truncated": page.truncated, "next_cursor": page.next_cursor}))
}

fn default_mentions(value: &coflow_language::cft::CftSchemaDefaultValue, symbol: &str) -> bool {
    use coflow_language::cft::CftSchemaDefaultValue as V;
    // 遍历已解析的类型身份，字符串内容与同名标识符不构成符号引用。
    match value {
        V::Enum { enum_name, .. } => enum_name.as_str() == symbol,
        V::RecordReference { type_name, .. } => type_name.as_str() == symbol,
        V::Object { type_name, fields } => type_name.as_str() == symbol || fields.iter().any(|(_, value)| default_mentions(value, symbol)),
        V::OptionSome(value) => default_mentions(value, symbol),
        V::Array(values) => values.iter().any(|value| default_mentions(value, symbol)),
        V::Dictionary(entries) => entries.iter().any(|(key, value)| default_mentions(key, symbol) || default_mentions(value, symbol)),
        _ => false,
    }
}

pub(super) fn default_static_paths(expression: &coflow_language::cft::syntax::ast::DefaultExpr, uses: &mut Vec<(String, coflow_language::source::Span)>) {
    use coflow_language::cft::syntax::ast::DefaultExprKind as E;
    // 编译会折叠常量值；源码 AST 保留实际常量路径和精确位置，字符串不参与匹配。
    match &expression.kind {
        E::StaticPath(path) => uses.push((path.canonical(), path.span)),
        E::OptionSome(value) => default_static_paths(value, uses),
        E::BitExpr { lhs, rhs, .. } => { default_static_paths(lhs, uses); default_static_paths(rhs, uses); }
        E::Array(values) => { for value in values { default_static_paths(value, uses); } }
        E::Object(fields) | E::TypedObject { fields, .. } => { for (_, value) in fields { default_static_paths(value, uses); } }
        E::Dictionary(entries) => { for (key, value) in entries { default_static_paths(key, uses); default_static_paths(value, uses); } }
        _ => {}
    }
}
