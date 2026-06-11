use coflow_cft::CftContainer;
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, DiagnosticJson, Project, SchemaBuild,
};

pub(crate) fn compile_project_schema(
    project: &Project,
) -> Result<Result<CftContainer, Vec<DiagnosticJson>>, String> {
    let build = compile_schema_project(project, None)?;
    let diagnostics = diagnostics_from_schema_build(&build);
    if diagnostics.is_empty() {
        build
            .container
            .ok_or_else(|| "schema compilation did not produce a container".to_string())
            .map(Ok)
    } else {
        Ok(Err(diagnostics))
    }
}

fn diagnostics_from_schema_build(build: &SchemaBuild) -> Vec<DiagnosticJson> {
    dedupe_cft_diagnostics(build.diagnostics.clone())
        .iter()
        .map(|diagnostic| DiagnosticJson::from_cft(diagnostic, &build.sources, &build.paths))
        .collect()
}
