use coflow_core::{
    contract::Contract,
    runtime::{HostValue, OptimizationProfile, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::{
        compiler::{compile, CompileContext},
        ExecutionLimits,
    },
};
use std::sync::Arc;

fn run(body: &str) -> i32 {
    let source = format!("data Point {{ x: int; y: int = 2; read: fn() -> int => {{ self.x + self.y }}; callbacks: [fn() -> int]; }} table Rule {{ run: fn() -> int => {{ {body} }}; }}");
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("builders"),
        source,
    )]))
    .unwrap();
    let contract = Arc::new(Contract::new(schema).unwrap());
    let mut returned = None;
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(contract.clone());
        builder.optimization_profile(profile);
        builder.add_text("a: Rule {}", None);
        let result = builder.build();
        let runtime = result
            .runtime
            .unwrap_or_else(|error| panic!("{error}: {:?}", result.diagnostics));
        let function = runtime
            .field(runtime.record("Rule", "a").unwrap(), "run")
            .unwrap();
        let HostValue::Int(value) = runtime
            .invoke(function, &[], ExecutionLimits::default())
            .unwrap()
        else {
            panic!("expected int");
        };
        runtime.collect().unwrap();
        assert_eq!(
            runtime.dynamic_value_count().unwrap(),
            0,
            "无外部返回根时构造缓冲与 self 环应回收"
        );
        if let Some(previous) = returned {
            assert_eq!(previous, value);
        }
        returned = Some(value);
    }
    returned.unwrap()
}

#[test]
fn constructs_data_with_defaults_branches_and_self_bound_functions() {
    assert_eq!(run("var point: Point = build Point as b { if true { b.x = 7; } else { b.x = 9; } b.read = fn() -> int { self.x * self.y }; }; point.read()"), 14);
    assert_eq!(run("var original: Point = Point { x: 3 }; var updated: Point = build (original) as b { b.x = b.x + 4; }; original.x * 10 + updated.x"), 37);
    assert_eq!(run("var point: Point = build Point as b { b.x = 5; b.callbacks = [fn() -> int { self.x }, fn() -> int { self.y }]; }; point.callbacks[0]() * 10 + point.callbacks[1]()"), 52);
}

#[test]
fn constructs_and_updates_collections_without_changing_inputs() {
    assert_eq!(run("var source: [int] = [1, 2, 3]; var result: [int] = build (source) as b { b[1] = 8; b.remove(0); for i in 0..3 { b.append(i); } }; source[0] * 100 + result[0] * 10 + result.len()"), 185);
    assert_eq!(run("var values: {int: int} = build {int: int} as b { b[1] = 10; b[2] = 20; b[1] = 11; b.remove(1); b[1] = 12; b.remove(99); }; values.keys()[0] * 100 + values.keys()[1] * 10 + values[1]"), 222);
}

#[test]
fn rejects_aliases_captures_uninitialized_fields_and_nested_mutation() {
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("invalid"),
        "data Point { x: int; y: int = 0; } table Item { x: int; }",
    )]))
    .unwrap();
    for source in [
        "fn() -> Point { build Point as b { if true { b.x = 1; } } }",
        "fn() -> Point { build Point as b { b.x = b.x + 1; } }",
        "fn() -> [int] { build [int] as b { var alias: [int] = b; } }",
        "fn() -> [int] { build [int] as b { var captured: fn() -> int = fn() -> int { b.len() }; } }",
        "fn() -> [int] { build [int] as b { return b; } }",
        "fn() -> [int] { build [int] as b { b = [1]; } }",
        "fn() -> Item { build Item as b { b.x = 1; } }",
        "fn() -> [Point] { build [Point] as b { b.append(Point { x: 1 }); b[0].x = 2; } }",
    ] {
        assert!(compile(&schema, source, "invalid", CompileContext::default()).is_err(), "{source}");
    }
}

#[test]
fn control_transfer_discards_unfrozen_builders() {
    assert_eq!(run("var result: int = 0; for i in 0..100 { var ignored: [int] = build [int] as b { b.append(i); if i < 99 { continue; } break; }; result += ignored.len(); } result"), 0);
    assert_eq!(
        run("var ignored: Point = build Point as b { return 13; }; ignored.x"),
        13
    );
}

#[test]
fn unpublished_self_bindings_cannot_escape_or_execute() {
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("invalid"),
        "data Point { x: int = 1; read: fn() -> int => { self.x }; }",
    )]))
    .unwrap();
    for source in [
        "fn() -> Point { build Point as b { b.x = b.read(); } }",
        "fn() -> int { var escaped: fn() -> int = fn() -> int { 0 }; for i in 0..2 { var ignored: Point = build Point as b { escaped = b.read; if i == 0 { continue; } }; } escaped() }",
        "fn() -> int { var escaped: fn() -> int = fn() -> int { 0 }; var ignored: Point = build Point as b { escaped = b.read; return escaped(); }; 0 }",
    ] {
        assert!(compile(&schema, source, "invalid", CompileContext::default()).is_err(), "{source}");
    }
    assert_eq!(run("var result: int = 0; var previous: fn() -> int = fn() -> int { 0 }; for i in 0..3 { var point: Point = build Point as b { b.x = previous(); }; previous = point.read; result += previous(); } result"), 12);
}

#[test]
fn builder_and_self_bindings_survive_collection_pressure() {
    assert_eq!(run("var point: Point = build Point as b { b.x = 40; for i in 0..3000 { var temporary: [int] = [i]; } }; point.read()"), 42);
    assert_eq!(run("var values: [int] = build [int] as b { for i in 0..3000 { var temporary: [int] = [i]; if i == 2999 { b.append(i); } } }; values[0]"), 2999);
}


#[test]
fn specialized_dictionary_freeze_copy_and_borrowed_keys_preserve_results() {
    assert_eq!(run(r#"var original: {int: int} = build {int: int} as b { b[-2] = 4; b[0] = 7; b[-1] = 8; }; var edited: {int: int} = build (original) as b { b.remove(0); b[0] = 9; }; original[0] * 100 + edited.keys()[1] * 10 + edited[0]"#), 699);
    assert_eq!(run(r#"var key: string = "a"; for i in 0..2 { key = key + "b"; } var values: {string: int} = build {string: int} as b { b[key] = 17; }; values[key]"#), 17);
}
