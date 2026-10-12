#![cfg(feature = "internal-check-bench")]

#[path = "../benches/common/mod.rs"]
mod common;

#[test]
fn benchmark_inputs_use_the_current_declarations_and_execute_checks() {
    // 小规模运行真实夹具，防止基准代码编译成功却使用已移除的语言契约。
    let cases = [
        common::fixture(2),
        common::nested_fixture(2),
        common::dimension_fixture(2, 2),
        common::inheritance_fixture(2),
        common::limit_fixture(2),
        common::worst_fixture(2),
    ];
    for (schema, model) in &cases {
        let tasks = common::full_tasks(schema, model);
        assert!(!tasks.is_empty());
        let output = coflow_runtime::execute_checks(
            schema,
            model,
            tasks,
            coflow_runtime::CheckLimits::default(),
        );
        assert!(
            output
                .results
                .iter()
                .all(|result| result.diagnostics.is_empty()),
            "{output:?}"
        );
    }
    assert_eq!(cases[2].1.dimension_variants("language").count(), 2);
    let (schema, model) = common::diagnostic_fixture(2);
    let output = coflow_runtime::execute_checks(
        &schema,
        &model,
        common::full_tasks(&schema, &model),
        coflow_runtime::CheckLimits::default(),
    );
    assert!(output
        .results
        .iter()
        .any(|result| !result.diagnostics.is_empty()));
}
