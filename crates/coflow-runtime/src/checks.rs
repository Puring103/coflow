pub(crate) mod impact;
mod render;
mod store;

use crate::api::DiagnosticSet;
use crate::data_model::{CfdDataModel, RecordOrigin};
use crate::indexes::DiagnosticLogicalLocation;
use coflow_core::check::{execute_checks, CheckExecutionStats, CheckLimits, CheckOutput};
use coflow_core::schema::CftSchema;
use impact::CheckImpact;
use render::render_check_store;
use std::collections::BTreeMap;
pub(crate) use store::CheckDiagnosticStore;

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) state: CheckDiagnosticStore,
    pub(crate) statistics: CheckExecutionStats,
}

pub(crate) fn run_full_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> ProjectCheckOutput {
    // 无规则无需进入执行器；有规则必须明确报告未执行，不能伪造通过结果。
    let output = if schema.all_checks().next().is_none()
        && schema.all_types().all(|ty| ty.check.is_none())
    {
        CheckOutput::default()
    } else {
        execute_checks(
            schema,
            model,
            CheckLimits {
                evaluation: crate::limits::RuntimeLimits::default().evaluation,
            },
        )
    };
    let state = CheckDiagnosticStore {
        request_diagnostics: output.request_diagnostics,
    };
    render_check_store(schema, model, origins, state, output.statistics)
}

pub(crate) fn run_incremental_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
    _previous: &CheckDiagnosticStore,
    _impact: &CheckImpact,
) -> ProjectCheckOutput {
    run_full_project_checks(schema, model, origins)
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_core::schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
    fn schema(source: &str) -> CftSchema {
        build_schema(
            &parse_modules([CftFile::from_source(ModuleId::from("main"), source)]),
            &CftDimensionInputs::default(),
        )
        .expect("schema")
    }
    #[test]
    fn project_without_rules_does_not_enter_the_execution_stub() {
        let schema = schema("data Stats { hp: int; } table Item { stats: Stats; }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let output = run_full_project_checks(&schema, &model, &[]);
        assert!(output.diagnostics.is_empty());
        assert_eq!(output.statistics.executed_tasks, 0);
    }
    #[test]
    fn declared_check_reports_execution_unavailable() {
        let schema = schema("table Item { check { anything(); } } check Global { anything(); }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let output = run_full_project_checks(&schema, &model, &[]);
        assert!(output
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "EXEC-001"));
        assert_eq!(output.statistics.executed_tasks, 0);
    }
}
