use crate::api::{
    byte_range, map_diagnostics_with_origins, DiagnosticContext, DiagnosticSet, Label,
    SourceLocation,
};
use crate::checker::{CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats};
use crate::data_model::{CfdDataModel, CfdDiagnostics, RecordOrigin};
use coflow_language::CftSchema;

use super::{CheckDiagnosticStore, ProjectCheckOutput};
use crate::load::logical_locations_from_cfd;

pub(super) fn render_check_store(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    state: CheckDiagnosticStore,
    statistics: CheckExecutionStats,
) -> ProjectCheckOutput {
    let raw = state.diagnostics(model);
    let cfd = CfdDiagnostics::new(
        raw.iter()
            .map(|diagnostic| diagnostic.diagnostic.clone())
            .collect(),
    );
    let logical_locations = logical_locations_from_cfd(&cfd, |id| {
        model
            .record(id)
            .map(crate::data_model::CfdRecord::coordinate)
    });
    ProjectCheckOutput {
        diagnostics: map_check_diagnostics_with_origins(Some(schema), raw, origins),
        logical_locations,
        state,
        statistics,
    }
}

fn map_check_diagnostics_with_origins(
    schema: Option<&CftSchema>,
    diagnostics: Vec<CheckDiagnostic>,
    origins: &[RecordOrigin],
) -> DiagnosticSet {
    let (raw, metadata): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .map(|diagnostic| {
            (
                diagnostic.diagnostic,
                (diagnostic.contexts, diagnostic.schema_location),
            )
        })
        .unzip();
    let mut mapped = map_diagnostics_with_origins(CfdDiagnostics::new(raw), origins);
    for (diagnostic, (contexts, schema_location)) in mapped.diagnostics.iter_mut().zip(metadata) {
        diagnostic.contexts = contexts.into_iter().map(map_check_context).collect();
        if let (Some(schema), Some(location)) = (schema, schema_location) {
            if let Some(source) = schema.source(&location.module) {
                let range = byte_range(&source.source, location.span.start, location.span.end);
                let label = Label {
                    location: SourceLocation::FileSpan {
                        path: source.path.clone(),
                        start_line: range.start.line,
                        start_character: range.start.character,
                        end_line: range.end.line,
                        end_character: range.end.character,
                    },
                    message: Some("check declared here".to_string()),
                };
                if diagnostic.primary.is_none() {
                    diagnostic.primary = Some(label);
                } else {
                    diagnostic.related.push(label);
                }
            }
        }
    }
    mapped
}

fn map_check_context(context: CheckDiagnosticContext) -> DiagnosticContext {
    let mut mapped = DiagnosticContext::default();
    match context {
        CheckDiagnosticContext::Check { name } => {
            mapped.kind = "check".to_string();
            mapped.name = Some(name);
        }
        CheckDiagnosticContext::When { expression } => {
            mapped.kind = "when".to_string();
            mapped.expression = Some(expression);
        }
        CheckDiagnosticContext::Quantifier {
            kind,
            binding,
            item,
        } => {
            mapped.kind = "quantifier".to_string();
            mapped.quantifier = Some(kind);
            mapped.binding = Some(binding);
            mapped.item = Some(item);
        }
        CheckDiagnosticContext::Dimension { dimension, variant } => {
            mapped.kind = "dimension".to_string();
            mapped.dimension = Some(dimension);
            mapped.variant = Some(variant);
        }
    }
    mapped
}
