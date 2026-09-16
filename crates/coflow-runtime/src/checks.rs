mod render;
mod store;

use crate::api::DiagnosticSet;
use crate::data_model::{CfdDataModel, RecordOrigin};
use crate::indexes::DiagnosticLogicalLocation;
use coflow_core::check::{execute_checks, CheckExecutionStats, CheckLimits};
use coflow_core::schema::CftSchema;
use render::render_check_store;
use std::collections::BTreeMap;
pub(crate) use store::CheckDiagnosticStore;

#[derive(Debug)]
pub(crate) struct ProjectCheckOutput {
    pub(crate) diagnostics: DiagnosticSet,
    pub(crate) logical_locations: BTreeMap<usize, DiagnosticLogicalLocation>,
    pub(crate) statistics: CheckExecutionStats,
}

pub(crate) fn run_full_project_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    origins: &[RecordOrigin],
) -> ProjectCheckOutput {
    // 项目 check 同时验证所有函数的编译与链接；无规则时执行任务数仍为零。
    let output = execute_checks(
        schema,
        model,
        CheckLimits {
            evaluation: crate::limits::RuntimeLimits::default().evaluation,
        },
    );
    let state = CheckDiagnosticStore {
        request_diagnostics: output.request_diagnostics,
    };
    render_check_store(schema, model, origins, state, output.statistics)
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
    fn project_without_rules_has_no_execution_tasks() {
        let schema = schema("data Stats { hp: int; } table Item { stats: Stats; }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let output = run_full_project_checks(&schema, &model, &[]);
        assert!(output.diagnostics.is_empty());
        assert_eq!(output.statistics.executed_tasks, 0);
    }
    #[test]
    fn project_check_compiles_functions_even_without_check_rules() {
        let schema = schema("table Item { run: fn() -> int => { missing() }; }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let output = run_full_project_checks(&schema, &model, &[]);
        assert!(!output.diagnostics.is_empty());
        assert_eq!(output.statistics.executed_tasks, 0);
    }
    #[test]
    fn global_check_executes_and_reports_failure() {
        let schema = schema("use Coflow::Check::require; table Item {} check Global { require(false, \"failed\"); }");
        let model = CfdDataModel::builder(&schema).build().expect("model");
        let output = run_full_project_checks(&schema, &model, &[]);
        assert!(output
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.message == "failed"));
        assert_eq!(output.statistics.executed_tasks, 1);
    }
}
