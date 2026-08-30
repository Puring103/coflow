#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "checker_common/mod.rs"]
mod common;
use coflow_language::{CheckOwner, CheckStatementId, TypeName};
use coflow_runtime::{execute_checks, CheckLimits, CheckProjection, CheckTarget, CheckTask};
use common::*;

fn type_statements(schema: &CftSchema, owner: &str) -> Vec<CheckStatementId> {
    let owner = CheckOwner::Type(TypeName::new(owner).unwrap());
    schema
        .all_check_statements()
        .filter(|statement| statement.owner == owner)
        .map(|statement| statement.id)
        .collect()
}

#[test]
fn explicit_task_executes_only_the_selected_root_statement() {
    let schema = compile_schema(
        "type Item { price: int; enabled: bool; check { price > 0: \"price\"; enabled: \"enabled\"; } }",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("price", LoadedValueDraft::from(-1_i64)),
            ("enabled", LoadedValueDraft::from(false)),
        ],
    );
    let model = builder.build().expect("model");
    let record = record_id_at(&model, 0);
    let statements = type_statements(&schema, "Item");

    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: statements[1],
            target: CheckTarget::Record(record),
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );

    assert_eq!(output.results.len(), 1);
    assert_eq!(output.results[0].diagnostics.len(), 1);
    assert_eq!(
        output.results[0].diagnostics[0].diagnostic.message,
        "enabled"
    );
}

#[test]
fn project_statement_runs_for_an_empty_model() {
    let schema = compile_schema(
        "type Item {} check Required { records(Item).len() > 0: \"missing item\"; }",
    );
    let model = CfdDataModel::builder(&schema).build().expect("model");
    let statement = schema
        .all_check_statements()
        .find(|info| matches!(info.owner, CheckOwner::Project(_)))
        .expect("project statement")
        .id;
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement,
            target: CheckTarget::Project,
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );

    let diagnostic = &output.results[0].diagnostics[0];
    assert_eq!(diagnostic.diagnostic.message, "missing item");
    assert!(diagnostic.diagnostic.primary.is_none());
    assert!(matches!(
        diagnostic.contexts.as_slice(),
        [coflow_runtime::CheckDiagnosticContext::Check { name }] if name == "Required"
    ));
}

#[test]
fn nested_owner_statement_walks_every_instance_in_the_target_record() {
    let schema = compile_schema(
        r#"
            type Reward { count: int; check { count > 0: "bad reward"; } }
            type Item { rewards: [Reward]; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "rewards",
            LoadedValueDraft::Array(vec![
                LoadedValueDraft::object_with_declared_type([("count", (-1_i64).into())]),
                LoadedValueDraft::object_with_declared_type([("count", 2_i64.into())]),
            ]),
        )],
    );
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Reward")[0],
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );

    assert_eq!(output.results[0].diagnostics.len(), 1);
    assert_eq!(
        output.results[0].diagnostics[0]
            .diagnostic
            .primary
            .as_ref()
            .map(|label| label.path.clone()),
        Some(CfdPath::root().field("rewards").index(0).field("count"))
    );
}

#[test]
fn nested_owner_statement_reads_objects_inside_option_values() {
    let schema = compile_schema(
        r#"
            type Reward { count: int; check { count > 0: "bad reward"; } }
            type Item { reward: Option<Reward> = None; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "reward",
            LoadedValueDraft::OptionSome(Box::new(
                LoadedValueDraft::object_with_declared_type([("count", (-1_i64).into())]),
            )),
        )],
    );
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Reward")[0],
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );

    assert_eq!(output.results[0].diagnostics.len(), 1);
    let diagnostic = &output.results[0].diagnostics[0].diagnostic;
    assert_eq!(diagnostic.message, "bad reward");
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("reward").field("count"))
    );
}

#[test]
fn when_quantifier_and_formatted_message_stay_inside_one_task() {
    let schema = compile_schema(
        r#"
            type Item {
                enabled: bool;
                nums: [int];
                check {
                    when enabled { all value in nums { value > 0: f"bad {value}"; } }
                }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("enabled", true.into()),
            (
                "nums",
                LoadedValueDraft::Array(vec![1_i64.into(), (-2_i64).into()]),
            ),
        ],
    );
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Item")[0],
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );
    assert_eq!(
        output.results[0].diagnostics[0].diagnostic.message,
        "bad -2"
    );
    assert_eq!(output.results[0].diagnostics[0].contexts.len(), 2);
}

#[test]
fn duplicate_tasks_are_stably_deduplicated() {
    let schema = compile_schema("type Item { check { false; } }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = builder.build().expect("model");
    let task = CheckTask {
        statement: type_statements(&schema, "Item")[0],
        target: CheckTarget::Record(record_id_at(&model, 0)),
        projection: CheckProjection::Base,
    };
    let output = execute_checks(
        &schema,
        &model,
        [task.clone(), task],
        CheckLimits::default(),
    );
    assert_eq!(output.statistics.requested_tasks, 1);
    assert_eq!(output.results.len(), 1);
}

#[test]
fn mismatched_statement_target_returns_an_internal_diagnostic() {
    let schema = compile_schema("type Item { check { true; } }");
    let model = CfdDataModel::builder(&schema).build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Item")[0],
            target: CheckTarget::Project,
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );
    assert_eq!(output.results[0].diagnostics.len(), 1);
    assert!(output.results[0].diagnostics[0]
        .diagnostic
        .message
        .contains("does not match"));
}

#[test]
fn task_limit_rejects_the_request_and_work_limit_reports_unexecuted_tasks() {
    let schema = compile_schema("type Item { check { true; } }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("a", "Item", std::iter::empty::<(&str, LoadedValueDraft)>());
    builder.add_record("b", "Item", std::iter::empty::<(&str, LoadedValueDraft)>());
    let model = builder.build().expect("model");
    let statement = type_statements(&schema, "Item")[0];
    let tasks = model
        .records()
        .map(|(record, _)| CheckTask {
            statement,
            target: CheckTarget::Record(record),
            projection: CheckProjection::Base,
        })
        .collect::<Vec<_>>();

    let task_limited = execute_checks(
        &schema,
        &model,
        tasks.clone(),
        CheckLimits {
            max_tasks: 1,
            ..CheckLimits::default()
        },
    );
    assert_eq!(task_limited.statistics.executed_tasks, 0);
    assert_eq!(task_limited.statistics.rejected_tasks, 2);
    assert!(task_limited.results.is_empty());
    assert!(task_limited.request_diagnostics[0]
        .diagnostic
        .message
        .contains("task limit"));

    let exactly_limited = execute_checks(
        &schema,
        &model,
        tasks.clone(),
        CheckLimits {
            max_tasks: 2,
            ..CheckLimits::default()
        },
    );
    assert_eq!(exactly_limited.statistics.executed_tasks, 2);
    assert!(exactly_limited.request_diagnostics.is_empty());

    let work_limited = execute_checks(
        &schema,
        &model,
        tasks,
        CheckLimits {
            max_request_work: 1,
            ..CheckLimits::default()
        },
    );
    assert_eq!(work_limited.statistics.executed_tasks, 1);
    assert_eq!(work_limited.statistics.rejected_tasks, 1);
    assert!(work_limited.results[1].diagnostics[0]
        .diagnostic
        .message
        .contains("work limit"));
}
