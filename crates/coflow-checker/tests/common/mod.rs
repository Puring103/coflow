#![allow(dead_code, unused_imports)]
#![allow(clippy::redundant_pub_crate)]

pub(crate) use coflow_cft::{CftContainer, CftSchema, ModuleId};
pub(crate) use coflow_checker::CfdCheckExt;
pub(crate) use coflow_data_model::*;

pub(crate) fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema should parse");
    container.compile().expect("schema should compile");
    container
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
