#![allow(clippy::expect_used, clippy::unwrap_used)]
#[path = "checker_common/mod.rs"]
mod common;
use common::*;

#[test]
fn rule_order_is_parent_first_and_failures_do_not_stop_later_rules() {
    let schema = compile_schema(
        r#"
        table Child: Parent { check Last { false: "child"; } }
        table Parent { check First { false: "parent first"; } check Second { false: "parent second"; } }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "one",
        "Child",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert_eq!(
        output
            .diagnostics()
            .map(|d| d.diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        ["parent first", "parent second", "child"]
    );
    assert_eq!(
        output
            .results
            .iter()
            .map(|r| schema
                .check_statement(r.task.statement)
                .unwrap()
                .info
                .rule_id
                .as_str())
            .collect::<Vec<_>>(),
        ["Parent::First", "Parent::Second", "Child::Last"]
    );
}

#[test]
fn evaluation_error_stops_only_its_rule_and_when_reports_not_triggered() {
    let schema = compile_schema(
        r#"table Item { zero: int;
        check Broken { 1 // zero > 0; false: "must not run"; }
        check Next { false: "next rule"; }
        check Conditional { when false { false; } }
    }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("one", "Item", [("zero", 0_i64.into())]);
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert_eq!(
        output.results.iter().map(|r| r.status).collect::<Vec<_>>(),
        [
            CheckTaskStatus::EvaluationError,
            CheckTaskStatus::NotExecuted,
            CheckTaskStatus::Failed,
            CheckTaskStatus::NotTriggered
        ]
    );
    assert_eq!(output.statistics.executed_tasks, 3);
    assert_eq!(output.statistics.rejected_tasks, 1);
    let span = output.results[0].diagnostics[0]
        .schema_location
        .as_ref()
        .unwrap()
        .span;
    let rule = schema.resolve_type("Item").unwrap();
    assert!(span.start > rule.checks[0].name_span.end);
}

#[test]
fn block_messages_preserve_rule_identity_and_failing_element_location() {
    let schema = compile_schema(
        r#"table Item { values: [int]; check Positive: "invalid item" {
        when true: "active rule" { all value in values: "invalid values" { value > 0: f"bad {value}"; } }
    } }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "one",
        "Item",
        [(
            "values",
            LoadedValueDraft::Array(vec![1_i64.into(), (-1_i64).into()]),
        )],
    );
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    let diagnostic = &output.results[0].diagnostics[0];
    assert_eq!(
        diagnostic.diagnostic.message,
        "invalid item\nactive rule\ninvalid values\nbad -1"
    );
    assert_eq!(
        diagnostic.diagnostic.primary.as_ref().unwrap().path,
        CfdPath::root().field("values").index(1)
    );
    assert!(
        matches!(&diagnostic.contexts[0], CheckDiagnosticContext::Check { name } if name == "Item::Positive")
    );
}

#[test]
fn data_rules_read_their_own_id_field_and_validate_inline_option_objects() {
    let schema = compile_schema(
        r#"data Reward { id: int; check Positive { id > 0; } }
        table Item { reward: Option<Reward>; }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "one",
        "Item",
        [(
            "reward",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::object_with_declared_type([
                ("id", (-1_i64).into()),
            ]))),
        )],
    );
    let model = builder.build().unwrap();
    let statement = schema
        .check_statements_for_actual_type("Reward")
        .next()
        .unwrap();
    let target = CheckTarget::Data {
        record: record_id_at(&model, 0),
        path: CfdPath::root().field("reward"),
        dimension: None,
    };
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask { target, statement }],
        CheckLimits::default(),
    );
    assert_eq!(output.results[0].status, CheckTaskStatus::Failed);
    assert_eq!(
        output.results[0].diagnostics[0]
            .diagnostic
            .primary
            .as_ref()
            .unwrap()
            .path,
        CfdPath::root().field("reward").field("id")
    );
}

#[test]
fn rule_names_are_required_nonempty_and_unique_through_inheritance() {
    for source in [
        "table Item { check { true; } }",
        "table Item { check Empty {} }",
        "check Empty {}",
        "table Item { check Same { true; } check Same { true; } }",
        "data Parent { check Same { true; } } data Child: Parent { check Same { true; } }",
    ] {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        assert!(build_schema(&modules).is_err(), "{source}");
    }
    compile_schema("data First { check Valid { true; } } data Second { check Valid { true; } }");
}

#[test]
fn empty_quantifiers_keep_boolean_meanings_and_quantifier_message_locations() {
    let schema = compile_schema(
        r#"table Item { values: [int];
        check Empty { all value in values { false; } none value in values { true; }
            any value in values: "no matching value" { true; }
        }
    }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "one",
        "Item",
        [("values", LoadedValueDraft::Array(Vec::new()))],
    );
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert_eq!(
        output
            .results
            .iter()
            .map(|result| result.status)
            .collect::<Vec<_>>(),
        [
            CheckTaskStatus::Passed,
            CheckTaskStatus::Passed,
            CheckTaskStatus::Failed
        ]
    );
    let diagnostic = &output.results[2].diagnostics[0];
    assert!(diagnostic.diagnostic.message.contains("no matching value"));
    assert!(diagnostic.diagnostic.message.contains("0 / 0"));
    assert_eq!(
        diagnostic.diagnostic.primary.as_ref().unwrap().path,
        CfdPath::root().field("values")
    );
    let statement = schema
        .check_statement(output.results[2].task.statement)
        .unwrap()
        .statement;
    if let coflow_language::cft::CftSchemaCheckStmt::Quantifier { span, .. } = statement {
        assert_eq!(diagnostic.schema_location.as_ref().unwrap().span, *span);
    }
}
