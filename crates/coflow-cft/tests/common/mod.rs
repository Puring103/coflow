#![allow(dead_code, unused_imports)]
#![allow(clippy::redundant_pub_crate)]

pub(crate) use coflow_cft::{
    build_schema, parse_modules, CftConstValue, CftDiagnostics, CftDimensions, CftErrorCode,
    CftFile, CftModuleSet, CftSchema, CftSeverity, CftStage, ModuleId,
};

pub(crate) fn add_source(source: &str) -> Result<CftModuleSet, CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    if modules.diagnostics().is_empty() {
        Ok(modules)
    } else {
        Err(modules.diagnostics().clone())
    }
}

pub(crate) fn compile_one(source: &str) -> Result<CftSchema, CftDiagnostics> {
    let modules = add_source(source)?;
    build_schema(&modules, &CftDimensions::default())
}

pub(crate) fn assert_has_code(diags: &CftDiagnostics, code: CftErrorCode) {
    assert!(
        diags.diagnostics.iter().any(|diag| diag.code == code),
        "expected {code}, got {:?}",
        diags
            .diagnostics
            .iter()
            .map(|diag| diag.code)
            .collect::<Vec<_>>()
    );
}

pub(crate) fn assert_primary_stage(diags: &CftDiagnostics, code: CftErrorCode, stage: CftStage) {
    let diag = diags
        .diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .expect("diagnostic code");
    assert_eq!(diag.stage, stage);
    assert_eq!(diag.severity, CftSeverity::Error);
    assert!(diag.primary.is_some());
}
