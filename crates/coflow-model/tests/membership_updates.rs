#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;
use std::sync::Arc;

fn builder<'a>(schema: &'a CftSchema, records: &'a [LoadedRecordDraft]) -> CfdModelBuilder<'a> {
    let mut builder = CfdModelBuilder::new(schema);
    builder.add_borrowed_records(records);
    builder
}

fn reuse_map(
    old: &[LoadedRecordDraft],
    next: &[LoadedRecordDraft],
    model: &CfdDataModel,
) -> Vec<Option<CfdRecordHandle>> {
    next.iter()
        .map(|input| {
            model
                .record_by_type_id(&input.actual_type, &input.id)
                .filter(|id| input.same_values(&old[id.index()]))
        })
        .collect()
}

fn scalar(id: &str, score: i64) -> LoadedRecordDraft {
    LoadedRecordDraft::new(id, "Row", [("score", LoadedValueDraft::Int(score))])
}

fn assert_full_equivalence(
    schema: &CftSchema,
    old: &[LoadedRecordDraft],
    next: &[LoadedRecordDraft],
) -> CfdModelBuildOutput {
    let previous = builder(schema, old)
        .build()
        .expect("healthy previous model");
    let snapshot = previous.clone();
    let reuse = reuse_map(old, next, &previous);
    let actual = builder(schema, next)
        .try_build_membership_updates(&previous, &reuse)
        .expect("membership reuse");
    let expected = builder(schema, next).build_partial().expect("full build");
    assert_eq!(actual.diagnostics, expected.diagnostics);
    assert_eq!(actual.model, expected.model);
    assert_eq!(previous, snapshot, "old snapshot remains unchanged");
    actual
}

#[test]
fn members_insert_delete_rename_and_reorder_reuse_unchanged_value_trees() {
    let schema = compile_schema(
        "table Row { score: int; title: string = \"默认\"; values: [int] = [1, 2]; }",
    );
    let old = (0..2048)
        .map(|index| scalar(&format!("r{index}"), index))
        .collect::<Vec<_>>();
    let previous = builder(&schema, &old).build().unwrap();
    let mut next = old.clone();
    next.insert(0, scalar("new", 42));
    next.remove(1024);
    next[100].id = "renamed".into();
    next.rotate_left(17);
    let reuse = reuse_map(&old, &next, &previous);
    let actual = builder(&schema, &next)
        .try_build_membership_updates(&previous, &reuse)
        .unwrap();
    assert_eq!(actual.model, builder(&schema, &next).build().unwrap());
    assert_eq!(actual.statistics.validated_records, 2);
    assert_eq!(actual.statistics.materialized_records, 2);
    assert_eq!(actual.statistics.reused_records, 2046);
    assert_eq!(actual.statistics.resolved_root_fields, 6);
    assert_eq!(actual.statistics.identity_records_checked, 2048);
    let before = previous
        .record(previous.record_by_type_id("Row", "r2000").unwrap())
        .unwrap();
    let after = actual
        .model
        .record(actual.model.record_by_type_id("Row", "r2000").unwrap())
        .unwrap();
    assert!(Arc::ptr_eq(&before.object, &after.object));
    assert_eq!(previous.record_count(), 2048);
    assert!(previous.record_by_type_id("Row", "new").is_none());
}

#[test]
fn member_source_transfer_refreshes_origins_without_mutating_old_objects() {
    let schema = compile_schema("table Row { score: int; }");
    let mut old = vec![scalar("a", 1), scalar("b", 2)];
    for (index, input) in old.iter_mut().enumerate() {
        input.origin = RecordOrigin::File {
            path: "old.cfd".into(),
            span: Some(TextSpan {
                start_line: index,
                start_character: 0,
                end_line: index,
                end_character: 20,
            }),
        };
    }
    let previous = builder(&schema, &old).build().unwrap();
    let mut next = old.clone();
    next.reverse();
    next[0].origin = RecordOrigin::File {
        path: "new.cfd".into(),
        span: None,
    };
    let reuse = reuse_map(&old, &next, &previous);
    let actual = builder(&schema, &next)
        .try_build_membership_updates(&previous, &reuse)
        .unwrap();
    assert_eq!(actual.model, builder(&schema, &next).build().unwrap());
    assert_eq!(actual.statistics.validated_records, 0);
    assert_eq!(actual.statistics.resolved_root_fields, 0);
    let before = previous
        .record(previous.record_by_type_id("Row", "b").unwrap())
        .unwrap();
    let after = actual
        .model
        .record(actual.model.record_by_type_id("Row", "b").unwrap())
        .unwrap();
    assert!(Arc::ptr_eq(&before.object, &after.object));
    assert_eq!(
        before.origin.local_path().unwrap().to_str(),
        Some("old.cfd")
    );
    assert_eq!(after.origin.local_path().unwrap().to_str(), Some("new.cfd"));
}

fn template(type_name: Option<&str>, id: Option<&str>, path: &[&str]) -> LoadedValueDraft {
    LoadedValueDraft::FormattedString(LoadedFormattedString {
        source: format!("template:{type_name:?}:{id:?}:{path:?}"),
        segments: vec![LoadedFormatSegment::Reference(LoadedFieldReference {
            type_name: type_name.map(str::to_owned),
            id: id.map(str::to_owned),
            path: path.iter().map(|part| (*part).to_owned()).collect(),
        })],
    })
}

#[test]
fn members_recompute_nested_default_and_virtual_identity_templates() {
    let schema = compile_schema(
        r#"data Cell { text: string; } table Node { name: string; parent: &Node; caption: string = f"{self.parent.name}"; cells: [Cell] = []; }"#,
    );
    let mut old = vec![
        LoadedRecordDraft::new(
            "a",
            "Node",
            [
                ("name", LoadedValueDraft::String("A".into())),
                ("parent", LoadedValueDraft::RecordRef("b".into())),
            ],
        ),
        LoadedRecordDraft::new(
            "b",
            "Node",
            [
                ("name", LoadedValueDraft::String("B".into())),
                ("parent", LoadedValueDraft::RecordRef("b".into())),
                ("caption", template(Some("Node"), Some("a"), &["id"])),
            ],
        ),
    ];
    old[0].fields.insert(
        "cells".into(),
        LoadedValueDraft::Array(vec![LoadedValueDraft::Object {
            actual_type: Some("Cell".into()),
            fields: std::iter::once(("text".into(), template(Some("Node"), Some("b"), &["name"])))
                .collect(),
        }]),
    );
    let mut next = old.clone();
    next[1]
        .fields
        .insert("name".into(), LoadedValueDraft::String("更新🙂".into()));
    next.reverse();
    next.push(LoadedRecordDraft::new(
        "c",
        "Node",
        [
            ("name", LoadedValueDraft::String("C".into())),
            ("parent", LoadedValueDraft::RecordRef("a".into())),
        ],
    ));
    let output = assert_full_equivalence(&schema, &old, &next);
    assert_eq!(output.statistics.materialized_records, 3);
    assert_eq!(output.statistics.reused_records, 0);
    // 成员重排后的依赖句柄必须属于当前代次，后续单字段增量仍与全量结果一致。
    let target = output.model.record_by_type_id("Node", "b").unwrap();
    let mut edited = next.clone();
    edited[target.index()]
        .fields
        .insert("name".into(), LoadedValueDraft::String("下一代".into()));
    let changed = std::iter::once((
        target,
        std::iter::once(FieldName::new("name").unwrap()).collect(),
    ))
    .collect();
    let incremental = builder(&schema, &edited)
        .try_build_field_updates(&output.model, &changed)
        .unwrap();
    assert_eq!(
        incremental.model,
        builder(&schema, &edited).build().unwrap()
    );
    assert_eq!(incremental.statistics.materialized_records, 2);
    // 直接读取虚拟 id 的模板没有普通字段读边，删除目标仍必须交由完整诊断路径。
    let previous = builder(&schema, &old).build().unwrap();
    let next = vec![old[1].clone()];
    assert!(builder(&schema, &next)
        .try_build_membership_updates(&previous, &reuse_map(&old, &next, &previous))
        .is_none());
}

#[test]
fn members_revalidate_explicit_reference_types_when_inheritance_target_changes() {
    let schema = compile_schema("abstract table Root { value: int; } table Left : Root {} table Right : Root {} table Holder { link: &Root; }");
    let old = vec![
        LoadedRecordDraft::new("target", "Left", [("value", LoadedValueDraft::Int(1))]),
        LoadedRecordDraft::new(
            "holder",
            "Holder",
            [(
                "link",
                LoadedValueDraft::TypedRecordRef {
                    type_name: "Left".into(),
                    id: "target".into(),
                },
            )],
        ),
    ];
    let previous = builder(&schema, &old).build().unwrap();
    let mut next = old.clone();
    next[0].actual_type = "Right".into();
    assert!(builder(&schema, &next)
        .try_build_membership_updates(&previous, &reuse_map(&old, &next, &previous))
        .is_none());
    assert!(builder(&schema, &next).build().is_err());
    // 未携带显式子类型的父表引用可继续指向同一继承域中的新子类型。
    next[1]
        .fields
        .insert("link".into(), LoadedValueDraft::RecordRef("target".into()));
    assert_full_equivalence(&schema, &old, &next);
}

#[test]
fn membership_reuse_preserves_identity_singleton_and_structural_guards() {
    let schema = compile_schema("singleton Settings { value: int; } table Row { score: int; }");
    let old = vec![
        LoadedRecordDraft::new(
            "Settings",
            "Settings",
            [("value", LoadedValueDraft::Int(1))],
        ),
        scalar("row", 2),
    ];
    let previous = builder(&schema, &old).build().unwrap();
    let mut next = old.clone();
    next.push(scalar("other", 3));
    assert_full_equivalence(&schema, &old, &next);
    for next in [
        vec![old[1].clone()],
        vec![old[0].clone(), old[1].clone(), old[1].clone()],
        vec![old[0].clone(), scalar("bad-id", 3)],
    ] {
        assert!(builder(&schema, &next)
            .try_build_membership_updates(&previous, &reuse_map(&old, &next, &previous))
            .is_none());
    }
    let mut next = old.clone();
    next.push(LoadedRecordDraft::new(
        "invalid",
        "Row",
        [("score", LoadedValueDraft::String("bad".into()))],
    ));
    assert!(builder(&schema, &next)
        .try_build_membership_updates(&previous, &reuse_map(&old, &next, &previous))
        .is_none());
    assert!(builder(&schema, &old)
        .with_structural_limits(StructuralLimits::new(1, 1, 1))
        .try_build_membership_updates(&previous, &reuse_map(&old, &old, &previous))
        .is_none());
    let mut dimensional = builder(&schema, &old);
    dimensional.add_dimension_variant("language".try_into().unwrap(), "ja".try_into().unwrap());
    assert!(dimensional
        .try_build_membership_updates(&previous, &reuse_map(&old, &old, &previous))
        .is_none());
}
