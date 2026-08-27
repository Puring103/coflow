use coflow_language::{
    build_schema, parse_modules, CftConstValue, CftErrorCode, CftFile, CftValueType, ModuleId,
};

fn compile(source: &str) -> Result<coflow_language::CftSchema, coflow_language::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &Default::default())
}

#[test]
fn resolves_schema_guided_compound_constants_and_references() {
    let schema = compile(
        r#"
enum Mode { Primary = 1, Secondary = 2 }
sealed type Stats { hp: int; mode: Mode; }
type Item { name: string; }

const BASE: int = 10;
const VALUES: [int] = [BASE, 20];
const WEIGHTS: {string: int} = { "fire": BASE, "ice": 5 };
const DEFAULT_STATS: Stats = { hp: BASE, mode: Mode::Primary };
const MAYBE_ITEM: Option<&Item> = Some(&Item::wooden_sword);
const NO_ITEM: Option<&Item> = None;
const RESULT: Result<int, string> = Ok(BASE);
"#,
    )
    .expect("compound constants compile");

    let values = schema.resolve_const("VALUES").expect("VALUES");
    assert_eq!(
        values.value_type,
        CftValueType::Array(Box::new(CftValueType::Int))
    );
    assert_eq!(
        values.value,
        CftConstValue::Array(vec![CftConstValue::Int(10), CftConstValue::Int(20)])
    );

    let weights = schema.resolve_const("WEIGHTS").expect("WEIGHTS");
    assert_eq!(
        weights.value,
        CftConstValue::Dictionary(vec![
            (CftConstValue::String("fire".into()), CftConstValue::Int(10)),
            (CftConstValue::String("ice".into()), CftConstValue::Int(5)),
        ])
    );

    let stats = schema.resolve_const("DEFAULT_STATS").expect("DEFAULT_STATS");
    assert!(matches!(
        &stats.value,
        CftConstValue::Object { fields, .. }
            if fields.len() == 2
                && matches!(fields[0].1, CftConstValue::Int(10))
                && matches!(fields[1].1, CftConstValue::Enum { value: 1, .. })
    ));

    let maybe = schema.resolve_const("MAYBE_ITEM").expect("MAYBE_ITEM");
    assert!(matches!(
        &maybe.value,
        CftConstValue::OptionSome(value)
            if matches!(value.as_ref(), CftConstValue::RecordReference { key, .. } if key == "wooden_sword")
    ));
    assert!(matches!(
        schema.resolve_const("NO_ITEM").expect("NO_ITEM").value,
        CftConstValue::OptionNone
    ));
    assert!(matches!(
        schema.resolve_const("RESULT").expect("RESULT").value,
        CftConstValue::ResultOk(_)
    ));
}

#[test]
fn infers_unambiguous_constant_types_and_rejects_dependency_cycles() {
    let schema = compile("const NUMBERS = [1, 2, 3];").expect("array type inferred");
    assert_eq!(
        schema.resolve_const("NUMBERS").expect("NUMBERS").value_type,
        CftValueType::Array(Box::new(CftValueType::Int))
    );

    let diagnostics = compile("const A: int = B; const B: int = A;")
        .expect_err("constant cycle must fail");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidConstValue));
}

#[test]
fn object_constants_apply_field_defaults_and_reject_missing_required_fields() {
    let schema = compile(
        r#"
sealed type Stats { hp: int; attack: int = 3; }
const DEFAULT_STATS: Stats = { hp: 10 };
"#,
    )
    .expect("object field defaults apply to constants");
    assert!(matches!(
        &schema.resolve_const("DEFAULT_STATS").expect("DEFAULT_STATS").value,
        CftConstValue::Object { fields, .. }
            if fields.iter().any(|(name, value)| name.as_str() == "attack" &&
                matches!(value, CftConstValue::Int(3)))
    ));

    let diagnostics = compile(
        "sealed type Stats { hp: int; attack: int; } const DEFAULT_STATS: Stats = { hp: 10 };",
    )
    .expect_err("missing required object fields must fail during schema compilation");
    assert!(diagnostics.diagnostics.iter().any(|diagnostic|
        diagnostic.code == CftErrorCode::InvalidConstValue &&
        diagnostic.message.contains("missing field `attack`")));
}
