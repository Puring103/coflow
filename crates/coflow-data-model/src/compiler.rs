mod defaults;
mod indexes;
mod resolve;
mod validate;

use crate::compiler_context::DataModelCompilerContext;
use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdPath};
use crate::edge_index::{build_ref_indexes, build_spread_indexes};
use crate::model::{
    CfdDataModel, CfdDimensionFieldValues, CfdDimensionValue, CfdInputDimensionValue,
    CfdInputRecord, CfdObject, CfdRecord, CfdRecordId,
};
use coflow_cft::{CftSchema, CftValueType, RecordKey};
use coflow_structure::StructuralLimits;
use resolve::ValueResolver;
use std::collections::BTreeMap;
use validate::Validator;

pub(crate) struct ModelCompiler<'a> {
    schema: DataModelCompilerContext<'a>,
    input: Vec<CfdInputRecord>,
    dimension_values: Vec<CfdInputDimensionValue>,
    diagnostics: Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
}

impl<'a> ModelCompiler<'a> {
    pub(crate) fn new(
        schema_source: &'a CftSchema,
        input: Vec<CfdInputRecord>,
        dimension_values: Vec<CfdInputDimensionValue>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema: DataModelCompilerContext::new(schema_source),
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
        let indexes = indexes::build_indexes(&self.schema, &drafts, &mut self.diagnostics);

        // Phase 2b: singleton validation. We run this even when phase 2 has
        // already collected diagnostics so that singleton-specific codes
        // (SingletonRecordCountInvalid / SingletonKeyMissingOrInvalid /
        // SingletonKeyCollision) are surfaced alongside generic ones; this
        // gives users a complete picture in a single build pass.
        // Localized record-key identifier requirements are already covered by
        // the generic `InvalidRecordKey` path because `record_key_ident_error`
        // and `is_cft_identifier` currently use the same rule set; the spec
        // leaves `LocalizedRecordKeyInvalid` reserved for future divergence.
        indexes::validate_singletons(
            &self.schema,
            &drafts,
            &indexes.tables,
            &mut self.diagnostics,
        );
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

        let mut resolved_dimension_values = Vec::new();
        {
            let mut seen = std::collections::BTreeSet::new();
            for input in &self.dimension_values {
                let Some(domain) = self.schema.type_domain_id(input.source_type.as_str()) else {
                    self.diagnostics.push(CfdDiagnostic::error(
                        crate::CfdErrorCode::UnknownType,
                        format!("unknown dimension source type `{}`", input.source_type),
                    ));
                    continue;
                };
                let Some(record_id) = indexes
                    .record_by_domain_key
                    .get(&domain)
                    .and_then(|records| records.get(input.source_key.as_str()))
                    .copied()
                else {
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                    self.diagnostics.push(CfdDiagnostic::error(
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
                if let Some(draft) = draft {
                    resolved_dimension_values.push((record_id, input, draft, path));
                }
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let mut dimension_values = Vec::with_capacity(resolved_dimension_values.len());
        {
            let mut resolver = ValueResolver::new(
                &self.schema,
                &drafts,
                &indexes.record_by_domain_key,
                &mut self.diagnostics,
                self.structural_limits,
            );
            for (record_id, input, draft, path) in resolved_dimension_values {
                if let Some(value) = resolver.resolve_dimension_value(record_id, &draft, path) {
                    dimension_values.push((record_id, input, value));
                }
            }
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

        let spread_indexes =
            build_spread_indexes(&drafts, &indexes.record_by_domain_key, &self.schema);
        let ref_indexes = build_ref_indexes(
            &records,
            &indexes.record_by_domain_key,
            &self.schema,
            &spread_indexes.edges,
        );

        Ok(CfdDataModel {
            tables: indexes.tables,
            inheritance_index: indexes.inheritance_index,
            domain_index: self.schema.domain_index().clone(),
            record_by_type_key: indexes.record_by_type_key,
            record_by_domain_key: indexes.record_by_domain_key,
            records,
            ref_edges: ref_indexes.edges,
            ref_by_site: ref_indexes.by_site,
            ref_by_host: ref_indexes.by_host,
            ref_by_target: ref_indexes.by_target,
            spread_edges: spread_indexes.edges,
            spread_by_site: spread_indexes.by_site,
            spread_by_source: spread_indexes.by_source,
        })
    }
}
