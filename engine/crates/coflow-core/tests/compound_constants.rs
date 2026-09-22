#![allow(clippy::expect_used, clippy::needless_raw_string_hashes)]

use coflow_core::schema::{
    build_schema, parse_modules, CftStaticValue, CftFile, CftValueType, ModuleId,
};
use coflow_language::diagnostics::CftErrorCode;

fn compile(
    source: &str,
) -> Result<coflow_core::schema::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules)
}

#[test]
fn resolves_schema_guided_compound_constants_and_references() {
    let schema = compile(
        r#"
enum Mode { Primary = 1, Secondary = 2 }
sealed data Stats { hp: int; mode: Mode; }
table Item { name: string; }

const BASE: int = 10;
const VALUES: [int] = [BASE, 20];
const WEIGHTS: {string: int} = { "fire": BASE, "ice": 5 };
const DEFAULT_STATS: Stats = Stats { hp: BASE, mode: Mode::Primary };
const MAYBE_ITEM: Item? = &Item::wooden_sword;
const NO_ITEM: Item? = None;
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
        CftStaticValue::Array(vec![CftStaticValue::Int(10), CftStaticValue::Int(20)])
    );

    let weights = schema.resolve_const("WEIGHTS").expect("WEIGHTS");
    assert_eq!(
        weights.value,
        CftStaticValue::Dictionary(vec![
            (CftStaticValue::String("fire".into()), CftStaticValue::Int(10)),
            (CftStaticValue::String("ice".into()), CftStaticValue::Int(5)),
        ])
    );

    let stats = schema
        .resolve_const("DEFAULT_STATS")
        .expect("DEFAULT_STATS");
    assert!(matches!(
        &stats.value,
        CftStaticValue::Object { fields, .. }
            if fields.len() == 2
                && matches!(fields[0].1, CftStaticValue::Int(10))
                && matches!(fields[1].1, CftStaticValue::Enum { value: 1, .. })
    ));

    let maybe = schema.resolve_const("MAYBE_ITEM").expect("MAYBE_ITEM");
    assert!(matches!(
        &maybe.value,
        CftStaticValue::OptionSome(value)
            if matches!(value.as_ref(), CftStaticValue::RecordReference { key, .. } if key == "wooden_sword")
    ));
    assert!(matches!(
        schema.resolve_const("NO_ITEM").expect("NO_ITEM").value,
        CftStaticValue::OptionNone
    ));
}

#[test]
fn constant_types_are_explicit_and_dependency_cycles_are_rejected() {
    assert!(compile("const NUMBERS = [1, 2, 3];").is_err());
    let schema = compile("const NUMBERS: [int] = [1, 2, 3];").expect("explicit type");
    assert_eq!(
        schema.resolve_const("NUMBERS").expect("NUMBERS").value_type,
        CftValueType::Array(Box::new(CftValueType::Int))
    );

    let diagnostics =
        compile("const A: int = B; const B: int = A;").expect_err("constant cycle must fail");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == CftErrorCode::InvalidConstValue));
}

#[test]
fn object_constants_apply_field_defaults_and_reject_missing_required_fields() {
    let schema = compile(
        r#"
sealed data Stats { hp: int; attack: int = 3; }
const DEFAULT_STATS: Stats = Stats { hp: 10 };
"#,
    )
    .expect("object field defaults apply to constants");
    assert!(matches!(
        &schema.resolve_const("DEFAULT_STATS").expect("DEFAULT_STATS").value,
        CftStaticValue::Object { fields, .. }
            if fields.iter().any(|(name, value)| name.as_str() == "attack" &&
                matches!(value, CftStaticValue::Int(3)))
    ));

    let diagnostics = compile(
        "sealed data Stats { hp: int; attack: int; } const DEFAULT_STATS: Stats = Stats { hp: 10 };",
    )
    .expect_err("missing required object fields must fail during schema compilation");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(
            |diagnostic| diagnostic.code == CftErrorCode::InvalidConstValue
                && diagnostic.message.contains("missing field `attack`")
        ));
}
