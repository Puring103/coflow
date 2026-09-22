use coflow_core::{
    contract::Contract,
    runtime::{HostValue, OptimizationProfile, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::executor::ExecutionLimits,
};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::sync::Arc;

fn runtime(fields: &str, body: &str) -> Arc<Runtime> {
    runtime_with_profile(fields, body, OptimizationProfile::Release)
}

fn runtime_with_profile(
    fields: &str,
    body: &str,
    profile: OptimizationProfile,
) -> Arc<Runtime> {
    let source = format!("table Rule {{ {fields} run: fn() -> int => {{ {body} }}; }}");
    let modules = parse_modules([CftFile::from_source(ModuleId::from("bench"), source)]);
    let schema = build_schema(&modules).expect("benchmark schema must compile");
    let contract = Arc::new(Contract::new(schema).expect("benchmark contract must compile"));
    let mut builder = RuntimeBuilder::new(contract);
    builder.optimization_profile(profile);
    builder.add_text("rule: Rule {}", Some("bench.cfd"));
    builder.build().runtime.expect("benchmark runtime must build")
}

fn invoke(runtime: &Runtime) -> HostValue {
    let rule = runtime.record("Rule", "rule").expect("benchmark record must exist");
    let run = runtime.field(rule, "run").expect("benchmark function must exist");
    runtime
        .invoke(
            run,
            &[],
            ExecutionLimits {
                max_work: u64::MAX / 4,
                max_iterations: u64::MAX / 4,
                ..ExecutionLimits::default()
            },
        )
        .expect("benchmark invocation must succeed")
}

fn vm(c: &mut Criterion) {
    let mut cases = vec![
        (
            "int_loop_1m",
            "",
            "var total: int = 0; for i in 0..=999999 { total += 1; } total",
        ),
        (
            "int_eq_200k",
            "",
            "var hits: int = 0; for i in 0..=199999 { if i == 123456 { hits += 1; } } hits",
        ),
        (
            "map_10x10000",
            "",
            "var values: [int] = [0,1,2,3,4,5,6,7,8,9]; var total: int = 0; for i in 0..=9999 { total += values.map(fn(x: int) -> int { x + 1 }).sum(); } total",
        ),
        (
            "closure_call_200k",
            "",
            "var f: fn(int) -> int = fn(x: int) -> int { x + 1 }; var total: int = 0; for i in 0..=199999 { total = f(i); } total",
        ),
        (
            "closure_create_200k",
            "",
            "var total: int = 0; for i in 0..=199999 { var f: fn() -> int = fn() -> int { i }; f(); total += 1; } total",
        ),
        (
            "fib_20",
            "fib: fn(n: int) -> int => { if n < 2 { n } else { self.fib(n - 1) + self.fib(n - 2) } };",
            "self.fib(20)",
        ),
        (
            "array_iter_10x10000",
            "",
            "var values: [int] = [1,2,3,4,5,6,7,8,9,0]; var total: int = 0; for w in 0..=9999 { for value in values { total += value; } } total",
        ),
        (
            "array_2bind_10x10000",
            "",
            "var values: [int] = [1,2,3,4,5,6,7,8,9,0]; var total: int = 0; for w in 0..=9999 { for index, value in values { total += index + value; } } total",
        ),
        (
            "array_build_10k",
            "",
            "var total: int = 0; for i in 0..=9999 { var values: [int] = [1,2,3,4,5]; total += values[4]; } total",
        ),
        (
            "string_concat_20k",
            "",
            "var text: string = \"\"; for i in 0..=19999 { text = text + \"x\"; } text.len()",
        ),
        (
            "string_index_1k",
            "",
            "var text: string = \"\"; for i in 0..=1999 { text = text + \"y\"; } var total: int = 0; for i in 0..=999 { total += text[0].len(); } total",
        ),
        (
            "self_field_10k",
            "value: int = 7;",
            "var total: int = 0; for i in 0..=9999 { total += self.value; } total",
        ),
        (
            "template_1k",
            "value: int = 7;",
            "var total: int = 0; for i in 0..=999 { total += f\"x{self.value}y\".len(); } total",
        ),
        (
            "record_ref_field_100k",
            "value: int = 7;",
            "var total: int = 0; for i in 0..=99999 { total += &Rule::rule.value; } total",
        ),
    ];
    let entries = (0..100)
        .map(|value| format!("{value}:{value}"))
        .collect::<Vec<_>>()
        .join(",");
    let dictionary = format!(
        "var values: {{int: int}} = {{{entries}}}; var total: int = 0; for i in 0..=99999 {{ total += values[i % 100]; }} total"
    );
    cases.push(("dict_lookup_100k", "", &dictionary));
    for (name, fields, body) in cases {
        let runtime = runtime(fields, body);
        c.bench_function(name, |b| b.iter(|| black_box(invoke(&runtime))));
    }

    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let runtime = runtime_with_profile(
            "value: int = 7;",
            "var total: int = 0; for i in 0..=99999 { total += &Rule::rule.value; } total",
            profile,
        );
        c.bench_function(
            match profile {
                OptimizationProfile::Debug => "record_ref_field_100k_debug",
                OptimizationProfile::Release => "record_ref_field_100k_release",
            },
            |b| b.iter(|| black_box(invoke(&runtime))),
        );
    }
}

criterion_group!(benches, vm);
criterion_main!(benches);
