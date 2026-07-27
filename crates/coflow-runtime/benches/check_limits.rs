mod common;

use coflow_checker::{execute_checks, CheckLimits};
use coflow_runtime::check_benchmark_support::plan_full_with_limit;
use common::{full_tasks, limit_fixture, sample};
use std::hint::black_box;

fn main() {
    println!(
        "scenario,records,max_tasks,plan_median_ms,execute_median_ms,planned_tasks,executed_tasks,rejected_tasks,request_diagnostics"
    );
    let records = 5_000;
    let (schema, model) = limit_fixture(records);
    let tasks = full_tasks(&schema, &model);
    for (scenario, max_tasks) in [
        ("exact_limit", tasks.len()),
        ("overflow", tasks.len().saturating_sub(1)),
    ] {
        let (_, plan_median, _) = sample(|| {
            let _ = black_box(plan_full_with_limit(
                black_box(&schema),
                black_box(&model),
                max_tasks,
            ));
        });
        let (_, execute_median, _) = sample(|| {
            let _ = black_box(execute_checks(
                black_box(&schema),
                black_box(&model),
                tasks.clone(),
                CheckLimits {
                    max_tasks,
                    ..CheckLimits::default()
                },
            ));
        });
        let planned = plan_full_with_limit(&schema, &model, max_tasks);
        let output = execute_checks(
            &schema,
            &model,
            tasks.clone(),
            CheckLimits {
                max_tasks,
                ..CheckLimits::default()
            },
        );
        println!(
            "{scenario},{records},{max_tasks},{:.3},{:.3},{},{},{},{}",
            ms(plan_median),
            ms(execute_median),
            planned.as_ref().map_or(0, Vec::len),
            output.statistics.executed_tasks,
            output.statistics.rejected_tasks,
            output.request_diagnostics.len(),
        );
    }
}

fn ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}
