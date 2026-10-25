mod context;
mod defaults;
mod draft;
mod incremental;
mod inputs;
mod resolve;
mod validate;

pub(crate) use context::BuildSchema;
pub(crate) use draft::{RecordDraft, ValueDraft};
pub(crate) use incremental::TemplateDependencies;

use crate::diagnostics::{CfdDiagnostic, CfdDiagnostics, CfdLabel, CfdPath, RecordOrigin};
use crate::indexes::{self, build_ref_indexes, ReferenceFieldPlan};
use crate::ingest::{DimensionValueDraft, LoadedRecordDraft, LoadedValueDraft};
use crate::model::{
    CfdDataModel, CfdDimensionFieldValues, CfdDimensionValue, CfdObject, CfdRecord,
    CfdRecordHandle, CfdValue,
};
use crate::semantics::{
    CfdValueSemanticContext, CfdValueSemanticErrorKind, ValueValidationMode, ValueValidationRequest,
};
use coflow_language::cft::{CftSchema, DimensionName, FieldName, RecordId, TypeName, VariantName};
use coflow_language::limits::StructuralLimits;
use resolve::ValueResolver;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::Arc;
use validate::Validator;
use inputs::RecordInputs;

#[derive(Debug)]
pub struct CfdModelBuilder<'a> {
    schema: &'a CftSchema,
    records: RecordInputs<'a>,
    dimension_values: Vec<DraftInput<'a, DimensionValueDraft>>,
    dimension_variants: BTreeMap<DimensionName, BTreeSet<VariantName>>,
    structural_limits: StructuralLimits,
}

/// 拥有态放在 Box 内，使借用缓存时的输入表每项只占指针大小量级。
/// Cow<LoadedRecordDraft> 会让每个借用项也承担整条拥有态记录的内联布局。
#[derive(Debug)]
pub(crate) enum DraftInput<'a, T> {
    Borrowed(&'a T),
    Owned(Box<T>),
}
pub(crate) type RecordInput<'a> = DraftInput<'a, LoadedRecordDraft>;

impl<T> Deref for DraftInput<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(input) => input,
            Self::Owned(input) => input,
        }
    }
}

#[derive(Debug)]
pub struct CfdModelBuildOutput {
    pub model: CfdDataModel,
    pub diagnostics: CfdDiagnostics,
    pub statistics: CfdModelBuildStats,
    /// 当前代次中值语义发生变化的记录；仅来源位置变化不在其中。
    pub changed_records: Vec<CfdRecordHandle>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CfdModelBuildStats {
    pub validated_records: usize,
    pub materialized_records: usize,
    pub reused_records: usize,
    pub rebuilt_ref_hosts: usize,
    pub rebuilt_ref_edges: usize,
}

impl<'a> CfdModelBuilder<'a> {
    #[must_use]
    pub fn new(schema: &'a CftSchema) -> Self {
        Self {
            schema,
            records: RecordInputs::default(),
            dimension_values: Vec::new(),
            dimension_variants: BTreeMap::new(),
            structural_limits: StructuralLimits::default(),
        }
    }

    #[must_use]
    pub fn with_structural_limits(mut self, structural_limits: StructuralLimits) -> Self {
        self.structural_limits = structural_limits;
        self
    }

    pub fn add_record(
        &mut self,
        id: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> &mut Self {
        self.records
            .push(RecordInput::Owned(Box::new(LoadedRecordDraft::new(
                id,
                actual_type,
                fields,
            ))));
        self
    }

    pub fn add_loaded_record(&mut self, record: LoadedRecordDraft) -> &mut Self {
        self.records.push(RecordInput::Owned(Box::new(record)));
        self
    }

    /// 复用文件缓存中的不可变输入，构建过程中不克隆整条原始值树。
    pub fn add_borrowed_record(&mut self, record: &'a LoadedRecordDraft) -> &mut Self {
        self.records.push(RecordInput::Borrowed(record));
        self
    }

    /// 按不可变源批次借用，不为每条记录另建输入指针；批次顺序就是记录顺序。
    pub fn add_borrowed_records(&mut self, records: &'a [LoadedRecordDraft]) -> &mut Self {
        self.records.push_batch(records);
        self
    }

    pub fn add_dimension_variant(
        &mut self,
        dimension: DimensionName,
        variant: VariantName,
    ) -> &mut Self {
        self.dimension_variants
            .entry(dimension)
            .or_default()
            .insert(variant);
        self
    }

    pub fn add_dimension_value_draft(&mut self, value: DimensionValueDraft) -> &mut Self {
        self.dimension_values.push(DraftInput::Owned(Box::new(value)));
        self
    }

    pub fn add_dimension_value_drafts(
        &mut self,
        values: impl IntoIterator<Item = DimensionValueDraft>,
    ) -> &mut Self {
        self.dimension_values.extend(values.into_iter().map(|value| DraftInput::Owned(Box::new(value))));
        self
    }

    /// 维度草稿与普通记录一样借用源缓存，局部修改不深复制所有覆盖值树。
    pub fn add_borrowed_dimension_value_drafts(&mut self, values: impl IntoIterator<Item=&'a DimensionValueDraft>) -> &mut Self {
        self.dimension_values.extend(values.into_iter().map(DraftInput::Borrowed));
        self
    }

    /// Builds a validated in-memory data model from source-neutral drafts.
    ///
    /// # Errors
    ///
    /// Returns data-model diagnostics for invalid values, duplicate keys, or
    /// unresolved references.
    pub fn build(self) -> Result<CfdDataModel, CfdDiagnostics> {
        let output = self.build_partial()?;
        if output.diagnostics.is_empty() {
            Ok(output.model)
        } else {
            Err(output.diagnostics)
        }
    }

    /// Builds a model while preserving source states that can be repaired by
    /// later mutations.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when invalid input cannot be represented without
    /// dropping records or values.
    pub fn build_partial(mut self) -> Result<CfdModelBuildOutput, CfdDiagnostics> {
        // 变体来自已选 CFD，空覆盖块同样声明可供选择的变体。
        for value in &self.dimension_values {
            self.dimension_variants
                .entry(value.dimension.clone())
                .or_default()
                .insert(value.variant.clone());
        }
        let mut output = ModelCompiler::new(
            self.schema,
            self.records,
            self.dimension_values,
            self.dimension_variants.clone(),
            self.structural_limits,
        )
        .build()?;
        output.model.dimension_variants = self.dimension_variants.into();
        Ok(output)
    }

    /// 尝试复用同一 schema、同一记录身份与顺序的健康模型。
    /// `changed` 必须包含所有基础值或维度覆盖发生变化的宿主；来源位置自动刷新。
    /// 不满足增量条件或需要重新归并完整诊断时返回 `Ok(None)`，调用方执行全量构建。
    pub fn try_build_partial_incremental(
        self,
        previous: &CfdDataModel,
        changed: &[CfdRecordHandle],
    ) -> Result<Option<CfdModelBuildOutput>, CfdDiagnostics> {
        incremental::rebuild(self, previous, changed)
    }
}

pub(crate) struct ModelCompiler<'a> {
    schema: BuildSchema<'a>,
    input: RecordInputs<'a>,
    dimension_values: Vec<DraftInput<'a, DimensionValueDraft>>,
    dimension_variants: BTreeMap<DimensionName, BTreeSet<VariantName>>,
    diagnostics: Vec<CfdDiagnostic>,
    editable_diagnostics: Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
    template_reads: BTreeMap<CfdRecordHandle, BTreeSet<CfdRecordHandle>>,
}

impl<'a> ModelCompiler<'a> {
    pub(crate) fn new(
        schema_source: &'a CftSchema,
        input: RecordInputs<'a>,
        dimension_values: Vec<DraftInput<'a, DimensionValueDraft>>,
        dimension_variants: BTreeMap<DimensionName, BTreeSet<VariantName>>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema: BuildSchema::new(schema_source),
            input,
            dimension_values,
            dimension_variants,
            diagnostics: Vec::new(),
            editable_diagnostics: Vec::new(),
            structural_limits,
            template_reads: BTreeMap::new(),
        }
    }

    pub(crate) fn build(mut self) -> Result<CfdModelBuildOutput, CfdDiagnostics> {
        let drafts = self.validate_input_records();
        self.move_editable_diagnostics(|code| {
            matches!(
                code,
                crate::CfdErrorCode::MissingRequiredField | crate::CfdErrorCode::InvalidEnumVariant
            )
        });
        self.fail_if_diagnostics()?;

        let indexes = indexes::build_indexes(self.schema, &drafts, &mut self.diagnostics);
        // Singleton diagnostics intentionally join the rest of index validation.
        indexes::validate_singletons(self.schema, &drafts, &indexes.tables, &mut self.diagnostics);
        self.move_editable_diagnostics(|code| {
            matches!(
                code,
                crate::CfdErrorCode::SingletonRecordCountInvalid
                    | crate::CfdErrorCode::SingletonKeyMissingOrInvalid
                    | crate::CfdErrorCode::SingletonKeyCollision
            )
        });
        self.fail_if_diagnostics()?;

        let mut records = self.resolve_records(&drafts, &indexes.record_by_domain_id);
        self.fail_if_diagnostics()?;
        let reference_plan = ReferenceFieldPlan::new(self.schema);
        validate_resolved_records(
            self.schema,
            &records,
            &indexes.record_by_domain_id,
            &mut self.diagnostics,
            &reference_plan,
            self.editable_diagnostics.is_empty(),
        );
        self.move_editable_diagnostics(|code| {
            matches!(
                code,
                crate::CfdErrorCode::RefTargetNotFound | crate::CfdErrorCode::RefTargetTypeMismatch
            )
        });
        self.fail_if_diagnostics()?;

        let validated_dimension_values =
            self.validate_dimension_values(&records, &indexes.record_by_domain_id);
        self.move_editable_diagnostics(|_| true);

        let (dimension_values, effective_values) = self.resolve_dimension_values(
            &drafts,
            &indexes.record_by_domain_id,
            &validated_dimension_values,
        );
        self.move_editable_diagnostics(|_| true);
        attach_dimension_values(&mut records, dimension_values);
        for (record_id, field, dimension, variant, value) in effective_values {
            if let Some(record) = records.get_mut(record_id.index()) {
                Arc::make_mut(&mut record.dimension_fields)
                    .entry(field)
                    .or_insert_with(|| CfdDimensionFieldValues {
                        dimension,
                        variants: BTreeMap::new(),
                        effective_variants: BTreeMap::new(),
                    })
                    .effective_variants
                    .insert(variant, value);
            }
        }

        let ref_indexes = build_ref_indexes(
            &records,
            &indexes.record_by_domain_id,
            self.schema,
            &reference_plan,
        );
        let record_count = records.len();
        let ref_count = ref_indexes.edges().count();
        let incremental_ready = self.editable_diagnostics.is_empty();
        Ok(CfdModelBuildOutput {
            model: CfdDataModel {
                tables: indexes.tables.into(),
                record_sets: Arc::new(self.schema.cft().all_types().map(|ty| (ty.name.clone(), std::sync::OnceLock::new())).collect()),
                record_by_domain_id: indexes.record_by_domain_id.into(),
                records: records.into_iter().map(Arc::new).collect::<coflow_storage::PagedVec<_>>().into(),
                refs: ref_indexes.into(),
                dimension_variants: BTreeMap::new().into(),
                projection: None,
                template_dependencies: Arc::new(TemplateDependencies::new(self.template_reads)),
                incremental_ready,
                structural_limits: self.structural_limits,
                schema_compilation: self.schema.cft().compilation_id(),
            },
            diagnostics: CfdDiagnostics::new(self.editable_diagnostics),
            statistics: CfdModelBuildStats {
                validated_records: drafts.len(),
                materialized_records: record_count,
                reused_records: 0,
                rebuilt_ref_hosts: record_count,
                rebuilt_ref_edges: ref_count,
            },
            changed_records: (0..record_count).map(CfdRecordHandle::new).collect(),
        })
    }

    fn move_editable_diagnostics(&mut self, predicate: impl Fn(crate::CfdErrorCode) -> bool) {
        let (editable, blocking) = std::mem::take(&mut self.diagnostics)
            .into_iter()
            .partition(|diagnostic| predicate(diagnostic.code));
        self.editable_diagnostics.extend(editable);
        self.diagnostics = blocking;
    }

    fn validate_input_records(&mut self) -> Vec<RecordDraft> {
        let mut drafts = Vec::new();
        let mut validator =
            Validator::new(&self.schema, &mut self.diagnostics, self.structural_limits);
        for (input_index, record) in std::mem::take(&mut self.input).into_iter().enumerate() {
            let id = CfdRecordHandle::new(input_index);
            let Some(mut draft) = validator.validate_top_level_record(
                None,
                &record.id,
                &record.actual_type,
                &record.fields,
                Some(id),
                CfdPath::root(),
            ) else {
                continue;
            };
            draft.origin = record.origin.clone();
            drafts.push(draft);
        }
        drafts
    }

    fn resolve_records(
        &mut self,
        drafts: &[RecordDraft],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    ) -> Vec<CfdRecord> {
        let mut records = Vec::with_capacity(drafts.len());
        // 无维度记录共享同一个空索引，不能为每条记录单独分配空 Arc。
        let empty_dimension_fields = Arc::default();
        let mut resolver = ValueResolver::new(
            &self.schema,
            drafts,
            record_by_domain_id,
            &mut self.diagnostics,
            self.structural_limits,
        );
        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordHandle::new(index);
            let Some(fields) = resolver.resolve_record_fields(record_id, None) else {
                continue;
            };
            let Ok(id) = RecordId::new(&draft.id) else {
                continue;
            };
            records.push(CfdRecord {
                id,
                object: CfdObject {
                    actual_type: draft.actual_type.clone(),
                    fields,
                }
                .into(),
                origin: draft.origin.clone(),
                dimension_fields: Arc::clone(&empty_dimension_fields),
            });
        }
        self.editable_diagnostics
            .extend(resolver.take_reference_diagnostics());
        merge_reads(&mut self.template_reads, resolver.take_reads());
        records
    }

    fn validate_dimension_values(
        &mut self,
        records: &[CfdRecord],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    ) -> Vec<ValidatedDimensionValue<'a>> {
        let mut seen = BTreeSet::new();
        let mut values = Vec::new();
        for input in std::mem::take(&mut self.dimension_values) {
            if let Some(value) =
                self.validate_dimension_value(input, records, record_by_domain_id, &mut seen)
            {
                values.push(value);
            }
        }
        values
    }

    #[allow(clippy::too_many_lines)]
    fn validate_dimension_value(
        &mut self,
        input: DraftInput<'a, DimensionValueDraft>,
        records: &[CfdRecord],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
        seen: &mut BTreeSet<(CfdRecordHandle, FieldName, VariantName)>,
    ) -> Option<ValidatedDimensionValue<'a>> {
        let path = CfdPath::root().field(input.field.as_str());
        let Some(inheritance_root) = self.schema.inheritance_root(input.source_type.as_str())
        else {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                None,
                path,
                crate::CfdErrorCode::UnknownType,
                format!("unknown dimension source type `{}`", input.source_type),
            ));
            return None;
        };
        let Some(record_id) = record_by_domain_id
            .get(inheritance_root)
            .and_then(|records| records.get(input.source_id.as_str()))
            .copied()
        else {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                None,
                path,
                crate::CfdErrorCode::RefTargetNotFound,
                format!(
                    "dimension target `{}::{}` does not exist",
                    input.source_type, input.source_id
                ),
            ));
            return None;
        };
        let record = records.get(record_id.index())?;
        if !self
            .schema
            .is_assignable(record.actual_type(), input.source_type.as_str())
        {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                Some(record_id),
                path,
                crate::CfdErrorCode::ObjectTypeMismatch,
                format!(
                    "dimension owner type `{}` is not assignable to `{}`",
                    record.actual_type(),
                    input.source_type
                ),
            ));
            return None;
        }
        let Some(field) = self.schema.field(record.actual_type(), &input.field) else {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                Some(record_id),
                path,
                crate::CfdErrorCode::UnknownField,
                format!(
                    "unknown dimension field `{}.{}`",
                    record.actual_type(),
                    input.field
                ),
            ));
            return None;
        };
        let Some(binding) = &field.dimension else {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                Some(record_id),
                path,
                crate::CfdErrorCode::TypeMismatch,
                format!(
                    "field `{}.{}` is not dimensional",
                    record.actual_type(),
                    input.field
                ),
            ));
            return None;
        };
        if binding.dimension != input.dimension {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                Some(record_id),
                path,
                crate::CfdErrorCode::TypeMismatch,
                format!(
                    "field `{}.{}` uses dimension `{}`, not `{}`",
                    record.actual_type(),
                    input.field,
                    binding.dimension,
                    input.dimension
                ),
            ));
            return None;
        }
        if !seen.insert((record_id, input.field.clone(), input.variant.clone())) {
            self.diagnostics.push(dimension_diagnostic(
                &input,
                Some(record_id),
                path,
                crate::CfdErrorCode::DuplicateId,
                format!(
                    "duplicate dimension value `{}:{}.{}/{}`",
                    input.source_type, input.source_id, input.field, input.variant
                ),
            ));
            return None;
        }
        let field_ty = field.value_type.clone();
        let path = CfdPath::root().field(input.field.as_str());
        let diagnostic_start = self.diagnostics.len();
        let draft = Validator::new(&self.schema, &mut self.diagnostics, self.structural_limits)
            .validate_value(
                &field_ty,
                &input.value,
                Some(record_id),
                path.clone(),
                coflow_language::limits::TraversalCursor::root(),
            );
        attach_origin_to_diagnostics(
            &mut self.diagnostics[diagnostic_start..],
            &input.origin,
            Some(record_id),
            &path,
        );
        draft.map(|draft| (record_id, input, draft, path))
    }

    fn resolve_dimension_values(
        &mut self,
        drafts: &[RecordDraft],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
        validated: &[ValidatedDimensionValue<'_>],
    ) -> (Vec<ResolvedDimensionValue>, Vec<EffectiveDimensionValue>) {
        let mut explicit_values = Vec::new();
        let mut effective_values = Vec::new();
        let inputs = validated
            .iter()
            .map(|(record, input, _, _)| {
                (
                    (
                        *record,
                        input.field.clone(),
                        input.dimension.clone(),
                        input.variant.clone(),
                    ),
                    &**input,
                )
            })
            .collect::<DimensionInputs<'_>>();
        let mut diagnostic_contexts = Vec::new();
        {
            let mut resolver = ValueResolver::new(
                &self.schema,
                drafts,
                record_by_domain_id,
                &mut self.diagnostics,
                self.structural_limits,
            );
            resolver.set_dimension_values(validated);
            // 对全部已发现单维上下文完整求值，未标注维度的模板也可能经引用受覆盖影响。
            // 每条记录共享一次结构预算，不预计算维度组合的笛卡尔积。
            for (dimension, variants) in &self.dimension_variants {
                for variant in variants {
                    let start = resolver.diagnostic_count();
                    for (index, owner) in drafts.iter().enumerate() {
                        let record_id = CfdRecordHandle::new(index);
                        if let Some(mut fields) = resolver.resolve_record_fields(
                            record_id,
                            Some((dimension.clone(), variant.clone())),
                        ) {
                            for field in self.schema.full_fields(owner.actual_type.as_str()) {
                                if field
                                    .dimension
                                    .as_ref()
                                    .is_none_or(|binding| &binding.dimension != dimension)
                                {
                                    continue;
                                }
                                let Some(value) = fields.remove(&field.name) else {
                                    continue;
                                };
                                // 显式来源与有效结果共享同一不可变值，避免维度大容器保存两份。
                                let value = Arc::new(value);
                                if let Some(input) = inputs.get(&(
                                    record_id,
                                    field.name.clone(),
                                    dimension.clone(),
                                    variant.clone(),
                                )) {
                                    explicit_values.push((
                                        record_id,
                                        (*input).clone(),
                                        value.clone(),
                                    ));
                                }
                                effective_values.push((
                                    record_id,
                                    field.name.clone(),
                                    dimension.clone(),
                                    variant.clone(),
                                    value,
                                ));
                            }
                        }
                    }
                    let mut reference_errors = resolver.take_reference_diagnostics();
                    attach_dimension_context(
                        &mut reference_errors,
                        drafts,
                        &inputs,
                        dimension,
                        variant,
                    );
                    self.editable_diagnostics.extend(reference_errors);
                    diagnostic_contexts.push((
                        start,
                        resolver.diagnostic_count(),
                        dimension.clone(),
                        variant.clone(),
                    ));
                }
            }
            merge_reads(&mut self.template_reads, resolver.take_reads());
        }
        for (start, end, dimension, variant) in diagnostic_contexts {
            attach_dimension_context(
                &mut self.diagnostics[start..end],
                drafts,
                &inputs,
                &dimension,
                &variant,
            );
        }
        (explicit_values, effective_values)
    }

    fn fail_if_diagnostics(&mut self) -> Result<(), CfdDiagnostics> {
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            let mut diagnostics = std::mem::take(&mut self.diagnostics);
            diagnostics.append(&mut self.editable_diagnostics);
            Err(CfdDiagnostics::new(diagnostics))
        }
    }
}

fn merge_reads(
    target: &mut BTreeMap<CfdRecordHandle, BTreeSet<CfdRecordHandle>>,
    reads: BTreeMap<CfdRecordHandle, BTreeSet<CfdRecordHandle>>,
) {
    for (host, values) in reads {
        target.entry(host).or_default().extend(values);
    }
}

type ValidatedDimensionValue<'a> = (CfdRecordHandle, DraftInput<'a, DimensionValueDraft>, ValueDraft, CfdPath);
type ResolvedDimensionValue = (CfdRecordHandle, DimensionValueDraft, Arc<CfdValue>);
type EffectiveDimensionValue = (
    CfdRecordHandle,
    FieldName,
    DimensionName,
    VariantName,
    Arc<CfdValue>,
);

type DimensionInputs<'a> =
    BTreeMap<(CfdRecordHandle, FieldName, DimensionName, VariantName), &'a DimensionValueDraft>;

fn attach_dimension_context(
    diagnostics: &mut [CfdDiagnostic],
    drafts: &[RecordDraft],
    inputs: &DimensionInputs<'_>,
    dimension: &DimensionName,
    variant: &VariantName,
) {
    for diagnostic in diagnostics {
        diagnostic.message = format!(
            "{} (dimension `{dimension}`, variant `{variant}`)",
            diagnostic.message
        );
        // 诊断可能来自被引用记录；按实际标签字段定位覆盖来源，不能覆盖为调用方来源。
        for label in diagnostic.primary.iter_mut().chain(&mut diagnostic.related) {
            let Some(record) = label.record else {
                continue;
            };
            let override_origin = match label.path.segments.first() {
                Some(crate::CfdPathSegment::Field(name)) => FieldName::new(&name)
                    .ok()
                    .and_then(|field| {
                        inputs.get(&(record, field, dimension.clone(), variant.clone()))
                    })
                    .map(|input| &input.origin),
                _ => None,
            };
            label.origin = override_origin
                .or_else(|| drafts.get(record.index()).map(|draft| &draft.origin))
                .cloned();
        }
    }
}

fn attach_dimension_values(records: &mut [CfdRecord], values: Vec<ResolvedDimensionValue>) {
    for (record_id, input, value) in values {
        let Some(record) = records.get_mut(record_id.index()) else {
            continue;
        };
        let field_values = Arc::make_mut(&mut record.dimension_fields)
            .entry(input.field)
            .or_insert_with(|| CfdDimensionFieldValues {
                dimension: input.dimension,
                variants: BTreeMap::default(),
                effective_variants: BTreeMap::default(),
            });
        field_values.variants.insert(
            input.variant,
            CfdDimensionValue {
                value,
                origin: input.origin,
            },
        );
    }
}

fn dimension_diagnostic(
    input: &DimensionValueDraft,
    record: Option<CfdRecordHandle>,
    path: CfdPath,
    code: crate::CfdErrorCode,
    message: impl Into<String>,
) -> CfdDiagnostic {
    CfdDiagnostic::error(code, message)
        .with_primary(record, path)
        .with_primary_origin(input.origin.clone())
}

fn attach_origin_to_diagnostics(
    diagnostics: &mut [CfdDiagnostic],
    origin: &RecordOrigin,
    record: Option<CfdRecordHandle>,
    path: &CfdPath,
) {
    for diagnostic in diagnostics {
        match &mut diagnostic.primary {
            Some(primary) => primary.origin = Some(origin.clone()),
            None => {
                diagnostic.primary = Some(CfdLabel {
                    record,
                    path: path.clone(),
                    message: None,
                    origin: Some(origin.clone()),
                });
            }
        }
    }
}

struct BuildValueSemanticContext<'a> {
    records: &'a [CfdRecord],
    record_by_domain_id: &'a BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
}

impl CfdValueSemanticContext for BuildValueSemanticContext<'_> {
    fn record_by_domain_id(
        &self,
        inheritance_root: &TypeName,
        id: &str,
    ) -> Option<CfdRecordHandle> {
        self.record_by_domain_id
            .get(inheritance_root)?
            .get(id)
            .copied()
    }

    fn record_actual_type(&self, id: CfdRecordHandle) -> Option<&str> {
        self.records.get(id.index()).map(CfdRecord::actual_type)
    }
}

fn validate_resolved_records(
    schema: BuildSchema<'_>,
    records: &[CfdRecord],
    record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    diagnostics: &mut Vec<CfdDiagnostic>,
    plan: &ReferenceFieldPlan<'_>,
    references_only: bool,
) {
    let context = BuildValueSemanticContext {
        records,
        record_by_domain_id,
    };
    for (index, record) in records.iter().enumerate() {
        let record_id = CfdRecordHandle::new(index);
        // 健康输入的普通值已由 Validator 校验，求值只新增引用目标与模板结果。
        // 有可编辑诊断时仍完整检查，以保留缺失嵌套字段等错误的级别和顺序。
        for field in plan.fields(record.actual_type(), references_only) {
            let Some(value) = record.field(field.name.as_str()) else {
                continue;
            };
            let request = ValueValidationRequest::new(
                &field.value_type,
                value,
                ValueValidationMode::Complete,
            );
            if let Err(error) =
                crate::semantics::validate_value_for_schema(schema.cft(), &context, request)
            {
                diagnostics.push(
                    CfdDiagnostic::error(semantic_error_code(error.kind()), error.message())
                        .with_primary(
                            Some(record_id),
                            prefixed_field_path(&field.name, error.path()),
                        ),
                );
            }
        }
    }
}

fn prefixed_field_path(field: &FieldName, relative: &CfdPath) -> CfdPath {
    let mut path = CfdPath::root().field(field.as_str());
    path.segments.extend(relative.segments.iter().cloned());
    path
}

pub(super) const fn semantic_error_code(kind: CfdValueSemanticErrorKind) -> crate::CfdErrorCode {
    match kind {
        CfdValueSemanticErrorKind::UnknownType => crate::CfdErrorCode::UnknownType,
        CfdValueSemanticErrorKind::AbstractType => crate::CfdErrorCode::AbstractRecordType,
        CfdValueSemanticErrorKind::SingletonType | CfdValueSemanticErrorKind::TypeMismatch => {
            crate::CfdErrorCode::TypeMismatch
        }
        CfdValueSemanticErrorKind::ObjectTypeMismatch => crate::CfdErrorCode::ObjectTypeMismatch,
        CfdValueSemanticErrorKind::UnknownField => crate::CfdErrorCode::UnknownField,
        CfdValueSemanticErrorKind::MissingRequiredField => {
            crate::CfdErrorCode::MissingRequiredField
        }
        CfdValueSemanticErrorKind::InvalidEnumVariant => crate::CfdErrorCode::InvalidEnumVariant,
        CfdValueSemanticErrorKind::RefTargetNotFound => crate::CfdErrorCode::RefTargetNotFound,
        CfdValueSemanticErrorKind::RefTargetTypeMismatch => {
            crate::CfdErrorCode::RefTargetTypeMismatch
        }
    }
}
