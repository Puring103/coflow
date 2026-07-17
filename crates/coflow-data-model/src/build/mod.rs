mod context;
mod defaults;
mod draft;
mod resolve;
mod validate;

pub(crate) use context::BuildSchema;
pub(crate) use draft::{RecordDraft, SpreadFieldSource, ValueDraft};

use crate::diagnostics::{CfdDiagnostic, CfdDiagnostics, CfdLabel, CfdPath, RecordOrigin};
use crate::indexes::{
    self, build_ref_indexes, build_spread_indexes, extend_dimension_spread_indexes,
    SpreadIndexContext,
};
use crate::ingest::{DimensionValueDraft, LoadedRecordDraft, LoadedValueDraft};
use crate::model::{
    CfdDataModel, CfdDimensionFieldValues, CfdDimensionValue, CfdObject, CfdRecord, CfdRecordId,
    DimensionRefCoordinate,
};
use crate::semantics::{
    CfdValueSemanticContext, CfdValueSemanticErrorKind, ValueValidationMode, ValueValidationRequest,
};
use coflow_cft::{CftSchema, CftValueType, FieldName, RecordKey, TypeName};
use coflow_structure::StructuralLimits;
use resolve::ValueResolver;
use std::collections::BTreeMap;
use validate::Validator;

#[derive(Debug)]
pub struct CfdModelBuilder<'a> {
    schema: &'a CftSchema,
    records: Vec<LoadedRecordDraft>,
    dimension_values: Vec<DimensionValueDraft>,
    structural_limits: StructuralLimits,
}

impl<'a> CfdModelBuilder<'a> {
    #[must_use]
    pub fn new(schema: &'a CftSchema) -> Self {
        Self {
            schema,
            records: Vec::new(),
            dimension_values: Vec::new(),
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
        key: impl Into<String>,
        actual_type: impl Into<String>,
        fields: impl IntoIterator<Item = (impl Into<String>, LoadedValueDraft)>,
    ) -> &mut Self {
        self.records
            .push(LoadedRecordDraft::new(key, actual_type, fields));
        self
    }

    pub fn add_loaded_record(&mut self, record: LoadedRecordDraft) -> &mut Self {
        self.records.push(record);
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
        ModelCompiler::new(
            self.schema,
            self.records,
            self.dimension_values,
            self.structural_limits,
        )
        .build()
    }
}

pub(crate) struct ModelCompiler<'a> {
    schema: BuildSchema<'a>,
    input: Vec<LoadedRecordDraft>,
    dimension_values: Vec<DimensionValueDraft>,
    diagnostics: Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
}

impl<'a> ModelCompiler<'a> {
    pub(crate) fn new(
        schema_source: &'a CftSchema,
        input: Vec<LoadedRecordDraft>,
        dimension_values: Vec<DimensionValueDraft>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema: BuildSchema::new(schema_source),
            input,
            dimension_values,
            diagnostics: Vec::new(),
            structural_limits,
        }
    }

    pub(crate) fn build(mut self) -> Result<CfdDataModel, CfdDiagnostics> {
        // Phase 1: validate input records into drafts. Capture each record's
        // origin so it can flow through to the final `CfdRecord`.
        let mut drafts = Vec::new();
        let input = std::mem::take(&mut self.input);
        {
            let mut v = Validator::new(&self.schema, &mut self.diagnostics, self.structural_limits);
            for (input_index, record) in input.into_iter().enumerate() {
                let id = CfdRecordId::new(input_index);
                if let Some(mut draft) = v.validate_top_level_record(
                    None,
                    &record.key,
                    &record.actual_type,
                    &record.spreads,
                    &record.fields,
                    Some(id),
                    CfdPath::root(),
                ) {
                    // Top-level draft inherits the input's origin.
                    draft.origin = record.origin;
                    drafts.push(draft);
                }
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        // Phase 2: build primary / secondary / polymorphic indexes.
        let indexes = indexes::build_indexes(self.schema, &drafts, &mut self.diagnostics);

        // Phase 2b: singleton validation. We run this even when phase 2 has
        // already collected diagnostics so that singleton-specific codes
        // (SingletonRecordCountInvalid / SingletonKeyMissingOrInvalid /
        // SingletonKeyCollision) are surfaced alongside generic ones; this
        // gives users a complete picture in a single build pass.
        // Localized record-key identifier requirements are already covered by
        // the generic `InvalidRecordKey` path because `record_key_ident_error`
        // and `is_cft_identifier` currently use the same rule set; the spec
        // leaves `LocalizedRecordKeyInvalid` reserved for future divergence.
        indexes::validate_singletons(self.schema, &drafts, &indexes.tables, &mut self.diagnostics);
        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        // Phase 3: resolve refs and spread dependencies through one stateful
        // resolver so shared values are memoized and cycles become diagnostics.
        let mut records = Vec::with_capacity(drafts.len());
        {
            let mut resolver = ValueResolver::new(
                &self.schema,
                &drafts,
                &indexes.record_by_domain_key,
                &mut self.diagnostics,
                self.structural_limits,
            );
            for (index, draft) in drafts.iter().enumerate() {
                let record_id = CfdRecordId::new(index);
                let Some(fields) = resolver.resolve_record_fields(record_id) else {
                    continue;
                };
                let Ok(key) = RecordKey::new(draft.key.clone()) else {
                    continue;
                };
                records.push(CfdRecord {
                    key,
                    object: CfdObject {
                        actual_type: draft.actual_type.clone(),
                        fields,
                    },
                    origin: draft.origin.clone(),
                    dimension_fields: BTreeMap::default(),
                });
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        validate_resolved_records(
            self.schema,
            &records,
            &indexes.record_by_domain_key,
            &mut self.diagnostics,
        );
        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let mut resolved_dimension_values = Vec::new();
        {
            let mut seen = std::collections::BTreeSet::new();
            for input in &self.dimension_values {
                let Some(inheritance_root) =
                    self.schema.inheritance_root(input.source_type.as_str())
                else {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        None,
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::UnknownType,
                        format!("unknown dimension source type `{}`", input.source_type),
                    ));
                    continue;
                };
                let Some(record_id) = indexes
                    .record_by_domain_key
                    .get(inheritance_root)
                    .and_then(|records| records.get(input.source_key.as_str()))
                    .copied()
                else {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        None,
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::RefTargetNotFound,
                        format!(
                            "dimension owner `{}:{}` was not found",
                            input.source_type, input.source_key
                        ),
                    ));
                    continue;
                };
                let Some(record) = records.get(record_id.index()) else {
                    continue;
                };
                if !self
                    .schema
                    .is_assignable(record.actual_type(), input.source_type.as_str())
                {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::ObjectTypeMismatch,
                        format!(
                            "dimension owner type `{}` is not assignable to `{}`",
                            record.actual_type(),
                            input.source_type
                        ),
                    ));
                    continue;
                }
                let Some(field) = self
                    .schema
                    .full_fields(record.actual_type())
                    .find(|field| field.name == input.field)
                else {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::UnknownField,
                        format!(
                            "unknown dimension field `{}.{}`",
                            record.actual_type(),
                            input.field
                        ),
                    ));
                    continue;
                };
                let Some(binding) = &field.dimension else {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::TypeMismatch,
                        format!(
                            "field `{}.{}` is not dimensional",
                            record.actual_type(),
                            input.field
                        ),
                    ));
                    continue;
                };
                if binding.dimension != input.dimension {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::TypeMismatch,
                        format!(
                            "field `{}.{}` uses dimension `{}`, not `{}`",
                            record.actual_type(),
                            input.field,
                            binding.dimension,
                            input.dimension
                        ),
                    ));
                    continue;
                }
                if self
                    .schema
                    .cft()
                    .resolve_dimension(input.dimension.as_str())
                    .and_then(|dimension| dimension.variant(input.variant.as_str()))
                    .is_none()
                {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::TypeMismatch,
                        format!(
                            "unknown variant `{}` for dimension `{}`",
                            input.variant, input.dimension
                        ),
                    ));
                    continue;
                }
                let coordinate = (record_id, input.field.clone(), input.variant.clone());
                if !seen.insert(coordinate) {
                    self.diagnostics.push(dimension_diagnostic(
                        input,
                        Some(record_id),
                        CfdPath::root().field(input.field.as_str()),
                        crate::CfdErrorCode::DuplicateId,
                        format!(
                            "duplicate dimension value `{}:{}.{}/{}`",
                            input.source_type, input.source_key, input.field, input.variant
                        ),
                    ));
                    continue;
                }
                let nullable_ty =
                    CftValueType::Nullable(Box::new(field.value_type.non_nullable().clone()));
                let path = CfdPath::root().field(input.field.as_str());
                let diagnostic_start = self.diagnostics.len();
                let draft = {
                    let mut validator =
                        Validator::new(&self.schema, &mut self.diagnostics, self.structural_limits);
                    validator.validate_value(
                        &nullable_ty,
                        &input.value,
                        Some(record_id),
                        path.clone(),
                        coflow_structure::TraversalCursor::root(),
                    )
                };
                attach_dimension_origins(
                    &mut self.diagnostics[diagnostic_start..],
                    input,
                    Some(record_id),
                    &path,
                );
                if let Some(draft) = draft {
                    resolved_dimension_values.push((record_id, input, draft, path));
                }
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let spread_context =
            SpreadIndexContext::new(&drafts, &indexes.record_by_domain_key, self.schema);
        let mut spread_indexes = build_spread_indexes(spread_context);
        for (record_id, input, draft, path) in &resolved_dimension_values {
            let coordinate = DimensionRefCoordinate {
                field: input.field.clone(),
                dimension: input.dimension.clone(),
                variant: input.variant.clone(),
            };
            extend_dimension_spread_indexes(
                &mut spread_indexes,
                draft,
                *record_id,
                path,
                &coordinate,
                spread_context,
            );
        }

        let mut dimension_values = Vec::with_capacity(resolved_dimension_values.len());
        let mut dimension_diagnostic_origins = Vec::new();
        {
            let mut resolver = ValueResolver::new(
                &self.schema,
                &drafts,
                &indexes.record_by_domain_key,
                &mut self.diagnostics,
                self.structural_limits,
            );
            for (record_id, input, draft, path) in resolved_dimension_values {
                let diagnostic_start = resolver.diagnostic_count();
                if let Some(value) = resolver.resolve_dimension_value(record_id, &draft, path) {
                    dimension_values.push((record_id, input, value));
                }
                let diagnostic_end = resolver.diagnostic_count();
                dimension_diagnostic_origins.push((
                    diagnostic_start,
                    diagnostic_end,
                    input.origin.clone(),
                    record_id,
                    CfdPath::root().field(input.field.as_str()),
                ));
            }
        }
        for (start, end, origin, record_id, path) in dimension_diagnostic_origins {
            attach_origin_to_diagnostics(
                &mut self.diagnostics[start..end],
                &origin,
                Some(record_id),
                &path,
            );
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        for (record_id, input, value) in dimension_values {
            let Some(record) = records.get_mut(record_id.index()) else {
                continue;
            };
            let field_values = record
                .dimension_fields
                .entry(input.field.clone())
                .or_insert_with(|| CfdDimensionFieldValues {
                    dimension: input.dimension.clone(),
                    variants: BTreeMap::default(),
                });
            field_values.variants.insert(
                input.variant.clone(),
                CfdDimensionValue {
                    value,
                    origin: input.origin.clone(),
                },
            );
        }

        let ref_indexes = build_ref_indexes(
            &records,
            &indexes.record_by_domain_key,
            self.schema,
            &spread_indexes.edges,
        );

        Ok(CfdDataModel {
            tables: indexes.tables,
            record_by_type_key: indexes.record_by_type_key,
            record_by_domain_key: indexes.record_by_domain_key,
            records,
            ref_edges: ref_indexes.edges,
            ref_by_site: ref_indexes.by_site,
            ref_by_host: ref_indexes.by_host,
            ref_by_target: ref_indexes.by_target,
            spread_edges: spread_indexes.edges,
            spread_by_host: spread_indexes.by_host,
            spread_by_source: spread_indexes.by_source,
        })
    }
}

fn dimension_diagnostic(
    input: &DimensionValueDraft,
    record: Option<CfdRecordId>,
    path: CfdPath,
    code: crate::CfdErrorCode,
    message: impl Into<String>,
) -> CfdDiagnostic {
    CfdDiagnostic::error(code, message)
        .with_primary(record, path)
        .with_primary_origin(input.origin.clone())
}

fn attach_dimension_origins(
    diagnostics: &mut [CfdDiagnostic],
    input: &DimensionValueDraft,
    record: Option<CfdRecordId>,
    path: &CfdPath,
) {
    attach_origin_to_diagnostics(diagnostics, &input.origin, record, path);
}

fn attach_origin_to_diagnostics(
    diagnostics: &mut [CfdDiagnostic],
    origin: &RecordOrigin,
    record: Option<CfdRecordId>,
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
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
}

impl CfdValueSemanticContext for BuildValueSemanticContext<'_> {
    fn record_by_domain_key(&self, inheritance_root: &TypeName, key: &str) -> Option<CfdRecordId> {
        self.record_by_domain_key
            .get(inheritance_root)?
            .get(key)
            .copied()
    }

    fn record_actual_type(&self, id: CfdRecordId) -> Option<&str> {
        self.records.get(id.index()).map(CfdRecord::actual_type)
    }
}

fn validate_resolved_records(
    schema: BuildSchema<'_>,
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    diagnostics: &mut Vec<CfdDiagnostic>,
) {
    let context = BuildValueSemanticContext {
        records,
        record_by_domain_key,
    };
    for (index, record) in records.iter().enumerate() {
        let record_id = CfdRecordId::new(index);
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
        CfdValueSemanticErrorKind::RefTargetTypeMismatch => crate::CfdErrorCode::TypeMismatch,
    }
}
