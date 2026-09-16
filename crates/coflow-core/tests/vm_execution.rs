use coflow_core::{
    contract::Contract,
    runtime::{HostValue, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::executor::ExecutionLimits,
};
use std::sync::Arc;
fn make_runtime(body: &str, signature: &str) -> Arc<Runtime> {
    let source = format!("table Rule {{ value: int = 7; run: {signature} => {{ {body} }}; }}");
    let modules = parse_modules([CftFile::from_source(ModuleId::from("test"), source)]);
    let schema = build_schema(&modules).expect("schema");
    let contract = Arc::new(Contract::new(schema).expect("contract"));
    let mut builder = RuntimeBuilder::new(contract);
    builder.add_text("rule: Rule {}", Some("test.cfd"));
    builder.build().runtime.expect("runtime")
}
fn call(
    runtime: &Runtime,
    args: &[HostValue],
) -> Result<HostValue, coflow_core::vm::ExecutionError> {
    let rule = runtime.record("Rule", "rule")?;
    let run = runtime.field(rule, "run")?;
    runtime.invoke(run, args, ExecutionLimits::default())
}
#[test]
fn executes_checked_arithmetic_and_float_promotion() {
    for (body, expected) in [
        ("2 + 3 * 4", 14),
        ("-7 // 3", -2),
        ("-7 % 3", -1),
        ("2 ** 3 ** 2", 512),
        ("2147483647 << 1", -2),
        ("self.value + 1", 8),
        ("-2147483648", i32::MIN),
    ] {
        let runtime = make_runtime(body, "fn() -> int");
        assert!(
            matches!(call(&runtime,&[]).expect("result"),HostValue::Int(actual)if actual==expected),
            "{body}"
        );
    }
    let runtime = make_runtime("7 / 2", "fn() -> float");
    assert!(matches!(call(&runtime,&[]).expect("result"),HostValue::Float(value)if value==3.5));
}
#[test]
fn loops_use_mutable_locals_and_closest_break_continue() {
    let body="var total: int = 0; for value in 0..=10 { if value == 3 { continue; } if value == 8 { break; } total += value; } while total < 30 { total += 1; } total";
    let runtime = make_runtime(body, "fn() -> int");
    assert!(matches!(
        call(&runtime, &[]).expect("result"),
        HostValue::Int(30)
    ));
    let runtime = make_runtime(
        "var count: int = 0; for value in 2147483647..=2147483647 { count += 1; } count",
        "fn() -> int",
    );
    assert!(matches!(
        call(&runtime, &[]).expect("result"),
        HostValue::Int(1)
    ));
}
#[test]
fn branches_short_circuit_and_optional_propagation() {
    let runtime = make_runtime(
        "if false && 1 // 0 > 0 { 0 } else if true || 1 // 0 > 0 { 7 } else { 9 }",
        "fn() -> int",
    );
    assert!(matches!(
        call(&runtime, &[]).expect("result"),
        HostValue::Int(7)
    ));
    let runtime = make_runtime(
        "if value is Some(number) && number > 0 { number } else { 0 }",
        "fn(value: int?) -> int",
    );
    assert!(matches!(
        call(&runtime, &[HostValue::None]).expect("None"),
        HostValue::Int(0)
    ));
    assert!(matches!(
        call(&runtime, &[HostValue::Int(3)]).expect("Some"),
        HostValue::Int(3)
    ));
    let runtime = make_runtime("value? + 1", "fn(value: int?) -> int?");
    assert!(matches!(
        call(&runtime, &[HostValue::None]).expect("None"),
        HostValue::None
    ));
    assert!(matches!(
        call(&runtime, &[HostValue::Int(3)]).expect("Some"),
        HostValue::Int(4)
    ));
}
#[test]
fn arrays_dictionaries_and_unicode_strings_execute() {
    for (body, expected) in [
        (
            "var total: int = 0; for index, value in [2, 4, 6] { total += index + value; } total",
            15,
        ),
        (
            "var total: int = 0; for key, value in {\"z\": 2, \"a\": 3} { total += value; } total",
            5,
        ),
        ("\"😀e\\u{301}\".len()", 3),
        ("[2, 4, 6].sum()", 12),
    ] {
        let runtime = make_runtime(body, "fn() -> int");
        assert!(
            matches!(call(&runtime,&[]).expect("result"),HostValue::Int(actual)if actual==expected),
            "{body}"
        );
    }
    let runtime = make_runtime("\"😀e\\u{301}\"[0]", "fn() -> string");
    assert!(matches!(call(&runtime,&[]).expect("result"),HostValue::String(value)if value=="😀"));
}
#[test]
fn returned_closure_preserves_captured_snapshot_and_self() {
    let runtime=make_runtime("var value: int = 2; var read: fn() -> int = fn() -> int { value + self.value }; value = 9; read","fn() -> fn() -> int");
    let HostValue::Existing { value, .. } = call(&runtime, &[]).expect("closure") else {
        panic!("closure");
    };
    runtime.collect().expect("collect");
    assert!(matches!(
        runtime
            .invoke(value, &[], ExecutionLimits::default())
            .expect("call closure"),
        HostValue::Int(9)
    ));
    runtime.release_value(value).expect("release");
    runtime.collect().expect("collect");
    assert!(runtime.value(value).is_err());
}
#[test]
fn template_reads_run_bytecode_and_copy_plain_text() {
    let runtime = make_runtime(
        "var name: string = \"值\"; f\"{name}: {self.value + 1}\"",
        "fn() -> string",
    );
    assert!(
        matches!(call(&runtime,&[]).expect("result"),HostValue::String(value)if value=="值: 8")
    );
}
#[test]
fn execution_faults_have_source_spans_and_do_not_poison_runtime() {
    let runtime = make_runtime("100 // divisor", "fn(divisor: int) -> int");
    let error = call(&runtime, &[HostValue::Int(0)]).expect_err("division");
    assert!(matches!(error,coflow_core::vm::ExecutionError::Fault{span,..}if span.end>span.start));
    assert!(matches!(
        call(&runtime, &[HostValue::Int(4)]).expect("next call"),
        HostValue::Int(25)
    ));
    let runtime = make_runtime("while true {} 1", "fn() -> int");
    let function = runtime
        .field(runtime.record("Rule", "rule").expect("record"), "run")
        .expect("function");
    let error = runtime
        .invoke(
            function,
            &[],
            ExecutionLimits {
                max_work: 100,
                ..ExecutionLimits::default()
            },
        )
        .expect_err("budget");
    assert!(error.to_string().contains("预算"));
}
#[test]
fn higher_order_collections_use_normal_calls_and_short_circuit() {
    for(body,expected)in [
        ("[1, 2, 3].map(fn(x: int) -> int { x * 2 }).sum()",12),
        ("[1, 2, 3].filter(fn(x: int) -> bool { x > 1 }).sum()",5),
        ("[1, 2, 3].fold(10, fn(total: int, x: int) -> int { total + x })",16),
        ("{\"z\": 1, \"a\": 2}.map(fn(key: string, value: int) -> int { value + 1 }).sum()",5),
        ("{\"z\": 1, \"a\": 2}.filter(fn(key: string, value: int) -> bool { key == \"a\" }).values().sum()",2),
        ("var values: [int] = []; values.fold(7, fn(total: int, value: int) -> int { total + value })",7),
    ]{
        let runtime=make_runtime(body,"fn() -> int");assert!(matches!(call(&runtime,&[]).expect("result"),HostValue::Int(actual)if actual==expected),"{body}");
    }
    for (body, expected) in [
        (
            "[0, 1].any(fn(x: int) -> bool { x == 0 || 1 // 0 == 0 })",
            true,
        ),
        (
            "[0, 1].all(fn(x: int) -> bool { x != 0 && 1 // 0 == 0 })",
            false,
        ),
        (
            "var values: [int] = []; values.all(fn(x: int) -> bool { false })",
            true,
        ),
        (
            "var values: [int] = []; values.any(fn(x: int) -> bool { true })",
            false,
        ),
    ] {
        let runtime = make_runtime(body, "fn() -> bool");
        assert!(
            matches!(call(&runtime,&[]).expect("result"),HostValue::Bool(actual)if actual==expected),
            "{body}"
        );
    }
}

#[test]
fn builtin_matrix_handles_empty_values_unicode_and_numeric_boundaries() {
    for body in [
        "\"\\u{2003}\\n\".isBlank()",
        "\"😀abc\".startsWith(\"😀\")",
        "\"😀abc\".endsWith(\"bc\")",
        "\"abc123\".matches(\"[0-9]+\")",
        "\"abc\".contains(\"b\")",
        "[true, false].isUnique()",
        "![1,1].isUnique()",
        "[1,2,2].isSorted()",
        "![1,2,2].isStrictlySorted()",
        "[1,2].intersects([2,3])",
        "[1,2].isDisjoint([3,4])",
        "[1,2].isSubsetOf([2,1,3])",
        "[1,2,3].isSupersetOf([2])",
        "{true: 1}.containsKey(true)",
        "{true: 1}.containsValue(1)",
        "{true: 1}.contains(true)",
        "if \"+2147483647\".parseInt() is Some(value) { value == 2147483647 } else { false }",
        "\"2147483648\".parseInt().isNone()",
        "\" 1\".parseInt().isNone()",
        "if \"1e2\".parseFloat() is Some(value) { value == 100.0 } else { false }",
        "\"-inf\".parseFloat().isSome()",
        "\"1.0x\".parseFloat().isNone()",
        "(-3).abs() == 3",
        "(-1.5).abs() == 1.5",
        "1.0.approxEqual(1.1, 0.11)",
        "!inf.isFinite()",
        "(0.0 / 0.0) != (0.0 / 0.0)",
        "![0.0 / 0.0].isSorted()",
        "![0.0 / 0.0].isStrictlySorted()",
        "[1,3,2].min() == 1",
        "[1,3,2].max() == 3",
        "(-1.9).int() == -1",
        "7.float() == 7.0",
        "var values: [float] = []; values.sum() == 0.0",
        "true.string() == \"true\"",
        "(-0.0).string() == \"-0\"",
    ] {
        let runtime = make_runtime(body, "fn() -> bool");
        assert!(
            matches!(
                call(&runtime, &[]).unwrap_or_else(|error| panic!("{body}: {error}")),
                HostValue::Bool(true)
            ),
            "{body}"
        );
    }
    for body in [
        "(-2147483648).abs()",
        "2147483647 + 1",
        "1 << 32",
        "1 >> -1",
        "1 // 0",
        "(-2147483648) // -1",
        "inf.int()",
        "[2147483647,1].sum()",
        "var values: [int] = []; values.min()",
    ] {
        let runtime = make_runtime(body, "fn() -> int");
        assert!(call(&runtime, &[]).is_err(), "{body}");
    }
}
