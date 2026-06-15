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
            codes: &[CftErrorCode::UnexpectedCharacter],
        },
        Case {
            name: "invalid string escape",
            phase: Phase::AddModule,
            source: r#"const NAME = "bad\q";"#,
            codes: &[CftErrorCode::InvalidStringEscape],
        },
        Case {
            name: "unterminated string",
            phase: Phase::AddModule,
            source: r#"const NAME = "bad;"#,
            codes: &[CftErrorCode::UnterminatedString],
        },
        Case {
            name: "invalid int literal",
            phase: Phase::AddModule,
            source: "const N = 999999999999999999999999999999;",
            codes: &[CftErrorCode::InvalidIntLiteral],
        },
        Case {
            name: "invalid float literal",
            phase: Phase::AddModule,
            source: "const N = 1.;",
            codes: &[CftErrorCode::InvalidFloatLiteral],
        },
        Case {
            name: "unexpected token",
            phase: Phase::AddModule,
            source: "type A { check { (true)(); } }",
            codes: &[CftErrorCode::UnexpectedToken],
        },
        Case {
            name: "unexpected eof",
            phase: Phase::AddModule,
            source: "type A { value: int;",
            codes: &[CftErrorCode::UnexpectedEof],
        },
        Case {
            name: "expected identifier",
            phase: Phase::AddModule,
            source: "type 1 {}",
            codes: &[CftErrorCode::ExpectedIdentifier],
        },
        Case {
            name: "expected token",
            phase: Phase::AddModule,
            source: "const N = 1",
            codes: &[CftErrorCode::ExpectedToken],
        },
        Case {
            name: "invalid top level item",
            phase: Phase::AddModule,
            source: "let x = 1;",
            codes: &[CftErrorCode::InvalidTopLevelItem],
        },
        Case {
            name: "invalid chain comparison",
            phase: Phase::AddModule,
            source: "type A { value: int; check { 0 == value == 10; } }",
            codes: &[CftErrorCode::InvalidChainComparison],
        },
        Case {
            name: "check block must be last",
            phase: Phase::AddModule,
            source: "type A { check { true; } value: int; }",
            codes: &[CftErrorCode::CheckBlockMustBeLast],
        },
        Case {
            name: "invalid annotation syntax",
            phase: Phase::AddModule,
            source: "@display(,) type A {}",
            codes: &[CftErrorCode::InvalidAnnotationSyntax],
        },
        Case {
            name: "invalid check statement",
            phase: Phase::AddModule,
            source: "type A { check { true } }",
            codes: &[CftErrorCode::InvalidCheckStatement],
        },
        Case {
            name: "duplicate check block",
            phase: Phase::AddModule,
            source: "type A { check { true; } check { true; } }",
            codes: &[CftErrorCode::DuplicateCheckBlock],
        },
        Case {
            name: "duplicate module",
            phase: Phase::DuplicateModule,
            source: "",
            codes: &[CftErrorCode::DuplicateModule],
        },
        Case {
            name: "duplicate global name",
            phase: Phase::Compile,
            source: "type A {} enum A { X, }",
            codes: &[CftErrorCode::DuplicateGlobalName],
        },
        Case {
            name: "duplicate field name",
            phase: Phase::Compile,
            source: "type A { x: int; x: int; }",
            codes: &[CftErrorCode::DuplicateFieldName],
        },
        Case {
            name: "duplicate enum variant",
            phase: Phase::Compile,
            source: "enum E { A, A, }",
            codes: &[CftErrorCode::DuplicateEnumVariant],
        },
        Case {
            name: "duplicate enum value",
            phase: Phase::Compile,
            source: "enum E { A = 1, B = 1, }",
            codes: &[CftErrorCode::DuplicateEnumValue],
        },
        Case {
            name: "unknown named type",
            phase: Phase::Compile,
            source: "type A { missing: Missing; }",
            codes: &[CftErrorCode::UnknownNamedType],
        },
        Case {
            name: "parent must be type",
            phase: Phase::Compile,
            source: "enum E { A, } type Child : E {}",
            codes: &[CftErrorCode::ParentMustBeType],
        },
        Case {
            name: "unknown const",
            phase: Phase::Compile,
            source: "type A { value: int = Missing; }",
            codes: &[CftErrorCode::UnknownConst],
        },
        Case {
            name: "inheritance cycle",
            phase: Phase::Compile,
            source: "type A : B {} type B : A {}",
            codes: &[CftErrorCode::InheritanceCycle],
        },
        Case {
            name: "inherit sealed type",
            phase: Phase::Compile,
            source: "sealed type Base {} type Child : Base {}",
            codes: &[CftErrorCode::InheritSealedType],
        },
        Case {
            name: "duplicate inherited field",
            phase: Phase::Compile,
            source: "type Base { x: int; } type Child : Base { x: int; }",
            codes: &[CftErrorCode::DuplicateInheritedField],
        },
        Case {
            name: "conflicting type modifiers",
            phase: Phase::Compile,
            source: "abstract sealed type A {}",
            codes: &[CftErrorCode::ConflictingTypeModifiers],
        },
        Case {
            name: "invalid dict key type",
            phase: Phase::Compile,
            source: "type A { values: {[int]: int}; }",
            codes: &[CftErrorCode::InvalidDictKeyType],
        },
        Case {
            name: "invalid default expression",
            phase: Phase::Compile,
            source: "type A { values: [int] = [1]; }",
            codes: &[CftErrorCode::InvalidDefaultExpression],
        },
        Case {
            name: "default type mismatch",
            phase: Phase::Compile,
            source: r#"type A { value: int = "x"; }"#,
            codes: &[CftErrorCode::DefaultTypeMismatch],
        },
        Case {
            name: "default references field",
            phase: Phase::Compile,
            source: "type A { value: int; copy: int = value; }",
            codes: &[CftErrorCode::DefaultReferencesField],
        },
        Case {
            name: "invalid enum value sequence",
            phase: Phase::Compile,
            source: "enum E { Max = 9223372036854775807, Next, }",
            codes: &[CftErrorCode::InvalidEnumValueSequence],
        },
        Case {
            name: "invalid flag enum value",
            phase: Phase::Compile,
            source: "@flag enum F { A = 3, }",
            codes: &[CftErrorCode::InvalidFlagEnumValue],
        },
        Case {
            name: "unknown annotation",
            phase: Phase::Compile,
            source: "@editor type A {}",
            codes: &[CftErrorCode::UnknownAnnotation],
        },
        Case {
            name: "duplicate annotation",
            phase: Phase::Compile,
            source: "@deprecated @deprecated type A {}",
            codes: &[CftErrorCode::DuplicateAnnotation],
        },
        Case {
            name: "annotation without target",
            phase: Phase::Compile,
            source: "@deprecated",
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "invalid annotation target",
            phase: Phase::Compile,
            source: "@keyAsEnum(\"AKey\") enum A { X, }",
            codes: &[CftErrorCode::InvalidAnnotationTarget],
        },
        Case {
            name: "invalid annotation argument",
            phase: Phase::Compile,
            source: "@display(1) type A {}",
            codes: &[CftErrorCode::InvalidAnnotationArgument],
        },
        Case {
            name: "invalid annotated field type",
            phase: Phase::Compile,
            source: "type Pos { x: float; } type A { @expand value: float; }",
            codes: &[CftErrorCode::InvalidAnnotatedFieldType],
        },
        Case {
            name: "struct requires sealed type",
            phase: Phase::Compile,
            source: "@struct type A {}",
            codes: &[CftErrorCode::StructRequiresSealedType],
        },
        Case {
            name: "enum variant default on non-enum",
            phase: Phase::Compile,
            source: "const C = 1; type A { value: int = C.Value; }",
            codes: &[CftErrorCode::EnumVariantOnNonEnum],
        },
        Case {
            name: "unknown enum variant default",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E = E.Missing; }",
            codes: &[CftErrorCode::UnknownEnumVariant],
        },
        Case {
            name: "invalid const value",
            phase: Phase::AddModule,
            source: "const A = null;",
            codes: &[CftErrorCode::InvalidConstValue],
        },
        Case {
            name: "reserved identifier",
            phase: Phase::Compile,
            source: "type A { id: string; }",
            codes: &[CftErrorCode::ReservedIdentifier],
        },
        Case {
            name: "unknown value name",
            phase: Phase::Compile,
            source: "type A { check { missing; } }",
            codes: &[CftErrorCode::UnknownValueName],
        },
        Case {
            name: "unknown field",
            phase: Phase::Compile,
            source: "type Inner {} type A { inner: Inner; check { inner.missing == 1; } }",
            codes: &[CftErrorCode::UnknownField],
        },
        Case {
            name: "type unknown enum variant",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { E.Missing == value; } }",
            codes: &[CftErrorCode::TypeUnknownEnumVariant],
        },
        Case {
            name: "type enum variant on non-enum",
            phase: Phase::Compile,
            source: "type Named {} type A { check { Named.Value == 1; } }",
            codes: &[CftErrorCode::TypeEnumVariantOnNonEnum],
        },
        Case {
            name: "operator type mismatch",
            phase: Phase::Compile,
            source: "type A { value: string; check { value + value == value; } }",
            codes: &[CftErrorCode::OperatorTypeMismatch],
        },
        Case {
            name: "comparison type mismatch",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { value == 1; } }",
            codes: &[CftErrorCode::ComparisonTypeMismatch],
        },
        Case {
            name: "condition must be bool",
            phase: Phase::Compile,
            source: "type A { value: int; check { value; } }",
            codes: &[CftErrorCode::ConditionMustBeBool],
        },
        Case {
            name: "unknown function",
            phase: Phase::Compile,
            source: "type A { check { nope(); } }",
            codes: &[CftErrorCode::UnknownFunction],
        },
        Case {
            name: "function arity mismatch",
            phase: Phase::Compile,
            source: "type A { values: [int]; check { len(values, values) == 0; } }",
            codes: &[CftErrorCode::FunctionArityMismatch],
        },
        Case {
            name: "function arg type mismatch",
            phase: Phase::Compile,
            source: "type A { value: int; check { len(value) == 0; } }",
            codes: &[CftErrorCode::FunctionArgTypeMismatch],
        },
        Case {
            name: "field access on non-object",
            phase: Phase::Compile,
            source: "type A { value: int; check { value.missing == 0; } }",
            codes: &[CftErrorCode::FieldAccessOnNonObject],
        },
        Case {
            name: "index on non-indexable",
            phase: Phase::Compile,
            source: "type A { value: int; check { value[0] == 0; } }",
            codes: &[CftErrorCode::IndexOnNonIndexable],
        },
        Case {
            name: "index type mismatch",
            phase: Phase::Compile,
            source: r#"type A { values: [int]; check { values["x"] == 0; } }"#,
            codes: &[CftErrorCode::IndexTypeMismatch],
        },
        Case {
            name: "invalid is predicate",
            phase: Phase::Compile,
            source: "enum E { A, } type A { check { null is E; } }",
            codes: &[CftErrorCode::InvalidIsPredicate],
        },
        Case {
            name: "quantifier requires collection",
            phase: Phase::Compile,
            source: "type A { value: int; check { all item in value { true; } } }",
            codes: &[CftErrorCode::QuantifierRequiresCollection],
        },
        Case {
            name: "unique unsupported element type",
            phase: Phase::Compile,
            source: "type A { values: [float]; check { unique(values); } }",
            codes: &[CftErrorCode::UniqueUnsupportedElementType],
        },
        Case {
            name: "bitwise requires int or flag enum",
            phase: Phase::Compile,
            source: "enum E { A, } type A { value: E; check { ~value == E.A; } }",
            codes: &[CftErrorCode::BitwiseRequiresIntOrFlagEnum],
        },
        Case {
            name: "shift requires int",
            phase: Phase::Compile,
            source: "type A { value: string; check { value << 1 == 0; } }",
            codes: &[CftErrorCode::ShiftRequiresInt],
        },
        Case {
            name: "regex pattern must be literal",
            phase: Phase::Compile,
            source: r#"const PAT = "x"; type A { value: string; check { matches(value, PAT); } }"#,
            codes: &[CftErrorCode::RegexPatternMustBeLiteral],
        },
        Case {
            name: "invalid regex pattern",
            phase: Phase::Compile,
            source: r#"type A { value: string; check { matches(value, "["); } }"#,
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
fn important_error_code_branches_emit_stable_codes() {
    let branch_cases = [
        Case {
            name: "annotation syntax unterminated args",
            phase: Phase::AddModule,
            source: "@display(",
            codes: &[CftErrorCode::InvalidAnnotationSyntax],
        },
        Case {
            name: "annotation without target in enum",
            phase: Phase::Compile,
            source: "enum E { @display(\"dangling\") }",
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "annotation without target in type",
            phase: Phase::Compile,
            source: "type A { @display(\"dangling\") }",
            codes: &[CftErrorCode::AnnotationWithoutTarget],
        },
        Case {
            name: "invalid annotation target enum variant",
            phase: Phase::Compile,
            source: "enum E { @keyAsEnum(\"EKey\") A, }",
            codes: &[CftErrorCode::InvalidAnnotationTarget],
        },
        Case {
            name: "invalid annotation argument keyAsEnum name arg",
            phase: Phase::Compile,
            source: "@keyAsEnum(AKey) type A { key: string; }",
            codes: &[CftErrorCode::InvalidAnnotationArgument],
        },
        Case {
            name: "invalid annotated field type expand",
            phase: Phase::Compile,
            source: "type A { @expand values: [int]; }",
            codes: &[CftErrorCode::InvalidAnnotatedFieldType],
        },
        Case {
            name: "enum variant default unknown enum",
            phase: Phase::Compile,
            source: "type A { value: int = Missing.Value; }",
            codes: &[CftErrorCode::EnumVariantOnNonEnum],
        },
        Case {
            name: "unknown named type from const symbol",
            phase: Phase::Compile,
            source: "const C = 1; type A { value: C; }",
            codes: &[CftErrorCode::UnknownNamedType],
        },
        Case {
            name: "invalid dict key nullable",
            phase: Phase::Compile,
            source: "type A { values: {string?: int}; }",
            codes: &[CftErrorCode::InvalidDictKeyType],
        },
        Case {
            name: "invalid object default expression",
            phase: Phase::Compile,
            source: "type A { values: {string: int} = {x: 1}; }",
            codes: &[CftErrorCode::InvalidDefaultExpression],
        },
        Case {
            name: "unknown field on dict entry",
            phase: Phase::Compile,
            source: "type A { values: {string: int}; check { all entry in values { entry.other == 0; } } }",
            codes: &[CftErrorCode::UnknownField],
        },
        Case {
            name: "type enum variant on const",
            phase: Phase::Compile,
            source: "const C = 1; type A { check { C.Value == 1; } }",
            codes: &[CftErrorCode::TypeEnumVariantOnNonEnum],
        },
        Case {
            name: "dict index type mismatch",
            phase: Phase::Compile,
            source: "type A { values: {int: string}; check { values[\"x\"] != \"\"; } }",
            codes: &[CftErrorCode::IndexTypeMismatch],
        },
        Case {
            name: "matches first arg type mismatch",
            phase: Phase::Compile,
            source: r#"type A { value: int; check { matches(value, "x"); } }"#,
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
