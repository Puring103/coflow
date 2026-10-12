use coflow_format::format_cfd;

#[test]
fn cfd_formatter_expands_records_and_separates_top_level_entries() {
    let source = "Product { notebook { name: \"Notebook\", price: 12, } pencil { name: \"Pencil\", price: 2, } }\nwelcome: Notice { text: \"Ready\", }\n";
    assert_eq!(
        format_cfd(source),
        "Product {\n  notebook {\n    name: \"Notebook\",\n    price: 12,\n  }\n\n  pencil {\n    name: \"Pencil\",\n    price: 2,\n  }\n}\n\nwelcome: Notice {\n  text: \"Ready\",\n}\n"
    );
    assert_eq!(format_cfd(&format_cfd(source)), format_cfd(source));
}

#[test]
fn cfd_formatter_separates_array_close_after_structural_object() {
    let source =
        "bundle: EffectBundle {\n  additional: [\n    HealEffect {\n      amount: 5,\n    }],\n}\n";
    let expected = "bundle: EffectBundle {\n  additional: [\n    HealEffect {\n      amount: 5,\n    }\n  ],\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
    assert_eq!(
        format_cfd("bundle: EffectBundle { additional: [HealEffect { amount: 5, }], }"),
        expected
    );
}

#[test]
fn cfd_formatter_expands_dimension_blocks_and_keeps_templates_intact() {
    let source = "language[ja]{sword:Item{name:\"剣\",label:f\"{{text}} {self.name}\"}}";
    let expected = "language[ja] {\n  sword: Item {\n    name: \"剣\",\n    label: f\"{{text}} {self.name}\"\n  }\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
    let (_, errors) = coflow_language::cfd::parse_cfd(expected);
    assert!(errors.is_empty(), "{errors:?}");
}
