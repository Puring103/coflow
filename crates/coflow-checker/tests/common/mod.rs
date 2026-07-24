#![allow(dead_code, unused_imports)]
#![allow(clippy::redundant_pub_crate)]

pub(crate) use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId,
};
pub(crate) use coflow_checker::StructuralLimits;
pub(crate) use coflow_data_model::*;

pub(crate) fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(
        &modules,
        &CftDimensionInputs::try_new([
            ("language", vec!["zh".to_string(), "en".to_string()]),
            ("platform", vec!["pc".to_string(), "mobile".to_string()]),
        ])
        .expect("valid dimension fixture"),
    )
    .expect("schema should compile")
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

pub(crate) fn run_model_checks(
    model: &CfdDataModel,
    schema: &CftSchema,
) -> Result<(), CfdDiagnostics> {
    coflow_checker::execute_checks(
        schema,
        model,
        base_tasks(schema, model),
        coflow_checker::CheckLimits::default(),
    )
    .into_result()
}

pub(crate) fn run_model_checks_with_limits(
    model: &CfdDataModel,
    schema: &CftSchema,
    structural_limits: StructuralLimits,
) -> Result<(), CfdDiagnostics> {
    coflow_checker::execute_checks(
        schema,
        model,
        base_tasks(schema, model),
        coflow_checker::CheckLimits {
            structure: structural_limits,
            ..coflow_checker::CheckLimits::default()
        },
    )
    .into_result()
}

pub(crate) fn base_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
) -> Vec<coflow_checker::CheckTask> {
    let mut tasks = std::collections::BTreeSet::new();
    for (record, value) in model.records() {
        tasks.extend(
            schema
                .check_statements_for_actual_type(value.actual_type())
                .map(|statement| coflow_checker::CheckTask {
                    statement,
                    target: coflow_checker::CheckTarget::Record(record),
                    projection: coflow_checker::CheckProjection::Base,
                }),
        );
    }
    tasks.extend(
        schema
            .all_check_statements()
            .filter(|statement| matches!(statement.owner, coflow_cft::CheckOwner::Project(_)))
            .map(|statement| coflow_checker::CheckTask {
                statement: statement.id,
                target: coflow_checker::CheckTarget::Project,
                projection: coflow_checker::CheckProjection::Base,
            }),
    );
    tasks.into_iter().collect()
}
