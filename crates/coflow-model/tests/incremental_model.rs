#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use coflow_model::CfdInputBatch;

#[test]
fn immutable_batches_limit_identity_and_origin_checks_to_changed_input() {
    let schema = compile_schema("table Item { value: int; }");
    let records = |start, end| (start..end).map(|index| {
        LoadedRecordDraft::new(format!("i{index}"), "Item", [("value", LoadedValueDraft::from(i64::from(index)))])
            .with_origin(origin(index as usize))
    }).collect::<Vec<_>>();
    let first = CfdInputBatch::from(records(0, 17));
    let second = CfdInputBatch::from(records(17, 1050));
    let build = |a: &CfdInputBatch<LoadedRecordDraft>, b: &CfdInputBatch<LoadedRecordDraft>, old: Option<&CfdDataModel>| {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_input_batch(a).add_input_batch(b);
        match old {
            Some(old) => builder.try_build_partial_incremental(old, &[id(old, "Item", "i0")]).unwrap(),
            None => builder.build_partial().unwrap(),
        }
    };
    let initial = build(&first, &second, None);
    let mut changed = first.to_vec();
    changed[0].fields.insert("value".into(), 9000_i64.into());
    // 同文件后续记录来源全部移动，未变化文件不能参与位置核对。
    for record in &mut changed { record.origin = origin(2000); }
    let changed = CfdInputBatch::from(changed);
    let updated = build(&changed, &second, Some(&initial.model));
    assert_eq!(updated.statistics.identity_records_checked, 17);
    assert_eq!(updated.statistics.origin_records_checked, 17);
    assert_eq!(updated.model, build(&changed, &second, None).model);
    assert_eq!(initial.model.record(id(&initial.model, "Item", "i0")).unwrap().origin, origin(0));
    let reused = build(&changed, &second.clone(), Some(&updated.model));
    assert_eq!(reused.statistics.identity_records_checked, 0);
    assert_eq!(reused.statistics.origin_records_checked, 0);
    // 内容相同的新批次没有冒用旧身份，仍会执行身份验证。
    let copied = CfdInputBatch::from(second.to_vec());
    let checked = build(&changed, &copied, Some(&updated.model));
    assert_eq!(checked.statistics.identity_records_checked, second.len());
    let mut invalid = changed.to_vec();
    invalid[0].id = "replacement".into();
    let invalid = CfdInputBatch::from(invalid);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_batch(&invalid).add_input_batch(&second);
    assert!(builder.try_build_partial_incremental(&updated.model, &[]).is_none());
}

fn builder<'a>(
    schema: &'a CftSchema,
    records: &'a [LoadedRecordDraft],
    overlays: &[DimensionValueDraft],
) -> CfdModelBuilder<'a> {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_borrowed_record(record);
    }
    builder.add_dimension_value_drafts(overlays.iter().cloned());
    builder
}

fn id(model: &CfdDataModel, ty: &str, key: &str) -> CfdRecordHandle {
    model.record_by_type_id(ty, key).unwrap()
}

fn origin(line: usize) -> RecordOrigin {
    RecordOrigin::File {
        path: "data.cfd".into(),
        span: Some(TextSpan {
            start_line: line,
            start_character: 0,
            end_line: line + 1,
            end_character: 0,
        }),
    }
}

#[test]
fn mixed_borrowed_batches_preserve_handles_origins_and_incremental_results() {
    let schema = compile_schema("table Item { value: int; }");
    let mut inputs = (0..1030)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("i{index}"),
                "Item",
                [(
                    "value",
                    LoadedValueDraft::from(i64::try_from(index).expect("fixture index fits i64")),
                )],
            )
            .with_origin(origin(index))
        })
        .collect::<Vec<_>>();
    let mixed = |records: &[LoadedRecordDraft], previous: Option<&CfdDataModel>| {
        let mut mixed = CfdDataModel::builder(&schema);
        mixed.add_borrowed_records(&[]);
        mixed.add_borrowed_records(&records[..511]);
        mixed.add_loaded_record(records[511].clone());
        mixed.add_borrowed_record(&records[512]);
        mixed.add_borrowed_records(&records[513..1024]);
        mixed.add_borrowed_records(&[]);
        mixed.add_borrowed_records(&records[1024..]);
        match previous {
            Some(previous) => {
                mixed
                    .try_build_partial_incremental(
                        previous,
                        &[id(previous, "Item", "i511"), id(previous, "Item", "i1024")],
                    )
                    .unwrap()
                    .model
            }
            None => mixed.build().unwrap(),
        }
    };
    let previous = mixed(&inputs, None);
    assert_eq!(previous, builder(&schema, &inputs, &[]).build().unwrap());
    inputs[511].fields.insert("value".into(), 9000_i64.into());
    inputs[1024].fields.insert("value".into(), 9001_i64.into());
    for record in &mut inputs[513..] {
        record.origin = origin(2000);
    }
    let updated = mixed(&inputs, Some(&previous));
    let full = builder(&schema, &inputs, &[]).build().unwrap();
    assert_eq!(updated, full);
    for ((left_id, left), (right_id, right)) in updated.records().zip(full.records()) {
        assert_eq!(left_id, right_id);
        assert_eq!(left.origin, right.origin);
    }
    assert_eq!(
        previous
            .record(id(&previous, "Item", "i511"))
            .unwrap()
            .field("value"),
        Some(&CfdValue::Int(511))
    );
}

#[test]
fn local_update_reuses_values_and_preserves_previous_snapshot() {
    let schema = compile_schema("table Item { value: int; }");
    let mut inputs = (0..8)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("i{index}"),
                "Item",
                [(
                    "value",
                    LoadedValueDraft::from(i64::try_from(index).expect("fixture index fits i64")),
                )],
            )
            .with_origin(origin(index))
        })
        .collect::<Vec<_>>();
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let changed = id(&previous, "Item", "i0");
    inputs[0].fields.insert("value".into(), 50_i64.into());
    for input in &mut inputs {
        input.origin = origin(100);
    }
    let output = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[changed])
        .unwrap();
    assert_eq!(
        output.model,
        builder(&schema, &inputs, &[]).build().unwrap()
    );
    assert_eq!(output.statistics.validated_records, 1);
    assert_eq!(output.statistics.materialized_records, 1);
    assert_eq!(output.statistics.reused_records, 7);
    assert_eq!(output.changed_records, vec![changed]);
    assert_eq!(
        previous.record(changed).unwrap().field("value"),
        Some(&CfdValue::Int(0))
    );
    for index in 1..8 {
        let handle = id(&previous, "Item", &format!("i{index}"));
        let old = previous.record(handle).unwrap();
        let new = output.model.record(handle).unwrap();
        assert!(Arc::ptr_eq(&old.object, &new.object));
        assert_eq!(old.origin, origin(index));
        assert_eq!(new.origin, origin(100));
    }
}

#[test]
fn reference_retarget_updates_both_directions_without_rebuilding_target_values() {
    let schema = compile_schema("table Item { target: Option<&Item> = None; }");
    let mut inputs = vec![
        LoadedRecordDraft::new(
            "a",
            "Item",
            [(
                "target",
                LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("b"))),
            )],
        ),
        LoadedRecordDraft::new("b", "Item", std::iter::empty::<(&str, LoadedValueDraft)>()),
        LoadedRecordDraft::new("c", "Item", std::iter::empty::<(&str, LoadedValueDraft)>()),
    ];
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let a = id(&previous, "Item", "a");
    let b = id(&previous, "Item", "b");
    let c = id(&previous, "Item", "c");
    inputs[0].fields.insert(
        "target".into(),
        LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("c"))),
    );
    let output = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[a])
        .unwrap();
    assert_eq!(
        output.model,
        builder(&schema, &inputs, &[]).build().unwrap()
    );
    assert_eq!(output.statistics.validated_records, 1);
    assert_eq!(output.statistics.rebuilt_ref_hosts, 1);
    assert_eq!(output.statistics.rebuilt_ref_edges, 1);
    assert_eq!(previous.ref_edges_to_target(b).count(), 1);
    assert_eq!(output.model.ref_edges_to_target(b).count(), 0);
    assert_eq!(output.model.ref_edges_to_target(c).count(), 1);
    assert_eq!(
        output
            .model
            .resolve_ref(&RefSite::new(a, CfdPath::root().field("target"))),
        Some(c)
    );
}

#[test]
fn template_dependency_closure_is_transitive_and_retargets() {
    let schema = compile_schema(
        r#"
        table Source { name: string; }
        table Offer { target: &Source; text: string = f"{self.target.name}"; }
        table Display { target: &Offer; text: string = f"{self.target.text}"; }
    "#,
    );
    let mut inputs = vec![
        LoadedRecordDraft::new("a", "Source", [("name", "Alpha".into())]),
        LoadedRecordDraft::new("b", "Source", [("name", "Beta".into())]),
        LoadedRecordDraft::new(
            "offer",
            "Offer",
            [("target", LoadedValueDraft::record_ref("a"))],
        ),
        LoadedRecordDraft::new(
            "display",
            "Display",
            [("target", LoadedValueDraft::record_ref("offer"))],
        ),
    ];
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let a = id(&previous, "Source", "a");
    inputs[0].fields.insert("name".into(), "Changed".into());
    let first = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[a])
        .unwrap();
    assert_eq!(first.model, builder(&schema, &inputs, &[]).build().unwrap());
    assert_eq!(first.changed_records.len(), 3);
    assert_eq!(first.statistics.materialized_records, 3);
    let offer = id(&first.model, "Offer", "offer");
    inputs[2]
        .fields
        .insert("target".into(), LoadedValueDraft::record_ref("b"));
    let second = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&first.model, &[offer])
        .unwrap();
    assert_eq!(
        second.model,
        builder(&schema, &inputs, &[]).build().unwrap()
    );
    inputs[0].fields.insert("name".into(), "Detached".into());
    let third = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&second.model, &[a])
        .unwrap();
    assert_eq!(third.model, builder(&schema, &inputs, &[]).build().unwrap());
    assert_eq!(third.statistics.materialized_records, 1);
    assert_eq!(third.changed_records, vec![a]);
}

fn overlay(value: &str, source_origin: RecordOrigin) -> DimensionValueDraft {
    DimensionValueDraft {
        source_type: TypeName::new("Source").unwrap(),
        source_id: RecordId::new("a").unwrap(),
        field: FieldName::new("name").unwrap(),
        dimension: DimensionName::new("language").unwrap(),
        variant: VariantName::new("ja").unwrap(),
        value: value.into(),
        origin: source_origin,
    }
}

#[test]
fn dimension_changes_refresh_cross_record_templates_and_share_origin_only_updates() {
    let schema = compile_schema(
        r#"
        table Source { @localized name: string; }
        table Offer { target: &Source; @localized text: string = f"{self.target.name}"; }
    "#,
    );
    let inputs = vec![
        LoadedRecordDraft::new("a", "Source", [("name", "Alpha".into())]),
        LoadedRecordDraft::new(
            "offer",
            "Offer",
            [("target", LoadedValueDraft::record_ref("a"))],
        ),
    ];
    let mut overlays = vec![overlay("First", origin(0))];
    let previous = builder(&schema, &inputs, &overlays).build().unwrap();
    let a = id(&previous, "Source", "a");
    overlays[0] = overlay("Second", origin(1));
    let output = builder(&schema, &inputs, &overlays)
        .try_build_partial_incremental(&previous, &[a])
        .unwrap();
    assert_eq!(
        output.model,
        builder(&schema, &inputs, &overlays).build().unwrap()
    );
    assert_eq!(output.changed_records.len(), 2);
    let selections = BTreeMap::from([("language".into(), "ja".into())]);
    assert_eq!(
        output
            .model
            .project_dimensions(&schema, &selections)
            .unwrap(),
        builder(&schema, &inputs, &overlays)
            .build()
            .unwrap()
            .project_dimensions(&schema, &selections)
            .unwrap()
    );
    overlays[0].origin = origin(200);
    let moved = builder(&schema, &inputs, &overlays)
        .try_build_partial_incremental(&output.model, &[])
        .unwrap();
    assert_eq!(
        moved.model,
        builder(&schema, &inputs, &overlays).build().unwrap()
    );
    assert_eq!(moved.statistics.validated_records, 0);
    assert_eq!(moved.statistics.materialized_records, 0);
    assert_eq!(moved.changed_records, [] as [CfdRecordHandle; 0]);
    let old = &output
        .model
        .record(a)
        .unwrap()
        .dimension_field("name")
        .unwrap()
        .variants["ja"]
        .value;
    let new = &moved
        .model
        .record(a)
        .unwrap()
        .dimension_field("name")
        .unwrap()
        .variants["ja"]
        .value;
    assert!(Arc::ptr_eq(old, new));
}

#[test]
fn identity_schema_variant_and_diagnostic_changes_require_full_compilation() {
    let schema = compile_schema("table Item { value: int; }");
    let mut inputs = vec![LoadedRecordDraft::new(
        "a",
        "Item",
        [("value", 1_i64.into())],
    )];
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let a = id(&previous, "Item", "a");
    let cloned_schema = schema.clone();
    assert!(builder(&cloned_schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[])
        .is_some());
    let other_schema = compile_schema("table Item { value: int; }");
    assert!(builder(&other_schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[])
        .is_none());
    inputs[0].id = "renamed".into();
    assert!(builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[a])
        .is_none());
    inputs[0].id = "a".into();
    inputs[0].fields.clear();
    assert!(builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[a])
        .is_none());
    let invalid = builder(&schema, &inputs, &[]).build_partial().unwrap();
    assert!(!invalid.diagnostics.is_empty());
    assert!(builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&invalid.model, &[])
        .is_none());
}

#[test]
fn new_template_cycle_preserves_full_diagnostics() {
    let schema = compile_schema("table Item { text: string; }");
    let mut inputs = vec![LoadedRecordDraft::new(
        "a",
        "Item",
        [("text", "good".into())],
    )];
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let cell_value::ParsedCell::Value(cycle) =
        cell_value::parse_cell(&schema, "string", r#"f"{self.text}""#).unwrap()
    else {
        panic!("formatted string");
    };
    inputs[0].fields.insert("text".into(), cycle);
    assert!(builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[id(&previous, "Item", "a")])
        .is_none());
    assert_has_code(
        &builder(&schema, &inputs, &[]).build().unwrap_err(),
        CfdErrorCode::ValueDependencyCycle,
    );
}

#[test]
fn incremental_attempt_preserves_structural_budget_boundaries() {
    let schema = compile_schema("table Item { values: [int]; }");
    let limits = StructuralLimits::new(8, 8, 100);
    let mut inputs = vec![LoadedRecordDraft::new(
        "a",
        "Item",
        [("values", LoadedValueDraft::Array(vec![1_i64.into()]))],
    )];
    let previous = builder(&schema, &inputs, &[])
        .with_structural_limits(limits)
        .build()
        .unwrap();
    inputs[0].fields.insert(
        "values".into(),
        LoadedValueDraft::Array(vec![1_i64.into(); 20]),
    );
    assert!(builder(&schema, &inputs, &[])
        .with_structural_limits(limits)
        .try_build_partial_incremental(&previous, &[id(&previous, "Item", "a")])
        .is_none());
    assert_has_code(
        &builder(&schema, &inputs, &[])
            .with_structural_limits(limits)
            .build()
            .unwrap_err(),
        CfdErrorCode::DataStructureLimitExceeded,
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "同一连续编辑序列逐步对照完整构建，集中保留输入和断言便于核对。"
)]
fn repeated_mixed_updates_match_full_builds_including_overlay_removal() {
    let schema = compile_schema(
        r#"
        data Details { numbers: [int]; }
        table Item {
            @localized name: string;
            parent: &Item;
            @localized label: string = f"{self.parent.name}";
            details: Details;
        }
    "#,
    );
    let mut inputs = (0..12)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("i{index}"),
                "Item",
                [
                    ("name", format!("Name {index}").into()),
                    (
                        "parent",
                        LoadedValueDraft::record_ref(format!("i{}", (index + 1) % 12)),
                    ),
                    (
                        "details",
                        LoadedValueDraft::object(
                            "Details",
                            [("numbers", LoadedValueDraft::Array(vec![1_i64.into()]))],
                        ),
                    ),
                ],
            )
        })
        .collect::<Vec<_>>();
    let make_overlay = |index: usize, step: usize| DimensionValueDraft {
        source_type: TypeName::new("Item").unwrap(),
        source_id: RecordId::new(format!("i{index}")).unwrap(),
        field: FieldName::new("name").unwrap(),
        dimension: DimensionName::new("language").unwrap(),
        variant: VariantName::new("ja").unwrap(),
        value: format!("Localized {step}").into(),
        origin: origin(step),
    };
    // i0 始终保留覆盖，单独测试覆盖增删而不混入变体集合的全局失效。
    let mut overlays = BTreeMap::from([(0, make_overlay(0, 0))]);
    let mut model = builder(
        &schema,
        &inputs,
        &overlays.values().cloned().collect::<Vec<_>>(),
    )
    .build()
    .unwrap();
    let mut seed = 17_u64;
    for step in 0..48 {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let index = 1 + ((seed >> 32) as usize % 11);
        match step % 4 {
            0 => {
                inputs[index]
                    .fields
                    .insert("name".into(), format!("Changed {step}").into());
            }
            1 => {
                inputs[index].fields.insert(
                    "parent".into(),
                    LoadedValueDraft::record_ref(format!("i{}", step % 12)),
                );
            }
            2 => {
                if overlays.remove(&index).is_none() {
                    overlays.insert(index, make_overlay(index, step));
                }
            }
            _ => {
                inputs[index].fields.insert(
                    "details".into(),
                    LoadedValueDraft::object(
                        "Details",
                        [(
                            "numbers",
                            LoadedValueDraft::Array(vec![
                                LoadedValueDraft::from(
                                    i64::try_from(step).expect("fixture index fits i64")
                                );
                                1 + step % 5
                            ]),
                        )],
                    ),
                );
            }
        }
        for input in &mut inputs {
            input.origin = origin(step);
        }
        let values = overlays.values().cloned().collect::<Vec<_>>();
        let changed = id(&model, "Item", &format!("i{index}"));
        let output = builder(&schema, &inputs, &values)
            .try_build_partial_incremental(&model, &[changed])
            .unwrap();
        assert_eq!(
            output.model,
            builder(&schema, &inputs, &values).build().unwrap(),
            "step {step}"
        );
        model = output.model;
    }
}

#[test]
fn signed_zero_invalidates_storage_and_templates_without_changing_numeric_equality() {
    let schema = compile_schema(
        r#"
        table Source { number: double; }
        table View { target: &Source; text: string = f"{self.target.number}"; }
    "#,
    );
    let mut inputs = vec![
        LoadedRecordDraft::new(
            "source",
            "Source",
            [("number", LoadedValueDraft::Float(0.0))],
        ),
        LoadedRecordDraft::new(
            "view",
            "View",
            [("target", LoadedValueDraft::record_ref("source"))],
        ),
    ];
    let previous_input = inputs[0].clone();
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    inputs[0]
        .fields
        .insert("number".into(), LoadedValueDraft::Float(-0.0));
    assert!(!previous_input.same_values(&inputs[0]));
    assert_eq!(CfdValue::Float(0.0), CfdValue::Float(-0.0));
    assert!(!CfdValue::Float(0.0).same_stored_value(&CfdValue::Float(-0.0)));
    let source = id(&previous, "Source", "source");
    let output = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &[source])
        .unwrap();
    let Some(CfdValue::Float(number)) = output.model.record(source).unwrap().field("number") else {
        panic!("double field");
    };
    assert_eq!(number.to_bits(), (-0.0_f64).to_bits());
    assert!(output.changed_records.contains(&source));
    assert_eq!(
        output.model,
        builder(&schema, &inputs, &[]).build().unwrap()
    );
    inputs[0]
        .fields
        .insert("number".into(), LoadedValueDraft::Float(f64::NAN));
    assert!(builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&output.model, &[source])
        .is_none());
    assert!(builder(&schema, &inputs, &[]).build().is_err());
}

#[test]
fn batched_hub_retarget_matches_full_build_and_preserves_old_incoming_order() {
    let schema = compile_schema("table Item { value: int; target: &Item; }");
    let mut inputs = (0..100)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("i{index}"),
                "Item",
                [
                    ("value", LoadedValueDraft::Int(0)),
                    ("target", LoadedValueDraft::record_ref("i0")),
                ],
            )
        })
        .collect::<Vec<_>>();
    let previous = builder(&schema, &inputs, &[]).build().unwrap();
    let saved = previous.clone();
    let changed = (0..60)
        .rev()
        .map(|index| id(&previous, "Item", &format!("i{index}")))
        .collect::<Vec<_>>();
    for (index, input) in inputs.iter_mut().take(60).enumerate() {
        input
            .fields
            .insert("value".into(), LoadedValueDraft::Int(1));
        if index % 2 == 0 {
            input
                .fields
                .insert("target".into(), LoadedValueDraft::record_ref("i99"));
        }
    }
    let output = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&previous, &changed)
        .unwrap();
    let expected = builder(&schema, &inputs, &[]).build().unwrap();
    assert_eq!(output.model, expected);
    for (handle, _) in expected.records() {
        assert_eq!(
            output.model.ref_edges_to_target(handle).collect::<Vec<_>>(),
            expected.ref_edges_to_target(handle).collect::<Vec<_>>()
        );
    }
    assert_eq!(previous, saved);
    assert_eq!(
        previous
            .ref_edges_to_target(id(&previous, "Item", "i0"))
            .count(),
        100
    );
}

#[test]
fn record_set_cache_is_shared_only_while_membership_is_stable() {
    let schema = compile_schema(
        "abstract table Base { value: int; } table First : Base {} table Second : Base {}",
    );
    let mut inputs = vec![
        LoadedRecordDraft::new("z", "Second", [("value", 1_i64.into())]),
        LoadedRecordDraft::new("a", "First", [("value", 2_i64.into())]),
    ];
    let old = builder(&schema, &inputs, &[]).build().unwrap();
    let members = old.shared_assignable_records(&schema, "Base");
    assert_eq!(
        members
            .iter()
            .map(|id| old.record(*id).unwrap().id())
            .collect::<Vec<_>>(),
        vec!["a", "z"]
    );
    assert!(Arc::ptr_eq(
        &members,
        &old.shared_assignable_records(&schema, "Base")
    ));
    inputs[0].fields.insert("value".into(), 3_i64.into());
    let new = builder(&schema, &inputs, &[])
        .try_build_partial_incremental(&old, &[id(&old, "Second", "z")])
        .unwrap()
        .model;
    assert!(Arc::ptr_eq(
        &members,
        &new.shared_assignable_records(&schema, "Base")
    ));
    inputs.push(LoadedRecordDraft::new(
        "b",
        "First",
        [("value", 4_i64.into())],
    ));
    let structural = builder(&schema, &inputs, &[]).build().unwrap();
    let next = structural.shared_assignable_records(&schema, "Base");
    assert_eq!(next.len(), 3);
    assert_eq!(members.len(), 2);
    assert_eq!(
        next.iter()
            .map(|id| structural.record(*id).unwrap().id())
            .collect::<Vec<_>>(),
        vec!["a", "b", "z"]
    );
}

#[test]
fn borrowed_dimension_inputs_match_owned_full_and_incremental_builds() {
    let schema = compile_schema("table Source { @localized name: string; }");
    let inputs = vec![LoadedRecordDraft::new(
        "a",
        "Source",
        [("name", "Base".into())],
    )];
    let mut overlays = vec![overlay("First", origin(0))];
    let build_borrowed = |overlays: &[DimensionValueDraft], previous: Option<&CfdDataModel>| {
        let mut candidate = builder(&schema, &inputs, &[]);
        candidate.add_borrowed_dimension_value_drafts(overlays);
        if let Some(previous) = previous {
            candidate
                .try_build_partial_incremental(previous, &[id(previous, "Source", "a")])
                .unwrap()
                .model
        } else {
            candidate.build().unwrap()
        }
    };
    let previous = build_borrowed(&overlays, None);
    assert_eq!(
        previous,
        builder(&schema, &inputs, &overlays).build().unwrap()
    );
    overlays[0] = overlay("Second", origin(3));
    let next = build_borrowed(&overlays, Some(&previous));
    assert_eq!(next, builder(&schema, &inputs, &overlays).build().unwrap());
}
