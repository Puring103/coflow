use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{CftContainer, CftDiagnostic, CftSchemaView, ModuleId};
use coflow_project::{normalize_path, Project};
use std::path::PathBuf;

use crate::dimensions;
use crate::indexes::DiagnosticsStore;
use crate::schema_diagnostics::{dedupe_cft_diagnostics, diagnostic_set_from_cft};
use crate::session::ProjectSchemaSession;

#[derive(Debug)]
pub struct SchemaBuild {
    pub container: Option<CftContainer>,
    pub diagnostics: Vec<CftDiagnostic>,
    pub sources: BTreeMap<String, String>,
    pub paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct SchemaSourceOverride {
    pub requested_module: Option<String>,
    pub normalized_path: PathBuf,
    pub source: String,
}

/// Compiles project schema sources with optional in-memory host overrides.
///
/// # Errors
///
/// Returns diagnostics when sources cannot be read or an override does not
/// identify a configured schema module.
pub fn compile_schema_project_with_overrides(
    project: &Project,
    overrides: &[SchemaSourceOverride],
) -> Result<SchemaBuild, DiagnosticSet> {
    let source_set = project.schema_sources()?;
    let mut matched_overrides = vec![false; overrides.len()];
    let mut sources = BTreeMap::new();
    let mut paths = BTreeMap::new();
    let mut container = CftContainer::new();
    let mut diagnostics = Vec::new();

    for module in source_set.modules {
        let source = if let Some((index, source_override)) = overrides
            .iter()
            .enumerate()
            .rev()
            .find(|(_, source_override)| {
                source_override
                    .requested_module
                    .as_deref()
                    .is_some_and(|requested| requested == module.module_id)
                    || normalize_path(&module.canonical_path) == source_override.normalized_path
            }) {
            matched_overrides[index] = true;
            source_override.source.clone()
        } else {
            module.source
        };
        sources.insert(module.module_id.clone(), source.clone());
        paths.insert(
            module.module_id.clone(),
            module.canonical_path.display().to_string(),
        );
        if let Err(errors) = container.add_module(ModuleId::new(module.module_id), source) {
            diagnostics.extend(errors.diagnostics);
        }
    }

    for (index, matched) in matched_overrides.into_iter().enumerate() {
        if !matched {
            let source_override = &overrides[index];
            let requested = source_override.requested_module.as_deref().map_or_else(
                || source_override.normalized_path.display().to_string(),
                str::to_string,
            );
            return Err(DiagnosticSet::one(Diagnostic::error(
                "SCHEMA-STDIN-PATH",
                "SCHEMA",
                format!("`--stdin-path {requested}` is not part of the configured schema"),
            )));
        }
    }

    let compiled = if diagnostics.is_empty() {
        match container.compile() {
            Ok(()) => Some(container),
            Err(errors) => {
                diagnostics.extend(errors.diagnostics);
                None
            }
        }
    } else {
        None
    };

    Ok(SchemaBuild {
        container: compiled,
        diagnostics,
        sources,
        paths,
    })
}

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
    let build = compile_schema_project_with_overrides(project, &[])?;
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
