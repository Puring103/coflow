#![allow(clippy::expect_used, clippy::unwrap_used)]
use coflow_language::cft::{
    build_schema, parse_modules, CftConstValue, CftFile, CftValueType, ModuleId,
};
fn compile(
    source: &str,
) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        source,
    )]))
}
#[test]
fn untyped_floating_literals_preserve_double_precision_and_range() {
    let schema = compile("const RATE = 0.1; const LARGE = 1e100;").unwrap();
    for (name, value) in [("RATE", 0.1), ("LARGE", 1e100)] {
        let constant = schema.resolve_const(name).unwrap();
        assert_eq!(constant.value_type, CftValueType::Double);
        assert_eq!(constant.value, CftConstValue::Double(value));
    }
}

#[test]
fn numeric_constants_use_declared_width_and_widen_without_narrowing() {
    let schema = compile("const I: int = -2147483648; const L: long = 9223372036854775807; const F: float = 0.1; const D: double = 0.1; const W: long = I; const X: double = W; const Inferred = 2147483648;").unwrap();
    assert_eq!(
        schema.resolve_const("L").unwrap().value,
        CftConstValue::Long(i64::MAX)
    );
    assert_eq!(
        schema.resolve_const("F").unwrap().value,
        CftConstValue::Float(f64::from(0.1_f32))
    );
    assert_eq!(
        schema.resolve_const("D").unwrap().value,
        CftConstValue::Double(0.1)
    );
    assert_eq!(
        schema.resolve_const("W").unwrap().value_type,
        CftValueType::Long
    );
    assert_eq!(
        schema.resolve_const("Inferred").unwrap().value_type,
        CftValueType::Long
    );
    for source in [
        "const I: int = 2147483648;",
        "const I: int = -2147483649;",
        "const F: float = 1e100;",
        "const L: long = 1; const I: int = L;",
        "const D: double = 1.0; const F: float = D;",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}
#[test]
fn numeric_types_work_in_collections_options_aliases_and_checks() {
    compile("type Wide = long; const M: {long: double} = {2147483648: 0.1}; table T { i: int = 1; l: Wide = iValue; f: float = 0.1; d: double = 0.1; list: [double] = [1, 2.0]; option: Option<long> = Some(2147483648); check Numeric { i + l > 0; l + f > 0.0; f + d > 0.0; l & i == 1; } } const iValue: int = 1;").unwrap();
    assert!(compile("table T { i: int; f: float; check Invalid { i & f == 1; } }").is_err());
}
#[test]
fn enums_default_to_int_and_int64_is_explicit() {
    assert!(compile("enum E { A = 2147483648 }").is_err());
    assert!(compile("enum E { A = 2147483647, B }").is_err());
    let schema = compile("enum Small { A = -2147483648, B = 2147483647 } @int64 enum Wide { A = -9223372036854775808, B = 9223372036854775807 }").unwrap();
    assert!(!schema.resolve_enum("Small").unwrap().is_int64);
    assert!(schema.resolve_enum("Wide").unwrap().is_int64);
    assert!(compile("@int64 enum Reset { High = 9223372036854775807, Low = 0, Next }").is_ok());
    assert!(compile("@int64(64) enum E { A }").is_err());
    assert!(compile("@int64 table T { i: int; }").is_err());
}
#[test]
fn flags_synthesize_members_from_actual_bits_including_empty_and_single_bit() {
    for (members, mask) in [("", 0), ("Read = 1", 1), ("Read = 1, Execute = 8", 9)] {
        let schema = compile(&format!("@flag enum Permission {{ {members} }} table T {{ empty: Permission = Permission::None; selected: Permission = Permission::Everything; check Bits {{ selected == Permission::Everything; }} }}")).unwrap();
        assert_eq!(schema.enum_variant_value("Permission", "None"), Some(0));
        assert_eq!(
            schema.enum_variant_value("Permission", "Everything"),
            Some(mask)
        );
    }
    for member in ["None = 0", "Everything = 1"] {
        assert!(compile(&format!("@flag enum E {{ {member} }}")).is_err());
    }
    assert!(compile("@flag enum E { High = 2147483648 }").is_err());
    assert!(compile("@flag @int64 enum E { High = 4611686018427387904 }").is_ok());
}
