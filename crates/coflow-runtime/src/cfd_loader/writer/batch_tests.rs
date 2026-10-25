#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::Ordering;

use super::CfdWriter;
use crate::api::{WriteCellRequest, WriteFieldPathSegment as Segment};
use crate::cfd_loader::load_cfd_model;
use crate::{CfdDictKey, CfdFormattedString, CfdObject, CfdValue, RecordOrigin};
use coflow_language::cft::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};

fn schema() -> CftSchema {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        r#"
        data Node { number: int = 0; label: string = ""; }
        table Item {
            number: int = 0;
            label: string = "";
            node: Node = Node {};
            nodes: [Node] = [];
            dict: {string: Node} = {};
            optional: Option<Node> = None;
            added: int = 0;
            other: int = 0;
        }
    "#,
    )]);
    build_schema(&modules).expect("batch schema")
}

fn field(name: &str) -> Segment {
    Segment::Field(name.to_string())
}

fn node(number: i64) -> CfdValue {
    CfdValue::Object(Box::new(
        CfdObject::try_new(
            "Node",
            BTreeMap::from([
                ("number".to_string(), CfdValue::Int(number)),
                (
                    "label".to_string(),
                    CfdValue::String(format!("节点 {number}")),
                ),
            ]),
        )
        .expect("node"),
    ))
}

struct Edit {
    record: String,
    path: Vec<Segment>,
    value: CfdValue,
    materialized: Option<CfdValue>,
}

fn edit(record: &str, path: Vec<Segment>, value: CfdValue) -> Edit {
    Edit {
        record: record.into(),
        path,
        value,
        materialized: None,
    }
}

/// 对照逐条写入的完整文本，而不仅是最终值，覆盖注释、缩进和 CRLF 保留。
fn compare_with_sequential(source: &str, edits: &[Edit], expected_parses: Option<usize>) -> String {
    let directory = tempfile::tempdir().expect("temporary batch project");
    let path = directory.path().join("items.cfd");
    fs::write(&path, source).expect("seed source");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let requests = edits
        .iter()
        .map(|edit| WriteCellRequest {
            origin: &origin,
            record_id: &edit.record,
            actual_type: "Item",
            field_path: &edit.path,
            new_value: &edit.value,
            materialized_top_level: edit.materialized.as_ref(),
            schema: &schema,
        })
        .collect::<Vec<_>>();
    let sequential = CfdWriter::new();
    for (index, request) in requests.iter().enumerate() {
        sequential
            .write_field(request)
            .unwrap_or_else(|diagnostics| {
                panic!("sequential field write {index}: {diagnostics:?}");
            });
    }
    let writer = CfdWriter::new();
    let outcomes = writer.write_field_batch(&requests).expect("field batch");
    assert_eq!(outcomes.len(), requests.len());
    let result = writer
        .source_overrides()
        .expect("candidate source")
        .remove(0)
        .source;
    assert_eq!(
        result,
        sequential.source_overrides().expect("sequential source")[0].source
    );
    if let Some(expected) = expected_parses {
        assert_eq!(writer.field_batch_parses.load(Ordering::Relaxed), expected);
    }
    let mut preloaded = writer
        .take_preloaded_sources()
        .expect("move lowered sources");
    let loaded = preloaded
        .remove(&crate::normalize_path(&path))
        .expect("lowered candidate");
    assert!(preloaded.is_empty());
    let expected = crate::cfd_loader::CfdLoader::load_partial(
        crate::api::CfdLoadContext {
            schema: &schema,
            source_text: Some(&result),
        },
        &crate::api::CfdSource {
            location: crate::api::CfdSourcePath::new(path.clone()),
            display_name: "items.cfd".into(),
        },
    )
    .expect("ordinary source load");
    assert_eq!(loaded.source, expected.source);
    assert_eq!(loaded.records, expected.records);
    assert_eq!(loaded.dimensions, expected.dimensions);
    assert_eq!(loaded.dimension_variants, expected.dimension_variants);
    assert_eq!(loaded.diagnostics, expected.diagnostics);
    assert!(writer
        .take_preloaded_sources()
        .expect("sources already transferred")
        .is_empty());
    assert_eq!(fs::read_to_string(&path).expect("unchanged disk"), source);
    load_cfd_model(&schema, &result).expect("valid final model");
    writer.publish().expect("atomic batch publication");
    assert_eq!(fs::read_to_string(path).expect("published source"), result);
    result
}

#[test]
fn independent_complex_record_updates_parse_the_file_only_twice() {
    let mut source = String::from("# 文件前注释\r\nItem {\r\n");
    let mut edits = Vec::new();
    for index in 0..128 {
        let record = format!("item{index}");
        source.push_str(&format!("  {record} {{ node: Node {{ number: 1 }} }}\r\n"));
        edits.push(edit(&record, vec![field("node")], node(index)));
    }
    source.push_str("}\r\n# 文件后注释\r\n");
    let result = compare_with_sequential(&source, &edits, Some(2));
    assert!(result.starts_with("# 文件前注释\r\nItem {\r\n"));
    assert!(result.ends_with("}\r\n# 文件后注释\r\n"));
}

#[test]
fn absent_sibling_fields_share_one_patch_and_keep_comments() {
    for source in [
        "a: Item {}\n",
        "a: Item { number: 1 # 保留末尾注释\n}\n",
        "a: Item { number: 1,\r\n}\r\n",
    ] {
        compare_with_sequential(
            source,
            &[
                edit("a", vec![field("added")], CfdValue::Int(4)),
                edit("a", vec![field("other")], CfdValue::Int(8)),
                edit("a", vec![field("label")], CfdValue::String("文本".into())),
            ],
            Some(2),
        );
    }
    compare_with_sequential(
        "a: Item { node: Node {} }\n",
        &[
            edit("a", vec![field("node"), field("number")], CfdValue::Int(4)),
            edit(
                "a",
                vec![field("node"), field("label")],
                CfdValue::String("嵌套".into()),
            ),
        ],
        Some(2),
    );
}

#[test]
fn repeated_missing_field_is_updated_after_its_first_insertion() {
    let result = compare_with_sequential(
        "a: Item {}\n",
        &[
            edit("a", vec![field("added")], CfdValue::Int(4)),
            edit("a", vec![field("other")], CfdValue::Int(8)),
            edit("a", vec![field("added")], CfdValue::Int(12)),
        ],
        Some(3),
    );
    assert_eq!(result.matches("added:").count(), 1);
    assert!(result.contains("added: 12"));
}

#[test]
fn overlap_collections_options_and_materialized_defaults_keep_request_order() {
    let source = "a: Item { number: 1, node: Node {}, nodes: [], dict: {}, optional: None }\r\nb: Item {}\r\n";
    let mut materialized = edit("b", vec![field("node"), field("number")], CfdValue::Int(31));
    materialized.materialized = Some(node(31));
    let edits = vec![
        edit("a", vec![field("number")], CfdValue::Int(2)),
        edit("a", vec![field("number")], CfdValue::Int(3)),
        edit("a", vec![field("node")], node(4)),
        edit("a", vec![field("node"), field("number")], CfdValue::Int(5)),
        edit(
            "a",
            vec![field("nodes")],
            CfdValue::Array(vec![node(6), node(7)]),
        ),
        edit(
            "a",
            vec![field("nodes"), Segment::Index(1), field("number")],
            CfdValue::Int(8),
        ),
        edit(
            "a",
            vec![field("dict")],
            CfdValue::Dict(vec![(CfdDictKey::String("x".into()), node(9))]),
        ),
        edit(
            "a",
            vec![field("dict"), Segment::DictKey("x".into()), field("number")],
            CfdValue::Int(10),
        ),
        edit(
            "a",
            vec![field("optional")],
            CfdValue::OptionSome(Box::new(node(11))),
        ),
        edit(
            "a",
            vec![field("optional"), field("number")],
            CfdValue::Int(12),
        ),
        materialized,
        edit(
            "b",
            vec![field("node"), field("label")],
            CfdValue::String("后写保留".into()),
        ),
        edit(
            "a",
            vec![field("label")],
            CfdValue::FormattedString(CfdFormattedString {
                source: "f\"number={self.number}\"".into(),
                rendered: "number=3".into(),
            }),
        ),
    ];
    let result = compare_with_sequential(source, &edits, None);
    assert!(result.contains("number: 12"));
    assert!(result.contains("后写保留"));
}

#[test]
fn earlier_multiline_patch_preserves_later_same_line_rendering() {
    compare_with_sequential(
        "a: Item { node: Node {}, nodes: [], optional: None }\n",
        &[
            edit("a", vec![field("node")], node(1)),
            edit("a", vec![field("nodes")], CfdValue::Array(vec![node(2)])),
            edit(
                "a",
                vec![field("optional")],
                CfdValue::OptionSome(Box::new(node(3))),
            ),
        ],
        None,
    );
}

#[test]
fn optional_object_paths_support_explicit_and_implicit_some() {
    for value in ["Some(Node { number: 1 })", "Node { number: 1 }"] {
        let source = format!("a: Item {{ optional: {value} }}\n");
        let result = compare_with_sequential(
            &source,
            &[
                edit(
                    "a",
                    vec![field("optional"), field("number")],
                    CfdValue::Int(12),
                ),
                edit(
                    "a",
                    vec![field("optional"), field("label")],
                    CfdValue::String("可选对象".into()),
                ),
            ],
            Some(2),
        );
        assert!(result.contains("number: 12"));
    }
}

#[test]
fn failed_batch_keeps_original_workspace_and_reports_first_bad_operation() {
    let directory = tempfile::tempdir().expect("batch failures");
    let path = directory.path().join("items.cfd");
    let source = "a: Item { number: 1, label: \"first\" }\nb: Item { number: 2 }\n";
    fs::write(&path, source).expect("seed");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let edits = [
        edit("a", vec![field("number")], CfdValue::Int(42)),
        edit(
            "a",
            vec![field("label")],
            CfdValue::FormattedString(CfdFormattedString {
                source: "f\"unterminated".into(),
                rendered: String::new(),
            }),
        ),
        edit("b", vec![field("number")], CfdValue::Int(99)),
    ];
    let requests = edits
        .iter()
        .map(|edit| WriteCellRequest {
            origin: &origin,
            record_id: &edit.record,
            actual_type: "Item",
            field_path: &edit.path,
            new_value: &edit.value,
            materialized_top_level: None,
            schema: &schema,
        })
        .collect::<Vec<_>>();
    let writer = CfdWriter::new();
    let failure = writer
        .write_field_batch(&requests)
        .expect_err("invalid generated source");
    assert_eq!(failure.index, 1);
    assert!(failure.diagnostics.contains("failed to parse"));
    assert_eq!(
        writer.source_overrides().expect("original workspace")[0].source,
        source
    );
    assert_eq!(fs::read_to_string(&path).expect("unchanged source"), source);
}

/// 小预算直接覆盖真实批次解析入口，避免回归测试需要分配百万节点。
fn check_prefix_node_budget(
    source: &str,
    edits: &[Edit],
    max_nodes: u64,
    expected_failure: Option<usize>,
) {
    let directory = tempfile::tempdir().expect("batch node budget");
    let path = directory.path().join("items.cfd");
    fs::write(&path, source).expect("seed");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let requests = edits
        .iter()
        .map(|edit| WriteCellRequest {
            origin: &origin,
            record_id: &edit.record,
            actual_type: "Item",
            field_path: &edit.path,
            new_value: &edit.value,
            materialized_top_level: edit.materialized.as_ref(),
            schema: &schema,
        })
        .collect::<Vec<_>>();
    let options = coflow_language::cfd::CfdParseOptions {
        structural_limits: coflow_language::limits::StructuralLimits::new(
            256, max_nodes, 10_000_000,
        ),
    };
    let sequential = CfdWriter::new();
    let sequential_failure = requests.iter().enumerate().find_map(|(index, request)| {
        sequential
            .write_field_batch_with_options(std::slice::from_ref(request), options)
            .err()
            .map(|failure| (index, failure.diagnostics))
    });
    assert_eq!(
        sequential_failure.as_ref().map(|(index, _)| *index),
        expected_failure
    );
    let batch = CfdWriter::new();
    let result = batch.write_field_batch_with_options(&requests, options);
    match sequential_failure {
        Some((index, diagnostics)) => {
            let failure = result.expect_err("intermediate node budget must remain enforced");
            assert_eq!(failure.index, index);
            assert_eq!(failure.diagnostics, diagnostics);
            assert!(failure.diagnostics.contains("nodes limit"));
            assert_eq!(batch.field_batch_parse_count(), 3);
            assert_eq!(
                batch.source_overrides().expect("unchanged workspace")[0].source,
                source
            );
            assert!(batch.take_preloaded_sources().expect("no candidate").is_empty());
        }
        None => {
            result.expect("all prefixes fit the structural budget");
            assert_eq!(batch.field_batch_parse_count(), 2);
            assert_eq!(
                batch.source_overrides().expect("batch text")[0].source,
                sequential.source_overrides().expect("sequential text")[0].source
            );
        }
    }
    assert_eq!(fs::read_to_string(path).expect("unchanged disk"), source);
}

#[test]
fn shrinking_a_later_value_does_not_hide_an_intermediate_node_budget_failure() {
    let source = "a: Item { number: 1, nodes: [Node {}, Node {}] }\nb: Item { nodes: [Node {}, Node {}] }\n";
    let empty_node = || {
        CfdValue::Object(Box::new(
            CfdObject::try_new("Node", BTreeMap::new()).expect("empty node"),
        ))
    };
    check_prefix_node_budget(
        source,
        &[
            edit("a", vec![field("number")], CfdValue::Int(42)),
            edit(
                "a",
                vec![field("nodes")],
                CfdValue::Array((0..4).map(|_| empty_node()).collect()),
            ),
            edit("b", vec![field("nodes")], CfdValue::Array(Vec::new())),
        ],
        20,
        Some(1),
    );
    // 先释放节点预算再扩容是合法顺序，仍只解析初始和最终两份完整源码。
    check_prefix_node_budget(
        source,
        &[
            edit("a", vec![field("number")], CfdValue::Int(42)),
            edit("b", vec![field("nodes")], CfdValue::Array(Vec::new())),
            edit(
                "a",
                vec![field("nodes")],
                CfdValue::Array((0..4).map(|_| empty_node()).collect()),
            ),
        ],
        20,
        None,
    );
}

#[test]
fn merged_missing_siblings_charge_each_intermediate_field_node() {
    check_prefix_node_budget(
        "a: Item { nodes: [Node {}, Node {}] }\n",
        &[
            edit("a", vec![field("added")], CfdValue::Int(1)),
            edit("a", vec![field("other")], CfdValue::Int(2)),
            edit("a", vec![field("nodes")], CfdValue::Array(Vec::new())),
        ],
        11,
        Some(1),
    );
}

#[test]
fn materialized_default_parent_charges_its_actual_written_subtree() {
    let mut materialized = edit("a", vec![field("node"), field("number")], CfdValue::Int(7));
    materialized.materialized = Some(node(7));
    check_prefix_node_budget(
        "a: Item {}\nb: Item { nodes: [Node {}, Node {}] }\n",
        &[
            materialized,
            edit("b", vec![field("nodes")], CfdValue::Array(Vec::new())),
        ],
        16,
        Some(0),
    );
}

#[test]
fn batch_publication_detects_external_edits_without_overwriting_them() {
    let directory = tempfile::tempdir().expect("batch conflict");
    let path = directory.path().join("items.cfd");
    fs::write(&path, "a: Item { number: 1 }\n").expect("seed");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let path_segments = [field("number")];
    let value = CfdValue::Int(2);
    let writer = CfdWriter::new();
    writer
        .write_field_batch(&[WriteCellRequest {
            origin: &origin,
            record_id: "a",
            actual_type: "Item",
            field_path: &path_segments,
            new_value: &value,
            materialized_top_level: None,
            schema: &schema,
        }])
        .expect("stage");
    let external = "a: Item { number: 88 }\n";
    fs::write(&path, external).expect("external change");
    assert!(writer.publish().is_err());
    assert_eq!(
        fs::read_to_string(path).expect("external contents"),
        external
    );
}

#[test]
fn later_insert_does_not_hide_an_earlier_source_expression_error() {
    let directory = tempfile::tempdir().expect("batch syntax dependency");
    let path = directory.path().join("items.cfd");
    fs::write(&path, "a: Item { label: \"before\" }\n").expect("seed");
    let origin = RecordOrigin::File { path, span: None };
    let schema = schema();
    let label = [field("label")];
    let added = [field("added")];
    let raw_source = CfdValue::FormattedString(CfdFormattedString {
        source: "1 #".into(),
        rendered: String::new(),
    });
    let value = CfdValue::Int(2);
    let writer = CfdWriter::new();
    let failure = writer
        .write_field_batch(&[
            WriteCellRequest {
                origin: &origin,
                record_id: "a",
                actual_type: "Item",
                field_path: &label,
                new_value: &raw_source,
                materialized_top_level: None,
                schema: &schema,
            },
            WriteCellRequest {
                origin: &origin,
                record_id: "a",
                actual_type: "Item",
                field_path: &added,
                new_value: &value,
                materialized_top_level: None,
                schema: &schema,
            },
        ])
        .expect_err("first operation must fail before a later newline is inserted");
    assert_eq!(failure.index, 0);
}

#[test]
fn later_text_staging_discards_the_previous_lowered_candidate() {
    let directory = tempfile::tempdir().expect("source generation");
    let path = directory.path().join("items.cfd");
    fs::write(&path, "a: Item { number: 1 }\n").expect("seed");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let segments = [field("number")];
    let value = CfdValue::Int(2);
    let writer = CfdWriter::new();
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_id: "a",
            actual_type: "Item",
            field_path: &segments,
            new_value: &value,
            materialized_top_level: None,
            schema: &schema,
        })
        .expect("field candidate");
    writer
        .write_source(&path, "a: Item { number: 99 }\n")
        .expect("later structural write");
    assert!(writer
        .take_preloaded_sources()
        .expect("invalidated lowered source")
        .is_empty());
    writer.publish().expect("publish latest text");
    assert_eq!(
        fs::read_to_string(path).expect("published"),
        "a: Item { number: 99 }\n"
    );
}

#[test]
fn preloaded_source_keeps_unrelated_lowering_diagnostics() {
    let directory = tempfile::tempdir().expect("partial source");
    let path = directory.path().join("items.cfd");
    fs::write(
        &path,
        "a: Item { number: 1 }\nb: Item { number: \"invalid\" }\n",
    )
    .expect("seed");
    let origin = RecordOrigin::File {
        path: path.clone(),
        span: None,
    };
    let schema = schema();
    let segments = [field("number")];
    let value = CfdValue::Int(2);
    let writer = CfdWriter::new();
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_id: "a",
            actual_type: "Item",
            field_path: &segments,
            new_value: &value,
            materialized_top_level: None,
            schema: &schema,
        })
        .expect("edit valid record");
    let loaded = writer
        .take_preloaded_sources()
        .expect("lowered source")
        .remove(&crate::normalize_path(&path))
        .expect("candidate");
    let expected = crate::cfd_loader::CfdLoader::load_partial(
        crate::api::CfdLoadContext {
            schema: &schema,
            source_text: Some(&loaded.source),
        },
        &crate::api::CfdSource {
            location: crate::api::CfdSourcePath::new(path),
            display_name: "items.cfd".into(),
        },
    )
    .expect("ordinary partial load");
    assert!(!loaded.diagnostics.is_empty());
    assert_eq!(loaded.diagnostics, expected.diagnostics);
    assert_eq!(loaded.records, expected.records);
}
