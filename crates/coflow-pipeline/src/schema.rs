use coflow_cft::CftContainer;
use coflow_project::{
    compile_schema_project, dedupe_cft_diagnostics, diagnostic_set_from_cft, Project, SchemaBuild,
};

pub fn compile_project_schema(
    project: &Project,
) -> Result<Result<CftContainer, coflow_api::DiagnosticSet>, String> {
    let project_diagnostics = project.schema_diagnostic_set();
    if !project_diagnostics.is_empty() {
        return Ok(Err(project_diagnostics));
    }
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

fn diagnostics_from_schema_build(build: &SchemaBuild) -> coflow_api::DiagnosticSet {
    diagnostic_set_from_cft(
        dedupe_cft_diagnostics(build.diagnostics.clone()),
        &build.sources,
        &build.paths,
    )
}
