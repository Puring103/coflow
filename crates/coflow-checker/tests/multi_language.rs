#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;
use common::*;

use coflow_checker::{
    run_checks, run_checks_for_dimensions, run_checks_for_dimensions_with_deps, DimensionCheckPlan,
};

fn build_simple_model(schema: &CftSchema) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    builder.add_input_record(CfdInputRecord::new(
        "potion",
        "Item",
        [("name", CfdInputValue::from("Potion"))],
    ));
    builder.build().expect("model builds")
}

fn build_dimension_model(
    schema: &CftSchema,
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

fn dimension_schema() -> CftSchema {
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

    run_checks_for_dimensions(&schema, &model, &dimensions)
        .expect("null zh variant skips check");
}

#[test]
fn missing_dimension_variant_record_is_an_eval_error_not_a_skip() {
    let schema = dimension_schema();
    let model = build_simple_model(&schema);

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
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

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
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
        &schema,
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
fn dimension_variant_inline_objects_resolve_refs_from_storage_paths() {
    let schema = compile_schema(
        r#"
            type Target { value: int; }
            type Snapshot { target: &Target; }
            type Item {
                @localized
                copy: Snapshot;
                check { copy.target.value > 0; }
            }

            @__coflow_dimension_storage("language", "Item", "copy")
            type Item_copyVariants {
                default: Snapshot?;
                zh: Snapshot?;
            }
        "#,
    );
    let snapshot = |target| {
        CfdInputValue::object_with_declared_type([("target", CfdInputValue::record_ref(target))])
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "good",
        "Target",
        [("value", CfdInputValue::from(1_i64))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "bad",
        "Target",
        [("value", CfdInputValue::from(0_i64))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("copy", snapshot("good"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_copyVariants",
        [("default", snapshot("good")), ("zh", snapshot("bad"))],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    )
    .expect_err("zh storage object points at the failing target");

    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    assert!(
        err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("copy.target.value = 0")),
        "diagnostics: {err:?}"
    );
    assert!(!err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CfdErrorCode::CheckEvalTypeError));
}

#[test]
fn localized_object_variants_run_nested_type_checks_at_logical_paths() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized
                text: Text;
            }

            @__coflow_dimension_storage("language", "Item", "text")
            type Item_textVariants {
                default: Text?;
                zh: Text?;
                en: Text?;
            }
        "#,
    );
    let text =
        |label| CfdInputValue::object_with_declared_type([("label", CfdInputValue::from(label))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("text", text("Potion"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_textVariants",
        [
            ("default", text("Potion")),
            ("zh", text("")),
            ("en", text("Potion")),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(&schema, &model, &language_dimensions())
        .expect_err("the zh materialized object should run Text checks");
    let failures = err
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .collect::<Vec<_>>();

    assert_eq!(failures.len(), 1, "diagnostics: {err:?}");
    assert!(failures[0].message.contains("[language=zh]"));
    let primary = failures[0].primary.as_ref().expect("primary location");
    assert_eq!(primary.record, model.lookup_assignable("Item", "item"));
    assert_eq!(primary.path, CfdPath::root().field("text").field("label"));
    assert!(failures[0].related.iter().any(|label| {
        label.record == model.lookup_assignable("Item_textVariants", "item")
            && label.path == CfdPath::root().field("zh").field("label")
    }));
}

#[test]
fn localized_aggregate_variants_preserve_array_paths() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized
                texts: [Text];
            }

            @__coflow_dimension_storage("language", "Item", "texts")
            type Item_textsVariants {
                default: [Text]?;
                zh: [Text]?;
            }
        "#,
    );
    let text =
        |label| CfdInputValue::object_with_declared_type([("label", CfdInputValue::from(label))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [(
            "texts",
            CfdInputValue::Array(vec![text("One"), text("Two")]),
        )],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_textsVariants",
        [
            (
                "default",
                CfdInputValue::Array(vec![text("One"), text("Two")]),
            ),
            ("zh", CfdInputValue::Array(vec![text("一"), text("")])),
        ],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    )
    .expect_err("the second zh array item should fail");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .expect("comparison diagnostic");

    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("texts").index(1).field("label"))
    );
    assert!(diagnostic
        .related
        .iter()
        .any(|label| { label.path == CfdPath::root().field("zh").index(1).field("label") }));
}

#[test]
fn localized_aggregate_variants_preserve_dict_paths() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized
                texts_by_slot: {string: Text};
            }

            @__coflow_dimension_storage("language", "Item", "texts_by_slot")
            type Item_textsBySlotVariants {
                default: {string: Text}?;
                zh: {string: Text}?;
            }
        "#,
    );
    let text =
        |label| CfdInputValue::object_with_declared_type([("label", CfdInputValue::from(label))]);
    let entries = |label| CfdInputValue::dict([(CfdInputDictKey::from("ui"), text(label))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("texts_by_slot", entries("Menu"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_textsBySlotVariants",
        [("default", entries("Menu")), ("zh", entries(""))],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    )
    .expect_err("the zh dict value should fail");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .expect("comparison diagnostic");

    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(
            CfdPath::root()
                .field("texts_by_slot")
                .dict_key("\"ui\"")
                .field("label")
        )
    );
    assert!(diagnostic.related.iter().any(|label| {
        label.path
            == CfdPath::root()
                .field("zh")
                .dict_key("\"ui\"")
                .field("label")
    }));
}

#[test]
fn null_localized_aggregate_variants_skip_nested_checks() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized
                text: Text;
            }

            @__coflow_dimension_storage("language", "Item", "text")
            type Item_textVariants {
                default: Text?;
                zh: Text?;
            }
        "#,
    );
    let text = CfdInputValue::object_with_declared_type([("label", CfdInputValue::from("Potion"))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("text", text.clone())],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_textVariants",
        [("default", text), ("zh", CfdInputValue::Null)],
    ));
    let model = builder.build().expect("model builds");

    run_checks_for_dimensions(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    )
    .expect("null aggregate variants skip the complete nested subtree");
}

#[test]
fn localized_object_nested_checks_resolve_refs_from_variant_storage() {
    let schema = compile_schema(
        r#"
            type Target { value: int; }
            type Snapshot {
                target: &Target;
                check { target.value > 0; }
            }
            type Item {
                @localized
                copy: Snapshot;
            }

            @__coflow_dimension_storage("language", "Item", "copy")
            type Item_copyVariants {
                default: Snapshot?;
                zh: Snapshot?;
            }
        "#,
    );
    let snapshot = |target| {
        CfdInputValue::object_with_declared_type([("target", CfdInputValue::record_ref(target))])
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "good",
        "Target",
        [("value", CfdInputValue::from(1_i64))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "bad",
        "Target",
        [("value", CfdInputValue::from(0_i64))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("copy", snapshot("good"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_copyVariants",
        [("default", snapshot("good")), ("zh", snapshot("bad"))],
    ));
    let model = builder.build().expect("model builds");

    let err = run_checks_for_dimensions(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    )
    .expect_err("the ref inside the zh storage object should resolve to bad");
    let diagnostic = err
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == CfdErrorCode::CheckComparisonFailed)
        .expect("comparison diagnostic");

    assert_eq!(
        diagnostic.primary.as_ref().and_then(|label| label.record),
        model.lookup_assignable("Target", "bad")
    );
    assert_eq!(
        diagnostic.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("value"))
    );
    assert!(!err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CfdErrorCode::CheckEvalTypeError));
}

#[test]
fn nested_variant_checks_record_storage_dependencies_without_parent_checks() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized
                text: Text;
            }

            @__coflow_dimension_storage("language", "Item", "text")
            type Item_textVariants {
                default: Text?;
                zh: Text?;
            }
        "#,
    );
    let text =
        |label| CfdInputValue::object_with_declared_type([("label", CfdInputValue::from(label))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item",
        [("text", text("Potion"))],
    ));
    builder.add_input_record(CfdInputRecord::new(
        "item",
        "Item_textVariants",
        [("default", text("Potion")), ("zh", text("药水"))],
    ));
    let model = builder.build().expect("model builds");
    let item = model.lookup_assignable("Item", "item").expect("item");
    let storage = model
        .lookup_assignable("Item_textVariants", "item")
        .expect("storage");

    let (result, graph) = run_checks_for_dimensions_with_deps(
        &schema,
        &model,
        &DimensionCheckPlan::from_variants("language", ["zh"]),
    );

    result.expect("all nested checks pass");
    assert!(graph
        .reads_from
        .get(&item)
        .is_some_and(|reads| reads.contains(&storage)));
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
        &schema,
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
    let dimensions = dimension_plan("platform", ["pc", "mobile"]);

    let err = run_checks_for_dimensions(&schema, &model, &dimensions)
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

    let err = run_checks_for_dimensions(&schema, &model, &dimensions)
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
