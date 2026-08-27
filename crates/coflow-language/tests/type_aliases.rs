use coflow_language::{
    build_schema, parse_modules, CftDimensionInputs, CftErrorCode, CftFile, CftValueType,
    EnumName, ModuleId,
};

fn compile(source: &str) -> Result<coflow_language::CftSchema, coflow_language::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main.cft"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
}

#[test]
fn aliases_expand_in_fields_constants_and_nested_function_results() {
    let schema = compile(
        r#"
            enum Kind { First }
            type Key = Kind;
            type Predicate = fn(int) -> bool;
            type RuleFactory = fn(int) -> Predicate;
            type Scores = {Key: int};
            const EMPTY: Scores = {};

            type Rule {
                create: RuleFactory;
                scores: Scores = EMPTY;
            }
        "#,
    )
    .expect("aliases should expand to their value types");

    let rule = schema.resolve_type("Rule").expect("Rule");
    assert_eq!(
        rule.field("create").expect("create").value_type,
        CftValueType::Function(
            vec![CftValueType::Int],
            Box::new(CftValueType::Function(
                vec![CftValueType::Int],
                Box::new(CftValueType::Bool),
            )),
        )
    );
    assert_eq!(
        rule.field("scores").expect("scores").value_type,
        CftValueType::Dict(
            Box::new(CftValueType::Enum(
                EnumName::new("Kind").expect("enum name")
            )),
            Box::new(CftValueType::Int),
        )
    );
    assert!(schema.resolve_type("Predicate").is_none());
    assert!(schema.resolve_type("RuleFactory").is_none());
}

#[test]
fn aliases_resolve_across_namespaces_and_use_declarations() {
    let modules = parse_modules([
        CftFile::from_source(
            ModuleId::from("shared.cft"),
            r#"
                namespace shared;
                type Identifier = string;
                type MaybeIdentifier = Option<Identifier>;
            "#,
        ),
        CftFile::from_source(
            ModuleId::from("main.cft"),
            r#"
                namespace app;
                use shared::MaybeIdentifier as ImportedIdentifier;
                type LocalIdentifier = ImportedIdentifier;
                type Item { identifier: LocalIdentifier = None; }
            "#,
        ),
    ]);
    let schema = build_schema(&modules, &CftDimensionInputs::default())
        .expect("qualified aliases should resolve");
    assert_eq!(
        schema
            .resolve_type("app::Item")
            .expect("Item")
            .field("identifier")
            .expect("identifier")
            .value_type,
        CftValueType::Option(Box::new(CftValueType::String))
    );
}

#[test]
fn alias_cycles_are_rejected_with_a_dedicated_diagnostic() {
    let diagnostics = compile(
        r#"
            type A = Option<B>;
            type B = Result<int, C>;
            type C = A;
            type Item { value: A; }
        "#,
    )
    .expect_err("alias cycles must be rejected");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::TypeAliasCycle));
}

#[test]
fn aliases_reject_unknown_targets_invalid_dict_keys_and_inheritance() {
    let unknown = compile("type MissingAlias = Missing;")
        .expect_err("unknown alias targets must be rejected");
    assert!(unknown
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::UnknownNamedType));

    let invalid_key = compile("type BadKey = [int]; type Values = {BadKey: string};")
        .expect_err("expanded dict keys must be validated");
    assert!(invalid_key
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidDictKeyType));

    let parent = compile("type Base {} type Alias = Base; type Child : Alias {}")
        .expect_err("aliases cannot stand in for inheritance declarations");
    assert!(parent
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::ParentMustBeType));
}

#[test]
fn type_aliases_do_not_accept_annotations_or_object_modifiers() {
    let annotation = compile("@label(\"identifier\") type Identifier = string;")
        .expect_err("annotations do not target aliases");
    assert!(annotation
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidAnnotationTarget));

    assert!(compile("sealed type Identifier = string;").is_err());
    assert!(compile("abstract type Identifier = string;").is_err());
}
