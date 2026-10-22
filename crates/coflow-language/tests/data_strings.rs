use coflow_language::cfd::{parse_cfd, CfdFormatSegment, CfdValue};

#[test]
fn interpolation_is_explicit_and_only_accepts_paths() {
    let (ast, errors) = parse_cfd("a: Item { literal: \"{self.name}\", label: f\"{{text}} {self.name} {&Item::other.name}\" }");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        matches!(&ast.records[0].fields[0].value, CfdValue::QuotedString(value, _) if value == "{self.name}")
    );
    let CfdValue::FormattedString(template) = &ast.records[0].fields[1].value else {
        panic!("template expected")
    };
    assert!(matches!(&template.segments[0], CfdFormatSegment::Text(text) if text == "{text} "));
    assert!(
        matches!(&template.segments[1], CfdFormatSegment::Reference(value) if value.path == ["name"] && value.id.is_none())
    );
    for text in [
        "f\"{name}\"",
        "f\"{self.price + 1}\"",
        "f\"{self.price:000}\"",
        "f\"{self.name()}\"",
        "f\"}\"",
        "f\"{self.name\"",
    ] {
        let (_, errors) = parse_cfd(&format!("a: Item {{ label: {text} }}"));
        assert!(!errors.is_empty(), "{text}");
    }
}

#[test]
fn function_values_are_not_data_syntax() {
    let (_, errors) = parse_cfd("a: Item { value: fn(x: int) -> int { x } }");
    assert!(!errors.is_empty());
}

#[test]
fn template_reference_spans_use_source_bytes_after_escapes_and_unicode() {
    let source = r#"a: Item { label: f"\n{{text}}你好 {self.name} {&Item::other.name}" }"#;
    let (ast, errors) = parse_cfd(source);
    assert!(errors.is_empty(), "{errors:?}");
    let CfdValue::FormattedString(template) = &ast.records[0].fields[0].value else {
        panic!("template expected")
    };
    assert!(
        matches!(&template.segments[0], CfdFormatSegment::Text(text) if text == "\n{text}你好 ")
    );
    let references = template
        .segments
        .iter()
        .filter_map(|segment| match segment {
            CfdFormatSegment::Reference(reference) => {
                Some(&source[reference.span.start..reference.span.end])
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(references, vec!["{self.name}", "{&Item::other.name}"]);
}
