#![allow(clippy::panic, clippy::unwrap_used)]

use coflow_cfd::{parse_cfd, CfdAst, CfdBlockEntry, CfdValue};

fn parse_ok(source: &str) -> CfdAst {
    let (ast, errors) = parse_cfd(source);
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    ast
}

fn parse_err(source: &str) -> Vec<coflow_cfd::CfdSyntaxDiagnostic> {
    let (_, errors) = parse_cfd(source);
    assert!(!errors.is_empty(), "expected at least one error");
    errors
}

// ── Positive tests ────────────────────────────────────────────────────────────

#[test]
fn simple_record_with_scalar_field() {
    let ast = parse_ok("sword: Item { damage: 42, }");
    assert_eq!(ast.records.len(), 1);
    let r = &ast.records[0];
    assert_eq!(r.key, "sword");
    assert_eq!(r.type_name, "Item");
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "damage");
    match &r.fields[0].value {
        CfdValue::Scalar(s, _) => assert_eq!(s, "42"),
        other => panic!("expected Scalar, got {other:?}"),
    }
}

#[test]
fn group_record_expands_to_multiple_records() {
    let source = "Item { sword {}, shield {}, }";
    let ast = parse_ok(source);
    assert_eq!(ast.records.len(), 2);
    assert_eq!(ast.records[0].key, "sword");
    assert_eq!(ast.records[0].type_name, "Item");
    assert_eq!(ast.records[1].key, "shield");
    assert_eq!(ast.records[1].type_name, "Item");
    for record in &ast.records {
        let (group_type, span) = record.group_type.as_ref().expect("group declaration type");
        assert_eq!(group_type, "Item");
        assert_eq!(&source[span.start..span.end], "Item");
    }
}

#[test]
fn group_record_commas_are_optional() {
    let ast = parse_ok(
        r"
        Item {
          sword { value: 1 }
          shield { value: 2 },
          bow { value: 3 }
        }
        ",
    );
    let coords = ast
        .records
        .iter()
        .map(|record| (record.type_name.as_str(), record.key.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        coords,
        vec![("Item", "sword"), ("Item", "shield"), ("Item", "bow")]
    );
}

#[test]
fn nested_block_as_field_value() {
    let ast = parse_ok("hero: Player { stats: Stats { hp: 100, }, }");
    assert_eq!(ast.records.len(), 1);
    let field = &ast.records[0].fields[0];
    assert_eq!(field.name, "stats");
    match &field.value {
        CfdValue::Block(b) => {
            assert!(b.type_marker.as_ref().is_some_and(|(n, _)| n == "Stats"));
        }
        other => panic!("expected Block, got {other:?}"),
    }
}

#[test]
fn array_field_value() {
    let ast = parse_ok("r: T { items: [1, 2, 3] }");
    match &ast.records[0].fields[0].value {
        CfdValue::Array(items, _) => assert_eq!(items.len(), 3),
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn trailing_comma_in_array() {
    let ast = parse_ok("r: T { items: [1, 2, 3,] }");
    match &ast.records[0].fields[0].value {
        CfdValue::Array(items, _) => assert_eq!(items.len(), 3),
        other => panic!("expected Array, got {other:?}"),
    }
}

#[test]
fn direct_ref_value() {
    let ast = parse_ok("r: T { target: &boss, }");
    match &ast.records[0].fields[0].value {
        CfdValue::Ref(r) => assert_eq!(r.key.0, "boss"),
        other => panic!("expected Ref, got {other:?}"),
    }
}

#[test]
fn negative_numeric_scalar() {
    let ast = parse_ok("r: T { x: -42, y: -3.14 }");
    let fields = &ast.records[0].fields;
    match &fields[0].value {
        CfdValue::Scalar(s, _) => assert_eq!(s, "-42"),
        other => panic!("expected Scalar, got {other:?}"),
    }
    match &fields[1].value {
        CfdValue::Scalar(s, _) => assert_eq!(s, "-3.14"),
        other => panic!("expected Scalar, got {other:?}"),
    }
}

#[test]
fn empty_input_produces_no_records() {
    let ast = parse_ok("");
    assert!(ast.records.is_empty());
}

#[test]
fn comment_only_input_produces_no_records() {
    let ast = parse_ok("# a comment\n# another comment\n");
    assert!(ast.records.is_empty());
}

#[test]
fn quoted_record_key() {
    let ast = parse_ok(r#""my key": Item { x: 1, }"#);
    assert_eq!(ast.records[0].key, "my key");
}

#[test]
fn semicolon_field_separator_is_rejected() {
    parse_err("r: T { a: 1; b: 2; }");
}

#[test]
fn null_literal() {
    let ast = parse_ok("r: T { x: null, }");
    assert!(matches!(ast.records[0].fields[0].value, CfdValue::Null(_)));
}

// ── Span accuracy ─────────────────────────────────────────────────────────────

#[test]
fn scalar_span_excludes_trailing_whitespace() {
    let source = "r: T { x: 42   , }";
    let (ast, _) = parse_cfd(source);
    let span = match &ast.records[0].fields[0].value {
        CfdValue::Scalar(_, s) => *s,
        other => panic!("expected Scalar, got {other:?}"),
    };
    // Span should cover exactly "42" (2 bytes), not "42   ".
    assert_eq!(&source[span.start..span.end], "42");
}

#[test]
fn block_type_marker_span_excludes_whitespace() {
    let source = "r: T { sub: Sub   { }, }";
    let (ast, _) = parse_cfd(source);
    let marker_span = match &ast.records[0].fields[0].value {
        CfdValue::Block(b) => b.type_marker.as_ref().unwrap().1,
        other => panic!("expected Block, got {other:?}"),
    };
    assert_eq!(&source[marker_span.start..marker_span.end], "Sub");
}

#[test]
fn span_byte_accuracy_for_key_and_type() {
    let source = "\n\nsword: Item { }";
    let (ast, _) = parse_cfd(source);
    let r = &ast.records[0];
    assert_eq!(&source[r.key_span.start..r.key_span.end], "sword");
    assert_eq!(&source[r.type_span.start..r.type_span.end], "Item");
}

// ── Negative / error tests ────────────────────────────────────────────────────

#[test]
fn missing_colon_produces_error() {
    parse_err("sword Item { }");
}

#[test]
fn unterminated_block_produces_error() {
    parse_err("r: T { name: 1");
}

#[test]
fn check_block_produces_helpful_error() {
    let errors = parse_err("r: T { check { x: 1 } }");
    let msg = &errors[0].message;
    assert!(
        msg.contains("check"),
        "error message should mention 'check', got: {msg}"
    );
}

#[test]
fn unterminated_string_produces_error() {
    parse_err(r#"r: T { name: "unterminated }"#);
}

#[test]
fn typed_refs_are_rejected() {
    let errors = parse_err("r: T { target: @Monster.boss }");
    assert!(
        errors.iter().any(|error| error.message.contains("`&key`")),
        "expected typed ref rejection, got {errors:?}"
    );
}

#[test]
fn direct_ref_paths_are_rejected() {
    let errors = parse_err("r: T { target: &boss.name }");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("reference paths")),
        "expected path ref rejection, got {errors:?}"
    );
}

#[test]
fn slash_slash_comments_are_rejected() {
    parse_err("// a comment\n");
}

#[test]
fn adjacent_fields_without_comma_are_rejected() {
    parse_err("r: T { a: 1 b: 2 }");
}

#[test]
fn adjacent_array_items_without_comma_are_rejected() {
    parse_err("r: T { items: [1 2] }");
}

// ── Error recovery ────────────────────────────────────────────────────────────

#[test]
fn error_recovery_continues_after_bad_record() {
    // First record is broken, second is valid.
    let source = "bad item\ngood: Item { x: 1 }";
    let (ast, errors) = parse_cfd(source);
    assert!(!errors.is_empty(), "expected at least one error");
    // Should still recover and parse the second record.
    assert!(
        ast.records.iter().any(|r| r.key == "good"),
        "should have recovered and parsed 'good'"
    );
}

// ── Spread syntax ─────────────────────────────────────────────────────────────

#[test]
fn spread_in_block() {
    let ast = parse_ok("r: T { ...&base, x: 1, }");
    let r = &ast.records[0];
    assert_eq!(r.entries.len(), 2);
    assert!(matches!(r.entries[0], CfdBlockEntry::Spread(_, _)));
    assert_eq!(r.fields.len(), 1);
    assert_eq!(r.fields[0].name, "x");
}
