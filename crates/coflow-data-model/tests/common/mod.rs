#![allow(dead_code, unused_imports)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_pub_crate,
    clippy::unwrap_used
)]

pub(crate) use coflow_cft::{
    build_schema, parse_modules, CftDimensions, CftFile, CftSchema, ModuleId,
};
pub(crate) use coflow_data_model::*;

pub(crate) fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::new(
        ModuleId::from("main"),
        std::path::PathBuf::from("main.cft"),
        source,
    )]);
    build_schema(&modules, &CftDimensions::default()).expect("schema should compile")
}

pub(crate) fn assert_has_code(diags: &CfdDiagnostics, code: CfdErrorCode) {
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

pub(crate) fn record_id_at(model: &CfdDataModel, index: usize) -> CfdRecordId {
    model
        .records()
        .map(|(record_id, _)| record_id)
        .find(|record_id| record_id.index() == index)
        .expect("record id should exist")
}

pub(crate) fn diagnostic_with_code(diags: &CfdDiagnostics, code: CfdErrorCode) -> &CfdDiagnostic {
    diags
        .diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| panic!("diagnostic with code {code} not found"))
}

pub(crate) fn primary_path_segments(diag: &CfdDiagnostic) -> &[CfdPathSegment] {
    &diag
        .primary
        .as_ref()
        .expect("primary label should be present")
        .path
        .segments
}
