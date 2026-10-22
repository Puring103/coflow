mod context;
mod defaults;
mod draft;
mod resolve;
mod validate;

pub(crate) use context::BuildSchema;
pub(crate) use draft::{RecordDraft, ValueDraft};

use crate::diagnostics::{CfdDiagnostic, CfdDiagnostics, CfdLabel, CfdPath, RecordOrigin};
use crate::indexes::{self, build_ref_indexes};
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
use validate::Validator;

#[derive(Debug)]
pub struct CfdModelBuilder<'a> {
    schema: &'a CftSchema,
    records: Vec<LoadedRecordDraft>,
    dimension_values: Vec<DimensionValueDraft>,
    dimension_variants: BTreeMap<DimensionName, BTreeSet<VariantName>>,
    structural_limits: StructuralLimits,
}

#[derive(Debug)]
pub struct CfdModelBuildOutput {
    pub model: CfdDataModel,
    pub diagnostics: CfdDiagnostics,
}

impl<'a> CfdModelBuilder<'a> {
    #[must_use]
    pub fn new(schema: &'a CftSchema) -> Self {
        Self {
            schema,
            records: Vec::new(),
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
            .push(LoadedRecordDraft::new(id, actual_type, fields));
        self
    }

    pub fn add_loaded_record(&mut self, record: LoadedRecordDraft) -> &mut Self {
        self.records.push(record);
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
        self.dimension_values.push(value);
        self
    }

    pub fn add_dimension_value_drafts(
        &mut self,
        values: impl IntoIterator<Item = DimensionValueDraft>,
    ) -> &mut Self {
        self.dimension_values.extend(values);
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
        output.model.dimension_variants = self.dimension_variants;
        Ok(output)
    }
}

pub(crate) struct ModelCompiler<'a> {
    schema: BuildSchema<'a>,
    input: Vec<LoadedRecordDraft>,
    dimension_values: Vec<DimensionValueDraft>,
    dimension_variants: BTreeMap<DimensionName, BTreeSet<VariantName>>,
    diagnostics: Vec<CfdDiagnostic>,
    editable_diagnostics: Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
}

impl<'a> ModelCompiler<'a> {
    pub(crate) fn new(
        schema_source: &'a CftSchema,
        input: Vec<LoadedRecordDraft>,
        dimension_values: Vec<DimensionValueDraft>,
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
        validate_resolved_records(
            self.schema,
            &records,
            &indexes.record_by_domain_id,
            &mut self.diagnostics,
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
            validated_dimension_values,
        );
        self.move_editable_diagnostics(|_| true);
        attach_dimension_values(&mut records, dimension_values);
        for (record_id, field, dimension, variant, value) in effective_values {
            if let Some(record) = records.get_mut(record_id.index()) {
                record
                    .dimension_fields
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

        let ref_indexes = build_ref_indexes(&records, &indexes.record_by_domain_id, self.schema);
        Ok(CfdModelBuildOutput {
            model: CfdDataModel {
                tables: indexes.tables,
                record_by_domain_id: indexes.record_by_domain_id,
                records,
                refs: ref_indexes,
                dimension_variants: BTreeMap::new(),
                projection: None,
            },
            diagnostics: CfdDiagnostics::new(self.editable_diagnostics),
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
            draft.origin = record.origin;
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
            let Ok(id) = RecordId::new(draft.id.clone()) else {
                continue;
            };
            records.push(CfdRecord {
                id,
                object: CfdObject {
                    actual_type: draft.actual_type.clone(),
                    fields,
                },
                origin: draft.origin.clone(),
                dimension_fields: BTreeMap::default(),
            });
        }
        self.editable_diagnostics
            .extend(resolver.take_reference_diagnostics());
        records
    }

    fn validate_dimension_values(
        &mut self,
        records: &[CfdRecord],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    ) -> Vec<ValidatedDimensionValue> {
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
        input: DimensionValueDraft,
        records: &[CfdRecord],
        record_by_domain_id: &BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
        seen: &mut BTreeSet<(CfdRecordHandle, FieldName, VariantName)>,
    ) -> Option<ValidatedDimensionValue> {
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
        let Some(field) = self
            .schema
            .full_fields(record.actual_type())
            .find(|field| field.name == input.field)
        else {
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
        validated: Vec<ValidatedDimensionValue>,
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
                    input,
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
            resolver.set_dimension_values(&validated);
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
                                if !field
                                    .dimension
                                    .as_ref()
                                    .is_some_and(|binding| &binding.dimension == dimension)
                                {
                                    continue;
                                }
                                let Some(value) = fields.remove(&field.name) else {
                                    continue;
                                };
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

type ValidatedDimensionValue = (CfdRecordHandle, DimensionValueDraft, ValueDraft, CfdPath);
type ResolvedDimensionValue = (CfdRecordHandle, DimensionValueDraft, CfdValue);
type EffectiveDimensionValue = (
    CfdRecordHandle,
    FieldName,
    DimensionName,
    VariantName,
    CfdValue,
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
                Some(crate::CfdPathSegment::Field(name)) => FieldName::new(name.clone())
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
        let field_values = record
            .dimension_fields
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
) {
    let context = BuildValueSemanticContext {
        records,
        record_by_domain_id,
    };
    for (index, record) in records.iter().enumerate() {
        let record_id = CfdRecordHandle::new(index);
        for (field_name, value) in record.fields() {
            let Some(field) = schema
                .cft()
                .field(record.actual_type(), field_name.as_str())
            else {
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
                            prefixed_field_path(field_name, error.path()),
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
