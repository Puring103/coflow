#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;
use common::*;

use coflow_checker::{
    run_checks, run_checks_for_dimensions, run_checks_for_dimensions_with_deps, DimensionCheckPlan,
};

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

            @__coflow_dimension_storage("language", "Item", "name")
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
    run_checks(&CompiledSchema::new(&schema), &model).expect("default round passes");
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

    let err = run_checks_for_dimensions(&CompiledSchema::new(&schema), &model, &dimensions)
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

    run_checks_for_dimensions(&CompiledSchema::new(&schema), &model, &dimensions)
        .expect("null zh variant skips check");
}

#[test]
fn missing_dimension_variant_record_is_an_eval_error_not_a_skip() {
    let schema = dimension_schema();
    let model = build_simple_model(&schema);

    let err = run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect_err("missing synthesized variant record should be reported");

    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("缺少变体存储记录")));
}

#[test]
fn missing_dimension_variant_field_is_an_eval_error_not_a_skip() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }

            @__coflow_dimension_storage("language", "Item", "name")
            type Item_nameVariants {
                default: string?;
                zh: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
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
            ("zh", CfdInputValue::from("药水")),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect_err("missing synthesized variant field should be reported");

    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("缺少 variant `en`")));
}

#[test]
fn variant_rounds_only_run_checks_that_read_top_level_dimension_fields() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                count: int;
                check {
                    count > 0;
                    name != "";
                }
            }

            @__coflow_dimension_storage("language", "Item", "name")
            type Item_nameVariants {
                default: string?;
                zh: string?;
                en: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [
            ("name", CfdInputValue::from("Potion")),
            ("count", CfdInputValue::from(0_i64)),
        ],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item_nameVariants",
        [
            ("default", CfdInputValue::from("Potion")),
            ("zh", CfdInputValue::from("")),
            ("en", CfdInputValue::from("Potion")),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect_err("default count and zh name checks should fail");

    assert_eq!(err.diagnostics.len(), 2, "diagnostics: {err:?}");
    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("count > 0"))
            .count(),
        1,
        "non-dimensional check should only run in default round: {err:?}"
    );
    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("[language=zh]")));
    assert!(!err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("[language=en]")));
}

#[test]
fn null_dimension_variant_skips_methods_and_operators_by_control_flow() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                tags: [string];
                check {
                    tags.len() > 0;
                    tags.contains("rare");
                    tags.len() > 0 && tags.contains("rare");
                }
            }

            @__coflow_dimension_storage("language", "Item", "tags")
            type Item_tagsVariants {
                default: [string]?;
                zh: [string]?;
                en: [string]?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [(
            "tags",
            CfdInputValue::Array(vec![CfdInputValue::from("rare")]),
        )],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item_tagsVariants",
        [
            (
                "default",
                CfdInputValue::Array(vec![CfdInputValue::from("rare")]),
            ),
            ("zh", CfdInputValue::Null),
            (
                "en",
                CfdInputValue::Array(vec![CfdInputValue::from("rare")]),
            ),
        ],
    ));
    let model = builder.build().expect("model builds");

    run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect("null zh variant should skip related checks without type errors");
}

#[test]
fn nested_dimension_fields_do_not_trigger_variant_round_checks() {
    let schema = compile_schema(
        r#"
            type Text {
                @localized
                label: string;
            }

            type Item {
                text: Text;
                check { text.label != ""; }
            }

            @__coflow_dimension_storage("language", "Text", "label")
            type Text_labelVariants {
                default: string?;
                zh: string?;
                en: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [(
            "text",
            CfdInputValue::object_with_declared_type([("label", CfdInputValue::from("Potion"))]),
        )],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Text_labelVariants",
        [
            ("default", CfdInputValue::from("Potion")),
            ("zh", CfdInputValue::from("")),
            ("en", CfdInputValue::from("")),
        ],
    ));
    let model = builder.build().expect("model builds");

    run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect("nested localized fields are not variant-expanded");
}

#[test]
fn nested_inline_record_checks_do_not_run_in_variant_rounds() {
    let schema = compile_schema(
        r#"
            type Text {
                @localized
                label: string;
                check { label != ""; }
            }

            type Item {
                text: Text;
                check { true; }
            }

            @__coflow_dimension_storage("language", "Text", "label")
            type Text_labelVariants {
                default: string?;
                zh: string?;
                en: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [(
            "text",
            CfdInputValue::object_with_declared_type([("label", CfdInputValue::from(""))]),
        )],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Text_labelVariants",
        [
            ("default", CfdInputValue::from("")),
            ("zh", CfdInputValue::from("药水")),
            ("en", CfdInputValue::from("Potion")),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect_err("default nested check should still fail");

    assert_eq!(err.diagnostics.len(), 1, "diagnostics: {err:?}");
    assert!(
        !err.diagnostics[0].message.contains("[language="),
        "nested inline record checks must not run in variant rounds: {err:?}"
    );
}

#[test]
fn inherited_dimension_checks_run_for_child_records() {
    let schema = compile_schema(
        r#"
            type Base {
                @localized
                name: string;
                check { name != ""; }
            }

            type Child : Base {
                power: int;
            }

            @__coflow_dimension_storage("language", "Base", "name")
            type Base_nameVariants {
                default: string?;
                zh: string?;
                en: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "child",
        "Child",
        [
            ("name", CfdInputValue::from("Potion")),
            ("power", CfdInputValue::from(1_i64)),
        ],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "child",
        "Base_nameVariants",
        [
            ("default", CfdInputValue::from("Potion")),
            ("zh", CfdInputValue::from("")),
            ("en", CfdInputValue::from("")),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
    .expect_err("inherited dimension checks must run for child records");

    assert_eq!(
        err.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
            .count(),
        2
    );
    assert!(err.diagnostics.iter().all(|diagnostic| diagnostic
        .primary
        .as_ref()
        .is_some_and(|label| { label.path == CfdPath::root().field("name") })));
}

#[test]
fn dimension_dependency_graph_includes_variant_records() {
    let schema = dimension_schema();
    let model = build_dimension_model(
        &schema,
        CfdInputValue::from("药水"),
        CfdInputValue::from("Potion"),
    );

    let (result, graph) = run_checks_for_dimensions_with_deps(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    );

    result.expect("checks pass");
    let source_id = model
        .lookup_assignable("Item", "potion")
        .expect("source record");
    let variant_id = model
        .lookup_assignable("Item_nameVariants", "potion")
        .expect("variant record");
    assert!(
        graph
            .reads_from
            .get(&source_id)
            .is_some_and(|reads| reads.contains(&variant_id)),
        "dependency graph should include source -> variant read edge: {graph:?}"
    );
}

fn language_dimensions() -> DimensionCheckPlan {
    dimension_plan("language", ["zh", "en"])
}

fn dimension_plan(
    dimension: impl Into<String>,
    variants: impl IntoIterator<Item = impl Into<String>>,
) -> DimensionCheckPlan {
    DimensionCheckPlan::from_variants(dimension, variants)
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
    run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &DimensionCheckPlan::default(),
    )
    .expect("default round passes");
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

    run_checks_for_dimensions(
        &CompiledSchema::new(&schema),
        &model,
        &language_dimensions(),
    )
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
    let dimensions = dimension_plan("platform", ["pc", "mobile"]);

    let err = run_checks_for_dimensions(&CompiledSchema::new(&schema), &model, &dimensions)
        .expect_err("default check still fails once");

    assert_eq!(err.diagnostics.len(), 1, "diagnostics: {err:?}");
    assert!(
        !err.diagnostics[0].message.contains("[platform="),
        "unknown dimensions should not add variant diagnostics: {}",
        err.diagnostics[0].message
    );
}

#[test]
fn variant_rounds_run_for_every_configured_dimension() {
    let schema = compile_schema(
        r#"
            type Item {
                @dimension("platform")
                name: string;
                check { name != ""; }
            }

            @__coflow_dimension_storage("platform", "Item", "name")
            type Item_nameVariants {
                default: string?;
                pc: string?;
                mobile: string?;
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
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
            ("pc", CfdInputValue::from("")),
            ("mobile", CfdInputValue::from("Potion")),
        ],
    ));
    let model = builder.build().expect("model builds");
    let dimensions = dimension_plan("platform", ["pc", "mobile"]);

    let err = run_checks_for_dimensions(&CompiledSchema::new(&schema), &model, &dimensions)
        .expect_err("empty pc variant should fail check");

    assert!(
        err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("[platform=pc]")),
        "expected platform variant diagnostic, got {err:?}"
    );
    assert!(
        !err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("[language=")),
        "configured platform dimension must not be reported as language: {err:?}"
    );
}
