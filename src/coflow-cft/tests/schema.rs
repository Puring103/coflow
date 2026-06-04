mod common;
use common::*;

#[test]
fn schema_reports_cross_module_duplicate_with_related_label() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("a"), "type Item { id: string; }")
        .unwrap();
    container
        .add_module(ModuleId::from("b"), "enum Item { A, }")
        .unwrap();
    let err = container.compile().unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CftErrorCode::DuplicateGlobalName)
        .unwrap();
    assert!(!diag.related.is_empty());
}

#[test]
fn schema_reports_duplicate_field_enum_value_and_unknown_type() {
    let source = r#"
        enum E { A = 1, B = 1, }
        type A { x: Missing; x: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateEnumValue);
    assert_has_code(&err, CftErrorCode::DuplicateFieldName);
    assert_has_code(&err, CftErrorCode::UnknownNamedType);
}

#[test]
fn schema_reports_inheritance_and_modifier_errors() {
    let source = r#"
        sealed type Parent { id: string; }
        abstract sealed type Bad { x: int; }
        type Child : Parent { id: string; }
        type A : B { x: int; }
        type B : A { y: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InheritSealedType);
    assert_has_code(&err, CftErrorCode::DuplicateInheritedField);
    assert_has_code(&err, CftErrorCode::ConflictingTypeModifiers);
    assert_has_code(&err, CftErrorCode::InheritanceCycle);
}

#[test]
fn schema_reports_id_annotation_and_flag_errors() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 3, }

        type Base { @id id: string; }
        type Child : Base { @id other: int; }

        @struct
        type NotSealed { x: int; }

        type BadRef {
            @ref(Flags)
            flag_id: string;
            @index
            xs: [int];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidFlagEnumValue);
    assert_has_code(&err, CftErrorCode::MultipleIdFieldsInTree);
    assert_has_code(&err, CftErrorCode::StructRequiresSealedType);
    assert_has_code(&err, CftErrorCode::RefTargetMustBeType);
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn schema_reports_default_errors() {
    let source = r#"
        const NAME = "x";
        enum Rarity { Common, }
        type Item {
            id: int = NAME;
            bad: int = Missing;
            field_ref: int = id;
            rarity: Rarity = Rarity.Missing;
            xs: [int] = [1];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DefaultTypeMismatch);
    assert_has_code(&err, CftErrorCode::UnknownConst);
    assert_has_code(&err, CftErrorCode::DefaultReferencesField);
    assert_has_code(&err, CftErrorCode::UnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::InvalidDefaultExpression);
}
