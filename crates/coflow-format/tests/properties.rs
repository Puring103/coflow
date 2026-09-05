use coflow_format::{format_cfd, format_cft};
use coflow_language::lexical::{tokenize_lossless, LosslessTokenKind};
use coflow_language::cfd::parse_cfd;
use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId,
};

fn semantic_tokens(source: &str) -> Vec<&str> {
    tokenize_lossless(source)
        .iter()
        .filter(|token| !token.is_trivia())
        .map(|token| token.text(source))
        .collect()
}

fn comments(source: &str) -> Vec<&str> {
    tokenize_lossless(source)
        .iter()
        .filter(|token| token.kind == LosslessTokenKind::Comment)
        .map(|token| token.text(source))
        .collect()
}

#[test]
fn formatting_preserves_non_trivia_tokens_and_comments() {
    for (source, formatted) in [
        (
            "type 变量 { value: Result<Option<int>, string>; # CFT 注释\n}",
            format_cft("type 变量 { value: Result<Option<int>, string>; # CFT 注释\n}"),
        ),
        (
            "记录: Item { label: \"# not a comment\", # CFD 注释\n values: [1, 2], }",
            format_cfd(
                "记录: Item { label: \"# not a comment\", # CFD 注释\n values: [1, 2], }",
            ),
        ),
    ] {
        assert_eq!(semantic_tokens(source), semantic_tokens(&formatted));
        assert_eq!(comments(source), comments(&formatted));
    }
}

#[test]
fn malformed_sources_format_stably() {
    for source in [
        "type Item { value: Result<\nint,\n",
        "check Rules { all item in records(Item) { item.value >",
        "item: Item { values: [Other { value: \"unterminated",
        "item: Item { callback: fn(value: int) -> int { if value > 0 { value }",
    ] {
        let cft = format_cft(source);
        assert_eq!(format_cft(&cft), cft);
        let cfd = format_cfd(source);
        assert_eq!(format_cfd(&cfd), cfd);
    }
}

#[test]
fn valid_sources_parse_before_and_after_formatting() {
    let cft = "type Item { name: string; items: [int]; count: int; check { count > 0; } }";
    let before_modules = parse_modules([CftFile::from_source(ModuleId::from("main"), cft)]);
    let before = build_schema(&before_modules, &CftDimensionInputs::default()).expect("schema");
    let formatted_cft = format_cft(cft);
    let after_modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        formatted_cft,
    )]);
    let after = build_schema(&after_modules, &CftDimensionInputs::default()).expect("schema");
    let shape = |schema: &coflow_language::cft::CftSchema| {
        schema
            .all_types()
            .map(|ty| {
                (
                    ty.name.as_str().to_string(),
                    ty.all_fields()
                        .map(|field| (field.name.as_str().to_string(), field.value_type.clone()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(shape(&before), shape(&after));

    let cfd = "item: Item { name: \"Widget\", values: [1, 2], }";
    let (before, before_diagnostics) = parse_cfd(cfd);
    let (after, after_diagnostics) = parse_cfd(&format_cfd(cfd));
    assert!(before_diagnostics.is_empty());
    assert!(after_diagnostics.is_empty());
    let record_shape = |ast: &coflow_language::cfd::CfdAst| {
        ast.records
            .iter()
            .map(|record| {
                (
                    record.key.clone(),
                    record.type_name.clone(),
                    record
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(record_shape(&before), record_shape(&after));
}
