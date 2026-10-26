use super::{Arguments, Result, Selector};
use crate::{Project, ReadOnlyProjectSession, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiffFieldState {
    pub assigned: bool,
    pub origin: String,
    pub expression: Option<String>,
    pub effective: Option<super::ModelValue>,
    pub rendered: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticFieldDiff {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    pub field: String,
    pub dimension: Option<String>,
    pub variant: Option<String>,
    pub assignment_changed: bool,
    pub effective_changed: bool,
    pub rendered_changed: bool,
    pub before: Option<DiffFieldState>,
    pub after: Option<DiffFieldState>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticRecordDiff {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
    pub change: String,
    pub before_file: Option<String>,
    pub after_file: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SchemaDifference {
    pub section: String,
    pub symbol: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticDifference {
    pub semantic_available: bool,
    pub schema_available: bool,
    pub schema: Vec<SchemaDifference>,
    pub records: Vec<SemanticRecordDiff>,
    pub fields: Vec<SemanticFieldDiff>,
    pub dimensions: Vec<SemanticFieldDiff>,
    pub changed_records: usize,
    pub changed_fields: usize,
    pub changed_dimension_fields: usize,
    pub changed_schema_symbols: usize,
}

type Coordinate = (String, String);
type FieldCoordinate = (String, String, String, Option<String>, Option<String>);
#[derive(Debug, Default)]
pub(crate) struct SemanticSnapshot {
    pub(crate) available: bool,
    schema_available: bool,
    pub(crate) diagnostics: crate::DiagnosticSet,
    schema: BTreeMap<(String, String), Value>,
    records: BTreeMap<Coordinate, Option<String>>,
    fields: BTreeMap<FieldCoordinate, DiffFieldState>,
}

impl SemanticSnapshot {
    pub fn capture(project: &Project, path: impl Fn(&Path) -> Result<String>) -> Result<Self> {
        let schema_session = match Runtime::open_schema_session(project.clone()) {
            Ok(session) => session,
            Err(diagnostics) => {
                return Ok(Self {
                    diagnostics,
                    ..Default::default()
                })
            }
        };
        Self::capture_from_schema(&schema_session, None, path)
    }

    /// 候选事务共享声明与最终校验的数据会话，差异计算不重复编译或加载同一后态。
    #[expect(
        clippy::too_many_lines,
        reason = "快照统一采集声明、主体和维度，确保同一会话的差异基线。"
    )]
    pub(crate) fn capture_from_schema(
        schema_session: &crate::ProjectSchemaSession,
        loaded: Option<&ReadOnlyProjectSession>,
        path: impl Fn(&Path) -> Result<String>,
    ) -> Result<Self> {
        let mut snapshot = Self {
            diagnostics: schema_session.diagnostics().as_set().clone(),
            ..Self::default()
        };
        if schema_session.schema().is_some() {
            snapshot.schema_available = true;
            let report = super::json(crate::inspect_schema(schema_session, None, false))?;
            for section in ["types", "enums", "consts", "dimensions"] {
                for item in report[section].as_array().into_iter().flatten() {
                    if let Some(name) = item["name"].as_str() {
                        snapshot
                            .schema
                            .insert((section.into(), name.into()), item.clone());
                    }
                }
            }
            if let Some(schema) = schema_session.schema() {
                for info in super::rules::named_rules(schema) {
                    let mut rule = super::json(super::rules::describe(
                        schema,
                        info,
                        schema_session.project.root_dir(),
                    ))?;
                    rule.as_object_mut().map(|rule| rule.remove("location"));
                    snapshot
                        .schema
                        .insert(("rules".into(), super::rules::identity(info)), rule);
                }
            }
        }
        let built;
        let session = if let Some(loaded) = loaded {
            loaded
        } else {
            built = match crate::sessions::session_build::open_data_session_from_schema(
                schema_session.clone(),
            ) {
                Ok(session) => ReadOnlyProjectSession { session },
                Err(diagnostics) => {
                    snapshot.diagnostics = diagnostics;
                    return Ok(snapshot);
                }
            };
            &built
        };
        if session.queries().has_diagnostics() {
            snapshot.diagnostics = session.queries().diagnostics().as_set().clone();
            return Ok(snapshot);
        }
        snapshot.available = true;
        let mut sources = super::query::RecordSources::default();
        let selector = Selector {
            view: "both".into(),
            ..Default::default()
        };
        for (_, record) in session.model().records() {
            let file = session
                .queries()
                .file_for_record(record.actual_type(), record.id())
                .map(|file| path(&schema_session.project.resolve_path(Path::new(file))))
                .transpose()?;
            snapshot
                .records
                .insert((record.actual_type().into(), record.id().into()), file);
            snapshot.capture_record(
                &schema_session.project,
                session,
                record,
                &selector,
                None,
                None,
                &mut sources,
            )?;
        }
        // 每个维度投影先整体合并再渲染，间接引用产生的变化也进入字段差异。
        for dimension in session.queries().dimensions() {
            for variant in dimension.variants {
                let selector = Selector {
                    view: "both".into(),
                    dimensions: BTreeMap::from([(dimension.name.clone(), variant.clone())]),
                    ..Default::default()
                };
                let projected =
                    match super::query::project_dimensions(session, &selector.dimensions) {
                        Ok(projected) => projected,
                        Err(failure) => {
                            snapshot.available = false;
                            snapshot.diagnostics.extend(failure.diagnostics);
                            return Ok(snapshot);
                        }
                    };
                for (_, record) in projected.records() {
                    snapshot.capture_record(
                        &schema_session.project,
                        session,
                        record,
                        &selector,
                        Some(&dimension.name),
                        Some(variant.as_str()),
                        &mut sources,
                    )?;
                }
            }
        }
        Ok(snapshot)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "快照采集显式传入记录身份、维度、来源及会话，共享调用方缓存。"
    )]
    fn capture_record(
        &mut self,
        project: &Project,
        session: &ReadOnlyProjectSession,
        record: &crate::CfdRecord,
        selector: &Selector,
        dimension: Option<&str>,
        variant: Option<&str>,
        sources: &mut super::query::RecordSources,
    ) -> Result<()> {
        let result: super::RecordResult = serde_json::from_value(
            super::query::record_json_with_sources(project, session, record, selector, sources)?,
        )?;
        for (field, source) in result.source.unwrap_or_default() {
            self.fields.insert(
                (
                    record.actual_type().into(),
                    record.id().into(),
                    field.clone(),
                    dimension.map(str::to_string),
                    variant.map(str::to_string),
                ),
                DiffFieldState {
                    assigned: source.assigned,
                    origin: source.origin,
                    expression: source.expression,
                    effective: source.effective,
                    rendered: result.rendered.get(&field).cloned(),
                },
            );
        }
        Ok(())
    }
}

pub(crate) fn compare(before: &SemanticSnapshot, after: &SemanticSnapshot) -> SemanticDifference {
    let available = before.available && after.available;
    let schema_available = before.schema_available && after.schema_available;
    let schema: Vec<_> = before
        .schema
        .keys()
        .chain(after.schema.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|_| schema_available)
        .filter(|key| before.schema.get(*key) != after.schema.get(*key))
        .map(|(section, symbol)| SchemaDifference {
            section: section.clone(),
            symbol: symbol.clone(),
            before: before
                .schema
                .get(&(section.clone(), symbol.clone()))
                .cloned(),
            after: after
                .schema
                .get(&(section.clone(), symbol.clone()))
                .cloned(),
        })
        .collect();
    let mut records = Vec::new();
    let mut fields = Vec::new();
    let mut dimensions = Vec::new();
    if available {
        // 两端共有键只比较一次，保持排序与去重语义。
        for key in BTreeSet::from_iter(before.fields.keys().chain(after.fields.keys())) {
            let previous = before.fields.get(key);
            let current = after.fields.get(key);
            let assignment_changed = previous.map(assignment) != current.map(assignment);
            let effective_changed = previous.and_then(|value| value.effective.as_ref())
                != current.and_then(|value| value.effective.as_ref());
            let rendered_changed = previous.and_then(|value| value.rendered.as_ref())
                != current.and_then(|value| value.rendered.as_ref());
            if !assignment_changed && !effective_changed && !rendered_changed {
                continue;
            }
            let change = SemanticFieldDiff {
                actual_type: key.0.clone(),
                id: key.1.clone(),
                field: key.2.clone(),
                dimension: key.3.clone(),
                variant: key.4.clone(),
                assignment_changed,
                effective_changed,
                rendered_changed,
                before: previous.cloned(),
                after: current.cloned(),
            };
            if key.3.is_some() {
                dimensions.push(change);
            } else {
                fields.push(change);
            }
        }
        // 两端共有键只比较一次，保持排序与去重语义。
        for key in BTreeSet::from_iter(before.records.keys().chain(after.records.keys())) {
            let previous = before.records.get(key);
            let current = after.records.get(key);
            if previous != current
                || fields
                    .iter()
                    .chain(&dimensions)
                    .any(|field| field.actual_type == key.0 && field.id == key.1)
            {
                records.push(SemanticRecordDiff {
                    actual_type: key.0.clone(),
                    id: key.1.clone(),
                    change: if previous.is_none() {
                        "added"
                    } else if current.is_none() {
                        "deleted"
                    } else {
                        "modified"
                    }
                    .into(),
                    before_file: previous.cloned().flatten(),
                    after_file: current.cloned().flatten(),
                });
            }
        }
    }
    let changed_fields = fields
        .iter()
        .chain(&dimensions)
        .map(|field| (&field.actual_type, &field.id, &field.field))
        .collect::<BTreeSet<_>>()
        .len();
    SemanticDifference {
        semantic_available: available,
        schema_available,
        changed_schema_symbols: schema.len(),
        schema,
        changed_records: records.len(),
        changed_fields,
        changed_dimension_fields: dimensions.len(),
        records,
        fields,
        dimensions,
    }
}

fn assignment(value: &DiffFieldState) -> (bool, &str, Option<Vec<&str>>) {
    // 排版与注释属于源码差异，不把它们计入显式赋值变化。
    (
        value.assigned,
        &value.origin,
        value.expression.as_deref().map(|source| {
            coflow_language::lexical::tokenize_lossless(source)
                .into_iter()
                .filter(|token| !token.is_trivia())
                .map(|token| token.text(source))
                .collect()
        }),
    )
}

pub(crate) fn select(mut difference: SemanticDifference, args: &Arguments) -> Result<Value> {
    if let Some(ty) = args.get("type") {
        difference.records.retain(|record| record.actual_type == ty);
        difference.fields.retain(|field| field.actual_type == ty);
        difference
            .dimensions
            .retain(|field| field.actual_type == ty);
        difference.changed_records = difference.records.len();
        difference.changed_fields = difference
            .fields
            .iter()
            .chain(&difference.dimensions)
            .map(|field| (&field.actual_type, &field.id, &field.field))
            .collect::<BTreeSet<_>>()
            .len();
        difference.changed_dimension_fields = difference.dimensions.len();
    }
    let mut result = super::json(&difference)?;
    if args.flag("summary") {
        result["schema"] = json!(difference
            .schema
            .iter()
            .map(|item| json!({"section":item.section,"symbol":item.symbol}))
            .collect::<Vec<_>>());
        for section in ["fields", "dimensions"] {
            result.as_object_mut().map(|value| value.remove(section));
        }
    }
    match args.get("view") {
        None => {}
        Some(view @ ("source" | "schema" | "record" | "field" | "dimension")) => {
            let keep = match view {
                "record" => "records",
                "field" => "fields",
                "dimension" => "dimensions",
                other => other,
            };
            for section in ["schema", "records", "fields", "dimensions"] {
                if section != keep {
                    result.as_object_mut().map(|value| value.remove(section));
                }
            }
        }
        Some(_) => {
            return Err(super::Failure::request(
                "diff view must be source, schema, record, field or dimension",
            ))
        }
    }
    Ok(result)
}
