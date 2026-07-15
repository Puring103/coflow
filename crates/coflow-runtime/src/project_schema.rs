use std::collections::BTreeSet;

use coflow_api::{Diagnostic, DiagnosticSet, Label, Severity, SourceLocation};
use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftModuleSet, CftSchema, ModuleId,
};
use coflow_project::{normalize_path, Project};
use std::path::PathBuf;
use std::sync::Arc;

use crate::dimensions;
use crate::indexes::DiagnosticsStore;
use crate::schema_diagnostics::{dedupe_cft_diagnostics, diagnostic_set_from_cft};
use crate::session::ProjectSchemaSession;

#[derive(Debug, Clone)]
pub struct SchemaTextOverride {
    pub requested_module: Option<String>,
    pub normalized_path: PathBuf,
    pub source: String,
}

#[derive(Debug)]
struct ProjectSchemaAttempt {
    schema: Option<CftSchema>,
    modules: CftModuleSet,
    diagnostics: DiagnosticSet,
}
/// Collects the effective project source text before parsing it exactly once.
/// Overrides are host snapshots (for example open LSP documents), never a
/// second schema input model.
fn collect_project_schema(
    project: &Project,
    overrides: &[SchemaTextOverride],
) -> Result<ProjectSchemaAttempt, DiagnosticSet> {
    let source_set = project.schema_sources()?;
    let mut matched_overrides = vec![false; overrides.len()];
    let mut files = Vec::new();

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
        files.push(CftFile::new(
            ModuleId::new(module.module_id),
            module.canonical_path,
            source,
        ));
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

    let modules = parse_modules(files);
    let sources = modules
        .modules()
        .map(|(id, module)| (id.as_str().to_string(), module.source().to_string()))
        .collect();
    let paths = modules
        .modules()
        .map(|(id, module)| (id.as_str().to_string(), module.path().display().to_string()))
        .collect();
    let dimensions = CftDimensionInputs::new(
        project
            .config
            .dimensions
            .iter()
            .map(|(name, config)| (name.clone(), config.variants.clone())),
    );
    let (schema, cft_diagnostics) = match build_schema(&modules, &dimensions) {
        Ok(schema) => (Some(schema), Vec::new()),
        Err(errors) => (None, errors.diagnostics),
    };
    let diagnostics = diagnostic_set_from_cft(
        dedupe_cft_diagnostics(cft_diagnostics),
        &sources,
        &paths,
    );
    Ok(ProjectSchemaAttempt {
        schema,
        modules,
        diagnostics,
    })
}

/// Opens and compiles a project schema without validating or loading data
/// sources.
///
/// # Errors
///
/// Returns unrecoverable project/schema I/O errors. User-fixable project and
/// schema diagnostics are captured in the returned session diagnostics.
pub(crate) fn open_project_schema_session(
    project: Project,
) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let diagnostics = project.schema_diagnostic_set();
    open_project_schema_attempt(project, diagnostics, &[])
}

pub(crate) fn open_project_schema_attempt(
    project: Project,
    diagnostics: DiagnosticSet,
    overrides: &[SchemaTextOverride],
) -> Result<ProjectSchemaSession, DiagnosticSet> {
    let mut diagnostics = DiagnosticsStore::from_set(diagnostics);
    let (modules, schema) = if diagnostics.is_empty() {
        let build = collect_project_schema(&project, overrides)?;
        diagnostics.extend(build.diagnostics);
        match build.schema {
            Some(schema) => {
                diagnostics.extend(validate_dimension_schema_config(&project, &schema));
                (build.modules, schema)
            }
            None => (build.modules, CftSchema::empty()),
        }
    } else {
        (parse_modules(std::iter::empty::<CftFile>()), CftSchema::empty())
    };
    Ok(ProjectSchemaSession {
        project,
        modules: Arc::new(modules),
        schema: Arc::new(schema),
        diagnostics,
    })
}

fn validate_dimension_schema_config(project: &Project, schema: &CftSchema) -> DiagnosticSet {
    let mut diagnostics = DiagnosticSet::empty();
    let mut required = BTreeSet::new();
    for field in dimensions::dimension_fields(schema) {
        required.insert(field.dimension.to_string());
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
