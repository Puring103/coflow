mod common;

use coflow_cft::CftSchema;
use coflow_checker::{execute_checks, CheckLimits};
use coflow_data_model::CfdDataModel;
use common::{
    diagnostic_fixture, dimension_fixture, fixture, full_tasks, incremental_tasks,
    inheritance_fixture, nested_fixture, sample_pair, worst_fixture, Scenario,
};
use std::hint::black_box;

fn main() {
    println!(
        "scenario,records,variants,changes,full_plan_ms,full_execute_ms,incremental_plan_ms,incremental_execute_ms,full_tasks,incremental_tasks,full_diagnostics,incremental_diagnostics,full_work,incremental_work,full_projected_records,incremental_projected_records,full_records_per_s,incremental_records_per_s,speedup,incremental_planning_pct"
    );
    for records in [1_000, 5_000, 20_000] {
        let (schema, model) = fixture(records);
        bench_case(&schema, &model, Scenario::DirectField, records, 0, 1);
    }

    let (schema, model) = fixture(5_000);
    for scenario in [
        Scenario::EmptyImpact,
        Scenario::DuplicateImpact,
        Scenario::CrossTypeFanout,
        Scenario::IndependentField,
        Scenario::ProjectRecordSet,
        Scenario::Batch(1),
        Scenario::Batch(10),
        Scenario::Batch(100),
    ] {
        let changes = match scenario {
            Scenario::Batch(count) => count,
            Scenario::EmptyImpact => 0,
            Scenario::DuplicateImpact => 100,
            _ => 1,
        };
        bench_case(&schema, &model, scenario, 5_000, 0, changes);
    }

    let (schema, model) = nested_fixture(5_000);
    bench_case(&schema, &model, Scenario::NestedObject, 5_000, 0, 1);

    for variants in [2, 5, 10] {
        let (schema, model) = dimension_fixture(5_000, variants);
        for scenario in [
            Scenario::DimensionVariant,
            Scenario::DimensionBase,
            Scenario::DimensionNonDimension,
        ] {
            bench_case(&schema, &model, scenario, 5_000, variants, 1);
        }
    }

    let (schema, model) = inheritance_fixture(5_000);
    bench_case(&schema, &model, Scenario::InheritedField, 5_000, 0, 1);

    let (schema, model) = diagnostic_fixture(1_000);
    bench_case(&schema, &model, Scenario::DiagnosticDense, 1_000, 0, 1);

    let (schema, model) = worst_fixture(5_000);
    bench_case(&schema, &model, Scenario::WorstCaseFanout, 5_000, 0, 1);
}

fn bench_case(
    schema: &CftSchema,
    model: &CfdDataModel,
    scenario: Scenario,
    records: usize,
    variants: usize,
    changes: usize,
) {
    if !selected(scenario, records) {
        return;
    }
    let ((_, full_plan, _), (_, incremental_plan, _)) = sample_pair(
        || {
            black_box(full_tasks(black_box(schema), black_box(model)));
        },
        || {
            black_box(incremental_tasks(
                black_box(schema),
                black_box(model),
                scenario,
            ));
        },
    );
    let full = full_tasks(schema, model);
    let incremental = incremental_tasks(schema, model, scenario);
    let ((_, full_execute, _), (_, incremental_execute, _)) = sample_pair(
        || {
            black_box(execute_checks(
                black_box(schema),
                black_box(model),
                full.clone(),
                CheckLimits::default(),
            ));
        },
        || {
            black_box(execute_checks(
                black_box(schema),
                black_box(model),
                incremental.clone(),
                CheckLimits::default(),
            ));
        },
    );
    let full_output = execute_checks(schema, model, full.clone(), CheckLimits::default());
    let incremental_output =
        execute_checks(schema, model, incremental.clone(), CheckLimits::default());
    let full_diagnostics = full_output.diagnostics().count();
    let incremental_diagnostics = incremental_output.diagnostics().count();
    let full_total = full_plan + full_execute;
    let incremental_total = incremental_plan + incremental_execute;
    let speedup = full_total.as_secs_f64() / incremental_total.as_secs_f64().max(f64::EPSILON);
    let planning_pct =
        100.0 * incremental_plan.as_secs_f64() / incremental_total.as_secs_f64().max(f64::EPSILON);
    let full_throughput = records as f64 / full_execute.as_secs_f64().max(f64::EPSILON);
    let incremental_throughput =
        records as f64 / incremental_execute.as_secs_f64().max(f64::EPSILON);
    println!(
        "{},{records},{variants},{changes},{:.3},{:.3},{:.3},{:.3},{},{},{full_diagnostics},{incremental_diagnostics},{},{},{},{},{full_throughput:.0},{incremental_throughput:.0},{speedup:.2},{planning_pct:.1}",
        scenario.name(),
        ms(full_plan),
        ms(full_execute),
        ms(incremental_plan),
        ms(incremental_execute),
        full.len(),
        incremental.len(),
        full_output.statistics.work_used,
        incremental_output.statistics.work_used,
        full_output.statistics.dimension_projected_records,
        incremental_output.statistics.dimension_projected_records,
    );
}

fn selected(scenario: Scenario, records: usize) -> bool {
    std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with('-'))
        .is_none_or(|filter| {
            filter == scenario.name() || filter == format!("{}:{records}", scenario.name())
        })
}

fn ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
