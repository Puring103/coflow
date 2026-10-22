use coflow_language::cfd::parse_cfd;

#[test]
fn overlays_are_syntactically_distinct_from_base_records() {
    let source = "language[ja] { sword: Item { name: \"剣\" } }\nsword: Item { name: \"Sword\" }\nregion[jp] { sword: Item { price: 120 } }";
    let (ast, errors) = parse_cfd(source);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.dimensions.len(), 2);
    assert_eq!(ast.dimensions[0].dimension, "language");
    assert_eq!(ast.dimensions[0].variant, "ja");
    assert_eq!(ast.dimensions[0].records[0].id, "sword");
    assert_eq!(ast.dimensions[0].records[0].fields.len(), 1);
    assert_eq!(ast.dimensions[1].records[0].fields[0].name, "price");
    let span = ast.dimensions[0].span;
    assert_eq!(
        &source[span.start..span.end],
        "language[ja] { sword: Item { name: \"剣\" } }"
    );
}

#[test]
fn overlay_targets_require_explicit_record_types() {
    for source in [
        "language[ja] { sword { name: \"剣\" } }",
        "\"language\"[ja] {}",
        "language[\"ja\"] {}",
        "language[ja] { language[en] {} }",
    ] {
        let (_, errors) = parse_cfd(source);
        assert!(!errors.is_empty(), "{source}");
    }
}
