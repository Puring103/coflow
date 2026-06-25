#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;
use common::*;

use coflow_checker::{run_checks, run_checks_for_dimensions};
use coflow_project::DimensionConfig;
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

fn build_dimension_model(
    schema: &CftContainer,
    zh_name: CfdInputValue,
    en_name: CfdInputValue,
) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [("name", CfdInputValue::from("Potion"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item_nameVariants",
        [
            ("default", CfdInputValue::from("Potion")),
            ("zh", zh_name),
            ("en", en_name),
        ],
    ));
    builder.build().expect("model builds")
}

fn dimension_schema() -> CftContainer {
    compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }

            type Item_nameVariants {
                default: string?;
                zh: string?;
                en: string?;
            }
        "#,
    )
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
fn dimension_variant_record_can_make_a_passing_check_fail_for_one_language() {
    let schema = dimension_schema();
    let model = build_dimension_model(
        &schema,
        CfdInputValue::from(""),
        CfdInputValue::from("Potion"),
    );
    let dimensions = language_dimensions();

    let err = run_checks_for_dimensions(&schema, &model, &dimensions)
        .expect_err("empty zh variant should fail check");

    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    let msg = &err.diagnostics[0].message;
    assert!(
        msg.contains("[language=zh]"),
        "expected dimension variant tag in diagnostic, got `{msg}`"
    );
}

#[test]
fn null_dimension_variant_skips_that_field_check() {
    let schema = dimension_schema();
    let model = build_dimension_model(&schema, CfdInputValue::Null, CfdInputValue::from("Potion"));
    let dimensions = language_dimensions();

    run_checks_for_dimensions(&schema, &model, &dimensions).expect("null zh variant skips check");
}

fn language_dimensions() -> BTreeMap<String, DimensionConfig> {
    BTreeMap::from([(
        "language".to_string(),
        DimensionConfig {
            variants: vec!["zh".to_string(), "en".to_string()],
            out_dir: None,
        },
    )])
}

#[test]
fn empty_dimensions_map_runs_default_round() {
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
    run_checks_for_dimensions(&schema, &model, &BTreeMap::new()).expect("default round passes");
}

#[test]
fn non_dimensional_fields_are_unaffected_by_dimension_rounds() {
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

    run_checks_for_dimensions(&schema, &model, &language_dimensions())
        .expect("non-dimensional is unchanged");
}

#[test]
fn unknown_dimensions_are_accepted_but_do_not_run_extra_check_rounds() {
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
        [("name", CfdInputValue::from(""))],
    ));
    let model = builder.build().expect("model builds");
    let dimensions = BTreeMap::from([(
        "platform".to_string(),
        DimensionConfig {
            variants: vec!["pc".to_string(), "mobile".to_string()],
            out_dir: None,
        },
    )]);

    let err = run_checks_for_dimensions(&schema, &model, &dimensions)
        .expect_err("default check still fails once");

    assert_eq!(err.diagnostics.len(), 1, "diagnostics: {err:?}");
    assert!(
        !err.diagnostics[0].message.contains("[platform="),
        "unknown dimensions should not add variant diagnostics: {}",
        err.diagnostics[0].message
    );
}
