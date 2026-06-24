#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;
use common::*;

use coflow_checker::{run_checks, run_checks_for_languages, LocalizationOverrides};
use std::collections::BTreeMap;

fn build_simple_model(schema: &CftContainer) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [("name", CfdInputValue::from("Potion"))],
    ));
    builder.build().expect("model builds")
}

#[test]
fn default_round_passes_when_default_value_satisfies_check() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }
        "#,
    );
    let model = build_simple_model(&schema);
    run_checks(&schema, &model).expect("default round passes");
}

#[test]
fn translation_substitution_can_make_a_passing_check_fail_for_one_language() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }
        "#,
    );
    let model = build_simple_model(&schema);

    let mut translations = BTreeMap::new();
    translations.insert("Item/name/potion".to_string(), String::new()); // empty
    let overrides = vec![LocalizationOverrides::new("zh_CN", translations)];

    let err = run_checks_for_languages(&schema, &model, &overrides)
        .expect_err("empty translation should fail check");
    assert_has_code(&err, CfdErrorCode::CheckFailed);
    let msg = &err.diagnostics[0].message;
    assert!(
        msg.contains("[lang=zh_CN]"),
        "expected language tag in diagnostic, got `{msg}`"
    );
}

#[test]
fn missing_translation_falls_back_to_default_value() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }
        "#,
    );
    let model = build_simple_model(&schema);

    // No translations supplied — every key falls back to the default value
    // (which is "Potion") so the check passes.
    let overrides = vec![LocalizationOverrides::new("en", BTreeMap::new())];
    run_checks_for_languages(&schema, &model, &overrides).expect("fallback passes");
}

#[test]
fn empty_overrides_list_is_a_noop() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }
        "#,
    );
    let model = build_simple_model(&schema);
    run_checks_for_languages(&schema, &model, &[]).expect("noop");
}

#[test]
fn non_localized_fields_are_unaffected_by_overrides() {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
                check { name != ""; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [("name", CfdInputValue::from("Potion"))],
    ));
    let model = builder.build().expect("model builds");

    // Even an entry that *would* match the formatted key has no effect because
    // the field is not @localized.
    let mut translations = BTreeMap::new();
    translations.insert("Item/potion/name".to_string(), String::new());
    let overrides = vec![LocalizationOverrides::new("zh_CN", translations)];
    run_checks_for_languages(&schema, &model, &overrides).expect("non-localized is unchanged");
}
