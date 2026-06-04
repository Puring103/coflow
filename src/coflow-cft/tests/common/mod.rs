#![allow(dead_code, unused_imports)]

pub(crate) use coflow_cft::{
    CftConstValue, CftContainer, CftDiagnostics, CftErrorCode, CftSeverity, CftStage, ModuleId,
};

pub(crate) fn add_source(source: &str) -> Result<CftContainer, CftDiagnostics> {
    let mut container = CftContainer::new();
    container.add_module(ModuleId::from("main"), source)?;
    Ok(container)
}

pub(crate) fn compile_one(source: &str) -> Result<CftContainer, CftDiagnostics> {
    let mut container = add_source(source)?;
    container.compile()?;
    Ok(container)
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
