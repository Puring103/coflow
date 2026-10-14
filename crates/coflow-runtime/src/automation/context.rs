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
    let target = if let Some(key) = args.get("key") {
        let record = records
            .iter()
            .find(|record| record.actual_type() == ty && record.key() == key)
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
            .map(|(_, record)| json!({"type": record.actual_type(), "key": record.key()}))
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
                    && args.get("key").is_none_or(|key| {
                        location
                            .record_key
                            .as_deref()
                            .is_none_or(|name| name == key)
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
    let mut result = json!({"schema": declarations, "target": target,
        "rules": super::rules::query("rule.list",session.schema(),project.root_dir(),args)?, "examples": examples, "references": references,
        "dimensions": session.queries().dimensions().into_iter().filter(|dimension|dimension.fields.iter().any(|field|
            session.schema().is_assignable(ty,&field.source_type) || session.schema().is_assignable(&field.source_type,ty))).collect::<Vec<_>>(), "diagnostics": diagnostics,
        "omitted": [], "queries": [format!("data list --type {ty}"), format!("schema inspect --type {ty} --related")]});
    trim(&mut result, ty, args.number("budget", 16000)?)?;
    super::json(serde_json::from_value::<ContextResult>(result)?)
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
            reference_targets(schema, inner, depth, visited, targets)
        }
        CftValueType::Dict(key, value) => {
            reference_targets(schema, key, depth, visited, targets);
            reference_targets(schema, value, depth, visited, targets);
        }
        _ => {}
    }
}

fn omit(result: &mut Value, section: &str, query: String) {
    result["omitted"]
        .as_array_mut()
        .unwrap()
        .push(json!({"section": section, "query": query}));
}

fn trim(result: &mut Value, ty: &str, budget: usize) -> Result<()> {
    // 预算按完整 JSON 字符数计量，省略提示本身也计入预算。
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
        if result.to_string().chars().count() <= budget {
            return Ok(());
        }
        result.as_object_mut().unwrap().remove(name);
        omit(result, name, query);
    }
    if result.to_string().chars().count() > budget {
        if let Some(types) = result["schema"]["types"].as_array_mut() {
            types.retain(|item| item["name"] == ty);
        }
        for name in ["enums", "consts", "dimensions", "diagnostics", "omitted"] {
            result["schema"].as_object_mut().unwrap().remove(name);
        }
        omit(
            result,
            "related_declarations",
            format!("schema inspect --type {ty} --related"),
        );
    }
    if result.to_string().chars().count() > budget && !result["target"].is_null() {
        let key = result["target"]["key"].as_str().unwrap().to_string();
        result.as_object_mut().unwrap().remove("target");
        omit(
            result,
            "target",
            format!("data get --type {ty} --key {key} --view both"),
        );
    }
    // 大声明逐字段裁剪，始终保留类型身份与剩余字段，而不是丢弃整个核心声明。
    let mut omitted_fields = Vec::new();
    while result.to_string().chars().count() > budget {
        let field = result["schema"]["types"]
            .as_array_mut()
            .and_then(|types| types.iter_mut().find(|item| item["name"] == ty))
            .and_then(|item| item["fields"].as_array_mut())
            .and_then(Vec::pop);
        let Some(field) = field else {
            break;
        };
        omitted_fields.push(field["name"].clone());
    }
    if !omitted_fields.is_empty() {
        omit(result, "fields", format!("schema inspect --type {ty}"));
        // 字段列表可继续查询；提示采用单项以免预算被省略清单耗尽。
        while result.to_string().chars().count() > budget {
            let field = result["schema"]["types"]
                .as_array_mut()
                .and_then(|types| types.iter_mut().find(|item| item["name"] == ty))
                .and_then(|item| item["fields"].as_array_mut())
                .and_then(Vec::pop);
            if field.is_none() {
                break;
            }
        }
    }
    if result.to_string().chars().count() > budget {
        return Err(Failure::request(
            "context budget is too small for the core type declaration and omission queries",
        ));
    }
    Ok(())
}
