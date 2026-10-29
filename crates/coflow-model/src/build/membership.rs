use super::draft::RecordDraftSource;
use super::resolve::ValueResolver;
use super::validate::Validator;
use super::{
    BuildSchema, CfdModelBuildOutput, CfdModelBuildStats, CfdModelBuilder, DimensionInputIndex,
    RecordDraft, RecordInputs, TemplateDependencies,
};
use crate::indexes::{
    build_indexes, build_ref_indexes_iter, validate_singletons, ReferenceFieldPlan,
};
use crate::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdObject, CfdPath, CfdRecord, CfdRecordHandle,
    CfdValue, CfdValueSemanticContext, RecordOrigin, ValueValidationMode, ValueValidationRequest,
};
use coflow_language::cft::{RecordId, TypeName};
use coflow_language::limits::StructuralLimits;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;

type DraftPage = Box<[OnceCell<Option<Box<RecordDraft>>>]>;

/// 记录句柄随成员顺序重建；原始草稿只在新记录、变化记录或求值实际读取时验证。
struct MembershipDrafts<'a, 'input> {
    schema: BuildSchema<'a>,
    inputs: &'a RecordInputs<'input>,
    values: Vec<OnceCell<DraftPage>>,
    diagnostics: RefCell<Vec<CfdDiagnostic>>,
    validated: Cell<usize>,
    limits: StructuralLimits,
}

impl RecordDraftSource for MembershipDrafts<'_, '_> {
    fn get(&self, index: usize) -> Option<&RecordDraft> {
        if index >= self.inputs.len() {
            return None;
        }
        self.values
            .get(index / 512)?
            .get_or_init(|| (0..512).map(|_| OnceCell::new()).collect())
            .get(index % 512)?
            .get_or_init(|| {
                let input = self.inputs.get(index)?;
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
                    draft.origin = input.origin.clone();
                }
                self.validated.set(self.validated.get() + 1);
                self.diagnostics.borrow_mut().extend(diagnostics);
                draft.map(Box::new)
            })
            .as_deref()
    }

    fn actual_type(&self, index: usize) -> Option<&str> {
        self.inputs
            .get(index)
            .map(|input| input.actual_type.as_str())
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "成员复用的前提、原序求值、引用校验和新索引发布共同保持一次构建的语义边界。"
)]
pub(super) fn rebuild(
    builder: &CfdModelBuilder<'_>,
    previous: &CfdDataModel,
    unchanged: &[Option<CfdRecordHandle>],
) -> Option<CfdModelBuildOutput> {
    // 已验证维度上下文与旧句柄绑定；本路径仅接管无变体/覆盖的基础模型。
    if !previous.incremental_ready
        || previous.projection.is_some()
        || previous.schema_compilation != builder.schema.compilation_id()
        || previous.structural_limits != builder.structural_limits
        || !previous.dimension_variants.is_empty()
        || !builder.dimension_variants.is_empty()
        || !builder.dimension_values.is_empty()
        || builder
            .dimension_batches
            .iter()
            .any(|batch| !batch.is_empty())
        || unchanged.len() != builder.records.len()
    {
        return None;
    }
    let schema = BuildSchema::new(builder.schema);
    let drafts = MembershipDrafts {
        schema,
        inputs: &builder.records,
        values: (0..builder.records.len().div_ceil(512))
            .map(|_| OnceCell::new())
            .collect(),
        diagnostics: RefCell::new(Vec::new()),
        validated: Cell::new(0),
        limits: builder.structural_limits,
    };
    let mut metadata = Vec::with_capacity(builder.records.len());
    for (index, prior) in unchanged.iter().enumerate() {
        let input = &builder.records[index];
        let actual_type = if let Some(prior) = prior {
            let record = previous.record(*prior)?;
            if record.id() != input.id || record.actual_type() != input.actual_type {
                return None;
            }
            record.actual_type_name().clone()
        } else {
            drafts.get(index)?.actual_type.clone()
        };
        metadata.push(RecordDraft {
            id: input.id.clone(),
            actual_type,
            fields: BTreeMap::new(),
            origin: RecordOrigin::None,
        });
    }
    if !drafts.diagnostics.borrow().is_empty() {
        return None;
    }
    // 复用只省去值树工作；全部身份、继承域冲突和 singleton 约束仍使用原索引校验。
    let mut diagnostics = Vec::new();
    let indexes = build_indexes(schema, &metadata, &mut diagnostics);
    validate_singletons(schema, &metadata, &indexes.tables, &mut diagnostics);
    if !diagnostics.is_empty() {
        return None;
    }

    let mut records = Vec::<Arc<CfdRecord>>::with_capacity(builder.records.len());
    let mut materialized = 0;
    let mut reused = 0;
    let (reads, resolved_root_fields, reused_literal_fields);
    {
        let mut resolver = ValueResolver::new(
            &schema,
            &drafts,
            &indexes.record_by_domain_id,
            &mut diagnostics,
            builder.structural_limits,
        );
        for (index, prior) in unchanged.iter().enumerate() {
            let input = &builder.records[index];
            let reuse = prior
                .and_then(|id| previous.records.get(id.index()).map(|record| (id, record)))
                .filter(|(id, record)| {
                    // 所有模板根完整重算，覆盖默认值、内联对象、集合及虚拟 id 读取。
                    !record.fields().values().any(contains_template)
                        && reference_targets_stable(
                            *id,
                            previous,
                            schema,
                            &metadata,
                            &indexes.record_by_domain_id,
                        )
                });
            if let Some((_, old)) = reuse {
                records.push(if old.origin == input.origin {
                    Arc::clone(old)
                } else {
                    let mut header = old.as_ref().clone();
                    header.origin = input.origin.clone();
                    Arc::new(header)
                });
                reused += 1;
            } else {
                let fields = resolver.resolve_record_fields(CfdRecordHandle::new(index), None)?;
                records.push(Arc::new(CfdRecord {
                    id: RecordId::new(&input.id).ok()?,
                    object: Arc::new(CfdObject::new(metadata[index].actual_type.clone(), fields)),
                    origin: input.origin.clone(),
                    dimension_fields: Arc::default(),
                }));
                materialized += 1;
            }
        }
        if !resolver.take_reference_diagnostics().is_empty() {
            return None;
        }
        reads = resolver.take_reads();
        resolved_root_fields = resolver.resolved_root_fields;
        reused_literal_fields = resolver.reused_literal_fields;
    }
    if !diagnostics.is_empty() || !drafts.diagnostics.borrow().is_empty() {
        return None;
    }
    let plan = ReferenceFieldPlan::new(schema);
    let context = MembershipContext {
        records: &records,
        identities: &indexes.record_by_domain_id,
    };
    // 被删除或替换的目标必须重新检查；引用中保留的显式类型约束已在上面的受影响根重算。
    for record in &records {
        for field in plan.fields(record.actual_type(), true) {
            let value = record.field(field.name.as_str())?;
            let request = ValueValidationRequest::new(
                &field.value_type,
                value,
                ValueValidationMode::Complete,
            );
            if crate::validate_value_for_schema(builder.schema, &context, request).is_err() {
                return None;
            }
        }
    }
    let refs = build_ref_indexes_iter(
        records.iter().map(AsRef::as_ref),
        &indexes.record_by_domain_id,
        schema,
        &plan,
    );
    let count = records.len();
    let statistics = CfdModelBuildStats {
        resolved_root_fields,
        reused_literal_fields,
        dimension_inputs_checked: 0,
        identity_records_checked: count,
        origin_records_checked: count,
        validated_records: drafts.validated.get(),
        materialized_records: materialized,
        reused_records: reused,
        rebuilt_ref_hosts: count,
        rebuilt_ref_edges: refs.edges().count(),
    };
    let mut model = CfdDataModel {
        tables: Arc::new(indexes.tables),
        record_sets: Arc::new(CfdDataModel::empty_record_sets(builder.schema)),
        record_by_domain_id: Arc::new(indexes.record_by_domain_id),
        records: Arc::new(records.into_iter().collect()),
        refs: Arc::new(refs),
        dimension_variants: Arc::default(),
        projection: None,
        template_dependencies: Arc::new(TemplateDependencies::new(reads)),
        incremental_ready: true,
        input_manifest: builder.records.manifest().into(),
        dimension_input_index: None,
        structural_limits: builder.structural_limits,
        schema_compilation: builder.schema.compilation_id(),
    };
    // 空覆盖的批次索引也绑定新句柄，下一次字段修改继续使用普通局部更新路径。
    model.dimension_input_index = DimensionInputIndex::prepare(builder, &model, schema)
        .map(|prepared| Arc::new(prepared.index));
    Some(CfdModelBuildOutput {
        model,
        diagnostics: CfdDiagnostics::new(Vec::new()),
        statistics,
        // 句柄可能整体重排；成员调用方必须按拓扑变化处理后续检查与来源索引。
        changed_records: (0..count).map(CfdRecordHandle::new).collect(),
    })
}

fn contains_template(value: &CfdValue) -> bool {
    match value {
        CfdValue::FormattedString(_) => true,
        CfdValue::OptionSome(value) => contains_template(value),
        CfdValue::Object(object) => object.fields().values().any(contains_template),
        CfdValue::Array(values) => values.iter().any(contains_template),
        CfdValue::Dict(values) => values.iter().any(|(_, value)| contains_template(value)),
        _ => false,
    }
}

fn reference_targets_stable(
    host: CfdRecordHandle,
    previous: &CfdDataModel,
    schema: BuildSchema<'_>,
    metadata: &[RecordDraft],
    identities: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
) -> bool {
    previous.refs.edges_from_host(host).all(|edge| {
        let Some(old) = previous.record(edge.target) else {
            return false;
        };
        schema
            .inheritance_root(old.actual_type())
            .and_then(|root| identities.get(root))
            .and_then(|domain| domain.get(old.id()))
            .and_then(|id| metadata.get(id.index()))
            .is_some_and(|next| next.actual_type == *old.actual_type_name())
    })
}

struct MembershipContext<'a> {
    records: &'a [Arc<CfdRecord>],
    identities: &'a BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
}

impl CfdValueSemanticContext for MembershipContext<'_> {
    fn record_by_domain_id(
        &self,
        inheritance_root: &TypeName,
        id: &str,
    ) -> Option<CfdRecordHandle> {
        self.identities.get(inheritance_root)?.get(id).copied()
    }

    fn record_actual_type(&self, id: CfdRecordHandle) -> Option<&str> {
        self.records
            .get(id.index())
            .map(|record| record.actual_type())
    }
}
