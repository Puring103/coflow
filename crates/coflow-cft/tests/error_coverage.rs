#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::needless_raw_string_hashes
)]

mod common;
use common::*;

use std::collections::BTreeSet;

#[derive(Clone, Copy)]
enum Phase {
    AddModule,
    Compile,
    DuplicateModule,
}

struct Case {
    name: &'static str,
    phase: Phase,
    source: &'static str,
    adjacent_valid_source: &'static str,
    codes: &'static [CftErrorCode],
}

fn diagnostics_for(case: &Case) -> CftDiagnostics {
    match case.phase {
        Phase::AddModule => add_source(case.source).unwrap_err(),
        Phase::Compile => compile_one(case.source).unwrap_err(),
        Phase::DuplicateModule => {
            let mut container = CftContainer::new();
            container
                .add_module(ModuleId::from("main"), "type A {}")
                .unwrap();
            container
                .add_module(ModuleId::from("main"), "type B {}")
                .unwrap_err()
        }
    }
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "unexpected character",
            phase: Phase::AddModule,
            source: "type A {} $",
            adjacent_valid_source: "type A {}",
            codes: &[CftErrorCode::UnexpectedCharacter],
        },
        Case {
            name: "invalid string escape",
            phase: Phase::AddModule,
            source: r#"const NAME = "bad\q";"#,
            adjacent_valid_source: r#"const NAME = "bad\n";"#,
            codes: &[CftErrorCode::InvalidStringEscape],
        },
        Case {
            name: "unterminated string",
            phase: Phase::AddModule,
            source: r#"const NAME = "bad;"#,
            adjacent_valid_source: r#"const NAME = "bad";"#,
            codes: &[CftErrorCode::UnterminatedString],
        },
        Case {
            name: "invalid int literal",
            phase: Phase::AddModule,
            source: "const N = 999999999999999999999999999999;",
            adjacent_valid_source: "const N = 9223372036854775807;",
            codes: &[CftErrorCode::InvalidIntLiteral],
        },
        Case {
            name: "invalid float literal",
            phase: Phase::AddModule,
            source: "const N = 1.;",
            adjacent_valid_source: "const N = 1.0;",
            codes: &[CftErrorCode::InvalidFloatLiteral],
        },
        Case {
            name: "unexpected token",
            phase: Phase::AddModule,
            source: "type A { check { (true)(); } }",
            adjacent_valid_source: "type A { check { true; } }",
            codes: &[CftErrorCode::UnexpectedToken],
        },
        Case {
            name: "unexpected eof",
            phase: Phase::AddModule,
            source: "type A { value: int;",
            adjacent_valid_source: "type A { value: int; }",
            codes: &[CftErrorCode::UnexpectedEof],
        },
        Case {
            name: "expected identifier",
            phase: Phase::AddModule,
            source: "type 1 {}",
            adjacent_valid_source: "type A {}",
            codes: &[CftErrorCode::ExpectedIdentifier],
        },
        Case {
            name: "expected token",
            phase: Phase::AddModule,
            source: "const N = 1",
            adjacent_valid_source: "const N = 1;",
            codes: &[CftErrorCode::ExpectedToken],
        },
        Case {
            name: "invalid top level item",
            phase: Phase::AddModule,
            source: "let x = 1;",
            adjacent_valid_source: "const x = 1;",
            codes: &[CftErrorCode::InvalidTopLevelItem],
        },
        Case {
            name: "invalid chain comparison",
            phase: Phase::AddModule,
            source: "type A { value: int; check { 0 == value == 10; } }",
            adjacent_valid_source: "type A { value: int; check { value == 10; } }",
            codes: &[CftErrorCode::InvalidChainComparison],
        },
        Case {
            name: "check block must be last",
            phase: Phase::AddModule,
            source: "type A { check { true; } value: int; }",
            adjacent_valid_source: "type A { value: int; check { true; } }",
            codes: &[CftErrorCode::CheckBlockMustBeLast],
        },
        Case {
            name: "invalid annotation syntax",
            phase: Phase::AddModule,
            source: "@display(,) type A {}",
            adjacent_valid_source: r#"@display("A") type A {}"#,
            codes: &[CftErrorCode::InvalidAnnotationSyntax],
        },
        Case {
            name: "invalid check statement",
            phase: Phase::AddModule,
            source: "type A { check { true } }",
            adjacent_valid_source: "type A { check { true; } }",
            codes: &[CftErrorCode::InvalidCheckStatement],
        },
        Case {
            name: "duplicate check block",
            phase: Phase::AddModule,
            source: "type A { check { true; } check { true; } }",
            adjacent_valid_source: "type A { check { true; } }",
            codes: &[CftErrorCode::DuplicateCheckBlock],
        },
        Case {
            name: "duplicate module",
            phase: Phase::DuplicateModule,
            source: "",
            adjacent_valid_source: "type A {}",
            codes: &[CftErrorCode::DuplicateModule],
        },
        Case {
            name: "duplicate global name",
            phase: Phase::Compile,
            source: "type A {} enum A { X, }",
            adjacent_valid_source: "type A {} enum E { X, }",
            codes: &[CftErrorCode::DuplicateGlobalName],
        },
        Case {
            name: "duplicate field name",
            phase: Phase::Compile,
            source: "type A { x: int; x: int; }",
            adjacent_valid_source: "type A { x: int; y: int; }",
            codes: &[CftErrorCode::DuplicateFieldName],
        },
        Case {
            name: "duplicate enum variant",
            phase: Phase::Compile,
            source: "enum E { A, A, }",
            adjacent_valid_source: "enum E { A, B, }",
            codes: &[CftErrorCode::DuplicateEnumVariant],
        },
        Case {
            name: "duplicate enum value",
            phase: Phase::Compile,
            source: "enum E { A = 1, B = 1, }",
            adjacent_valid_source: "enum E { A = 1, B = 2, }",
            codes: &[CftErrorCode::DuplicateEnumValue],
        },
        Case {
            name: "unknown named type",
            phase: Phase::Compile,
            source: "type A { missing: Missing; }",
            adjacent_valid_source: "type Missing {} type A { missing: Missing; }",
            codes: &[CftErrorCode::UnknownNamedType],
        },
        Case {
            name: "parent must be type",
            phase: Phase::Compile,
            source: "enum E { A, } type Child : E {}",
            adjacent_valid_source: "type Parent {} type Child : Parent {}",
            codes: &[CftErrorCode::ParentMustBeType],
        },
        Case {
            name: "unknown const",
            phase: Phase::Compile,
            source: "type A { value: int = Missing; }",
            adjacent_valid_source: "const Missing = 1; type A { value: int = Missing; }",
            codes: &[CftErrorCode::UnknownConst],
        },
        Case {
            name: "inheritance cycle",
            phase: Phase::Compile,
            source: "type A : B {} type B : A {}",
            adjacent_valid_source: "type A {} type B : A {}",
            codes: &[CftErrorCode::InheritanceCycle],
        },
        Case {
            name: "inherit sealed type",
            phase: Phase::Compile,
            source: "sealed type Base {} type Child : Base {}",
            adjacent_valid_source: "type Base {} type Child : Base {}",
            codes: &[CftErrorCode::InheritSealedType],
        },
        Case {
            name: "duplicate inherited field",
            phase: Phase::Compile,
            source: "type Base { x: int; } type Child : Base { x: int; }",
            adjacent_valid_source: "type Base { x: int; } type Child : Base { y: int; }",
            codes: &[CftErrorCode::DuplicateInheritedField],
        },
        Case {
            name: "conflicting type modifiers",
            phase: Phase::Compile,
            source: "abstract sealed type A {}",
            adjacent_valid_source: "abstract type A {}",
            codes: &[CftErrorCode::ConflictingTypeModifiers],
        },
        Case {
            name: "invalid dict key type",
            phase: Phase::Compile,
            source: "type A { items: {[int]: int}; }",
            adjacent_valid_source: "type A { items: {int: int}; }",
            codes: &[CftErrorCode::InvalidDictKeyType],
        },
        Case {
            name: "invalid default expression",
            phase: Phase::Compile,
            source: "type A { items: [int] = [1]; }",
            adjacent_valid_source: "type A { items: [int] = []; }",
            codes: &[CftErrorCode::InvalidDefaultExpression],
        },
        Case {
            name: "default type mismatch",
            phase: Phase::Compile,
            source: r#"type A { value: int = "x"; }"#,
            adjacent_valid_source: "type A { value: int = 1; }",
            codes: &[CftErrorCode::DefaultTypeMismatch],
        },
        Case {
            name: "default references field",
            phase: Phase::Compile,
            source: "type A { value: int; copy: int = value; }",
            adjacent_valid_source: "const value = 1; type A { copy: int = value; }",
            codes: &[CftErrorCode::DefaultReferencesField],
        },
        Case {
            name: "invalid enum value sequence",
            phase: Phase::Compile,
            source: "enum E { Max = 9223372036854775807, Next, }",
            adjacent_valid_source: "enum E { Max = 9223372036854775807, }",
            codes: &[CftErrorCode::InvalidEnumValueSequence],
        },
        Case {
            name: "invalid flag enum value",
            phase: Phase::Compile,
            source: "@flag enum F { A = 3, }",
            adjacent_valid_source: "@flag enum F { A = 4, }",
            codes: &[CftErrorCode::InvalidFlagEnumValue],
        },
        Case {
            name: "unknown annotation",
            phase: Phase::Compile,
            source: "@editor type A {}",
            adjacent_valid_source: r#"@display("A") type A {}"#,
            codes: &[CftErrorCode::UnknownAnnotation],
        },
        Case {
            name: "duplicate annotation",
            phase: Phase::Compile,
            source: "@deprecated @deprecated type A {}",
            adjacent_valid_source: "@deprecated type A {}",
            codes: &[CftErrorCode::DuplicateAnnotation],
        },
        Case {
            name: "annotation without target",
            phase: Phase::Compile,
            source: "@deprecated",
            adjacent_valid_source: "@deprecated type A {}",
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "invalid annotation target",
            phase: Phase::Compile,
            source: "@idAsEnum(AKey) enum A { X, } enum AKey {}",
            adjacent_valid_source: r#"@idAsEnum(AKey) type A {} enum AKey {}"#,
            codes: &[CftErrorCode::InvalidAnnotationTarget],
        },
        Case {
            name: "invalid annotation argument",
            phase: Phase::Compile,
            source: "@display(1) type A {}",
            adjacent_valid_source: r#"@display("A") type A {}"#,
            codes: &[CftErrorCode::InvalidAnnotationArgument],
        },
        Case {
            name: "invalid annotated field type",
            phase: Phase::Compile,
            source: "type Pos { x: float; } type A { @expand value: float; }",
            adjacent_valid_source: "type Pos { x: float; } type A { @expand value: Pos; }",
            codes: &[CftErrorCode::InvalidAnnotatedFieldType],
        },
        Case {
            name: "struct requires sealed type",
            phase: Phase::Compile,
            source: "@struct type A {}",
            adjacent_valid_source: "@struct sealed type A {}",
            codes: &[CftErrorCode::StructRequiresSealedType],
        },
        Case {
            name: "idAsEnum requires empty enum",
            phase: Phase::Compile,
            source: "@idAsEnum(AKey) type A { key: string; } enum AKey { Existing, }",
            adjacent_valid_source: r#"@idAsEnum(AKey) type A { key: string; } enum AKey {}"#,
            codes: &[CftErrorCode::IdAsEnumRequiresEmptyEnum],
        },
        Case {
            name: "enum variant default on non-enum",
            phase: Phase::Compile,
            source: "const C = 1; type A { value: int = C.Value; }",
            adjacent_valid_source: "enum C { Value, } type A { value: C = C.Value; }",
            codes: &[CftErrorCode::EnumVariantOnNonEnum],
        },
        Case {
            name: "unknown enum variant default",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E = E.Missing; }",
            adjacent_valid_source: "enum E { A, } type A { value: E = E.A; }",
            codes: &[CftErrorCode::UnknownEnumVariant],
        },
        Case {
            name: "invalid const value",
            phase: Phase::AddModule,
            source: "const A = null;",
            adjacent_valid_source: "const A = 1;",
            codes: &[CftErrorCode::InvalidConstValue],
        },
        Case {
            name: "reserved identifier",
            phase: Phase::Compile,
            source: "type A { id: string; }",
            adjacent_valid_source: "type A { key: string; }",
            codes: &[CftErrorCode::ReservedIdentifier],
        },
        Case {
            name: "localized on invalid target",
            phase: Phase::Compile,
            source: "@localized type A { name: string; }",
            adjacent_valid_source: "type A { @localized name: string; }",
            codes: &[CftErrorCode::LocalizedOnInvalidTarget],
        },
        Case {
            name: "localized bucket must be identifier",
            phase: Phase::Compile,
            source: r#"type A { @localized(bucket = "bad-name") name: string; }"#,
            adjacent_valid_source: r#"type A { @localized(bucket = "ui") name: string; }"#,
            codes: &[CftErrorCode::LocalizedBucketNotIdentifier],
        },
        Case {
            name: "singleton on abstract type",
            phase: Phase::Compile,
            source: "@singleton abstract type A {}",
            adjacent_valid_source: "@singleton type A {}",
            codes: &[CftErrorCode::SingletonOnAbstractType],
        },
        Case {
            name: "singleton with idAsEnum",
            phase: Phase::Compile,
            source: "@singleton @idAsEnum(AKey) type A {} enum AKey {}",
            adjacent_valid_source: "@idAsEnum(AKey) type A {} enum AKey {}",
            codes: &[CftErrorCode::SingletonIdAsEnumConflict],
        },
        Case {
            name: "singleton not referenceable",
            phase: Phase::Compile,
            source: "@singleton type S {} type A { s: S; }",
            adjacent_valid_source: "type S {} type A { s: S; }",
            codes: &[CftErrorCode::SingletonNotReferenceable],
        },
        Case {
            name: "unknown value name",
            phase: Phase::Compile,
            source: "type A { check { missing; } }",
            adjacent_valid_source: "type A { value: bool; check { value; } }",
            codes: &[CftErrorCode::UnknownValueName],
        },
        Case {
            name: "unknown field",
            phase: Phase::Compile,
            source: "type Inner {} type A { inner: Inner; check { inner.missing == 1; } }",
            adjacent_valid_source:
                "type Inner { value: int; } type A { inner: Inner; check { inner.value == 1; } }",
            codes: &[CftErrorCode::UnknownField],
        },
        Case {
            name: "type unknown enum variant",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { E.Missing == value; } }",
            adjacent_valid_source: "enum E { A, } type A { value: E; check { E.A == value; } }",
            codes: &[CftErrorCode::TypeUnknownEnumVariant],
        },
        Case {
            name: "type enum variant on non-enum",
            phase: Phase::Compile,
            source: "type Named {} type A { check { Named.Value == 1; } }",
            adjacent_valid_source:
                "enum Named { Value, } type A { value: Named; check { Named.Value == value; } }",
            codes: &[CftErrorCode::TypeEnumVariantOnNonEnum],
        },
        Case {
            name: "operator type mismatch",
            phase: Phase::Compile,
            source: "type A { value: string; check { value + value == value; } }",
            adjacent_valid_source: "type A { value: int; check { value + value == value; } }",
            codes: &[CftErrorCode::OperatorTypeMismatch],
        },
        Case {
            name: "comparison type mismatch",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { value == 1; } }",
            adjacent_valid_source: "enum E { A, } type A { value: E; check { value == E.A; } }",
            codes: &[CftErrorCode::ComparisonTypeMismatch],
        },
        Case {
            name: "condition must be bool",
            phase: Phase::Compile,
            source: "type A { value: int; check { value; } }",
            adjacent_valid_source: "type A { value: int; check { value == 0; } }",
            codes: &[CftErrorCode::ConditionMustBeBool],
        },
        Case {
            name: "unknown function",
            phase: Phase::Compile,
            source: "type A { check { nope(); } }",
            adjacent_valid_source: "type A { items: [int]; check { items.len() == 0; } }",
            codes: &[CftErrorCode::UnknownFunction],
        },
        Case {
            name: "function arity mismatch",
            phase: Phase::Compile,
            source: "type A { items: [int]; check { items.len(items) == 0; } }",
            adjacent_valid_source: "type A { items: [int]; check { items.len() == 0; } }",
            codes: &[CftErrorCode::FunctionArityMismatch],
        },
        Case {
            name: "function arg type mismatch",
            phase: Phase::Compile,
            source: "type A { value: int; check { value.len() == 0; } }",
            adjacent_valid_source: "type A { items: [int]; check { items.len() == 0; } }",
            codes: &[CftErrorCode::FunctionArgTypeMismatch],
        },
        Case {
            name: "field access on non-object",
            phase: Phase::Compile,
            source: "type A { value: int; check { value.missing == 0; } }",
            adjacent_valid_source:
                "type Inner { value: int; } type A { inner: Inner; check { inner.value == 0; } }",
            codes: &[CftErrorCode::FieldAccessOnNonObject],
        },
        Case {
            name: "index on non-indexable",
            phase: Phase::Compile,
            source: "type A { value: int; check { value[0] == 0; } }",
            adjacent_valid_source: "type A { items: [int]; check { items[0] == 0; } }",
            codes: &[CftErrorCode::IndexOnNonIndexable],
        },
        Case {
            name: "index type mismatch",
            phase: Phase::Compile,
            source: r#"type A { items: [int]; check { items["x"] == 0; } }"#,
            adjacent_valid_source: "type A { items: [int]; check { items[0] == 0; } }",
            codes: &[CftErrorCode::IndexTypeMismatch],
        },
        Case {
            name: "invalid is predicate",
            phase: Phase::Compile,
            source: "enum E { A, } type A { check { null is E; } }",
            adjacent_valid_source: "type Item {} type A { maybe: Item?; check { maybe is Item; } }",
            codes: &[CftErrorCode::InvalidIsPredicate],
        },
        Case {
            name: "quantifier requires collection",
            phase: Phase::Compile,
            source: "type A { value: int; check { all item in value { true; } } }",
            adjacent_valid_source:
                "type A { items: [int]; check { all item in items { item >= 0; } } }",
            codes: &[CftErrorCode::QuantifierRequiresCollection],
        },
        Case {
            name: "unique unsupported element type",
            phase: Phase::Compile,
            source: "type A { items: [float]; check { items.unique(); } }",
            adjacent_valid_source: "type A { items: [int]; check { items.unique(); } }",
            codes: &[CftErrorCode::UniqueUnsupportedElementType],
        },
        Case {
            name: "bitwise requires int or flag enum",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { ~value == E.A; } }",
            adjacent_valid_source:
                "@flag enum E { A = 1, } type A { value: E; check { ~value == E.A; } }",
            codes: &[CftErrorCode::BitwiseRequiresIntOrFlagEnum],
        },
        Case {
            name: "shift requires int",
            phase: Phase::Compile,
            source: "type A { value: string; check { value << 1 == 0; } }",
            adjacent_valid_source: "type A { value: int; check { value << 1 == 0; } }",
            codes: &[CftErrorCode::ShiftRequiresInt],
        },
        Case {
            name: "regex pattern must be literal",
            phase: Phase::Compile,
            source: r#"const PAT = "x"; type A { value: string; check { value.matches(PAT); } }"#,
            adjacent_valid_source: r#"type A { value: string; check { value.matches("x"); } }"#,
            codes: &[CftErrorCode::RegexPatternMustBeLiteral],
        },
        Case {
            name: "invalid regex pattern",
            phase: Phase::Compile,
            source: r#"type A { value: string; check { value.matches("["); } }"#,
            adjacent_valid_source: r#"type A { value: string; check { value.matches("[a-z]"); } }"#,
            codes: &[CftErrorCode::InvalidRegexPattern],
        },
    ]
}

#[test]
fn every_error_code_has_a_diagnostic_case() {
    let declared = declared_error_code_names();
    let covered = cases()
        .iter()
        .flat_map(|case| case.codes.iter())
        .map(|code| format!("{code:?}"))
        .collect::<BTreeSet<_>>();

    let missing = declared.difference(&covered).cloned().collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "missing diagnostic coverage for error codes: {missing:?}"
    );
}

#[test]
fn error_code_cases_emit_expected_codes_with_metadata() {
    for case in cases() {
        let diags = diagnostics_for(&case);
        for code in case.codes {
            assert_has_code(&diags, *code);
            let diag = diags
                .diagnostics
                .iter()
                .find(|diag| diag.code == *code)
                .unwrap_or_else(|| panic!("{} did not emit {code:?}", case.name));
            assert_eq!(diag.stage, code.stage(), "{}", case.name);
            assert_eq!(diag.severity, CftSeverity::Error, "{}", case.name);
            assert!(diag.primary.is_some(), "{}", case.name);
        }
    }
}

#[test]
fn error_code_cases_accept_adjacent_valid_inputs() {
    for case in cases() {
        match case.phase {
            Phase::AddModule | Phase::Compile => {
                compile_one(case.adjacent_valid_source).unwrap_or_else(|err| {
                    panic!(
                        "{} adjacent-valid case should compile: {:?}",
                        case.name, err
                    )
                });
            }
            Phase::DuplicateModule => {
                let mut container = CftContainer::new();
                container
                    .add_module(ModuleId::from("main"), case.adjacent_valid_source)
                    .unwrap_or_else(|err| {
                        panic!("{} adjacent-valid case should add: {:?}", case.name, err)
                    });
                container
                    .add_module(ModuleId::from("other"), "type B {}")
                    .unwrap_or_else(|err| {
                        panic!(
                            "{} adjacent-valid second module should add: {:?}",
                            case.name, err
                        )
                    });
                container.compile().unwrap_or_else(|err| {
                    panic!(
                        "{} adjacent-valid modules should compile: {:?}",
                        case.name, err
                    )
                });
            }
        }
    }
}

#[test]
fn important_error_code_branches_emit_stable_codes() {
    let branch_cases = [
        Case {
            name: "annotation syntax unterminated args",
            phase: Phase::AddModule,
            source: "@display(",
            adjacent_valid_source: r#"@display("A") type A {}"#,
            codes: &[CftErrorCode::InvalidAnnotationSyntax],
        },
        Case {
            name: "annotation without target in enum",
            phase: Phase::Compile,
            source: "enum E { @display(\"dangling\") }",
            adjacent_valid_source: r#"enum E { @display("A") A, }"#,
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "annotation without target in type",
            phase: Phase::Compile,
            source: "type A { @display(\"dangling\") }",
            adjacent_valid_source: r#"type A { @display("value") value: int; }"#,
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "invalid annotation target enum variant",
            phase: Phase::Compile,
            source: "enum E { @idAsEnum(EKey) A, } enum EKey {}",
            adjacent_valid_source: r#"@idAsEnum(EKey) type E { value: int; } enum EKey {}"#,
            codes: &[CftErrorCode::InvalidAnnotationTarget],
        },
        Case {
            name: "invalid annotation argument idAsEnum name arg",
            phase: Phase::Compile,
            source: r#"@idAsEnum("AKey") type A { key: string; } enum AKey {}"#,
            adjacent_valid_source: r#"@idAsEnum(AKey) type A { key: string; } enum AKey {}"#,
            codes: &[CftErrorCode::InvalidAnnotationArgument],
        },
        Case {
            name: "idAsEnum requires empty enum",
            phase: Phase::Compile,
            source: "@idAsEnum(AKey) type A { key: string; } enum AKey { Existing, }",
            adjacent_valid_source: r#"@idAsEnum(AKey) type A { key: string; } enum AKey {}"#,
            codes: &[CftErrorCode::IdAsEnumRequiresEmptyEnum],
        },
        Case {
            name: "invalid annotated field type expand",
            phase: Phase::Compile,
            source: "type A { @expand items: [int]; }",
            adjacent_valid_source: "type Value { amount: int; } type A { @expand value: Value; }",
            codes: &[CftErrorCode::InvalidAnnotatedFieldType],
        },
        Case {
            name: "enum variant default unknown enum",
            phase: Phase::Compile,
            source: "type A { value: int = Missing.Value; }",
            adjacent_valid_source: "enum Missing { Value, } type A { value: Missing = Missing.Value; }",
            codes: &[CftErrorCode::EnumVariantOnNonEnum],
        },
        Case {
            name: "unknown named type from const symbol",
            phase: Phase::Compile,
            source: "const C = 1; type A { value: C; }",
            adjacent_valid_source: "type C {} type A { value: C; }",
            codes: &[CftErrorCode::UnknownNamedType],
        },
        Case {
            name: "invalid dict key nullable",
            phase: Phase::Compile,
            source: "type A { items: {string?: int}; }",
            adjacent_valid_source: "type A { items: {string: int}; }",
            codes: &[CftErrorCode::InvalidDictKeyType],
        },
        Case {
            name: "invalid object default expression",
            phase: Phase::Compile,
            source: "type A { items: {string: int} = {x: 1}; }",
            adjacent_valid_source: "type A { items: {string: int} = {}; }",
            codes: &[CftErrorCode::InvalidDefaultExpression],
        },
        Case {
            name: "unknown field on dict entry",
            phase: Phase::Compile,
            source: "type A { items: {string: int}; check { all entry in items { entry.other == 0; } } }",
            adjacent_valid_source: "type A { items: {string: int}; check { all entry in items { entry.value == 0; } } }",
            codes: &[CftErrorCode::UnknownField],
        },
        Case {
            name: "type enum variant on const",
            phase: Phase::Compile,
            source: "const C = 1; type A { check { C.Value == 1; } }",
            adjacent_valid_source: "enum C { Value, } type A { value: C; check { C.Value == value; } }",
            codes: &[CftErrorCode::TypeEnumVariantOnNonEnum],
        },
        Case {
            name: "dict index type mismatch",
            phase: Phase::Compile,
            source: "type A { items: {int: string}; check { items[\"x\"] != \"\"; } }",
            adjacent_valid_source: "type A { items: {int: string}; check { items[1] != \"\"; } }",
            codes: &[CftErrorCode::IndexTypeMismatch],
        },
        Case {
            name: "matches first arg type mismatch",
            phase: Phase::Compile,
            source: r#"type A { value: int; check { value.matches("x"); } }"#,
            adjacent_valid_source: r#"type A { value: string; check { value.matches("x"); } }"#,
            codes: &[CftErrorCode::FunctionArgTypeMismatch],
        },
    ];

    for case in branch_cases {
        let diags = diagnostics_for(&case);
        for code in case.codes {
            assert_has_code(&diags, *code);
        }
    }
}

fn declared_error_code_names() -> BTreeSet<String> {
    let source = include_str!("../src/error.rs");
    let enum_body = source
        .split("pub enum CftErrorCode {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .expect("CftErrorCode enum body");

    enum_body
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#[") {
                None
            } else {
                Some(line.trim_end_matches(',').to_string())
            }
        })
        .collect()
}
