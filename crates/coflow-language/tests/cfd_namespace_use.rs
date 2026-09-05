use coflow_language::cfd::parse_cfd;

#[test]
fn namespace_and_use_are_not_cfd_syntax() {
    for source in [
        "namespace game; Item { item {} }",
        "use common::Item; Item { item {} }",
        "use common::Item as Imported; Item { item {} }",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "source should fail: {source}");
    }
}

#[test]
fn cfd_type_names_are_short_and_static_paths_remain_valid() {
    for source in [
        "group::Item { item {} }",
        "item: group::Item {}",
        "Item { item { nested: group::Nested {} } }",
        "Item { item { target: &group::Item::other } }",
        "Item { item { label: \"{&group::Item::other.name}\" } }",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "source should fail: {source}");
    }

    let (ast, diagnostics) = parse_cfd(
        "Item { item { rarity: Quality::Good, target: &Item::other } other {} }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(ast.records.len(), 2);
}

#[test]
fn cfd_names_and_record_keys_use_unicode_xid_rules() {
    let (ast, diagnostics) = parse_cfd("\u{88C5}\u{5907} { \u{957F}\u{5251}\u{0301} {} }");
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(ast.records[0].key, "\u{957F}\u{5251}\u{0301}");
}
