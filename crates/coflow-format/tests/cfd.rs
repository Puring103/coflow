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
fn cfd_formatter_rejoins_split_function_headers() {
    let source = "default_calculator: Calculator {\n  classify:\n\n    fn(value: int) ->\n\n    string {\n      if value >= 10 {\n        \"large\"\n      } else {\n        \"small\"\n      }\n    },\n}\n";
    let expected = "default_calculator: Calculator {\n  classify: fn(value: int) -> string {\n    if value >= 10 {\n      \"large\"\n    } else {\n      \"small\"\n    }\n  },\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
}

#[test]
fn cfd_formatter_separates_array_close_after_structural_object() {
    let source = "bundle: EffectBundle {\n  additional: [\n    HealEffect {\n      amount: 5,\n    }],\n}\n";
    let expected = "bundle: EffectBundle {\n  additional: [\n    HealEffect {\n      amount: 5,\n    }\n  ],\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
    assert_eq!(
        format_cfd("bundle: EffectBundle { additional: [HealEffect { amount: 5, }], }"),
        expected
    );
}

#[test]
fn cfd_formatter_recovers_multiline_fields_functions_else_and_comments() {
    let source = "item: Example {\nname:\n\"Widget\",\ncallback:\nfn(\nvalue: int,\nfallback: fn(int) ->\nint\n)\n->\nResult<\nint,\nstring\n>\n{\nif value > 0 {\n\"ok\"\n}\nelse {\n\"bad\"\n}\n},\n# callback: fn(value: int) ->\nlabel: \"kept\",\n}\n";
    let expected = "item: Example {\n  name: \"Widget\",\n  callback: fn(\n    value: int,\n    fallback: fn(int) -> int\n  ) -> Result<\n    int,\n    string\n  > {\n    if value > 0 {\n      \"ok\"\n    } else {\n      \"bad\"\n    }\n  },\n  # callback: fn(value: int) ->\n  label: \"kept\",\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
}

#[test]
fn cfd_formatter_preserves_manual_blank_lines_inside_function_bodies() {
    let source = "calculator: Calculator {\n  classify: fn(\n    value: int\n  ) -> string\n  {\n\n\n    var label = \"small\"\n\n    if value >= 10 {\n\n      label = \"large\"\n\n    }\n\n    label\n\n  },\n}\n";
    let expected = "calculator: Calculator {\n  classify: fn(\n    value: int\n  ) -> string {\n\n    var label = \"small\"\n\n    if value >= 10 {\n\n      label = \"large\"\n\n    }\n\n    label\n\n  },\n}\n";
    assert_eq!(format_cfd(source), expected);
    assert_eq!(format_cfd(expected), expected);
}
