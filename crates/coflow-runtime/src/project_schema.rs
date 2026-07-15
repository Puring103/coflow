use coflow_api::{Diagnostic, DiagnosticSet};
use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftModuleSet, CftSchema, ModuleId,
};
use coflow_project::{normalize_path, Project};
use std::path::PathBuf;
use std::sync::Arc;

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
    let dimensions = CftDimensionInputs::try_new(
        project
            .config
            .dimensions
            .iter()
            .map(|(name, config)| (name.clone(), config.variants.clone())),
    )
    .map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "RUNTIME-INTERNAL",
            "RUNTIME",
            format!("validated project dimensions are invalid: {error}"),
        ))
    })?;
    let (schema, cft_diagnostics) = match build_schema(&modules, &dimensions) {
        Ok(schema) => (Some(schema), Vec::new()),
        Err(errors) => (None, errors.diagnostics),
    };
    let diagnostics =
        diagnostic_set_from_cft(dedupe_cft_diagnostics(cft_diagnostics), &sources, &paths);
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
            Some(schema) => (build.modules, Some(schema)),
            None => (build.modules, None),
        }
    } else {
        (parse_modules(std::iter::empty::<CftFile>()), None)
    };
    Ok(ProjectSchemaSession {
        project,
        modules: Arc::new(modules),
        schema: schema.map(Arc::new),
        diagnostics,
    })
}
