use coflow_language::cfd::parse_cfd;

#[test]
fn parses_namespace_and_use_header_before_records() {
    let (ast, diagnostics) = parse_cfd(
        r#"
namespace game::drops;
use game::items::Item;
use game::rules::Reward as DropReward;

Item {
  sword { rarity: Common }
}
"#,
    );

    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(ast.namespace.as_ref().map(|item| item.path.as_str()), Some("game::drops"));
    assert_eq!(ast.uses.len(), 2);
    assert_eq!(ast.uses[0].path, "game::items::Item");
    assert_eq!(ast.uses[0].local_name(), "Item");
    assert_eq!(ast.uses[1].path, "game::rules::Reward");
    assert_eq!(ast.uses[1].local_name(), "DropReward");
    assert_eq!(ast.records.len(), 1);
}

#[test]
fn rejects_misordered_or_invalid_header_declarations() {
    for source in [
        "Item { sword {} } namespace game::items;",
        "use game::items::Item; namespace game::drops; Item { sword {} }",
        "namespace game::; Item { sword {} }",
        "use game::*; Item { sword {} }",
        "use game::items::Item as; Item { sword {} }",
    ] {
        let (_, diagnostics) = parse_cfd(source);
        assert!(!diagnostics.is_empty(), "source should fail: {source}");
    }
}

#[test]
fn cfd_names_and_record_keys_use_unicode_xid_rules() {
    let (ast, diagnostics) = parse_cfd(
        "namespace \u{6E38}\u{620F}::\u{6389}\u{843D}; use \u{6E38}\u{620F}::\u{7269}\u{54C1}::\u{88C5}\u{5907}; \u{88C5}\u{5907} { \u{957F}\u{5251}\u{0301} {} }",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    assert_eq!(ast.records[0].key, "\u{957F}\u{5251}\u{0301}");
}
