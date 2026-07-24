#![allow(dead_code, clippy::expect_used, clippy::panic)]

use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, CheckDependency,
    CheckField, CheckOwner, FieldName, ModuleId, TypeName,
};
use coflow_checker::{
    execute_checks, CheckDiagnostic, CheckLimits, CheckProjection, CheckTarget, CheckTask,
};
use coflow_data_model::{CfdDataModel, LoadedDictKeyDraft, LoadedValueDraft};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(crate) enum Scenario {
    DirectField,
    CrossTypeFanout,
    IndependentField,
    ProjectRecordSet,
    NestedObject,
    Batch(usize),
    DimensionVariant,
    WorstCaseFanout,
}

impl Scenario {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::DirectField => "direct_field",
            Self::CrossTypeFanout => "cross_type_fanout",
            Self::IndependentField => "independent_field",
            Self::ProjectRecordSet => "project_record_set",
            Self::NestedObject => "nested_object",
            Self::Batch(1) => "batch_1",
            Self::Batch(10) => "batch_10",
            Self::Batch(100) => "batch_100",
            Self::Batch(_) => "batch",
            Self::DimensionVariant => "dimension_variant",
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
    let mut tasks = BTreeSet::new();
    for (record, value) in model.records() {
        for statement in schema.check_statements_for_actual_type(value.actual_type()) {
            insert_statement_tasks(schema, statement, CheckTarget::Record(record), &mut tasks);
        }
    }
    let project_statements = schema
        .all_check_statements()
        .filter(|info| matches!(info.owner, CheckOwner::Project(_)))
        .map(|info| info.id)
        .collect::<Vec<_>>();
    for statement in project_statements {
        insert_statement_tasks(schema, statement, CheckTarget::Project, &mut tasks);
    }
    finish_tasks(schema, tasks)
}

pub(crate) fn incremental_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    scenario: Scenario,
) -> Vec<CheckTask> {
    match scenario {
        Scenario::DirectField => direct_tasks(schema, model, "price"),
        Scenario::IndependentField => direct_tasks(schema, model, "score"),
        Scenario::CrossTypeFanout | Scenario::WorstCaseFanout => {
            fanout_tasks(schema, model, "Character", "level")
        }
        Scenario::ProjectRecordSet => {
            let dependency = CheckDependency::RecordSet(TypeName::new("Item").expect("type"));
            let mut tasks = BTreeSet::new();
            for statement in schema.check_statements_for_dependency(&dependency) {
                insert_statement_tasks(schema, statement, CheckTarget::Project, &mut tasks);
            }
            finish_tasks(schema, tasks)
        }
        Scenario::NestedObject => {
            let record = first_item(model);
            let mut tasks = BTreeSet::new();
            for statement in schema.check_statements_for_nested_field(
                &TypeName::new("Item").expect("type"),
                &FieldName::new("parts").expect("field"),
            ) {
                insert_statement_tasks(schema, statement, CheckTarget::Record(record), &mut tasks);
            }
            finish_tasks(schema, tasks)
        }
        Scenario::Batch(count) => {
            let dependency = field_dependency("Item", "price");
            let statements = schema
                .check_statements_for_dependency(&dependency)
                .collect::<Vec<_>>();
            let mut tasks = BTreeSet::new();
            for index in 0..count.min(item_count(model)) {
                let record = model
                    .record_by_type_key("Item", &item_key(index))
                    .expect("item");
                for statement in &statements {
                    insert_statement_tasks(
                        schema,
                        *statement,
                        CheckTarget::Record(record),
                        &mut tasks,
                    );
                }
            }
            finish_tasks(schema, tasks)
        }
        Scenario::DimensionVariant => {
            let dependency = field_dependency("Item", "name");
            let record = first_item(model);
            let mut tasks = BTreeSet::new();
            for statement in schema.check_statements_for_dependency(&dependency) {
                tasks.insert(CheckTask {
                    target: CheckTarget::Record(record),
                    statement,
                    projection: CheckProjection::Dimension {
                        dimension: coflow_cft::DimensionName::new("language").expect("dimension"),
                        variant: coflow_cft::VariantName::new("v0").expect("variant"),
                    },
                });
            }
            finish_tasks(schema, tasks)
        }
    }
}

fn direct_tasks(schema: &CftSchema, model: &CfdDataModel, field: &str) -> Vec<CheckTask> {
    let dependency = field_dependency("Item", field);
    let record = first_item(model);
    let mut tasks = BTreeSet::new();
    for statement in schema.check_statements_for_dependency(&dependency) {
        insert_statement_tasks(schema, statement, CheckTarget::Record(record), &mut tasks);
    }
    finish_tasks(schema, tasks)
}

fn fanout_tasks(
    schema: &CftSchema,
    model: &CfdDataModel,
    owner: &str,
    field: &str,
) -> Vec<CheckTask> {
    let dependency = field_dependency(owner, field);
    let statements = schema
        .check_statements_for_dependency(&dependency)
        .collect::<Vec<_>>();
    let mut tasks = BTreeSet::new();
    for (record, value) in model.records() {
        if value.actual_type() != "Item" {
            continue;
        }
        for statement in &statements {
            insert_statement_tasks(schema, *statement, CheckTarget::Record(record), &mut tasks);
        }
    }
    finish_tasks(schema, tasks)
}

fn insert_statement_tasks(
    schema: &CftSchema,
    statement: coflow_cft::CheckStatementId,
    target: CheckTarget,
    tasks: &mut BTreeSet<CheckTask>,
) {
    let info = schema.check_statement(statement).expect("statement").info;
    tasks.insert(CheckTask {
        target,
        statement,
        projection: CheckProjection::Base,
    });
    for dimension in &info.dimensions {
        if let Some(meta) = schema.resolve_dimension(dimension) {
            for variant in &meta.variants {
                tasks.insert(CheckTask {
                    target,
                    statement,
                    projection: CheckProjection::Dimension {
                        dimension: dimension.clone(),
                        variant: variant.clone(),
                    },
                });
            }
        }
    }
}

fn finish_tasks(schema: &CftSchema, tasks: BTreeSet<CheckTask>) -> Vec<CheckTask> {
    let mut tasks = tasks.into_iter().collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.execution_cmp(right, schema));
    tasks
}

fn field_dependency(owner: &str, field: &str) -> CheckDependency {
    CheckDependency::Field(CheckField {
        owner: TypeName::new(owner).expect("type"),
        field: FieldName::new(field).expect("field"),
    })
}

fn first_item(model: &CfdDataModel) -> coflow_data_model::CfdRecordId {
    model
        .record_by_type_key("Item", &item_key(0))
        .expect("first item")
}

fn item_count(model: &CfdDataModel) -> usize {
    model.records_of_type("Item").count()
}

fn item_key(index: usize) -> String {
    format!("item_{index:05}")
}

pub(crate) fn assert_scoped_equivalent(
    schema: &CftSchema,
    model: &CfdDataModel,
    full: &[CheckTask],
    incremental: &[CheckTask],
) -> usize {
    let full_output = execute_checks(schema, model, full.to_vec(), CheckLimits::default());
    let mut merged = full_output
        .results
        .iter()
        .map(|result| (result.task.clone(), result.diagnostics.clone()))
        .collect::<BTreeMap<CheckTask, Vec<CheckDiagnostic>>>();
    let incremental_output =
        execute_checks(schema, model, incremental.to_vec(), CheckLimits::default());
    for result in incremental_output.results {
        merged.insert(result.task, result.diagnostics);
    }
    let expected = full_output
        .results
        .into_iter()
        .map(|result| (result.task, result.diagnostics))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(merged, expected, "incremental scoped results diverged");
    expected.values().map(Vec::len).sum()
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
