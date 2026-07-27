#![allow(dead_code, clippy::expect_used, clippy::panic)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_checker::CheckTask;
use coflow_data_model::{CfdDataModel, LoadedDictKeyDraft, LoadedValueDraft};
use coflow_runtime::check_benchmark_support::{
    plan_full, plan_incremental, BenchmarkFieldChange, BenchmarkProjection,
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Scenario {
    EmptyImpact,
    DirectField,
    DuplicateImpact,
    CrossTypeFanout,
    IndependentField,
    ProjectRecordSet,
    NestedObject,
    Batch(usize),
    DimensionVariant,
    DimensionBase,
    DimensionNonDimension,
    InheritedField,
    DiagnosticDense,
    WorstCaseFanout,
}

impl Scenario {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::EmptyImpact => "empty_impact",
            Self::DirectField => "direct_field",
            Self::DuplicateImpact => "duplicate_impact",
            Self::CrossTypeFanout => "cross_type_fanout",
            Self::IndependentField => "independent_field",
            Self::ProjectRecordSet => "project_record_set",
            Self::NestedObject => "nested_object",
            Self::Batch(1) => "batch_1",
            Self::Batch(10) => "batch_10",
            Self::Batch(100) => "batch_100",
            Self::Batch(_) => "batch",
            Self::DimensionVariant => "dimension_variant",
            Self::DimensionBase => "dimension_base",
            Self::DimensionNonDimension => "dimension_non_dimension",
            Self::InheritedField => "inherited_field",
            Self::DiagnosticDense => "diagnostic_dense",
            Self::WorstCaseFanout => "worst_case_fanout",
        }
    }
}

pub(crate) fn fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let source = r#"
        type Character { level: int; }
        type Item {
            price: int;
            name: string;
            enabled: bool;
            score: int;
            limit: int;
            nums: [int];
            nums2: [int];
            tags: [string];
            attrs: {string: int};
            owner: &Character;
            optional: int? = null;
            check {
                price > 0;
                name != "";
                enabled;
                score >= 0;
                limit >= score;
                nums.len() == 32;
                nums.isUnique();
                nums.isSorted();
                nums.min() >= 0;
                nums.sum() >= 0;
                tags.contains("item");
                tags.isUnique();
                attrs.containsKey("weight");
                attrs["weight"] >= 0;
                owner.level > 0;
                optional == null || optional >= 0;
                all number in nums { number >= 0; }
                all number in nums2 { number >= 0; }
            }
        }
        check ProjectIntegrity { records(Item).len() > 0; }
    "#;
    let schema = compile(source, CftDimensionInputs::default());
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..100 {
        builder.add_record(
            format!("hero_{index:03}"),
            "Character",
            [("level", 10_i64.into())],
        );
    }
    for index in 0..record_count {
        builder.add_record(
            item_key(index),
            "Item",
            [
                ("price", 1_i64.into()),
                ("name", format!("Item {index}").into()),
                ("enabled", true.into()),
                ("score", 10_i64.into()),
                ("limit", 20_i64.into()),
                (
                    "nums",
                    LoadedValueDraft::Array((0_i64..32).map(Into::into).collect()),
                ),
                (
                    "nums2",
                    LoadedValueDraft::Array((0_i64..32).map(Into::into).collect()),
                ),
                (
                    "tags",
                    LoadedValueDraft::Array(vec!["item".into(), "bench".into()]),
                ),
                (
                    "attrs",
                    LoadedValueDraft::dict([(LoadedDictKeyDraft::from("weight"), 1_i64.into())]),
                ),
                ("owner", LoadedValueDraft::record_ref("hero_000")),
            ],
        );
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn nested_fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let schema = compile(
        "type Part { value: int; check { value >= 0; } } type Item { parts: [Part]; }",
        CftDimensionInputs::default(),
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..record_count {
        builder.add_record(
            item_key(index),
            "Item",
            [(
                "parts",
                LoadedValueDraft::Array(
                    (0..8)
                        .map(|_| LoadedValueDraft::object("Part", [("value", 1_i64.into())]))
                        .collect(),
                ),
            )],
        );
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn dimension_fixture(
    record_count: usize,
    variant_count: usize,
) -> (CftSchema, CfdDataModel) {
    let schema = compile(
        r#"
            type Item {
                @localized
                name: string;
                price: int;
                check { name != "" && price > 0; }
            }
        "#,
        CftDimensionInputs::try_new([(
            "language",
            (0..variant_count)
                .map(|index| format!("v{index}"))
                .collect(),
        )])
        .expect("dimensions"),
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..record_count {
        builder.add_record(
            item_key(index),
            "Item",
            [
                ("name", format!("Item {index}").into()),
                ("price", 1_i64.into()),
            ],
        );
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn inheritance_fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let schema = compile(
        r#"
            abstract type Base {
                value: int;
                check {
                    value >= 0;
                    value < 100;
                }
            }
            type Item : Base {}
        "#,
        CftDimensionInputs::default(),
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..record_count {
        builder.add_record(item_key(index), "Item", [("value", 1_i64.into())]);
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn diagnostic_fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let schema = compile(
        r#"
            type Item {
                nums: [int];
                check {
                    all value, index in nums {
                        value > 0: f"value {value} at {index} must be positive";
                    }
                }
            }
        "#,
        CftDimensionInputs::default(),
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..record_count {
        builder.add_record(
            item_key(index),
            "Item",
            [("nums", LoadedValueDraft::Array(vec![0_i64.into(); 32]))],
        );
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn limit_fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let schema = compile(
        "type Item { value: int; check { value >= 0; } }",
        CftDimensionInputs::default(),
    );
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..record_count {
        builder.add_record(item_key(index), "Item", [("value", 1_i64.into())]);
    }
    let model = builder.build().expect("model");
    (schema, model)
}

pub(crate) fn worst_fixture(record_count: usize) -> (CftSchema, CfdDataModel) {
    let checks = (0..18)
        .map(|threshold| format!("owner.level > {threshold};"))
        .collect::<String>();
    let source = format!(
        "type Character {{ level: int; }} type Item {{ owner: &Character; check {{ {checks} }} }}"
    );
    let schema = compile(&source, CftDimensionInputs::default());
    let mut builder = CfdDataModel::builder(&schema);
    for index in 0..100 {
        builder.add_record(
            format!("hero_{index:03}"),
            "Character",
            [("level", 100_i64.into())],
        );
    }
    for index in 0..record_count {
        builder.add_record(
            item_key(index),
            "Item",
            [("owner", LoadedValueDraft::record_ref("hero_000"))],
        );
    }
    let model = builder.build().expect("model");
    (schema, model)
}

fn compile(source: &str, dimensions: CftDimensionInputs) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("bench"), source)]);
    build_schema(&modules, &dimensions).expect("schema")
}

pub(crate) fn full_tasks(schema: &CftSchema, model: &CfdDataModel) -> Vec<CheckTask> {
    plan_full(schema, model)
}

pub(crate) fn incremental_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    scenario: Scenario,
) -> Vec<CheckTask> {
    let (fields, record_sets) = match scenario {
        Scenario::EmptyImpact => (Vec::new(), Vec::new()),
        Scenario::DirectField => (
            vec![field_change("Item", &item_key(0), "price")],
            Vec::new(),
        ),
        Scenario::DuplicateImpact => (
            vec![field_change("Item", &item_key(0), "price"); 100],
            Vec::new(),
        ),
        Scenario::IndependentField => (
            vec![field_change("Item", &item_key(0), "score")],
            Vec::new(),
        ),
        Scenario::CrossTypeFanout | Scenario::WorstCaseFanout => (
            vec![field_change("Character", "hero_000", "level")],
            Vec::new(),
        ),
        Scenario::ProjectRecordSet => (Vec::new(), vec!["Item".to_string()]),
        Scenario::NestedObject => (
            vec![field_change("Item", &item_key(0), "parts")],
            Vec::new(),
        ),
        Scenario::Batch(count) => (
            (0..count.min(item_count(model)))
                .map(|index| field_change("Item", &item_key(index), "price"))
                .collect(),
            Vec::new(),
        ),
        Scenario::DimensionVariant => (
            vec![BenchmarkFieldChange {
                actual_type: "Item".to_string(),
                key: item_key(0),
                field: "name".to_string(),
                projection: BenchmarkProjection::Dimension {
                    dimension: "language".to_string(),
                    variant: "v0".to_string(),
                },
            }],
            Vec::new(),
        ),
        Scenario::DimensionBase => (vec![field_change("Item", &item_key(0), "name")], Vec::new()),
        Scenario::DimensionNonDimension => (
            vec![field_change("Item", &item_key(0), "price")],
            Vec::new(),
        ),
        Scenario::InheritedField | Scenario::DiagnosticDense => (
            vec![field_change(
                "Item",
                &item_key(0),
                if matches!(scenario, Scenario::InheritedField) {
                    "value"
                } else {
                    "nums"
                },
            )],
            Vec::new(),
        ),
    };
    plan_incremental(schema, model, fields, record_sets).expect("benchmark impact")
}

fn field_change(actual_type: &str, key: &str, field: &str) -> BenchmarkFieldChange {
    BenchmarkFieldChange {
        actual_type: actual_type.to_string(),
        key: key.to_string(),
        field: field.to_string(),
        projection: BenchmarkProjection::Base,
    }
}

fn item_count(model: &CfdDataModel) -> usize {
    model.records_of_type("Item").count()
}

fn item_key(index: usize) -> String {
    format!("item_{index:05}")
}

pub(crate) fn sample(mut operation: impl FnMut()) -> (Duration, Duration, Duration) {
    for _ in 0..2 {
        operation();
    }
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    (
        samples[0],
        samples[samples.len() / 2],
        samples[samples.len() - 1],
    )
}

pub(crate) fn sample_pair(
    mut first: impl FnMut(),
    mut second: impl FnMut(),
) -> (
    (Duration, Duration, Duration),
    (Duration, Duration, Duration),
) {
    for _ in 0..2 {
        first();
        second();
    }
    let mut first_samples = Vec::with_capacity(7);
    let mut second_samples = Vec::with_capacity(7);
    for index in 0..7 {
        let measure = |operation: &mut dyn FnMut(), samples: &mut Vec<Duration>| {
            let start = Instant::now();
            operation();
            samples.push(start.elapsed());
        };
        if index % 2 == 0 {
            measure(&mut first, &mut first_samples);
            measure(&mut second, &mut second_samples);
        } else {
            measure(&mut second, &mut second_samples);
            measure(&mut first, &mut first_samples);
        }
    }
    first_samples.sort_unstable();
    second_samples.sort_unstable();
    let summarize = |samples: &[Duration]| {
        (
            samples[0],
            samples[samples.len() / 2],
            samples[samples.len() - 1],
        )
    };
    (summarize(&first_samples), summarize(&second_samples))
}
