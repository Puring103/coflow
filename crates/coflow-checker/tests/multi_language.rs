#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

mod common;
use common::*;

use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use coflow_checker::{
    CheckRequest, CheckRoot, CheckRound, DependencyCollection, DependencyGraph, DimensionCheckRound,
};

fn dimension_rounds(
    dimension: &str,
    variants: impl IntoIterator<Item = &'static str>,
) -> Vec<DimensionCheckRound> {
    let dimension = DimensionName::new(dimension).expect("dimension name");
    variants
        .into_iter()
        .map(|variant| {
            DimensionCheckRound::new(
                dimension.clone(),
                VariantName::new(variant).expect("variant name"),
            )
        })
        .collect()
}

fn language_plan() -> Vec<DimensionCheckRound> {
    dimension_rounds("language", ["zh", "en"])
}

fn run_checks_for_dimensions(
    schema: &CftSchema,
    model: &CfdDataModel,
    rounds: &[DimensionCheckRound],
) -> Result<(), CfdDiagnostics> {
    coflow_checker::run_checks(
        schema,
        model,
        CheckRequest::all().with_rounds(rounds.iter().cloned()),
    )
    .into_result()
}

fn run_checks_for_dimensions_with_deps(
    schema: &CftSchema,
    model: &CfdDataModel,
    rounds: &[DimensionCheckRound],
) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
    let mut output = coflow_checker::run_checks(
        schema,
        model,
        CheckRequest::all()
            .with_rounds(rounds.iter().cloned())
            .with_dependency_collection(DependencyCollection::Reads),
    );
    let dependencies = std::mem::take(&mut output.dependencies);
    (output.into_result(), dependencies)
}

fn add_overlay(
    builder: &mut CfdModelBuilder<'_>,
    source_type: &str,
    source_key: &str,
    field: &str,
    dimension: &str,
    variant: &str,
    value: LoadedValueDraft,
) {
    builder.add_dimension_value_draft(DimensionValueDraft {
        source_type: TypeName::new(source_type).unwrap(),
        source_key: RecordKey::new(source_key).unwrap(),
        field: FieldName::new(field).unwrap(),
        dimension: DimensionName::new(dimension).unwrap(),
        variant: VariantName::new(variant).unwrap(),
        value,
        origin: RecordOrigin::None,
    });
}

fn simple_schema() -> CftSchema {
    compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != ""; }
            }
        "#,
    )
}

fn simple_model(
    zh: Option<LoadedValueDraft>,
    en: Option<LoadedValueDraft>,
) -> (CftSchema, CfdDataModel) {
    let schema = simple_schema();
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "potion",
        "Item",
        [("name", LoadedValueDraft::from("Potion"))],
    );
    if let Some(value) = zh {
        add_overlay(
            &mut builder,
            "Item",
            "potion",
            "name",
            "language",
            "zh",
            value,
        );
    }
    if let Some(value) = en {
        add_overlay(
            &mut builder,
            "Item",
            "potion",
            "name",
            "language",
            "en",
            value,
        );
    }
    let model = builder.build().expect("model builds");
    (schema, model)
}

#[test]
fn default_round_uses_only_the_owner_field() {
    let (schema, model) = simple_model(None, None);
    run_model_checks(&model, &schema).expect("default round passes");
    assert_eq!(model.record_count(), 1);
}

#[test]
fn variant_round_can_fail_at_the_owner_field_path() {
    let (schema, model) = simple_model(
        Some(LoadedValueDraft::from("")),
        Some(LoadedValueDraft::from("Potion")),
    );
    let err = run_checks_for_dimensions(&schema, &model, &language_plan())
        .expect_err("empty zh value should fail");

    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
    assert!(err.diagnostics.iter().any(|diagnostic| {
        diagnostic.message.contains("[language=zh]")
            && diagnostic
                .primary
                .as_ref()
                .is_some_and(|label| label.path == CfdPath::root().field("name"))
    }));
}

#[test]
fn explicit_null_skips_while_missing_is_reported() {
    let (schema, null_model) = simple_model(
        Some(LoadedValueDraft::Null),
        Some(LoadedValueDraft::from("Potion")),
    );
    run_checks_for_dimensions(&schema, &null_model, &language_plan())
        .expect("explicit null skips the zh field check");

    let (schema, missing_model) = simple_model(None, Some(LoadedValueDraft::from("Potion")));
    let err = run_checks_for_dimensions(&schema, &missing_model, &language_plan())
        .expect_err("missing zh value is not a null skip");
    assert_has_code(&err, CfdErrorCode::CheckEvalTypeError);
    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("缺少 variant `zh`")));
}

#[test]
fn inherited_dimension_field_checks_child_owner_records() {
    let schema = compile_schema(
        r#"
            abstract type Base {
                @localized
                name: string;
                check { name != ""; }
            }
            type Child : Base { value: int; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "child",
        "Child",
        [
            ("name", LoadedValueDraft::from("Child")),
            ("value", LoadedValueDraft::from(1_i64)),
        ],
    );
    add_overlay(
        &mut builder,
        "Base",
        "child",
        "name",
        "language",
        "zh",
        LoadedValueDraft::from(""),
    );
    let model = builder.build().expect("model builds");
    let err = run_checks_for_dimensions(&schema, &model, &dimension_rounds("language", ["zh"]))
        .expect_err("inherited check should fail");
    assert_has_code(&err, CfdErrorCode::CheckComparisonFailed);
}

#[test]
fn nested_object_array_and_dict_checks_use_overlay_subtrees() {
    let schema = compile_schema(
        r#"
            type Text {
                label: string;
                check { label != ""; }
            }
            type Item {
                @localized text: Text;
                @localized texts: [Text];
                @localized by_slot: {string: Text};
            }
        "#,
    );
    let text = |label: &str| LoadedValueDraft::object("Text", [("label", label.into())]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("text", text("default")),
            ("texts", LoadedValueDraft::Array(vec![text("default")])),
            (
                "by_slot",
                LoadedValueDraft::dict([(LoadedDictKeyDraft::from("main"), text("default"))]),
            ),
        ],
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "text",
        "language",
        "zh",
        text(""),
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "texts",
        "language",
        "zh",
        LoadedValueDraft::Array(vec![text("")]),
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "by_slot",
        "language",
        "zh",
        LoadedValueDraft::dict([(LoadedDictKeyDraft::from("main"), text(""))]),
    );
    let model = builder.build().expect("model builds");
    let err = run_checks_for_dimensions(&schema, &model, &dimension_rounds("language", ["zh"]))
        .expect_err("nested overlay values should fail");

    let paths = err
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.primary.as_ref().map(|label| label.path.clone()))
        .collect::<Vec<_>>();
    assert!(paths.contains(&CfdPath::root().field("text").field("label")));
    assert!(paths.contains(&CfdPath::root().field("texts").index(0).field("label")));
    assert!(paths.contains(
        &CfdPath::root()
            .field("by_slot")
            .dict_key("\"main\"")
            .field("label")
    ));
}

#[test]
fn overlay_record_refs_resolve_without_storage_records() {
    let schema = compile_schema(
        r"
            type Target { value: int; }
            type Copy {
                target: &Target? = null;
                check { target == null || target.value > 0; }
            }
            type Item { @localized copy: Copy; }
        ",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "target",
        "Target",
        [("value", LoadedValueDraft::from(0_i64))],
    );
    builder.add_record(
        "item",
        "Item",
        [(
            "copy",
            LoadedValueDraft::object("Copy", [("target", LoadedValueDraft::Null)]),
        )],
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "copy",
        "language",
        "zh",
        LoadedValueDraft::object("Copy", [("target", LoadedValueDraft::record_ref("target"))]),
    );
    let model = builder.build().expect("overlay refs resolve");
    let rounds = dimension_rounds("language", ["zh"]);
    let err = run_checks_for_dimensions(&schema, &model, &rounds)
        .expect_err("nested ref check should fail");
    assert_has_code(&err, CfdErrorCode::CheckOrFailed);
    assert_eq!(model.record_count(), 2);

    let output = coflow_checker::run_checks(
        &schema,
        &model,
        CheckRequest::all()
            .with_rounds(rounds.iter().cloned())
            .with_dependency_collection(DependencyCollection::Reads),
    );
    let snapshot = output.snapshot.expect("stable dimension snapshot");
    let target = model
        .records()
        .find(|(_, record)| record.key() == "target")
        .expect("target")
        .1
        .coordinate();
    let item = model
        .records()
        .find(|(_, record)| record.key() == "item")
        .expect("item")
        .1
        .coordinate();
    let default_state = snapshot
        .roots
        .get(&CheckRoot {
            record: item.clone(),
            round: CheckRound::Default,
        })
        .expect("item default state");
    assert!(!default_state.reads_from.contains(&target));
    let dimension_state = snapshot
        .roots
        .get(&CheckRoot {
            record: item,
            round: CheckRound::Dimension(rounds[0].clone()),
        })
        .expect("item dimension state");
    assert!(dimension_state.reads_from.contains(&target));

    let incremental = coflow_checker::run_checks(
        &schema,
        &model,
        CheckRequest::incremental(&snapshot, &std::collections::BTreeSet::from([target]))
            .with_rounds(rounds),
    );
    assert_eq!(incremental.statistics.requested_roots, 2);
    assert_eq!(incremental.statistics.executed_rounds, 3);
}

#[test]
fn configured_dimensions_run_independently() {
    let schema = compile_schema(
        r#"
            type Item {
                @localized name: string;
                @dimension("platform") label: string;
                check { name != ""; label != ""; }
            }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("name", LoadedValueDraft::from("default")),
            ("label", LoadedValueDraft::from("default")),
        ],
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "name",
        "language",
        "zh",
        LoadedValueDraft::from("ok"),
    );
    add_overlay(
        &mut builder,
        "Item",
        "item",
        "label",
        "platform",
        "pc",
        LoadedValueDraft::from(""),
    );
    let model = builder.build().expect("model builds");
    let plan = vec![
        dimension_rounds("language", ["zh"])[0].clone(),
        dimension_rounds("platform", ["pc"])[0].clone(),
    ];
    let err = run_checks_for_dimensions(&schema, &model, &plan)
        .expect_err("platform round should fail independently");
    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("[platform=pc]")));
}

#[test]
fn dependency_graph_has_no_synthetic_record_edges() {
    let (schema, model) = simple_model(
        Some(LoadedValueDraft::from("药水")),
        Some(LoadedValueDraft::from("Potion")),
    );
    let (result, graph) = run_checks_for_dimensions_with_deps(&schema, &model, &language_plan());
    result.expect("checks pass");
    assert_eq!(model.record_count(), 1);
    assert!(graph.reads_from.values().all(|targets| targets
        .iter()
        .all(|target| target.index() < model.record_count())));
}
