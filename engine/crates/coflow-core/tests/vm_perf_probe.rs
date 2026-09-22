//! Coflow VM 性能基准：覆盖解释器、闭包、集合、字符串、记录与模板的典型负载。
//! 与 tests/perf/lua_bench.lua 中负载一一对应。
use coflow_core::{
    contract::Contract,
    runtime::{HostValue, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::ExecutionLimits,
};
use std::sync::Arc;
use std::time::Instant;

fn make_runtime(body: &str, signature: &str) -> Arc<Runtime> {
    let source = format!("table Rule {{ value: int = 7; run: {signature} => {{ {body} }}; }}");
    let source_bytes = source.len();
    let started = Instant::now();
    let modules = parse_modules([CftFile::from_source(ModuleId::from("test"), source)]);
    let schema = build_schema(&modules).expect("schema");
    let contract = Arc::new(Contract::new(schema).expect("contract"));
    let contract_ns = started.elapsed().as_nanos();
    let contract_bytes = contract.to_bytes().expect("contract bytes").len();
    let started = Instant::now();
    let mut builder = RuntimeBuilder::new(contract);
    builder.add_text("rule: Rule {}", Some("test.cfd"));
    let runtime = builder.build().runtime.expect("runtime");
    println!("construction: source_bytes={}, contract_ns={contract_ns}, contract_bytes={contract_bytes}, image_build_ns={}", source_bytes, started.elapsed().as_nanos());
    runtime
}

fn call(runtime: &Runtime, args: &[HostValue]) -> HostValue {
    let rule = runtime.record("Rule", "rule").expect("record");
    let run = runtime.field(rule, "run").expect("field");
    let limits = ExecutionLimits {
        max_work: u64::MAX / 4,
        max_iterations: u64::MAX / 4,
        ..ExecutionLimits::default()
    };
    runtime.invoke(run, args, limits).expect("invoke")
}

fn bench(name: &str, runtime: &Runtime, args: &[HostValue]) {
    let first = Instant::now();
    let first_result = call(runtime, args);
    let first_ns = first.elapsed().as_nanos();
    let expected = format!("{first_result:?}");
    println!("first_call {name}: ns={first_ns}, result={expected}");
    let mut samples = Vec::new();
    let mut result = call(runtime, args);
    for _ in 0..7 {
        let start = Instant::now();
        result = std::hint::black_box(call(runtime, args));
        samples.push(start.elapsed().as_nanos());
        assert_eq!(format!("{result:?}"), expected);
    }
    samples.sort_unstable();
    println!("{name}: median_ns={}, samples_ns={samples:?}, result={result:?}", samples[3]);
}

#[test]
fn perf_probe() {
    // 解释器主干
    bench(
        "[1] int-loop 1_000_000",
        &make_runtime(
            "var total: int = 0; for value in 0..=999999 { total += 1; } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[2] int-eq 200_000",
        &make_runtime(
            "var hits: int = 0; for value in 0..=199999 { if value == 123456 { hits += 1; } } hits",
            "fn() -> int",
        ),
        &[],
    );
    // 闭包
    bench(
        "[3] map-10x10000 calls",
        &make_runtime(
            "var base: [int] = [0,1,2,3,4,5,6,7,8,9]; var total: int = 0; for i in 0..=9999 { total += base.map(fn(x: int) -> int { x + 1 }).sum(); } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[7] closure-create 200_000",
        &make_runtime(
            "var total: int = 0; for i in 0..=199999 { var f: fn() -> int = fn() -> int { i }; f(); total += 1; } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[8] closure-call-hoisted 200_000",
        &make_runtime(
            "var f: fn(int) -> int = fn(x: int) -> int { x + 1 }; var total: int = 0; for i in 0..=199999 { total = f(i); } total",
            "fn() -> int",
        ),
        &[],
    );
    // 递归走 self.fib：同时覆盖 self 字段读取与深层调用链。
    let fib_runtime = {
        let source = "table Rule { value: int = 7; fib: fn(n: int) -> int => { if n < 2 { n } else { self.fib(n - 1) + self.fib(n - 2) } }; run: fn() -> int => { self.fib(20) }; }";
        let modules = parse_modules([CftFile::from_source(ModuleId::from("test"), source)]);
        let schema = build_schema(&modules).expect("schema");
        let contract = Arc::new(Contract::new(schema).expect("contract"));
        let mut builder = RuntimeBuilder::new(contract);
        builder.add_text("rule: Rule {}", Some("test.cfd"));
        builder.build().runtime.expect("runtime")
    };
    bench("[9] fib-self-recursion fib(20)", &fib_runtime, &[]);
    // 集合
    bench(
        "[10] array-iter 10x10_000",
        &make_runtime(
            "var values: [int] = [1,2,3,4,5,6,7,8,9,0]; var total: int = 0; for w in 0..=9999 { for v in values { total += v; } } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[11] array-2bind 10x10_000",
        &make_runtime(
            "var values: [int] = [1,2,3,4,5,6,7,8,9,0]; var total: int = 0; for w in 0..=9999 { for index, v in values { total += index + v; } } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[11] array-build 10_000x5",
        &make_runtime(
            "var total: int = 0; for w in 0..=9999 { var list: [int] = [1, 2, 3, 4, 5]; total += list[4]; } total",
            "fn() -> int",
        ),
        &[],
    );
    let dict100 = make_runtime(
        &format!(
            "var dict: {{int: int}} = {{ {} }}; var total: int = 0; for w in 0..=99999 {{ total += dict[w % 100]; }} total",
            (0..100).map(|k| format!("{k}: {k}")).collect::<Vec<_>>().join(", ")
        ),
        "fn() -> int",
    );
    bench("[12] dict-int-lookup 100_000", &dict100, &[]);
    bench(
        "[13] string-concat 20_000",
        &make_runtime(
            "var text: string = \"\"; for value in 0..=19999 { text = text + \"x\"; } text.len()",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[13] template-1_000",
        &make_runtime(
            "var total: int = 0; for i in 0..=999 { total += f\"x{self.value}y\".len(); } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[14] record-ref-field 100_000",
        &make_runtime(
            "var total: int = 0; for w in 0..=99999 { total += &Rule::rule.value; } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[5] string-index 1_000 over 2_000 chars",
        &make_runtime(
            "var text: string = \"\"; for value in 0..=1999 { text = text + \"y\"; } var total: int = 0; for value in 0..=999 { total += text[0].len(); } total",
            "fn() -> int",
        ),
        &[],
    );
    bench(
        "[6] field-read self.value x10000",
        &make_runtime(
            "var total: int = 0; for value in 0..=9999 { total += self.value; } total",
            "fn() -> int",
        ),
        &[],
    );
}
