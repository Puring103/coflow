use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{map_diagnostics_with_origins, DiagnosticContext, DiagnosticSet};
use coflow_cft::CftSchema;
use coflow_checker::{
    run_checks, CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats, CheckRequest,
    CheckSnapshot, DependencyCollection, DimensionCheckRound,
};
use coflow_data_model::{CfdDataModel, CfdDiagnostics, CfdRecordId, RecordOrigin};

use crate::indexes::DiagnosticLogicalLocation;
use crate::load::logical_locations_from_cfd;
use crate::RecordCoordinate;

pub(crate) type CheckState = CheckSnapshot;

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) state: CheckState,
    pub(crate) statistics: CheckExecutionStats,
}

pub(crate) fn run_full_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> ProjectCheckOutput {
    let output = run_checks(
        schema,
        model,
        CheckRequest::all()
            .with_rounds(dimension_check_rounds(schema))
            .with_dependency_collection(DependencyCollection::Reads),
    );
    let statistics = output.statistics;
    if let Some(snapshot) = output.snapshot {
        if let Some(rendered) = render_check_snapshot(model, origins, snapshot, statistics) {
            return rendered;
        }
    }
    render_raw_check_output(
        model,
        origins,
        output
            .diagnostics
            .into_iter()
            .map(|rooted| rooted.diagnostic)
            .collect(),
        statistics,
    )
}

pub(crate) fn run_incremental_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    previous: &CheckState,
    changed: &BTreeSet<RecordCoordinate>,
) -> Option<ProjectCheckOutput> {
    let output = run_checks(
        schema,
        model,
        CheckRequest::incremental(previous, changed).with_rounds(dimension_check_rounds(schema)),
    );
    render_check_snapshot(model, origins, output.snapshot?, output.statistics)
}

fn render_check_snapshot(
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    state: CheckSnapshot,
    statistics: CheckExecutionStats,
) -> Option<ProjectCheckOutput> {
    let raw = state.render_diagnostics(model)?;
    let cfd = CfdDiagnostics::new(
        raw.iter()
            .map(|diagnostic| diagnostic.diagnostic.clone())
            .collect(),
    );
    let logical_locations = logical_locations_from_cfd(&cfd, |id| coordinate_for_id(model, id));
    Some(ProjectCheckOutput {
        diagnostics: map_check_diagnostics_with_origins(raw, origins),
        logical_locations,
        state,
        statistics,
    })
}

fn coordinate_for_id(model: &CfdDataModel, id: CfdRecordId) -> Option<RecordCoordinate> {
    model
        .record(id)
        .map(coflow_data_model::CfdRecord::coordinate)
}

fn dimension_check_rounds(schema: &CftSchema) -> Vec<DimensionCheckRound> {
    schema
        .all_dimensions()
        .flat_map(|dimension| {
            dimension.variants.iter().cloned().filter_map(|variant| {
                DimensionCheckRound::try_new(schema, dimension.name.clone(), variant).ok()
            })
        })
        .collect()
}

fn render_raw_check_output(
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    diagnostics: Vec<CheckDiagnostic>,
    statistics: CheckExecutionStats,
) -> ProjectCheckOutput {
    let cfd = CfdDiagnostics::new(
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.diagnostic.clone())
            .collect(),
    );
    let logical_locations = logical_locations_from_cfd(&cfd, |id| coordinate_for_id(model, id));
    ProjectCheckOutput {
        diagnostics: map_check_diagnostics_with_origins(diagnostics, origins),
        logical_locations,
        state: CheckSnapshot::default(),
        statistics,
    }
}

fn map_check_diagnostics_with_origins(
    diagnostics: Vec<CheckDiagnostic>,
    origins: &[RecordOrigin],
) -> DiagnosticSet {
    let (raw, contexts): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.diagnostic, diagnostic.contexts))
        .unzip();
    let mut mapped = map_diagnostics_with_origins(CfdDiagnostics::new(raw), origins);
    for (diagnostic, contexts) in mapped.diagnostics.iter_mut().zip(contexts) {
        diagnostic.contexts = contexts.into_iter().map(map_check_context).collect();
    }
    mapped
}

fn map_check_context(context: CheckDiagnosticContext) -> DiagnosticContext {
    let mut mapped = DiagnosticContext::default();
    match context {
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

#[cfg(test)]
mod tests {
    use super::map_check_diagnostics_with_origins;
    use coflow_checker::{CheckDiagnostic, CheckDiagnosticContext, CheckSnapshot};
    use coflow_data_model::{CfdDiagnostic, CfdErrorCode};

    #[test]
    fn default_snapshot_disables_incremental_reuse() {
        assert!(!CheckSnapshot::default().is_reusable());
    }

    #[test]
    fn check_contexts_map_without_changing_custom_message() {
        let mapped = map_check_diagnostics_with_origins(
            vec![CheckDiagnostic {
                diagnostic: CfdDiagnostic::error(CfdErrorCode::CheckFailed, "custom message"),
                contexts: vec![CheckDiagnosticContext::When {
                    expression: "enabled".to_string(),
                }],
                is_custom_message: true,
            }],
            &[],
        );

        assert_eq!(mapped.diagnostics[0].message, "custom message");
        assert_eq!(mapped.diagnostics[0].contexts[0].kind, "when");
        assert_eq!(
            mapped.diagnostics[0].contexts[0].expression.as_deref(),
            Some("enabled")
        );
        assert_eq!(mapped.flat_diagnostics()[0].contexts.len(), 1);
    }
}
