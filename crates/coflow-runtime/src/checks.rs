use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{map_diagnostics_with_origins, DiagnosticSet};
use coflow_cft::CftSchema;
use coflow_checker::{
    run_checks, CheckRequest, CheckSnapshot, DependencyCollection, DimensionCheckRound,
};
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdRecordId, RecordOrigin};

use crate::indexes::DiagnosticLogicalLocation;
use crate::load::logical_locations_from_cfd;
use crate::RecordCoordinate;

pub(crate) type CheckState = CheckSnapshot;

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) state: CheckState,
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
    if let Some(snapshot) = output.snapshot {
        if let Some(rendered) = render_check_snapshot(model, origins, snapshot) {
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
    render_check_snapshot(model, origins, output.snapshot?)
}

fn render_check_snapshot(
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    state: CheckSnapshot,
) -> Option<ProjectCheckOutput> {
    let raw = CfdDiagnostics::new(state.render_diagnostics(model)?);
    let logical_locations = logical_locations_from_cfd(&raw, |id| coordinate_for_id(model, id));
    Some(ProjectCheckOutput {
        diagnostics: map_diagnostics_with_origins(raw, origins),
        logical_locations,
        state,
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
            dimension
                .variants
                .iter()
                .cloned()
                .map(|variant| DimensionCheckRound::new(dimension.name.clone(), variant))
        })
        .collect()
}

fn render_raw_check_output(
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    diagnostics: Vec<CfdDiagnostic>,
) -> ProjectCheckOutput {
    let raw = CfdDiagnostics::new(diagnostics);
    let logical_locations = logical_locations_from_cfd(&raw, |id| coordinate_for_id(model, id));
    ProjectCheckOutput {
        diagnostics: map_diagnostics_with_origins(raw, origins),
        logical_locations,
        state: CheckSnapshot::default(),
    }
}

#[cfg(test)]
mod tests {
    use coflow_checker::CheckSnapshot;

    #[test]
    fn default_snapshot_disables_incremental_reuse() {
        assert!(!CheckSnapshot::default().is_reusable());
    }
}
