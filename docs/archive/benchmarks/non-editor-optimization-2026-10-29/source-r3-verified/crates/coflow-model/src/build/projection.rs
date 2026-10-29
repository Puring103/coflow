//! 维度完整视图复用健康基础模型，仅重新求值显式覆盖及其模板依赖闭包。
use super::draft::{RecordDraft, RecordDraftSource};
use super::resolve::ValueResolver;
use super::validate::Validator;
use super::{BuildSchema, TemplateField};
use crate::indexes::{build_ref_indexes, ReferenceFieldPlan};
use crate::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdErrorCode, CfdObject, CfdPath,
    CfdRecord, CfdRecordHandle, CfdValue, LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft,
    ValueValidationMode, ValueValidationRequest,
};
use coflow_language::cft::CftSchema;
use coflow_language::limits::StructuralLimits;
use std::cell::{OnceCell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;

/// 只在本次求值需要记录时构造和验证临时输入；不向模型附加常驻输入树。
struct ProjectionDrafts<'a> {
    schema: BuildSchema<'a>,
    source: &'a CfdDataModel,
    selections: &'a BTreeMap<String, String>,
    records: Vec<OnceCell<Option<Box<RecordDraft>>>>,
    diagnostics: RefCell<Vec<CfdDiagnostic>>,
    limits: StructuralLimits,
}

impl RecordDraftSource for ProjectionDrafts<'_> {
    fn get(&self, index: usize) -> Option<&RecordDraft> {
        self.records
            .get(index)?
            .get_or_init(|| {
                let record = self.source.record(CfdRecordHandle::new(index))?;
                let input = match projection_input(record, self.selections) {
                    Ok(input) => input,
                    Err(errors) => {
                        self.diagnostics.borrow_mut().extend(errors.diagnostics);
                        return None;
                    }
                };
                let mut diagnostics = Vec::new();
                let mut draft = Validator::new(&self.schema, &mut diagnostics, self.limits)
                    .validate_top_level_record(
                        None,
                        &input.id,
                        &input.actual_type,
                        &input.fields,
                        Some(CfdRecordHandle::new(index)),
                        CfdPath::root(),
                    );
                if let Some(draft) = &mut draft {
                    draft.origin = input.origin;
                }
                self.diagnostics.borrow_mut().extend(diagnostics);
                draft.map(Box::new)
            })
            .as_deref()
    }

    fn actual_type(&self, index: usize) -> Option<&str> {
        // 不变的身份索引足以检查裸引用；读取目标字段才需要验证其完整输入和预算。
        self.source
            .record(CfdRecordHandle::new(index))
            .map(CfdRecord::actual_type)
    }
}

pub(crate) fn try_project(
    source: &CfdDataModel,
    schema: &CftSchema,
    selections: &BTreeMap<String, String>,
) -> Option<CfdDataModel> {
    project_with_limits(source, schema, selections, StructuralLimits::default())
}

#[expect(
    clippy::too_many_lines,
    reason = "共享前提、依赖闭包、预算求值与引用重建集中保持投影语义可审核。"
)]
fn project_with_limits(
    source: &CfdDataModel,
    schema: &CftSchema,
    selections: &BTreeMap<String, String>,
    limits: StructuralLimits,
) -> Option<CfdDataModel> {
    // 来源错误、schema 身份及预算变化均要求完整重新验证，不能复用旧代次的健康结论。
    if !source.incremental_ready
        || source.schema_compilation != schema.compilation_id()
        || source.structural_limits != limits
        || source.projection.is_some()
    {
        return None;
    }
    let mut seeds = Vec::new();
    for (id, record) in source.records() {
        for (name, variants) in record.dimension_fields.iter() {
            let Some(selected) = selections
                .get(variants.dimension.as_str())
                .and_then(|variant| variants.variants.get(variant.as_str()))
            else {
                continue;
            };
            let base = record.field(name.as_str())?;
            if !base.same_stored_value(&selected.value) {
                seeds.push(TemplateField {
                    record: id,
                    field: name.clone(),
                });
            }
        }
    }
    let affected = source.template_dependencies.affected(&seeds);
    let schema = BuildSchema::new(schema);
    let drafts = ProjectionDrafts {
        schema,
        source,
        selections,
        records: (0..source.record_count())
            .map(|_| OnceCell::new())
            .collect(),
        diagnostics: RefCell::new(Vec::new()),
        limits,
    };
    let mut model = source.clone();
    let mut diagnostics = Vec::new();
    {
        let mut resolver = ValueResolver::new(
            &schema,
            &drafts,
            &source.record_by_domain_id,
            &mut diagnostics,
            limits,
        );
        for id in &affected {
            let fields = resolver.resolve_record_fields(*id, None)?;
            let old = source.record(*id)?;
            let object = CfdObject::new(old.actual_type_name().clone(), fields);
            if !old.object.same_stored_value(&object) {
                let mut record = old.clone();
                record.object = Arc::new(object);
                Arc::make_mut(&mut model.records)[id.index()] = Arc::new(record);
            }
        }
        if !resolver.take_reference_diagnostics().is_empty() {
            return None;
        }
    }
    if !diagnostics.is_empty() || !drafts.diagnostics.borrow().is_empty() {
        return None;
    }

    // 原完整投影先建立无维度覆盖的基础引用索引，再附回覆盖元数据。
    // 临时记录头仅共享 object，不把原模型的全部维度引用边带入当前视图。
    let records = model
        .records()
        .map(|(_, record)| CfdRecord {
            id: record.id.clone(),
            object: Arc::clone(&record.object),
            origin: record.origin.clone(),
            dimension_fields: Arc::default(),
        })
        .collect::<Vec<_>>();
    let context = super::BuildValueSemanticContext {
        records: &records,
        record_by_domain_id: &model.record_by_domain_id,
    };
    let plan = ReferenceFieldPlan::new(schema);
    for id in &affected {
        let record = &records[id.index()];
        for field in plan.fields(record.actual_type(), true) {
            let Some(value) = record.field(field.name.as_str()) else {
                continue;
            };
            let request = ValueValidationRequest::new(
                &field.value_type,
                value,
                ValueValidationMode::Complete,
            );
            if crate::validate_value_for_schema(schema.cft(), &context, request).is_err() {
                return None;
            }
        }
    }
    model.refs = Arc::new(build_ref_indexes(
        &records,
        &model.record_by_domain_id,
        schema,
        &plan,
    ));
    model.input_manifest = Arc::default();
    model.dimension_input_index = None;
    Some(model)
}

/// 完整重新验证与惰性投影共用相同来源转换，模板始终取 source 而非旧 rendered。
pub(crate) fn projection_input(
    record: &CfdRecord,
    selections: &BTreeMap<String, String>,
) -> Result<LoadedRecordDraft, CfdDiagnostics> {
    let fields = record
        .fields()
        .iter()
        .map(|(name, base)| {
            let value = record
                .dimension_fields
                .get(name)
                .and_then(|field| {
                    selections
                        .get(field.dimension.as_str())
                        .and_then(|variant| field.variants.get(variant.as_str()))
                })
                .map_or(base, |value| &value.value);
            Ok((name.to_string(), draft(value)?))
        })
        .collect::<Result<BTreeMap<_, _>, CfdDiagnostics>>()?;
    Ok(
        LoadedRecordDraft::new(record.id(), record.actual_type(), fields)
            .with_origin(record.origin.clone()),
    )
}

fn draft(value: &CfdValue) -> Result<LoadedValueDraft, CfdDiagnostics> {
    use LoadedValueDraft as D;
    Ok(match value {
        CfdValue::OptionNone => D::OptionNone,
        CfdValue::OptionSome(value) => D::OptionSome(Box::new(draft(value)?)),
        CfdValue::Bool(value) => D::Bool(*value),
        CfdValue::Int(value) => D::Int(*value),
        CfdValue::Float(value) => D::Float(*value),
        CfdValue::String(value) => D::String(value.clone()),
        CfdValue::FormattedString(value) => D::FormattedString(
            crate::cell_value::parse_formatted_string(&value.source).map_err(|error| {
                CfdDiagnostics::new(
                    error
                        .diagnostics
                        .into_iter()
                        .map(|error| {
                            CfdDiagnostic::error(CfdErrorCode::TypeMismatch, error.message)
                        })
                        .collect(),
                )
            })?,
        ),
        CfdValue::Enum(value) => D::EnumValue {
            enum_name: value.enum_name.to_string(),
            value: value.value,
        },
        CfdValue::Ref(key) => D::RecordRef(key.to_string()),
        CfdValue::Object(value) => D::Object {
            actual_type: Some(value.actual_type().into()),
            fields: value
                .fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), draft(value)?)))
                .collect::<Result<_, CfdDiagnostics>>()?,
        },
        CfdValue::Array(values) => D::Array(values.iter().map(draft).collect::<Result<_, _>>()?),
        CfdValue::Dict(values) => D::Dict(
            values
                .iter()
                .map(|(key, value)| {
                    Ok((
                        match key {
                            CfdDictKey::String(value) => LoadedDictKeyDraft::String(value.clone()),
                            CfdDictKey::Int(value) => LoadedDictKeyDraft::Int(*value),
                            CfdDictKey::Enum(value) => LoadedDictKeyDraft::EnumValue {
                                enum_name: value.enum_name.to_string(),
                                value: value.value,
                            },
                        },
                        draft(value)?,
                    ))
                })
                .collect::<Result<_, CfdDiagnostics>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        reason = "小结构预算直接验证投影入口与完整构建的边界。"
    )]
    use super::*;
    use crate::{CfdModelBuilder, DimensionValueDraft, RecordOrigin};
    use coflow_language::cft::{
        build_schema, parse_modules, CftFile, DimensionName, FieldName, ModuleId, RecordId,
        TypeName, VariantName,
    };

    #[test]
    fn combined_projection_enforces_the_whole_record_budget_at_exact_boundary() {
        let schema = build_schema(&parse_modules([CftFile::new(
            ModuleId::from("main"),
            "main.cft".into(),
            r#"table Item { @dimension("left") a: [int]; @dimension("right") z: [int]; }"#,
        )]))
        .unwrap();
        let selections = BTreeMap::from([
            ("left".into(), "large".into()),
            ("right".into(), "large".into()),
        ]);
        for max_nodes in [6, 9] {
            let limits = StructuralLimits::new(10, max_nodes, 100);
            let mut builder = CfdModelBuilder::new(&schema).with_structural_limits(limits);
            builder.add_record(
                "a",
                "Item",
                [
                    ("a", LoadedValueDraft::Array(Vec::new())),
                    ("z", LoadedValueDraft::Array(Vec::new())),
                ],
            );
            for (field, dimension) in [("a", "left"), ("z", "right")] {
                builder.add_dimension_value_draft(DimensionValueDraft {
                    source_type: TypeName::new("Item").unwrap(),
                    source_id: RecordId::new("a").unwrap(),
                    field: FieldName::new(field).unwrap(),
                    dimension: DimensionName::new(dimension).unwrap(),
                    variant: VariantName::new("large").unwrap(),
                    origin: RecordOrigin::None,
                    value: LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into(), 3_i64.into()]),
                });
            }
            // 每个单维覆盖恰好占六个节点，合并两维后需要九个节点。
            let source = builder.build().unwrap();
            let fast = project_with_limits(&source, &schema, &selections, limits);
            let mut full = CfdModelBuilder::new(&schema).with_structural_limits(limits);
            for (_, record) in source.records() {
                full.add_loaded_record(projection_input(record, &selections).unwrap());
            }
            match (fast, full.build()) {
                (None, Err(diagnostics)) => {
                    assert_eq!(max_nodes, 6);
                    let diagnostic = diagnostics
                        .diagnostics
                        .iter()
                        .find(|diagnostic| {
                            diagnostic.code == CfdErrorCode::DataStructureLimitExceeded
                        })
                        .unwrap();
                    assert_eq!(
                        diagnostic.primary.as_ref().unwrap().path,
                        CfdPath::root().field("z").index(0)
                    );
                }
                (Some(fast), Ok(full)) => {
                    assert_eq!(max_nodes, 9);
                    for ((_, actual), (_, expected)) in fast.records().zip(full.records()) {
                        assert_eq!(actual.object, expected.object);
                    }
                    assert_eq!(fast.refs, full.refs);
                }
                (fast, full) => panic!("fast/full budget mismatch: {fast:?} / {full:?}"),
            }
        }
    }
}
