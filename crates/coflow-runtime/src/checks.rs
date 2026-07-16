use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{map_diagnostics_with_origins, DiagnosticSet};
use coflow_cft::CftSchema;
use coflow_checker::{
    run_checks, CheckRequest, DependencyCollection, DependencyGraph, DimensionCheckRound,
    RootedCheckDiagnostic,
};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdLabel, CfdPath, CfdRecordId, RecordOrigin,
};

use crate::indexes::DiagnosticLogicalLocation;
use crate::load::logical_locations_from_cfd;
use crate::RecordCoordinate;

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckState {
    diagnostics: Vec<StableCheckDiagnostic>,
    reads_from: BTreeMap<RecordCoordinate, BTreeSet<RecordCoordinate>>,
    incremental_ready: bool,
}

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) state: CheckState,
}

#[derive(Debug, Clone)]
struct StableCheckDiagnostic {
    root: RecordCoordinate,
    code: coflow_data_model::CfdErrorCode,
    stage: coflow_data_model::CfdStage,
    severity: coflow_data_model::CfdSeverity,
    message: String,
    primary: Option<StableCheckLabel>,
    related: Vec<StableCheckLabel>,
}

#[derive(Debug, Clone)]
struct StableCheckLabel {
    record: Option<RecordCoordinate>,
    path: CfdPath,
    message: Option<String>,
    origin: Option<RecordOrigin>,
}

pub(crate) fn run_full_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> ProjectCheckOutput {
    let targets = model.records().map(|(id, _)| id).collect::<Vec<_>>();
    let output = run_checks(
        schema,
        model,
        CheckRequest::records(&targets)
            .with_rounds(dimension_check_rounds(schema))
            .with_dependency_collection(DependencyCollection::Reads),
    );
    let diagnostics = output.diagnostics;
    let dependencies = output.dependencies;
    if check_state_is_stable(model, &diagnostics, &dependencies) {
        if let Some(output) = stabilize_check_state(model, diagnostics, dependencies)
            .and_then(|state| render_check_state(model, origins, state))
        {
            return output;
        }
        let fallback = run_checks(
            schema,
            model,
            CheckRequest::records(&targets)
                .with_rounds(dimension_check_rounds(schema))
                .with_dependency_collection(DependencyCollection::Reads),
        );
        return render_raw_check_output(
            model,
            origins,
            fallback
                .diagnostics
                .into_iter()
                .map(|rooted| rooted.diagnostic)
                .collect(),
        );
    }
    render_raw_check_output(
        model,
        origins,
        diagnostics
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
    if !previous.incremental_ready {
        return None;
    }
    let mut affected = changed.clone();
    for (reader, reads) in &previous.reads_from {
        if reads.iter().any(|coordinate| changed.contains(coordinate)) {
            affected.insert(reader.clone());
        }
    }
    let targets = affected
        .iter()
        .map(|coordinate| model.record_by_type_key(&coordinate.actual_type, &coordinate.key))
        .collect::<Option<Vec<_>>>()?;
    let output = run_checks(
        schema,
        model,
        CheckRequest::records(&targets)
            .with_rounds(dimension_check_rounds(schema))
            .with_dependency_collection(DependencyCollection::Reads),
    );
    let replacement =
        stabilize_check_state(model, output.diagnostics, output.dependencies)?;
    let mut state = previous.clone();
    state
        .diagnostics
        .retain(|diagnostic| !affected.contains(&diagnostic.root));
    state.diagnostics.extend(replacement.diagnostics);
    state
        .reads_from
        .retain(|reader, _| !affected.contains(reader));
    state.reads_from.extend(replacement.reads_from);
    render_check_state(model, origins, state)
}

fn stabilize_check_state(
    model: &CfdDataModel,
    diagnostics: Vec<RootedCheckDiagnostic>,
    dependencies: DependencyGraph,
) -> Option<CheckState> {
    let diagnostics = diagnostics
        .into_iter()
        .map(|rooted| stabilize_diagnostic(model, rooted))
        .collect::<Option<Vec<_>>>()?;
    let mut reads_from = BTreeMap::new();
    for (reader, reads) in dependencies.reads_from {
        let reader = coordinate_for_id(model, reader)?;
        let reads = reads
            .into_iter()
            .map(|id| coordinate_for_id(model, id))
            .collect::<Option<BTreeSet<_>>>()?;
        reads_from.insert(reader, reads);
    }
    Some(CheckState {
        diagnostics,
        reads_from,
        incremental_ready: true,
    })
}

fn check_state_is_stable(
    model: &CfdDataModel,
    diagnostics: &[RootedCheckDiagnostic],
    dependencies: &DependencyGraph,
) -> bool {
    let valid_id = |id| model.record(id).is_some();
    diagnostics.iter().all(|rooted| {
        valid_id(rooted.root)
            && rooted
                .diagnostic
                .primary
                .as_ref()
                .and_then(|label| label.record)
                .is_none_or(&valid_id)
            && rooted
                .diagnostic
                .related
                .iter()
                .all(|label| label.record.is_none_or(&valid_id))
    }) && dependencies
        .reads_from
        .iter()
        .all(|(reader, reads)| valid_id(*reader) && reads.iter().copied().all(&valid_id))
}

fn stabilize_diagnostic(
    model: &CfdDataModel,
    rooted: RootedCheckDiagnostic,
) -> Option<StableCheckDiagnostic> {
    let CfdDiagnostic {
        code,
        stage,
        severity,
        message,
        primary,
        related,
    } = rooted.diagnostic;
    Some(StableCheckDiagnostic {
        root: coordinate_for_id(model, rooted.root)?,
        code,
        stage,
        severity,
        message,
        primary: match primary {
            Some(label) => Some(stabilize_label(model, label)?),
            None => None,
        },
        related: related
            .into_iter()
            .map(|label| stabilize_label(model, label))
            .collect::<Option<Vec<_>>>()?,
    })
}

fn stabilize_label(model: &CfdDataModel, label: CfdLabel) -> Option<StableCheckLabel> {
    Some(StableCheckLabel {
        record: match label.record {
            Some(id) => Some(coordinate_for_id(model, id)?),
            None => None,
        },
        path: label.path,
        message: label.message,
        origin: label.origin,
    })
}

fn render_check_state(
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    mut state: CheckState,
) -> Option<ProjectCheckOutput> {
    state.diagnostics.sort_by_key(|diagnostic| {
        model
            .record_by_type_key(&diagnostic.root.actual_type, &diagnostic.root.key)
            .map_or(usize::MAX, CfdRecordId::index)
    });
    let diagnostics = state
        .diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| render_diagnostic(model, diagnostic))
        .collect::<Option<Vec<_>>>()?;
    let raw = CfdDiagnostics::new(diagnostics);
    let logical_locations = logical_locations_from_cfd(&raw, |id| coordinate_for_id(model, id));
    Some(ProjectCheckOutput {
        diagnostics: map_diagnostics_with_origins(raw, origins),
        logical_locations,
        state,
    })
}

fn render_diagnostic(
    model: &CfdDataModel,
    diagnostic: StableCheckDiagnostic,
) -> Option<CfdDiagnostic> {
    Some(CfdDiagnostic {
        code: diagnostic.code,
        stage: diagnostic.stage,
        severity: diagnostic.severity,
        message: diagnostic.message,
        primary: match diagnostic.primary {
            Some(label) => Some(render_label(model, label)?),
            None => None,
        },
        related: diagnostic
            .related
            .into_iter()
            .map(|label| render_label(model, label))
            .collect::<Option<Vec<_>>>()?,
    })
}

fn render_label(model: &CfdDataModel, label: StableCheckLabel) -> Option<CfdLabel> {
    Some(CfdLabel {
        record: match label.record {
            Some(coordinate) => {
                Some(model.record_by_type_key(&coordinate.actual_type, &coordinate.key)?)
            }
            None => None,
        },
        path: label.path,
        message: label.message,
        origin: label.origin,
    })
}

fn coordinate_for_id(model: &CfdDataModel, id: CfdRecordId) -> Option<RecordCoordinate> {
    let record = model.record(id)?;
    Some(record.coordinate())
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
        state: CheckState::default(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use coflow_checker::{DependencyGraph, RootedCheckDiagnostic};
    use coflow_data_model::{CfdDiagnostic, CfdErrorCode, CfdRecordId};

    use super::{check_state_is_stable, render_raw_check_output, stabilize_check_state};
    use crate::load::empty_model;

    #[test]
    fn unstable_check_state_preserves_raw_diagnostics_and_disables_incremental_reuse() {
        let modules = coflow_cft::parse_modules(std::iter::empty::<coflow_cft::CftFile>());
        let dimensions = coflow_cft::CftDimensionInputs::default();
        let schema = coflow_cft::build_schema(&modules, &dimensions).expect("build empty schema");
        let model = empty_model(&schema).expect("build empty model");
        let diagnostic = CfdDiagnostic::error(CfdErrorCode::CheckFailed, "preserve me");
        let rooted = RootedCheckDiagnostic {
            root: CfdRecordId::from_index(99),
            diagnostic: diagnostic.clone(),
        };

        assert!(!check_state_is_stable(
            &model,
            std::slice::from_ref(&rooted),
            &DependencyGraph::default(),
        ));
        assert!(stabilize_check_state(&model, vec![rooted], DependencyGraph::default()).is_none());
        let output = render_raw_check_output(&model, &[], vec![diagnostic]);

        assert_eq!(output.diagnostics.diagnostics.len(), 1);
        assert_eq!(output.diagnostics.diagnostics[0].code, "CFD-CHECK-001");
        assert!(!output.state.incremental_ready);
    }
}
