#![allow(clippy::unwrap_used)]
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

#[test]
fn anonymous_checks_in_different_modules_have_distinct_valid_identities() {
    let modules = parse_modules([
        CftFile::from_source(
            ModuleId::from("first"),
            "check { true; } check: \"second\" { true; }",
        ),
        CftFile::from_source(ModuleId::from("second"), "check { true; }"),
    ]);
    let schema = build_schema(&modules).unwrap();
    let ids: std::collections::BTreeSet<_> = schema
        .all_check_statements()
        .map(|info| info.rule_id.clone())
        .collect();
    assert_eq!(ids.len(), 3);
    for info in schema.all_check_statements() {
        let decoded = coflow_language::cft::CheckName::new(&info.rule_name).unwrap();
        assert_eq!(decoded, info.rule_name);
    }
}

#[test]
fn optional_names_preserve_duplicate_name_and_empty_block_validation() {
    for source in [
        "table T { check Same { true; } check Same { true; } }",
        "check Same { true; } check Same { true; }",
        "table T { check {} }",
        "check {}",
    ] {
        assert!(
            build_schema(&parse_modules([CftFile::from_source(
                ModuleId::from("main"),
                source
            )]))
            .is_err(),
            "{source}"
        );
    }
}

#[test]
fn anonymous_blocks_accept_formatted_messages() {
    let source = r#"table T { value: int = 1; check { value > 0; } check: f"value {value}" { value > 0; } }"#;
    build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        source,
    )]))
    .unwrap();
}
