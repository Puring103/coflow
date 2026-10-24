#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "checker_common/mod.rs"]
mod common;
use coflow_checker::{execute_checks, CheckLimits, CheckTarget, CheckTask};
use coflow_language::cft::{CheckOwner, CheckStatementId, TypeName};
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
        "table Item { price: int; enabled: bool; check Valid { price > 0: \"price\"; enabled: \"enabled\"; } }",
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
fn project_checks_query_singleton_fields_and_table_records_together() {
    let schema = compile_schema("table Item {} singleton Settings { count: int; } check Coverage { records(Settings).len() == 1; all settings in records(Settings) { settings.id == \"Settings\"; settings.count == records(Item).len(): \"incomplete coverage\"; } }");
    for count in [1_i64, 2_i64] {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "item",
            "Item",
            std::iter::empty::<(&str, LoadedValueDraft)>(),
        );
        builder.add_record(
            "Settings",
            "Settings",
            [("count", LoadedValueDraft::from(count))],
        );
        let model = builder.build().expect("model");
        let result = run_model_checks(&model, &schema);
        if count == 1 {
            result.expect("complete coverage");
        } else {
            let diagnostics = result.expect_err("incomplete coverage");
            assert_eq!(diagnostics.diagnostics.len(), 1);
            assert_eq!(diagnostics.diagnostics[0].message, "incomplete coverage");
        }
    }
}

#[test]
fn project_statement_runs_for_an_empty_model() {
    let schema = compile_schema(
        "table Item {} check Required { records(Item).len() > 0: \"missing item\"; }",
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
        }],
        CheckLimits::default(),
    );

    let diagnostic = &output.results[0].diagnostics[0];
    assert_eq!(diagnostic.diagnostic.message, "missing item");
    assert!(diagnostic.diagnostic.primary.is_none());
    assert!(matches!(
        diagnostic.contexts.as_slice(),
        [CheckDiagnosticContext::Check { name }] if name == "Required"
    ));
}

#[test]
fn table_check_walks_inline_collection_elements() {
    let schema = compile_schema(
        r#"
            data Reward { count: int; }
            table Item { rewards: [Reward]; check Valid { all reward in rewards { reward.count > 0: "bad reward"; } } }
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
            statement: type_statements(&schema, "Item")[0],
            target: CheckTarget::Record(record_id_at(&model, 0)),
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
fn table_check_reads_optional_inline_values() {
    let schema = compile_schema(
        r#"
            data Reward { count: int; }
            table Item { reward: Option<Reward> = None; check Valid { when reward is Some(value) { value.count > 0: "bad reward"; } } }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "reward",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::object_with_declared_type([
                ("count", (-1_i64).into()),
            ]))),
        )],
    );
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Item")[0],
            target: CheckTarget::Record(record_id_at(&model, 0)),
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
            table Item {
                enabled: bool;
                nums: [int];
                check Valid {
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
        }],
        CheckLimits::default(),
    );
    assert_eq!(
        output.results[0].diagnostics[0].diagnostic.message,
        "bad -2"
    );
    assert_eq!(output.results[0].diagnostics[0].contexts.len(), 3);
}

#[test]
fn duplicate_tasks_are_stably_deduplicated() {
    let schema = compile_schema("table Item { check Valid { false; } }");
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
    let schema = compile_schema("table Item { check Valid { true; } }");
    let model = CfdDataModel::builder(&schema).build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement: type_statements(&schema, "Item")[0],
            target: CheckTarget::Project,
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
    let schema = compile_schema("table Item { check Valid { true; } }");
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
    assert_eq!(task_limited.results, [] as [CheckTaskResult; 0]);
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
    assert_eq!(
        exactly_limited.request_diagnostics,
        [] as [CheckDiagnostic; 0]
    );

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
