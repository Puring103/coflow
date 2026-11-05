use super::{Arguments, Result, Selector};
use crate::{Project, ReadOnlyProjectSession, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// 维度视图只保存相对基础字段的差异；上下文单独登记，保留空覆盖变体的存在语义。
    contexts: BTreeMap<(String, String), BTreeMap<FieldCoordinate, DiffFieldState>>,
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
                snapshot
                    .contexts
                    .entry((dimension.name.clone(), variant.clone()))
                    .or_default();
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
        if let Some((dimension, variant)) = dimension.zip(variant) {
            let unchanged_object = session
                .model()
                .record_by_type_id(record.actual_type(), record.id())
                .and_then(|id| session.model().record(id))
                .is_some_and(|base| std::sync::Arc::ptr_eq(&base.object, &record.object));
            let explicit_override = record.dimension_fields.values().any(|field| {
                field.dimension.as_str() == dimension && field.variants.contains_key(variant)
            });
            // 投影共享的未影响记录具有相同有效值；没有显式覆盖时来源状态也与基础一致。
            if unchanged_object && !explicit_override {
                return Ok(());
            }
        }
        let result = super::query::record_result_with_sources(
            project, session, record, selector, sources, false,
        )?;
        for (field, source) in result.source.unwrap_or_default() {
            let state = DiffFieldState {
                assigned: source.assigned,
                origin: source.origin,
                expression: source.expression,
                effective: source.effective,
                rendered: result.rendered.get(&field).cloned(),
            };
            let base = (
                record.actual_type().into(),
                record.id().into(),
                field.clone(),
                None,
                None,
            );
            if dimension.is_some() && self.fields.get(&base) == Some(&state) {
                continue;
            }
            if let Some(context) = dimension.zip(variant) {
                self.contexts
                    .entry((context.0.into(), context.1.into()))
                    .or_default()
                    .insert(base, state);
            } else {
                self.fields.insert(base, state);
            }
        }
        Ok(())
    }

    fn field(
        &self,
        base: &FieldCoordinate,
        context: Option<&(String, String)>,
    ) -> Option<&DiffFieldState> {
        let Some(context) = context else {
            return self.fields.get(base);
        };
        self.contexts
            .get(context)?
            .get(base)
            .or_else(|| self.fields.get(base))
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
        let base_keys = BTreeSet::from_iter(before.fields.keys().chain(after.fields.keys()));
        let mut changed_base = BTreeSet::new();
        for key in &base_keys {
            if let Some(change) =
                field_difference(key, None, before.fields.get(*key), after.fields.get(*key))
            {
                changed_base.insert(*key);
                fields.push(change);
            }
        }
        for context in BTreeSet::from_iter(before.contexts.keys().chain(after.contexts.keys())) {
            let old = before.contexts.get(context);
            let new = after.contexts.get(context);
            // 已有上下文仅比较基础差异与显式差异并集；变体增删仍比较其全部有效字段。
            let keys = if old.is_some() && new.is_some() {
                old.into_iter()
                    .flat_map(BTreeMap::keys)
                    .chain(new.into_iter().flat_map(BTreeMap::keys))
                    .chain(changed_base.iter().copied())
                    .collect::<BTreeSet<_>>()
            } else {
                base_keys.clone()
            };
            for key in keys {
                if let Some(change) = field_difference(
                    key,
                    Some(context),
                    before.field(key, Some(context)),
                    after.field(key, Some(context)),
                ) {
                    dimensions.push(change);
                }
            }
        }
        dimensions.sort_by(|left, right| {
            (
                &left.actual_type,
                &left.id,
                &left.field,
                &left.dimension,
                &left.variant,
            )
                .cmp(&(
                    &right.actual_type,
                    &right.id,
                    &right.field,
                    &right.dimension,
                    &right.variant,
                ))
        });
        // 按变化记录建立一次索引，避免每条记录再扫描全部字段差异。
        let changed_coordinates = fields
            .iter()
            .chain(&dimensions)
            .map(|field| (field.actual_type.as_str(), field.id.as_str()))
            .collect::<BTreeSet<_>>();
        // 两端共有键只比较一次，保持排序与去重语义。
        for key in BTreeSet::from_iter(before.records.keys().chain(after.records.keys())) {
            let previous = before.records.get(key);
            let current = after.records.get(key);
            if previous != current
                || changed_coordinates.contains(&(key.0.as_str(), key.1.as_str()))
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

fn field_difference(
    key: &FieldCoordinate,
    context: Option<&(String, String)>,
    previous: Option<&DiffFieldState>,
    current: Option<&DiffFieldState>,
) -> Option<SemanticFieldDiff> {
    let assignment_changed = assignment_changed(previous, current);
    let effective_changed = previous.and_then(|value| value.effective.as_ref())
        != current.and_then(|value| value.effective.as_ref());
    let rendered_changed = previous.and_then(|value| value.rendered.as_ref())
        != current.and_then(|value| value.rendered.as_ref());
    (assignment_changed || effective_changed || rendered_changed).then(|| SemanticFieldDiff {
        actual_type: key.0.clone(),
        id: key.1.clone(),
        field: key.2.clone(),
        dimension: context.map(|(dimension, _)| dimension.clone()),
        variant: context.map(|(_, variant)| variant.clone()),
        assignment_changed,
        effective_changed,
        rendered_changed,
        before: previous.cloned(),
        after: current.cloned(),
    })
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

fn assignment_changed(before: Option<&DiffFieldState>, after: Option<&DiffFieldState>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => {
            // 大多数字段源码完全相同，无需为它们重新分词；有改动时仍忽略排版和注释。
            before.assigned != after.assigned
                || before.origin != after.origin
                || (before.expression != after.expression
                    && assignment(before) != assignment(after))
        }
        (None, None) => false,
        _ => true,
    }
}

#[cfg(test)]
mod assignment_tests {
    use super::*;

    #[test]
    fn assignment_comparison_preserves_trivia_and_missing_semantics() {
        let state = |expression: Option<&str>| DiffFieldState {
            assigned: true,
            origin: "explicit_base".into(),
            expression: expression.map(str::to_owned),
            effective: None,
            rendered: None,
        };
        for (before, after, changed) in [
            (Some("[1,2]"), Some("[1,2]"), false),
            (Some("[1,2]"), Some("[ 1, # note\n2 ]"), false),
            (Some("[1,2]"), Some("[1,3]"), true),
            (None, None, false),
            (None, Some("None"), true),
        ] {
            assert_eq!(
                assignment_changed(Some(&state(before)), Some(&state(after))),
                changed
            );
        }
        assert!(!assignment_changed(None, None));
        assert!(assignment_changed(Some(&state(None)), None));
        let before = state(Some("1"));
        let mut after = before.clone();
        after.origin = "schema_default".into();
        assert!(assignment_changed(Some(&before), Some(&after)));
        after = before.clone();
        after.assigned = false;
        assert!(assignment_changed(Some(&before), Some(&after)));
    }
}

#[cfg(test)]
#[path = "semantic_diff_sparse_tests.rs"]
mod sparse_tests;

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

#[cfg(test)]
#[path = "semantic_diff_capture_tests.rs"]
mod capture_tests;
