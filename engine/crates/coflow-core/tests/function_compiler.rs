#[path = "../examples/support/compact.rs"]
mod compact;
use compact::{decode_compact, encode_compact};
use coflow_core::{
    schema::{build_schema, parse_modules, CftFile, CftValueType, ModuleId},
    vm::{
        bytecode::{Instruction, Opcode},
        compiler::{compile, CompileContext},
    },
};

fn schema() -> coflow_core::schema::CftSchema {
    build_schema(&parse_modules([CftFile::from_source(ModuleId::from("test"), "table Item { price: int; calc: fn(int) -> int; } table Sword: Item { damage: int; } data Point { x: int; y: int = 0; }")])).expect("schema")
}
#[test]
fn compiles_scalar_control_flow_and_closure_programs() {
    let schema = schema();
    for source in [
        "fn(x: int) -> int { x * 2 + 1 }",
        "fn(x: int) -> float { if x > 0 { x } else { 0.5 } }",
        "fn(values: [int]) -> int { var total: int = 0; for index, value in values { total += value; } total }",
        "fn(limit: int) -> int { var total: int = 0; for i in 0..=limit { if i == 3 { continue; } total += i; } total }",
        "fn(x: int) -> fn() -> int { fn() -> int { x + 1 } }",
        "fn(x: int) -> string { f\"value {x}\" }",
        "fn(x: int?) -> int { if x is Some(v) && v > 0 { v } else { 0 } }",
        "fn(x: Item) -> int { if x is Sword { x.damage } else { x.price } }",
        "fn() -> int { var value: int = 2; while value < 5 { value += 1; } value }",
        "fn(value: int?) -> int? { value? + 1 }",
        "fn() -> Point { Point { x: 2 } }",
        "fn(values: {string: int}) -> [string] { values.keys() }",
    ] {
        let program = compile(&schema, source, "test", CompileContext::default()).unwrap_or_else(|error| panic!("{source}: {error}"));
        program.validate().expect("program");
        assert_eq!(decode_compact(&encode_compact(&program.instructions)).expect("decode"), program.instructions);
    }
}
#[test]
fn rejects_type_scope_control_flow_and_builtin_errors() {
    let schema = schema();
    for source in [
        "fn(x: int) -> bool { x + 1 }",
        "fn(x: int) -> int { x = 1; x }",
        "fn(x: int) -> int { var x: int = 1; x }",
        "fn() -> int { var x: int = 1; var x: int = 2; x }",
        "fn() -> int { if true { 1 } }",
        "fn() -> int { break; 1 }",
        "fn() -> int { continue; 1 }",
        "fn(x: int?) -> int { x? }",
        "fn(x: int?) -> int { if x is Some(v) { 1 } else { v } }",
        "fn(x: int?) -> int { if x is Some(v) { v; } v }",
        "fn(x: int?) -> int { if !(x is Some(v)) { v } else { 0 } }",
        "fn(x: int?) -> int { if x is Some(v) || true { v } else { 0 } }",
        "fn(x: int?) -> int { if (x is Some(v)) == false { v } else { 0 } }",
        "fn(x: Item) -> int { if (x is Sword) == false { x.damage } else { 0 } }",
        "fn(x: Item, other: Item) -> int { var item: Item = x; if item is Sword { item = other; item.damage } else { 0 } }",
        "fn(x: Item, other: Item) -> int { if x is Sword { var x: Item = other; x.damage } else { 0 } }",
        "fn() -> int? { Some(1) }",
        "fn(value: int?) -> bool { value == 1 }",
        "fn() -> int { 2147483648 }",
        "fn() -> int { 1.5 // 2.0 }",
        "fn() -> Item { Item { price: 1 } }",
        "fn() -> int { [1, 2].len(1) }",
        "fn(pattern: string) -> bool { \"x\".matches(pattern) }",
        "fn() -> bool { \"x\".matches(\"(?=x)\") }",
    ] {
        assert!(compile(&schema, source, "test", CompileContext::default()).is_err(), "{source}");
    }
}
#[test]
fn i32_minimum_and_float_return_are_distinct_from_positive_overflow() {
    let schema = schema();
    let program = compile(
        &schema,
        "fn() -> int { -2147483648 }",
        "min",
        CompileContext::default(),
    )
    .expect("minimum");
    assert_eq!(program.result, CftValueType::Int);
    // 数值常量现在直接内联进指令字：校验 i32::MIN 以位模式无损编码。
    assert!(program.instructions.iter().any(|instruction| {
        instruction.opcode() == Some(Opcode::Constant)
            && instruction.flags() == 1
            && instruction.index() as i32 == i32::MIN
    }));
}
#[test]
fn compact_encoding_handles_large_operands_flags_and_truncation() {
    let instructions = [
        Instruction::new(Opcode::Binary, 255, 0, 1, 0),
        Instruction::new(Opcode::Binary, 256, 65535, 1000, 17),
        Instruction::indexed(Opcode::Jump, 0, u32::MAX),
    ];
    let encoded = encode_compact(&instructions);
    assert_eq!(encoded.len(), 28);
    assert_eq!(decode_compact(&encoded).expect("round trip"), instructions);
    for len in [1, 2, 3, 5, 7, 8, 9, 15, 17, 27] {
        assert!(decode_compact(&encoded[..len]).is_err(), "{len}");
    }
    assert!(decode_compact(&[254, 0, 0, 0]).is_err());
    assert!(decode_compact(&[255, 1, 0, 0]).is_err());
    for instruction in instructions {
        assert_eq!(
            Instruction::from_le_bytes(instruction.to_le_bytes()),
            instruction
        );
    }
}

#[test]
fn long_infix_chain_compiles_on_a_normal_stack() {
    // Windows 默认线程栈大小，禁止用扩大栈掩盖递归降低的问题。
    std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
        let schema = schema();
        let parameters = (0..128).map(|i| format!("a{i}: int")).collect::<Vec<_>>().join(",");
        let sum = (0..128).map(|i| format!("a{i}")).collect::<Vec<_>>().join("+");
        compile(&schema, &format!("fn({parameters}) -> int {{ {sum} }}"), "long", CompileContext::default()).unwrap();
        for operator in ["&&", "||"] {
            let chain = std::iter::repeat_n("a", 128).collect::<Vec<_>>().join(operator);
            compile(&schema, &format!("fn(a: bool) -> bool {{ {chain} }}"), "logical", CompileContext::default()).unwrap();
        }
    }).unwrap().join().unwrap();
}
