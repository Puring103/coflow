use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{CftContainer, CftSchemaView};
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, diagnostic_set_from_cft, Project, SchemaBuild,
};

use crate::dimensions;
use crate::indexes::DiagnosticsStore;
use crate::session::ProjectSchemaSession;

/// Opens and compiles a project schema without validating or loading data
/// sources.
///
/// # Errors
///
/// Returns unrecoverable project/schema I/O errors. User-fixable project and
/// schema diagnostics are captured in the returned session diagnostics.
pub(crate) fn build_project_schema_session(
    project: Project,
) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let diagnostics = project.schema_diagnostic_set();
    build_project_schema_with_diagnostics(project, diagnostics)
}

pub(crate) fn build_project_schema_with_diagnostics(
    project: Project,
    diagnostics: DiagnosticSet,
) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let mut diagnostics = DiagnosticsStore::from_set(diagnostics);
    let schema = if diagnostics.is_empty() {
        match compile_project_schema(&project)? {
            Ok(mut schema) => {
                diagnostics.extend(validate_dimension_schema_config(&project, &schema));
                if diagnostics.is_empty() {
                    if let Err(err) =
                        dimensions::inject_dimension_types(&mut schema, &project.config.dimensions)
                    {
                        diagnostics.extend(diagnostic_set_from_cft(
                            err.diagnostics,
                            &BTreeMap::new(),
                            &BTreeMap::new(),
                        ));
                    }
                }
                schema
            }
            Err(schema_diagnostics) => {
                diagnostics.extend(schema_diagnostics);
                CftContainer::new()
            }
        }
    } else {
        CftContainer::new()
    };
    Ok(ProjectSchemaSession {
        project,
        schema,
        diagnostics,
    })
}

fn validate_dimension_schema_config(project: &Project, schema: &CftContainer) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let mut required = BTreeSet::new();
    let view = CftSchemaView::new(schema);
    for field in dimensions::dimension_fields(&view) {
        required.insert(field.dimension);
    }
    for dimension in required {
        if project.config.dimensions.contains_key(&dimension) {
            continue;
        }
        let message = if dimension == "language" {
            "schema contains @localized fields but dimensions.language is not configured"
                .to_string()
        } else {
            format!("schema contains @dimension(\"{dimension}\") fields but dimensions.{dimension} is not configured")
        };
        diagnostics.push(Diagnostic {
            code: "DIM-CONFIG-001".to_string(),
            stage: "PROJECT".to_string(),
            severity: Severity::Error,
            message,
            primary: Some(Label {
                location: SourceLocation::ProjectConfig {
                    path: project.config_path.clone(),
                    key_path: vec!["dimensions".to_string(), dimension],
                },
                message: None,
            }),
            related: Vec::new(),
        });
    }
    diagnostics
}

fn compile_project_schema(
    project: &Project,
) -> Result<Result<CftContainer, DiagnosticSet>, DiagnosticSet> {
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        return Ok(Err(project_diagnostics));
    }
    let build = compile_schema_project(project, None)?;
    let diagnostics = diagnostics_from_schema_build(&build);
    if diagnostics.is_empty() {
        build
            .container
            .ok_or_else(|| {
                DiagnosticSet::one(Diagnostic::error(
                    "PROJECT-SCHEMA",
                    "PROJECT",
                    "schema compilation did not produce a container",
                ))
            })
            .map(Ok)
    } else {
        Ok(Err(diagnostics))
    }
}

fn diagnostics_from_schema_build(build: &SchemaBuild) -> DiagnosticSet {
    diagnostic_set_from_cft(
        dedupe_cft_diagnostics(build.diagnostics.clone()),
        &build.sources,
        &build.paths,
    )
}
