mod common;

use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
use common::{
    diagnostic_fixture, dimension_fixture, fixture, full_tasks, incremental_tasks,
    inheritance_fixture, nested_fixture, sample, worst_fixture, Scenario,
};
use std::hint::black_box;

fn main() {
    println!(
        "scenario,records,variants,changes,full_min_ms,full_median_ms,full_max_ms,incremental_min_ms,incremental_median_ms,incremental_max_ms,full_tasks,incremental_tasks"
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
    let (full_min, full_median, full_max) = sample(|| {
        black_box(full_tasks(black_box(schema), black_box(model)));
    });
    let (incremental_min, incremental_median, incremental_max) = sample(|| {
        black_box(incremental_tasks(
            black_box(schema),
            black_box(model),
            scenario,
        ));
    });
    let full_count = full_tasks(schema, model).len();
    let incremental_count = incremental_tasks(schema, model, scenario).len();
    println!(
        "{},{records},{variants},{changes},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{full_count},{incremental_count}",
        scenario.name(),
        ms(full_min),
        ms(full_median),
        ms(full_max),
        ms(incremental_min),
        ms(incremental_median),
        ms(incremental_max),
    );
}

fn ms(duration: std::time::Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
