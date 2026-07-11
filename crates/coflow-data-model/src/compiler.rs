mod defaults;
mod indexes;
mod resolve;
mod validate;

use crate::compiler_context::DataModelCompilerContext;
use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdPath};
use crate::edge_index::{build_ref_indexes, build_spread_indexes};
use crate::model::{CfdDataModel, CfdInputRecord, CfdObject, CfdRecord, CfdRecordId};
use coflow_cft::CftContainer;
use coflow_structure::StructuralLimits;
use resolve::ValueResolver;
use validate::Validator;

pub(crate) struct ModelCompiler {
    schema: DataModelCompilerContext,
    input: Vec<CfdInputRecord>,
    diagnostics: Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
}

impl ModelCompiler {
    pub(crate) fn new(
        schema_source: &CftContainer,
        input: Vec<CfdInputRecord>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema: DataModelCompilerContext::new(schema_source),
            input,
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
                records.push(CfdRecord {
                    key: draft.key.clone(),
                    object: CfdObject {
                        actual_type: draft.actual_type.clone(),
                        fields,
                    },
                    origin: draft.origin.clone(),
                });
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
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
