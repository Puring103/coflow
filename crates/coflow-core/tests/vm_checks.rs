use coflow_core::{
    check::{CheckLimits, EvaluationLimits},
    contract::Contract,
    runtime::{CheckSelection, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId},
};
use std::sync::Arc;
fn runtime(source: &str, data: &str) -> Arc<coflow_core::runtime::Runtime> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("checks"), source)]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let contract = Arc::new(Contract::new(schema).expect("compiled contract"));
    let contract = Arc::new(
        Contract::from_bytes(&contract.to_bytes().expect("bytes")).expect("load bytecode"),
    );
    let mut builder = RuntimeBuilder::new(contract);
    builder.add_text(data, Some("data.cfd"));
    builder.build().runtime.expect("runtime")
}
#[test]
fn check_reports_all_failed_require_calls_and_runs_each_request() {
    let runtime=runtime("use Coflow::Check::require; table Item { value: int; check Positive { require(self.value > 0, \"positive\"); require(self.value > 10, \"large\"); } }","a: Item {value: -1}");
    for _ in 0..2 {
        let output = runtime.run_checks(CheckSelection::default(), CheckLimits::default());
        assert_eq!(output.statistics.executed_tasks, 1);
        assert_eq!(output.request_diagnostics.len(), 2);
        assert_eq!(output.request_diagnostics[0].diagnostic.message, "positive");
        assert_eq!(output.request_diagnostics[1].diagnostic.message, "large");
    }
}
#[test]
fn fault_ends_current_check_and_other_rules_continue() {
    let runtime=runtime("use Coflow::Check::require; table Item { check Bad { require(1 // 0 > 0, \"never\"); } check Next { require(false, \"next\"); } }","a: Item {}");
    let output = runtime.run_checks(CheckSelection::default(), CheckLimits::default());
    assert_eq!(output.statistics.executed_tasks, 2);
    assert_eq!(output.request_diagnostics.len(), 2);
    assert_eq!(output.request_diagnostics[1].diagnostic.message, "next");
}
#[test]
fn global_check_queries_records_and_checks_are_optional() {
    let runtime=runtime("use Coflow::Check::require; use Coflow::Check::records; table Item { value: int; } check All { for item in records(Item) { require(item.value > 0, item.id); } }","a: Item {value:-1} b: Item {value:1}");
    assert!(runtime.record("Item", "a").is_ok());
    let output = runtime.run_checks(CheckSelection::default(), CheckLimits::default());
    assert_eq!(output.request_diagnostics.len(), 1);
    assert_eq!(output.request_diagnostics[0].diagnostic.message, "a");
    assert!(runtime.record("Item", "a").is_ok());
}
#[test]
fn check_budget_stops_request_and_records_unfinished_tasks() {
    let runtime=runtime("use Coflow::Check::require; table Item { check Forever { while true {} } check Later { require(false, \"later\"); } }","a: Item {}");
    let output = runtime.run_checks(
        CheckSelection::default(),
        CheckLimits {
            evaluation: EvaluationLimits::new(20, 20),
        },
    );
    assert_eq!(output.statistics.executed_tasks, 1);
    assert_eq!(output.statistics.rejected_tasks, 1);
    assert_eq!(output.statistics.work_used, 20);
    assert!(!output.is_success());
}
#[test]
fn invalid_function_and_check_bodies_fail_contract_compilation() {
    for source in [
        "table Item { run: fn() -> int => { missing() }; }",
        "table Item { check { return; } }",
        "table Item { check { missing(); } }",
    ] {
        let schema = build_schema(
            &parse_modules([CftFile::from_source(ModuleId::from("invalid"), source)]),
            &CftDimensionInputs::default(),
        )
        .expect("declarations");
        assert!(Contract::new(schema).is_err(), "{source}");
    }
}

#[test]
fn inherited_rules_run_parent_first_and_require_reports_exact_call_sites() {
    let source="use Coflow::Check::require; table Parent { check Base { require(false, \"base\"); } } table Child: Parent { check ChildRule { require(false, \"child\"); } }";
    let runtime = runtime(source, "c: Child {}");
    let output = runtime.run_checks(CheckSelection::default(), CheckLimits::default());
    assert_eq!(
        output
            .request_diagnostics
            .iter()
            .map(|d| d.diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["base", "child"]
    );
    for diagnostic in &output.request_diagnostics {
        let location = diagnostic.schema_location.as_ref().expect("source");
        assert_eq!(
            &source[location.span.start..location.span.end],
            format!("require(false, \"{}\")", diagnostic.diagnostic.message)
        );
    }
    let selected = runtime.run_checks(
        CheckSelection {
            names: ["ChildRule".into()].into(),
            ..CheckSelection::default()
        },
        CheckLimits::default(),
    );
    assert_eq!(selected.statistics.executed_tasks, 1);
    assert_eq!(selected.request_diagnostics[0].diagnostic.message, "child");
}
