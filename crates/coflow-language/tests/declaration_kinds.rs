#![allow(clippy::expect_used)]
use coflow_language::cft::{build_schema, parse_modules, CftFile, CftValueType, ModuleId};

fn compile(
    source: &str,
) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("kinds"),
        source,
    )]))
}

#[test]
fn declaration_kinds_resolve_named_values_and_preserve_identity_boundaries() {
    let schema = compile("data Stats { hp: int; } abstract table Entity {} table Item: Entity { stats: Stats; next: Option<&Entity>; } singleton Settings: Entity { start: &Item; } type Link = Option<&Item>;").expect("three declaration kinds");
    assert!(schema.resolve_type("Stats").expect("Stats").is_data());
    assert!(schema.resolve_type("Item").expect("Item").is_table());
    assert!(schema
        .resolve_type("Settings")
        .expect("Settings")
        .is_singleton());
    assert!(matches!(
        schema.field("Item", "stats").expect("stats").value_type,
        CftValueType::Object(_)
    ));
    assert!(matches!(
        schema.field("Settings", "start").expect("start").value_type,
        CftValueType::RecordRef(_)
    ));
}

#[test]
fn declaration_categories_reject_cross_category_inheritance_and_old_protocols() {
    for source in [
        "type Item {}",
        "@singleton table Item {}",
        "table Item {} table Holder { item: Item; }",
        "data Payload {} table Holder { item: &Payload; }",
        "singleton Settings {} table Holder { settings: &Settings; }",
        "table Item {} table Holder { item: &&Item; }",
        "table Holder { item: &int; }",
        "table Holder { item: &[int]; }",
        "table Item {} type Link = Item;",
        "data Payload {} type Link = &Payload;",
        "@struct sealed table Item {}",
        "@struct sealed singleton Settings {}",
        "data Base {} table Item: Base {}",
        "data Base {} @struct sealed data Value: Base {}",
        "table Base {} data Item: Base {}",
        "singleton Base {} table Item: Base {}",
        "abstract singleton Settings {}",
        "data Payload { check { true; } }",
        "data Payload { @localized name: string; }",
        "enum Key {} @idAsEnum(Key) data Payload {}",
        "singleton Settings {} table Item { settings: Settings; }",
        "data Payload {} const REF = &Payload::entry;",
        "table Item {} const INLINE = Item {};",
        "data Payload {} check All { all value in records(Payload) { true; } }",
    ] {
        assert!(compile(source).is_err(), "must reject: {source}");
    }
}
