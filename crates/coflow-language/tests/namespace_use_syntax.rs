use coflow_language::syntax::parser::parse_module;
use coflow_language::{
    build_schema, parse_modules, CftDimensionInputs, CftErrorCode, CftFile, CftValueType,
    ModuleId, TypeName,
};

#[test]
fn parses_namespace_and_explicit_uses_before_definitions() {
    let ast = parse_module(
        &ModuleId::from("items.cft"),
        r#"
            namespace game::items;
            use game::common::Position;
            use game::services::Services as Api;

            type Item {
                position: Position;
                api: &Api;
            }
        "#,
    )
    .expect("namespace and use declarations should parse");

    assert_eq!(
        ast.namespace
            .as_ref()
            .expect("namespace")
            .path
            .canonical(),
        "game::items"
    );
    assert_eq!(ast.uses.len(), 2);
    assert_eq!(ast.uses[0].path.canonical(), "game::common::Position");
    assert_eq!(ast.uses[0].local_name().name, "Position");
    assert_eq!(ast.uses[1].path.canonical(), "game::services::Services");
    assert_eq!(ast.uses[1].local_name().name, "Api");
    assert_eq!(ast.items.len(), 1);
}

#[test]
fn rejects_namespace_or_use_after_a_definition() {
    for source in [
        "type Item {} namespace game::items;",
        "type Item {} use game::common::Position;",
        "use game::common::Position; namespace game::items; type Item {}",
    ] {
        let diagnostics = parse_module(&ModuleId::from("invalid.cft"), source)
            .expect_err("file header declarations must be ordered");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidTopLevelItem));
    }
}

#[test]
fn rejects_wildcard_and_incomplete_paths() {
    for source in [
        "use game::*; type Item {}",
        "namespace game::; type Item {}",
        "use game::services::Services as; type Item {}",
    ] {
        assert!(parse_module(&ModuleId::from("invalid.cft"), source).is_err());
    }
}

#[test]
fn schema_uses_qualified_type_identity_and_resolves_import_aliases() {
    let modules = parse_modules([
        CftFile::from_source(
            ModuleId::from("common.cft"),
            r#"
                namespace shared::common;
                type Position { x: int; }
                enum Quality { Good }
                enum ItemId {}
                const DEFAULT_LABEL: string = "default";
            "#,
        ),
        CftFile::from_source(
            ModuleId::from("items.cft"),
            r#"
                namespace game::items;
                use shared::common::Position as Point;
                use shared::common::Quality;
                use shared::common::ItemId;
                use shared::common::DEFAULT_LABEL as Label;

                @idAsEnum(ItemId)
                type Item {
                    local: ItemData;
                    imported: Point;
                    absolute: shared::common::Position;
                    quality: Quality = Quality::Good;
                    label: string = Label;
                    check {
                        quality == Quality::Good;
                    }
                }
                sealed type ItemData : shared::common::Position { value: int; }

                check ValidItems {
                    all item in records(game::items::Item) {
                        item.quality == Quality::Good;
                    }
                }
            "#,
        ),
        CftFile::from_source(
            ModuleId::from("other.cft"),
            "namespace other; type Item { name: string; }",
        ),
    ]);
    let schema = build_schema(&modules, &CftDimensionInputs::default())
        .expect("qualified declarations should compile");

    let item = schema
        .resolve_type("game::items::Item")
        .expect("qualified Item type");
    assert_eq!(
        item.field("local").expect("local field").value_type,
        CftValueType::Object(TypeName::new("game::items::ItemData").expect("qualified name"))
    );
    let position = CftValueType::Object(
        TypeName::new("shared::common::Position").expect("qualified name"),
    );
    assert_eq!(item.field("imported").expect("imported field").value_type, position);
    assert_eq!(
        item.field("absolute").expect("absolute field").value_type,
        position
    );
    assert_eq!(item.id_as_enum.as_ref().map(|name| name.as_str()), Some("shared::common::ItemId"));
    assert_eq!(
        item.field("label").expect("label field").default,
        Some(coflow_language::CftSchemaDefaultValue::String("default".to_string()))
    );
    assert_eq!(
        item.field("quality").expect("quality field").default,
        Some(coflow_language::CftSchemaDefaultValue::Enum {
            enum_name: coflow_language::EnumName::new("shared::common::Quality")
                .expect("qualified enum"),
            variant: coflow_language::EnumVariantName::new("Good").expect("variant"),
            value: 0,
        })
    );
    assert_eq!(
        schema
            .resolve_type("game::items::ItemData")
            .expect("derived type")
            .parent
            .as_ref()
            .map(|name| name.as_str()),
        Some("shared::common::Position")
    );
    assert!(schema.resolve_type("other::Item").is_some());
    assert!(schema.resolve_type("Item").is_none());
}

#[test]
fn same_qualified_name_and_unknown_use_are_rejected() {
    let duplicate = parse_modules([
        CftFile::from_source(
            ModuleId::from("one.cft"),
            "namespace shared; type Item {}",
        ),
        CftFile::from_source(
            ModuleId::from("two.cft"),
            "namespace shared; type Item {}",
        ),
    ]);
    let diagnostics = build_schema(&duplicate, &CftDimensionInputs::default())
        .expect_err("same qualified name must be unique");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::DuplicateGlobalName));

    let unknown = parse_modules([CftFile::from_source(
        ModuleId::from("main.cft"),
        "use missing::Position; type Item { position: Position; }",
    )]);
    assert!(build_schema(&unknown, &CftDimensionInputs::default()).is_err());
}

#[test]
fn legacy_dot_enum_static_access_is_not_accepted() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main.cft"),
        r#"
            enum Quality { Good }
            type Item {
                quality: Quality = Quality.Good;
                check { quality == Quality.Good; }
            }
        "#,
    )]);
    assert!(build_schema(&modules, &CftDimensionInputs::default()).is_err());
}
