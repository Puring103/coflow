use coflow_language::cfd::parse_cfd;

#[test]
fn imports_and_qualified_record_types_parse() {
    for source in [
        "use common::Item; item: Item {}",
        "item: common::Item { target: &common::Item::other }",
    ] {
        let (ast, diagnostics) = parse_cfd(source);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        assert_eq!(ast.records.len(), 1);
    }
}

#[test]
fn grouped_records_and_import_aliases_are_rejected() {
    for source in [
        "namespace game; item: Item {}",
        "Item { item {} }",
        "use common::Item as Imported; item: Imported {}",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "{source}");
    }
}

#[test]
fn cfd_names_and_record_keys_use_unicode_xid_rules() {
    let (ast, diagnostics) = parse_cfd("长剑́: 装备 {}");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(ast.records[0].key, "长剑́");
}
