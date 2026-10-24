use super::{Arguments, Failure, Result};
use crate::{Project, ReadOnlyProjectSession};
use coflow_language::cft::CftValueType;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// 上下文预算可省略整个附属段；契约保留这些段的具体结构。
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextSchema {
    pub types: Vec<super::InspectedType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enums: Option<Vec<super::InspectedEnum>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consts: Option<Vec<super::InspectedConst>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<Vec<crate::SchemaDimensionInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub omitted: Option<Vec<super::OmittedSymbol>>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextReference {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub total: usize,
    pub truncated: bool,
    pub items: Vec<super::RecordTarget>,
    pub query: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextOmission {
    pub section: String,
    pub query: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextDimension {
    pub name: String,
    pub display_name: String,
    pub variants: Vec<String>,
    pub dir: Option<String>,
    pub files: Vec<String>,
    pub fields: Vec<ContextDimensionField>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextDimensionField {
    pub source_type: String,
    pub source_field: String,
    pub is_singleton: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ContextResult {
    pub schema: ContextSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<super::RecordResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<super::RuleQuery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<super::RecordResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<ContextReference>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<Vec<ContextDimension>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Vec<super::CommandDiagnostic>>,
    pub omitted: Vec<ContextOmission>,
    pub queries: Vec<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
pub fn query(
    project: &Project,
    session: &ReadOnlyProjectSession,
    args: &Arguments,
) -> Result<Value> {
    let ty = args.require("type")?;
    let target_type = session
        .schema()
        .resolve_type(ty)
        .ok_or_else(|| Failure::missing(format!("unknown type `{ty}`")))?;
    let schema = crate::Runtime::open_schema_session(project.clone())?;
    let mut schema_args = args.clone();
    schema_args.0.insert("related".into(), vec!["true".into()]);
    schema_args
        .0
        .insert("include-derived".into(), vec!["true".into()]);
    schema_args.0.remove("limit");
    let mut declarations = super::schema_queries::inspect(&schema, &schema_args)?;
    let mut selector = args.selector()?;
    selector.view = "both".into();
    // 即使不请求示例，也验证字段与筛选条件；精确目标不受示例数量影响。
    let records = super::query::selected_records(session, &selector)?;
    let target = if let Some(key) = args.get("id") {
        let record = records
            .iter()
            .find(|record| record.actual_type() == ty && record.id() == key)
            .ok_or_else(|| Failure::missing(format!("record `{ty}::{key}` does not exist")))?;
        Some(super::query::record_json(
            project, session, record, &selector,
        )?)
    } else {
        None
    };
    let examples = records
        .iter()
        .take(args.number("examples", 3)?)
        .map(|record| super::query::record_json(project, session, record, &selector))
        .collect::<Result<Vec<_>>>()?;
    let mut reference_types = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for field in target_type.all_fields() {
        if !selector.fields.is_empty()
            && !selector
                .fields
                .iter()
                .any(|path| path.split(['.', '[']).next() == Some(field.name.as_str()))
        {
            continue;
        }
        reference_targets(
            session.schema(),
            &field.value_type,
            args.number("depth", 4)?,
            &mut visited,
            &mut reference_types,
        );
    }
    let mut references = Vec::new();
    for name in reference_types {
        let candidates: Vec<_> = session
            .model()
            .records()
            .filter(|(_, record)| session.schema().is_assignable(record.actual_type(), &name))
            .map(|(_, record)| json!({"type": record.actual_type(), "id": record.id()}))
            .collect();
        let count = candidates.len();
        let limit = args.number("ref-limit", 10)?;
        references.push(json!({"type": name, "total": count, "truncated": count > limit,
            "items": candidates.into_iter().take(limit).collect::<Vec<_>>(), "query": format!("data list --type {name} --include-derived")}));
    }
    if !selector.fields.is_empty() {
        if let Some(types) = declarations["types"].as_array_mut() {
            for declaration in types.iter_mut().filter(|item| item["name"] == ty) {
                if let Some(fields) = declaration["fields"].as_array_mut() {
                    fields.retain(|field| {
                        selector
                            .fields
                            .iter()
                            .any(|path| path.split(['.', '[']).next() == field["name"].as_str())
                    });
                }
            }
        }
    }
    let store = session.queries().diagnostics();
    let diagnostics: Vec<_> = store
        .as_set()
        .diagnostics
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            store.logical_location(*index).is_none_or(|location| {
                location
                    .actual_type
                    .as_deref()
                    .is_none_or(|name| name == ty)
                    && args.get("id").is_none_or(|key| {
                        location.record_id.as_deref().is_none_or(|name| name == key)
                    })
            })
        })
        .map(|(index, diagnostic)| {
            super::diagnostics::CommandDiagnostic::new(
                diagnostic,
                store.logical_location(index),
                Some(project.root_dir()),
                None,
            )
        })
        .collect();
    let result = json!({"schema": declarations, "target": target,
        "rules": super::rules::query("rule.list",session.schema(),project.root_dir(),args)?, "examples": examples, "references": references,
        "dimensions": session.queries().dimensions().into_iter().filter(|dimension|dimension.fields.iter().any(|field|
            session.schema().is_assignable(ty,&field.source_type) || session.schema().is_assignable(&field.source_type,ty))).collect::<Vec<_>>(), "diagnostics": diagnostics,
        "omitted": [], "queries": [format!("data list --type {ty}"), format!("schema inspect --type {ty} --related")]});
    let mut result: ContextResult = serde_json::from_value(result)?;
    trim(&mut result, ty, args.number("budget", 16000)?)?;
    super::json(result)
}

fn reference_targets(
    schema: &coflow_language::cft::CftSchema,
    ty: &CftValueType,
    depth: usize,
    visited: &mut BTreeSet<String>,
    targets: &mut BTreeSet<String>,
) {
    match ty {
        CftValueType::RecordRef(name) => {
            targets.insert(name.to_string());
        }
        CftValueType::Object(name) if depth > 0 && visited.insert(name.to_string()) => {
            // 当前路径上的递归对象不重复展开，其他路径仍按各自剩余深度遍历。
            if let Some(object) = schema.resolve_type(name) {
                for field in object.all_fields() {
                    reference_targets(schema, &field.value_type, depth - 1, visited, targets);
                }
            }
            visited.remove(name.as_str());
        }
        CftValueType::Array(inner) | CftValueType::Option(inner) => {
            reference_targets(schema, inner, depth, visited, targets);
        }
        CftValueType::Dict(key, value) => {
            reference_targets(schema, key, depth, visited, targets);
            reference_targets(schema, value, depth, visited, targets);
        }
        _ => {}
    }
}

// 在类型化结果上执行预算裁剪，省略操作不依赖 JSON 对象形状或运行时断言。
fn omit(result: &mut ContextResult, section: &str, query: String) {
    result.omitted.push(ContextOmission {
        section: section.into(),
        query,
    });
}

fn exceeds_budget(result: &ContextResult, budget: usize) -> Result<bool> {
    Ok(serde_json::to_string(result)?.chars().count() > budget)
}

fn pop_field(result: &mut ContextResult, ty: &str) -> bool {
    result
        .schema
        .types
        .iter_mut()
        .find(|item| item.name == ty)
        .and_then(|item| item.fields.pop())
        .is_some()
}

fn trim(result: &mut ContextResult, ty: &str, budget: usize) -> Result<()> {
    // 预算包含省略提示，目标与字段都保留可继续查询的入口。
    for (name, query) in [
        ("examples", format!("data list --type {ty}")),
        (
            "references",
            format!("schema inspect --type {ty} --related"),
        ),
        ("rules", format!("rule list --type {ty}")),
        ("dimensions", "dimension list".into()),
        ("diagnostics", format!("check --type {ty}")),
    ] {
        if !exceeds_budget(result, budget)? {
            return Ok(());
        }
        match name {
            "examples" => {
                result.examples = None;
            }
            "references" => {
                result.references = None;
            }
            "rules" => {
                result.rules = None;
            }
            "dimensions" => {
                result.dimensions = None;
            }
            "diagnostics" => {
                result.diagnostics = None;
            }
            _ => {}
        }
        omit(result, name, query);
    }
    if exceeds_budget(result, budget)? {
        result.schema.types.retain(|item| item.name == ty);
        result.schema.enums = None;
        result.schema.consts = None;
        result.schema.dimensions = None;
        result.schema.omitted = None;
        omit(
            result,
            "related_declarations",
            format!("schema inspect --type {ty} --related"),
        );
    }
    if exceeds_budget(result, budget)? {
        if let Some(target) = result.target.take() {
            omit(
                result,
                "target",
                format!("data get --type {ty} --id {} --view both", target.id),
            );
        }
    }
    let mut omitted_fields = false;
    while exceeds_budget(result, budget)? && pop_field(result, ty) {
        omitted_fields = true;
    }
    if omitted_fields {
        omit(result, "fields", format!("schema inspect --type {ty}"));
        while exceeds_budget(result, budget)? && pop_field(result, ty) {}
    }
    if exceeds_budget(result, budget)? {
        return Err(Failure::request(
            "context budget is too small for the core type declaration and omission queries",
        ));
    }
    Ok(())
}
