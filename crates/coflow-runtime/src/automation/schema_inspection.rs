use super::{Arguments, Failure, Result};
use coflow_language::cft::CftValueType;
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub fn inspect(session: &crate::ProjectSchemaSession, args: &Arguments) -> Result<Value> {
    let report = crate::inspect_schema(session, None, false);
    let mut result = super::json(report)?;
    let Some(schema) = session.schema() else {
        return super::json(serde_json::from_value::<super::SchemaInspection>(result)?);
    };
    let mut selected = BTreeSet::new();
    let mut queue = VecDeque::new();
    let mut omitted = Vec::new();
    let syntax_dependencies = constant_dependencies(session, schema);
    if let Some(name) = args.get("type") {
        if schema.resolve_type(name).is_none() {
            return Err(Failure::missing(format!("unknown type `{name}`")));
        }
        queue.push_back((name.to_string(), 0usize));
        if args.flag("include-derived") {
            for ty in schema
                .all_types()
                .filter(|t| schema.is_assignable(&t.name, name))
            {
                queue.push_back((ty.name.to_string(), 0));
            }
        }
    } else {
        for ty in schema.all_types() {
            queue.push_back((ty.name.to_string(), 0));
        }
        for enumeration in schema.all_enums() {
            queue.push_back((enumeration.name.to_string(), 0));
        }
        for constant in schema.all_consts() {
            queue.push_back((constant.name.to_string(), 0));
        }
    }
    let depth = args.number("depth", 4)?;
    let limit = args.number("limit", 100)?;
    while let Some((name, level)) = queue.pop_front() {
        if selected.contains(&name) {
            continue;
        }
        if selected.len() >= limit || level > depth {
            let query = if schema.resolve_type(&name).is_some() {
                format!("schema inspect --type {name}")
            } else {
                "schema inspect".into()
            };
            if !omitted.iter().any(|item: &Value| item["symbol"] == name) {
                omitted.push(json!({"symbol":name,"query":query}));
            }
            continue;
        }
        selected.insert(name.clone());
        if args.flag("related") {
            if let Some(ty) = schema.resolve_type(&name) {
                if let Some(parent) = &ty.parent {
                    queue.push_back((parent.to_string(), level + 1));
                }
                for field in ty.all_fields() {
                    dependencies(&field.value_type, &mut queue, level + 1);
                    if let Some(default) = &field.default {
                        default_dependencies(default, &mut queue, level + 1);
                    }
                }
            }
            if let Some(constant) = schema.resolve_const(&name) {
                dependencies(&constant.value_type, &mut queue, level + 1);
            }
            if let Some(constants) = syntax_dependencies.get(&name) {
                for constant in constants {
                    queue.push_back((constant.clone(), level + 1));
                }
            }
        }
    }
    if let Some(types) = result["types"].as_array_mut() {
        types.retain(|v| v["name"].as_str().is_some_and(|n| selected.contains(n)));
        for item in types {
            let Some(ty) = item["name"].as_str().and_then(|n| schema.resolve_type(n)) else {
                continue;
            };
            let source = schema.source(&ty.module);
            item["location"] = json!(source.map(|s| super::schema_queries::location(
                session.project.root_dir(),
                &s.path,
                &s.source,
                ty.span
            )));
            item["label"] = json!(ty.display.as_ref().and_then(|d| d.label.as_ref()));
            item["description"] = json!(ty.display.as_ref().and_then(|d| d.description.as_ref()));
            item["concrete_types"] = json!(schema.concrete_assignable_types(&ty.name));
            if let Some(fields) = item["fields"].as_array_mut() {
                for entry in fields {
                    let Some(field) = entry["name"]
                        .as_str()
                        .and_then(|n| schema.field(&ty.name, n))
                    else {
                        continue;
                    };
                    let source = schema
                        .resolve_type(&field.declaring_type)
                        .and_then(|t| schema.source(&t.module));
                    entry["declaring_type"] = json!(field.declaring_type);
                    entry["location"] = json!(source.map(|s| super::schema_queries::location(
                        session.project.root_dir(),
                        &s.path,
                        &s.source,
                        field.span
                    )));
                    entry["label"] = json!(field.display.as_ref().and_then(|d| d.label.as_ref()));
                    entry["description"] =
                        json!(field.display.as_ref().and_then(|d| d.description.as_ref()));
                    let mut expected = &field.value_type;
                    while let CftValueType::Option(inner) = expected {
                        expected = inner;
                    }
                    match expected {
                        CftValueType::Enum(name) => {
                            entry["enum_candidates"] = json!(schema.resolve_enum(name).map(|e| e
                                .variants
                                .iter()
                                .map(|v| json!({"name":v.name,"value":v.value.to_string()}))
                                .collect::<Vec<_>>()));
                        }
                        CftValueType::Object(name) => {
                            entry["concrete_types"] = json!(schema.concrete_assignable_types(name));
                        }
                        CftValueType::RecordRef(name) => {
                            entry["reference_type"] = json!(name);
                            entry["reference_query"] =
                                json!(format!("data list --type {name} --include-derived"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    for section in ["enums", "consts"] {
        if let Some(items) = result[section].as_array_mut() {
            items.retain(|item| {
                item["name"]
                    .as_str()
                    .is_some_and(|name| selected.contains(name))
            });
        }
    }
    for entry in result["enums"].as_array_mut().into_iter().flatten() {
        if let Some(declaration) = entry["name"]
            .as_str()
            .and_then(|name| schema.resolve_enum(name))
        {
            entry["location"] = json!(schema.source(&declaration.module).map(|source| {
                super::schema_queries::location(
                    session.project.root_dir(),
                    &source.path,
                    &source.source,
                    declaration.span,
                )
            }));
            entry["label"] = json!(declaration
                .display
                .as_ref()
                .and_then(|display| display.label.as_ref()));
            entry["description"] = json!(declaration
                .display
                .as_ref()
                .and_then(|display| display.description.as_ref()));
        }
    }
    for entry in result["consts"].as_array_mut().into_iter().flatten() {
        if let Some(declaration) = entry["name"]
            .as_str()
            .and_then(|name| schema.resolve_const(name))
        {
            entry["location"] = json!(schema.source(&declaration.module).map(|source| {
                super::schema_queries::location(
                    session.project.root_dir(),
                    &source.path,
                    &source.source,
                    declaration.span,
                )
            }));
        }
    }
    result["omitted"] = json!(omitted);
    super::json(serde_json::from_value::<super::SchemaInspection>(result)?)
}
fn dependencies(ty: &CftValueType, queue: &mut VecDeque<(String, usize)>, level: usize) {
    match ty {
        CftValueType::Object(n) | CftValueType::RecordRef(n) => {
            queue.push_back((n.to_string(), level));
        }
        CftValueType::Enum(n) => queue.push_back((n.to_string(), level)),
        CftValueType::Array(t) | CftValueType::Option(t) => dependencies(t, queue, level),
        CftValueType::Dict(k, v) => {
            dependencies(k, queue, level);
            dependencies(v, queue, level);
        }
        _ => {}
    }
}

fn constant_dependencies(
    session: &crate::ProjectSchemaSession,
    schema: &coflow_language::cft::CftSchema,
) -> std::collections::BTreeMap<String, Vec<String>> {
    use coflow_language::cft::syntax::ast::Item;
    let mut result = std::collections::BTreeMap::new();
    // 常量值已在编译中折叠，依赖图从保留的 AST 收集，再由 Schema 确认符号身份。
    for (_, module) in session.modules().modules() {
        let Some(ast) = module.ast() else {
            continue;
        };
        for item in &ast.items {
            let mut paths = Vec::new();
            let name = match item {
                Item::Type(ty) => {
                    for field in &ty.fields {
                        if let Some(default) = &field.default {
                            super::schema_queries::default_static_paths(default, &mut paths);
                        }
                    }
                    &ty.name
                }
                Item::Const(constant) => {
                    super::schema_queries::default_static_paths(&constant.value, &mut paths);
                    &constant.name
                }
                _ => continue,
            };
            let constants: BTreeSet<_> = paths
                .into_iter()
                .map(|(name, _)| name)
                .filter(|name| schema.resolve_const(name).is_some())
                .collect();
            result.insert(name.clone(), constants.into_iter().collect());
        }
    }
    result
}

fn default_dependencies(
    value: &coflow_language::cft::CftSchemaDefaultValue,
    queue: &mut VecDeque<(String, usize)>,
    level: usize,
) {
    use coflow_language::cft::CftSchemaDefaultValue as V;
    match value {
        V::Enum { enum_name, .. } => queue.push_back((enum_name.to_string(), level)),
        V::RecordReference { type_name, .. } => queue.push_back((type_name.to_string(), level)),
        V::Object { type_name, fields } => {
            queue.push_back((type_name.to_string(), level));
            for (_, value) in fields {
                default_dependencies(value, queue, level);
            }
        }
        V::OptionSome(value) => default_dependencies(value, queue, level),
        V::Array(values) => {
            for value in values {
                default_dependencies(value, queue, level);
            }
        }
        V::Dictionary(entries) => {
            for (key, value) in entries {
                default_dependencies(key, queue, level);
                default_dependencies(value, queue, level);
            }
        }
        _ => {}
    }
}
