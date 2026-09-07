#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "checker_common/mod.rs"]
mod common;
use common::*;

#[test]
fn option_patterns_bind_scalars_objects_collections_and_nested_options() {
    let schema = compile_schema(
        r#"
        abstract type Reward {}
        type Coins : Reward { amount: int; }
        type Item {
            missing: Option<int> = None;
            amount: Option<int> = Some(3);
            nested: Option<Option<int>> = Some(Some(4));
            reward: Option<Reward>;
            amounts: Option<[int]> = Some([1, 2]);
            mapping: Option<{string: int}> = Some({"a": 3});
            check {
                missing == None;
                None == missing;
                amount != None;
                None != amount;
                when (missing is Some(v)) { v > 100; }
                amount is Some(v) && v == 3;
                when (nested is Some(inner) && inner is Some(v)) { v == 4; }
                when (reward is Some(r) && r is Coins) { r.amount == 5; }
                when (amounts is Some(items)) {
                    items.len() == 2;
                    items[1] == 2;
                    all v in items { v > 0; }
                }
                when (mapping is Some(items)) { items["a"] == 3; }
            }
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "reward",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::object(
                "Coins",
                [("amount", 5_i64.into())],
            ))),
        )],
    );
    let model = builder.build().expect("model");
    run_model_checks(&model, &schema).expect("patterns evaluate");
}

#[test]
fn pattern_binding_preserves_reference_dependencies_and_diagnostic_paths() {
    let schema = compile_schema(
        r#"
        abstract type Reward {}
        type Coins : Reward { amount: int; }
        type Item {
            reward: Option<&Reward>;
            check { when (reward is Some(r) && r is Coins) { r.amount > 0; } }
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "coins",
        "Coins",
        [("amount", LoadedValueDraft::from(-1_i64))],
    );
    builder.add_record(
        "item",
        "Item",
        [(
            "reward",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("coins"))),
        )],
    );
    let model = builder.build().expect("model");
    let errors = run_model_checks(&model, &schema).expect_err("negative reward");
    assert_eq!(errors.diagnostics.len(), 1, "{errors:?}");
    let diagnostic = &errors.diagnostics[0];
    assert_eq!(
        diagnostic.primary.as_ref().unwrap().path,
        CfdPath::root().field("amount")
    );
    let statement = schema.all_check_statements().next().unwrap();
    assert!(format!("{:?}", statement.dependencies).contains("Coins"));
}

#[test]
fn polymorphic_fields_narrow_in_when_and_short_circuit_conjunctions() {
    let schema = compile_schema(
        r#"
        abstract type Reward {}
        type Coins : Reward { amount: int; }
        type Gem : Reward { color: string; }
        type Item {
            reward: Reward = Coins { amount: 3 };
            check {
                reward is Coins && reward.amount == 3;
                when (reward is Coins) { reward.amount > 0; }
                when (reward is Gem) { reward.color == "red"; }
            }
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [] as [(&str, LoadedValueDraft); 0]);
    let model = builder.build().expect("model");
    run_model_checks(&model, &schema).expect("polymorphic checks");
}

#[test]
fn option_comparison_and_pattern_scopes_reject_invalid_access() {
    for check in [
        "amount > None;",
        "1 == None;",
        "amount is Some(v); v > 0;",
        "amount is Some(v) || v > 0;",
        "when (amount != None) { amount > 0; }",
        "when (amount is Some(v)) { when (amount is Some(v)) { v > 0; } }",
        "when (amount is Some(v)) { v > 0; } v > 0;",
        "when (reward is Coins) { reward.amount > 0; } reward.amount > 0;",
        "reward is Coins || reward.amount > 0;",
        "1 is Some(v);",
    ] {
        let source = format!("abstract type Reward {{}} type Coins : Reward {{ amount: int; }} type Item {{ amount: Option<int>; reward: Reward; check {{ {check} }} }}");
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        assert!(
            build_schema(&modules, &CftDimensionInputs::default()).is_err(),
            "accepted {check}"
        );
    }
}

#[test]
fn narrowing_respects_aliases_ancestor_predicates_and_shadowing() {
    let schema = compile_schema(
        r#"
        abstract type Reward {}
        type Coins : Reward { amount: int; }
        type Currency = Coins;
        type Item {
            reward: Reward = Coins { amount: 3 };
            optional: Option<int> = Some(5);
            amounts: [int] = [1, 2];
            check {
                when (reward is Currency && reward is Reward) { reward.amount == 3; }
                when (reward is Coins) {
                    all reward in amounts { reward > 0; }
                    reward.amount == 3;
                }
                when (reward is Coins && optional is Some(reward)) { reward == 5; }
            }
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [] as [(&str, LoadedValueDraft); 0]);
    let model = builder.build().expect("model");
    run_model_checks(&model, &schema).expect("scope-aware narrowing");
}

#[test]
fn missing_option_reports_pattern_failure_without_evaluating_the_bound_operand() {
    let schema = compile_schema("type Item { amount: Option<int> = None; check { amount is Some(v); amount is Some(v) && 10 // v > 0; } }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [] as [(&str, LoadedValueDraft); 0]);
    let model = builder.build().expect("model");
    let errors = run_model_checks(&model, &schema).expect_err("missing option");
    assert_eq!(errors.diagnostics.len(), 2);
    assert_eq!(
        errors.diagnostics[0].code,
        CfdErrorCode::CheckTypePredicateFailed
    );
    assert!(errors.diagnostics[0].message.contains("None"));
    assert_eq!(errors.diagnostics[1].code, CfdErrorCode::CheckAndFailed);
}
