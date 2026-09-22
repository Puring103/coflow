//! 固定种子的受限程序生成器，以 Rust checked i32 作为独立语义 oracle。
use coflow_core::{contract::Contract, runtime::{HostValue, OptimizationProfile, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId}, vm::executor::ExecutionLimits};
use std::sync::Arc;

#[test]
fn generated_checked_programs_match_independent_oracle_in_both_profiles() {
    let mut seed = 0x9e3779b9u32;
    let mut next = || { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; seed };
    for case in 0..32 {
        let steps = (0..8).map(|_| (next() % 4, (next() % 17 + 1) as i32)).collect::<Vec<_>>();
        let mut body = "var value: int = input;".to_string();
        for (operation, constant) in &steps {
            body.push_str(&match operation {
                0 => format!("value = value + {constant};"),
                1 => format!("value = value - {constant};"),
                2 => format!("value = value * {constant};"),
                _ => format!("value = if value > 0 {{ value % {constant} }} else {{ value + {constant} }};"),
            });
        }
        // 重复标量表达式和局部数组同时经过 CSE、标量替换、CFG 与寄存器分配。
        body.push_str("var pair: [int] = [value + 1, value + 1]; pair[0] - pair[1]");
        let source = format!("table Rule {{ run: fn(input: int) -> int => {{ {body} }}; }}");
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("generated"), source)])).unwrap();
        let contract = Contract::new(schema).unwrap();
        let contract = Arc::new(Contract::from_bytes(&contract.to_bytes().unwrap()).unwrap());
        for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text("r: Rule {}", None);
            let built = builder.build();
            let runtime = built.runtime.unwrap_or_else(|error| panic!("{case}: {error}: {:?}", built.diagnostics));
            let function = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
            for input in [i32::MIN, i32::MIN + 1, -100000, -17, -1, 0, 1, 17, 100000, i32::MAX - 1, i32::MAX] {
                let mut expected = Some(input);
                for (operation, constant) in &steps {
                    expected = expected.and_then(|value| match operation {
                        0 => value.checked_add(*constant), 1 => value.checked_sub(*constant),
                        2 => value.checked_mul(*constant),
                        _ if value > 0 => value.checked_rem(*constant),
                        _ => value.checked_add(*constant),
                    });
                }
                expected = expected.and_then(|value| value.checked_add(1)).map(|_| 0);
                let actual = runtime.invoke(function, &[HostValue::Int(input)], ExecutionLimits::default());
                match expected {
                    Some(expected) => assert!(matches!(actual, Ok(HostValue::Int(value)) if value == expected), "case {case}, input {input}, {profile:?}: {actual:?}"),
                    None => assert!(actual.is_err(), "case {case}, input {input}, {profile:?}: missed checked fault"),
                }
            }
        }
    }
}
