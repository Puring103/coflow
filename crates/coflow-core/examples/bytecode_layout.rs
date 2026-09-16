//! 复现字节码的完整程序体积和解码成本。两种存储编码使用相同逻辑程序。
use coflow_core::{
    schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId},
    vm::{
        bytecode::{decode_compact, encode_compact, Instruction, Program},
        compiler::{compile, CompileContext},
    },
};
use std::{hint::black_box, time::Instant};
fn programs<'a>(program: &'a Program, out: &mut Vec<&'a Program>) {
    out.push(program);
    for child in &program.closures {
        programs(&child.program, out);
    }
}
fn main() {
    let schema = build_schema(
        &parse_modules([CftFile::from_source(
            ModuleId::from("benchmark"),
            "@Host singleton Service { read: fn(int) -> int; }",
        )]),
        &CftDimensionInputs::default(),
    )
    .unwrap();
    let wide = format!(
        "fn() -> int {{ [{}].sum() }}",
        (0..400)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let constants = format!(
        "fn() -> int {{ var value: int = 0; {} value }}",
        (0..400)
            .map(|n| format!("value += {n};"))
            .collect::<String>()
    );
    let cases=[
        ("arithmetic","fn() -> int { var total: int = 0; for i in 0..1000 { total += i; } total }"),
        ("wide-registers",wide.as_str()),("large-constants",constants.as_str()),
        ("closures","fn(x: int) -> int { var apply: fn(int) -> int = fn(y: int) -> int { x + y }; [1,2,3,4].map(apply).sum() }"),
        ("templates","fn(x: int) -> string { var message: fstring = f\"value: {x}, doubled: {x * 2}\"; message + message }"),
        ("host-calls","fn(x: int) -> int { Service.read(x) + Service.read(x * 2) }"),
        ("collections","fn() -> [int] { [1,2,3,4,5].filter(fn(x: int) -> bool { x > 2 }).map(fn(x: int) -> int { x * x }) }"),
    ];
    println!("| program | instructions | registers | fixed total bytes | compact total bytes | extension % | fixed decode us | compact decode us |");
    println!("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |");
    for (name, source) in cases {
        let program = compile(&schema, source, name, CompileContext::default()).unwrap();
        let mut all = Vec::new();
        programs(&program, &mut all);
        let instructions = all.iter().map(|p| p.instructions.len()).sum::<usize>();
        let total = bincode::serialize(&program).unwrap().len();
        let fixed = all
            .iter()
            .map(|p| {
                p.instructions
                    .iter()
                    .flat_map(|i| i.to_le_bytes())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let compact = all
            .iter()
            .map(|p| encode_compact(&p.instructions))
            .collect::<Vec<_>>();
        for (p, bytes) in all.iter().zip(&compact) {
            assert_eq!(decode_compact(bytes).unwrap(), p.instructions);
        }
        let compact_size = compact.iter().map(Vec::len).sum::<usize>();
        let extensions = (compact_size - 4 * instructions) / 8;
        let start = Instant::now();
        for _ in 0..1000 {
            for bytes in &fixed {
                black_box(
                    bytes
                        .chunks_exact(8)
                        .map(|b| Instruction::from_le_bytes(b.try_into().unwrap()))
                        .collect::<Vec<_>>(),
                );
            }
        }
        let fixed_time = start.elapsed().as_micros();
        let start = Instant::now();
        for _ in 0..1000 {
            for bytes in &compact {
                black_box(decode_compact(black_box(bytes)).unwrap());
            }
        }
        let compact_time = start.elapsed().as_micros();
        println!("| {name} | {instructions} | {} | {total} | {} | {:.1} | {fixed_time} | {compact_time} |",program.registers.len(),total-8*instructions+compact_size,100.0*extensions as f64/instructions as f64);
    }
}
