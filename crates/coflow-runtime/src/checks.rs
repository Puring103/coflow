use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{byte_range, map_diagnostics_with_origins, DiagnosticContext, DiagnosticSet, Label, SourceLocation};
use coflow_cft::CftSchema;
use coflow_checker::{
    run_checks, CheckDiagnostic, CheckDiagnosticContext, CheckExecutionStats, CheckRequest,
    CheckChangeSet, CheckSnapshot, DependencyCollection, DimensionCheckRound,
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
        if let Some(rendered) = render_check_snapshot(schema, model, origins, snapshot, statistics) {
            return rendered;
        }
    }
    render_raw_check_output(
        schema,
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
        CheckRequest::incremental(
            previous,
            &CheckChangeSet::from_records(schema, changed.iter().cloned()),
        )
        .with_rounds(dimension_check_rounds(schema)),
    );
    render_check_snapshot(schema, model, origins, output.snapshot?, output.statistics)
}

fn render_check_snapshot(
    schema: &CftSchema,
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
        diagnostics: map_check_diagnostics_with_origins(Some(schema), raw, origins),
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
    schema: &CftSchema,
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
        diagnostics: map_check_diagnostics_with_origins(Some(schema), diagnostics, origins),
        logical_locations,
        state: CheckSnapshot::default(),
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

#[cfg(test)]
mod tests {
    use super::map_check_diagnostics_with_origins;
    use coflow_checker::{CheckDiagnostic, CheckDiagnosticContext, CheckSnapshot};
    use coflow_data_model::{CfdDiagnostic, CfdErrorCode};
    use coflow_api::SourceLocation;
    use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId, Span};

    #[test]
    fn default_snapshot_disables_incremental_reuse() {
        assert!(!CheckSnapshot::default().is_reusable());
    }

    #[test]
    fn check_contexts_map_without_changing_custom_message() {
        let mapped = map_check_diagnostics_with_origins(
            None,
            vec![CheckDiagnostic {
                diagnostic: CfdDiagnostic::error(CfdErrorCode::CheckFailed, "custom message"),
                contexts: vec![CheckDiagnosticContext::When {
                    expression: "enabled".to_string(),
                }],
                is_custom_message: true,
                schema_location: None,
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

    #[test]
    fn schema_only_check_diagnostics_map_to_the_cft_file_span() {
        let source = "check Integrity { false; }";
        let modules = parse_modules([CftFile::new(
            ModuleId::from("rules"),
            std::path::PathBuf::from("schema/rules.cft"),
            source,
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
        let mapped = map_check_diagnostics_with_origins(
            Some(&schema),
            vec![CheckDiagnostic {
                diagnostic: CfdDiagnostic::error(CfdErrorCode::CheckFailed, "failed"),
                contexts: vec![CheckDiagnosticContext::Check {
                    name: "Integrity".to_string(),
                }],
                is_custom_message: false,
                schema_location: Some(coflow_checker::CheckSchemaLocation {
                    module: ModuleId::from("rules"),
                    span: Span::new(18, 24),
                }),
            }],
            &[],
        );

        assert!(matches!(
            &mapped.diagnostics[0].primary.as_ref().expect("primary").location,
            SourceLocation::FileSpan { path, .. }
                if path == &std::path::PathBuf::from("schema/rules.cft")
        ));
        assert_eq!(mapped.diagnostics[0].contexts[0].kind, "check");
        assert_eq!(mapped.diagnostics[0].contexts[0].name.as_deref(), Some("Integrity"));
    }
}
