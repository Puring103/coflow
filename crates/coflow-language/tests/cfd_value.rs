#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#![allow(clippy::literal_string_with_formatting_args)]

use coflow_language::cfd::{
    parse_cfd_value, parse_cfd_value_with_options, CfdFormatSegment, CfdParseOptions, CfdValue,
};
use coflow_language::cft::{build_schema, parse_modules, CftFile, CftSchemaDefaultValue, ModuleId};
use coflow_language::limits::StructuralLimits;

#[test]
fn standalone_values_consume_exactly_one_value() {
    for source in [
        "42",
        "-3.5",
        "true",
        "None",
        "Some([1, 2])",
        "&Item::one",
        "Permission::Read | Permission::Write",
        "Payload { id: 7 }",
        "{ key: [Some(1), None] }",
        r#"f"你好 {&Item::one.name}""#,
        " # leading\n [1] # trailing comment",
    ] {
        assert!(parse_cfd_value(source).is_ok(), "{source}");
    }
    for source in [
        "",
        "1, 2",
        "1 other: 2",
        "1 } injected: Item { value: 2 }",
        "Some(1",
        r#"f"{self.name()}""#,
        r#"f"{self.name + 1}""#,
    ] {
        assert!(parse_cfd_value(source).is_err(), "{source}");
    }
}

#[test]
fn standalone_values_enforce_depth_and_node_limits() {
    let options = CfdParseOptions {
        structural_limits: StructuralLimits::new(2, 3, 100),
    };
    assert!(parse_cfd_value_with_options("[1, 2]", options).is_ok());
    assert!(parse_cfd_value_with_options("[1, 2, 3]", options).is_err());
    assert!(parse_cfd_value_with_options("[[[1]]]", options).is_err());
}

#[test]
fn compiled_default_templates_preserve_original_source_positions() {
    let source = r#"
        table Item { name: string; label: string = f"你好\n {&Item::one.name}"; }
    "#;
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    let schema = build_schema(&modules).expect("valid template default");
    let Some(CftSchemaDefaultValue::FormattedString(template)) = schema
        .field("Item", "label")
        .and_then(|field| field.default.as_ref())
    else {
        panic!("expected a parsed template default");
    };
    assert_eq!(
        &source[template.span.start..template.span.end],
        template.source
    );
    let reference = template
        .segments
        .iter()
        .find_map(|segment| match segment {
            CfdFormatSegment::Reference(reference) => Some(reference),
            CfdFormatSegment::Text(_) => None,
        })
        .expect("reference segment");
    assert_eq!(
        &source[reference.span.start..reference.span.end],
        "{&Item::one.name}"
    );
    let CfdValue::FormattedString(value) = parse_cfd_value(&template.source).unwrap() else {
        panic!("template expected");
    };
    assert_eq!(value.source, template.source);
}
