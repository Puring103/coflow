#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;
use std::collections::BTreeSet;

fn template(path: &[&str]) -> LoadedValueDraft {
    LoadedValueDraft::FormattedString(LoadedFormattedString {
        source: format!("f\"{{self.{}}}\"", path.join(".")),
        segments: vec![LoadedFormatSegment::Reference(LoadedFieldReference {
            type_name: None,
            id: None,
            path: path.iter().map(|part| (*part).to_owned()).collect(),
        })],
    })
}

fn builder<'a>(schema: &'a CftSchema, records: &'a [LoadedRecordDraft]) -> CfdModelBuilder<'a> {
    let mut builder = CfdModelBuilder::new(schema);
    builder.add_borrowed_records(records);
    builder
}

type CanonicalDiagnostic = (CfdDiagnostic, Vec<Option<usize>>);

fn canonical_diagnostics(
    diagnostics: CfdDiagnostics,
    source_index: impl Fn(CfdRecordHandle) -> usize,
) -> Vec<CanonicalDiagnostic> {
    diagnostics
        .diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            // 公共句柄保持不透明；测试单独比较原输入下标，其余诊断字段仍完整比较。
            let records = diagnostic
                .primary
                .iter_mut()
                .chain(&mut diagnostic.related)
                .map(|label| label.record.take().map(&source_index))
                .collect();
            (diagnostic, records)
        })
        .collect()
}

/// 对照保留原逐轮完整构建算法，验证的不只是拒绝集合，还包括全部诊断顺序和标签。
fn sequential_oracle(
    schema: &CftSchema,
    records: &[LoadedRecordDraft],
    limits: StructuralLimits,
) -> (BTreeSet<usize>, Vec<CanonicalDiagnostic>) {
    let mut candidates = (0..records.len()).collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    let mut rejected = BTreeSet::new();
    loop {
        let mut builder = CfdModelBuilder::new(schema).with_structural_limits(limits);
        for &index in &candidates {
            builder.add_borrowed_record(&records[index]);
        }
        match builder.build_partial() {
            Ok(_) => return (rejected, diagnostics),
            Err(errors) => {
                let removed = errors
                    .diagnostics
                    .iter()
                    .flat_map(|error| error.primary.iter().chain(&error.related))
                    .filter_map(|label| label.record.map(CfdRecordHandle::index))
                    .collect::<BTreeSet<_>>();
                assert!(!removed.is_empty());
                diagnostics.extend(canonical_diagnostics(errors, |id| candidates[id.index()]));
                rejected.extend(removed.iter().map(|&index| candidates[index]));
                candidates = candidates
                    .into_iter()
                    .enumerate()
                    .filter_map(|(index, source)| (!removed.contains(&index)).then_some(source))
                    .collect();
            }
        }
    }
}

fn assert_matches_oracle(
    schema: &CftSchema,
    records: &[LoadedRecordDraft],
) -> CfdTemplateRejections {
    assert_matches_with_limits(schema, records, StructuralLimits::default())
}

fn assert_matches_with_limits(
    schema: &CftSchema,
    records: &[LoadedRecordDraft],
    limits: StructuralLimits,
) -> CfdTemplateRejections {
    let actual = builder(schema, records)
        .with_structural_limits(limits)
        .template_rejection_closure();
    let expected = sequential_oracle(schema, records, limits);
    assert_eq!(
        actual
            .rejected_records
            .iter()
            .map(|id| id.index())
            .collect::<BTreeSet<_>>(),
        expected.0
    );
    assert_eq!(
        canonical_diagnostics(actual.diagnostics.clone(), CfdRecordHandle::index),
        expected.1
    );
    actual
}

#[test]
fn rejection_replays_original_structure_budget_and_precise_blame() {
    let schema = compile_schema("table Node { name: string; parent: &Node; caption: string; }");
    let mut records = chain(16);
    for record in records.iter_mut().take(16) {
        record
            .fields
            .insert("caption".into(), template(&["parent", "caption"]));
    }
    for limits in [
        StructuralLimits::new(8, 100, 100),
        StructuralLimits::new(100, 10, 100),
    ] {
        let result = assert_matches_with_limits(&schema, &records, limits);
        assert!(result
            .diagnostics
            .diagnostics
            .iter()
            .any(|error| error.code == CfdErrorCode::DataStructureLimitExceeded));
    }
}

fn chain(count: usize) -> Vec<LoadedRecordDraft> {
    let mut records = (0..count)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("r{index}"),
                "Node",
                [
                    ("name", LoadedValueDraft::String(format!("值{index}"))),
                    (
                        "parent",
                        LoadedValueDraft::RecordRef(if index == 0 {
                            "missing".into()
                        } else {
                            format!("r{}", index - 1)
                        }),
                    ),
                    ("caption", template(&["parent", "name"])),
                ],
            )
        })
        .collect::<Vec<_>>();
    records.push(LoadedRecordDraft::new(
        "healthy",
        "Node",
        [
            ("name", LoadedValueDraft::String("保留🙂".into())),
            ("parent", LoadedValueDraft::RecordRef("healthy".into())),
            ("caption", LoadedValueDraft::String("literal".into())),
        ],
    ));
    records
}

#[test]
fn rejection_closure_matches_each_original_wave_and_uses_linear_root_work() {
    let schema = compile_schema("table Node { name: string; parent: &Node; caption: string; }");
    for count in [1, 8, 32] {
        let mut records = chain(count);
        for _ in 0..3 {
            let result = assert_matches_oracle(&schema, &records);
            assert_eq!(result.rejected_records.len(), count);
            assert_eq!(result.statistics.validated_records, count + 1);
            assert!(result.statistics.resolved_root_fields <= 6 * (count + 1));
            records.reverse();
            records.rotate_left(1);
        }
    }
    // 大规模只测工作量，不把测试时间绑定到机器速度，也不运行平方 oracle。
    let records = chain(2048);
    let result = builder(&schema, &records).template_rejection_closure();
    assert_eq!(result.rejected_records.len(), 2048);
    assert_eq!(result.statistics.validated_records, 2049);
    assert!(result.statistics.resolved_root_fields <= 6 * 2049);
}

#[test]
fn rejection_tracks_virtual_ids_nested_collection_reads_and_cycle_labels() {
    let schema = compile_schema("data Cell { target: &Node; text: string; } table Node { name: string; parent: &Node; caption: string; cells: [Cell] = []; }");
    let mut records = vec![
        LoadedRecordDraft::new(
            "a",
            "Node",
            [
                ("name", LoadedValueDraft::String("A".into())),
                ("parent", LoadedValueDraft::RecordRef("b".into())),
                ("caption", template(&["parent", "caption"])),
            ],
        ),
        LoadedRecordDraft::new(
            "b",
            "Node",
            [
                ("name", LoadedValueDraft::String("B".into())),
                ("parent", LoadedValueDraft::RecordRef("a".into())),
                ("caption", template(&["parent", "caption"])),
            ],
        ),
        LoadedRecordDraft::new(
            "id_reader",
            "Node",
            [
                ("name", LoadedValueDraft::String("C".into())),
                ("parent", LoadedValueDraft::RecordRef("a".into())),
                ("caption", template(&["parent", "id"])),
            ],
        ),
        LoadedRecordDraft::new(
            "nested",
            "Node",
            [
                ("name", LoadedValueDraft::String("D".into())),
                ("parent", LoadedValueDraft::RecordRef("nested".into())),
                ("caption", LoadedValueDraft::String("plain".into())),
                (
                    "cells",
                    LoadedValueDraft::Array(vec![LoadedValueDraft::Object {
                        actual_type: Some("Cell".into()),
                        fields: [
                            (
                                "target".into(),
                                LoadedValueDraft::RecordRef("id_reader".into()),
                            ),
                            ("text".into(), template(&["target", "name"])),
                        ]
                        .into_iter()
                        .collect(),
                    }]),
                ),
            ],
        ),
    ];
    for _ in 0..4 {
        assert_matches_oracle(&schema, &records);
        records.rotate_left(1);
    }
}

#[test]
fn rejection_matches_original_diagnostics_across_mixed_dependency_graphs() {
    let schema = compile_schema("table Node { name: string; parent: &Node; caption: string; }");
    // 固定枚举多种拓扑，覆盖同轮多错、多个循环、扇出和间接模板读取，结果与原算法逐项对照。
    for scenario in 0..96usize {
        let mut state = scenario + 1;
        let mut records = chain(6);
        for (index, record) in records.iter_mut().take(6).enumerate() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let target = state % 8;
            record.fields.insert(
                "parent".into(),
                LoadedValueDraft::RecordRef(if target < 6 {
                    format!("r{target}")
                } else {
                    "missing".into()
                }),
            );
            let field = match (scenario + index) % 3 {
                0 => "name",
                1 => "id",
                _ => "caption",
            };
            record
                .fields
                .insert("caption".into(), template(&["parent", field]));
        }
        records.rotate_left(scenario % 7);
        assert_matches_oracle(&schema, &records);
    }
}

#[test]
fn default_templates_and_reference_diagnostics_preserve_rejection_order() {
    let schema = compile_schema(
        r#"table Node { name: string; parent: &Node; caption: string = f"{self.parent.name}"; }"#,
    );
    let mut records = chain(12);
    for record in records.iter_mut().take(12) {
        record.fields.remove("caption");
    }
    assert_matches_oracle(&schema, &records);

    let schema = compile_schema("abstract table Root { text: string; } table Left : Root {} table Right : Root {} table Reader { link: &Root; text: string; }");
    let records = vec![
        LoadedRecordDraft::new(
            "right",
            "Right",
            [("text", LoadedValueDraft::String("right".into()))],
        ),
        LoadedRecordDraft::new(
            "bad_type",
            "Reader",
            [
                (
                    "link",
                    LoadedValueDraft::TypedRecordRef {
                        type_name: "Left".into(),
                        id: "right".into(),
                    },
                ),
                ("text", LoadedValueDraft::String("reader".into())),
            ],
        ),
        LoadedRecordDraft::new(
            "missing",
            "Reader",
            [
                ("link", LoadedValueDraft::RecordRef("absent".into())),
                ("text", template(&["link", "text"])),
            ],
        ),
    ];
    assert_matches_oracle(&schema, &records);
}

#[test]
fn rejection_leaves_other_compiler_stages_in_charge() {
    let schema = compile_schema("table Node { name: string; parent: &Node; caption: string; }");
    let mut records = chain(3);
    records[1].fields.remove("name");
    assert!(builder(&schema, &records)
        .template_rejection_closure()
        .rejected_records
        .is_empty());
    let records = vec![chain(1).remove(0), chain(1).remove(0)];
    assert!(builder(&schema, &records)
        .template_rejection_closure()
        .rejected_records
        .is_empty());
    let schema = compile_schema("singleton State { value: int; }");
    let records = [LoadedRecordDraft::new(
        "state",
        "State",
        [("value", LoadedValueDraft::Int(1))],
    )];
    assert!(builder(&schema, &records)
        .template_rejection_closure()
        .rejected_records
        .is_empty());
}

#[test]
fn failed_build_reports_work_without_changing_diagnostics() {
    let schema = compile_schema("table Node { name: string; parent: &Node; caption: string; }");
    let records = chain(8);
    let expected = builder(&schema, &records).build_partial().unwrap_err();
    let (actual, work) = builder(&schema, &records)
        .build_partial_with_statistics()
        .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(work.validated_records, 9);
    assert_eq!(work.identity_records_checked, 9);
    assert_eq!(work.resolved_root_fields, 27);
}
