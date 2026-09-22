use coflow_core::{
    contract::Contract,
    runtime::{HostValue, OptimizationProfile, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::{ExecutionError, ExecutionLimits},
};
use std::sync::Arc;

fn runtime(source: String) -> Arc<Runtime> {
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("budget"), source)])).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.optimization_profile(OptimizationProfile::Debug);
    builder.add_text("rule: Rule {}", None);
    builder.build().runtime.unwrap()
}

fn invoke(runtime: &Runtime, name: &str, arguments: &[HostValue], bytes: usize) -> Result<HostValue, ExecutionError> {
    let function = runtime.field(runtime.record("Rule", "rule")?, name)?;
    runtime.invoke(function, arguments, ExecutionLimits { max_heap_bytes: bytes, ..ExecutionLimits::default() })
}

#[test]
fn cached_capacity_does_not_change_request_feasibility() {
    let parameters = (0..128).map(|i| format!("a{i}: int")).collect::<Vec<_>>().join(",");
    let sum = (0..128).map(|i| format!("a{i}")).collect::<Vec<_>>().join("+");
    let image = runtime(format!("table Rule {{ wide: fn({parameters}) -> int => {{ {sum} }}; small: fn() -> int => {{ 7 }}; }}")).image();
    // 扫描临界预算，覆盖缓存本身可容纳、却挤占调用帧或根空间的情况。
    for bytes in (1024..8192).step_by(64) {
        let cold = Runtime::from_image(image.clone(), Default::default()).unwrap();
        let warm = Runtime::from_image(image.clone(), Default::default()).unwrap();
        invoke(&warm, "wide", &vec![HostValue::Int(1); 128], 1 << 20).unwrap();
        let cold_result = invoke(&cold, "small", &[], bytes);
        let warm_result = invoke(&warm, "small", &[], bytes);
        assert_eq!(cold_result.is_ok(), warm_result.is_ok(), "budget={bytes}, cold={cold_result:?}, warm={warm_result:?}");
    }
}

#[test]
fn fixed_text_comparison_does_not_materialize_the_payload() {
    let runtime = runtime(format!(
        "table Rule {{ text: string = \"{}\"; length: fn() -> int => {{ self.text.len() }}; same: fn() -> bool => {{ self.text == self.text }}; }}",
        "x".repeat(32768),
    ));
    assert!(matches!(invoke(&runtime, "length", &[], 8192), Ok(HostValue::Int(32768))));
    assert!(matches!(invoke(&runtime, "same", &[], 8192), Ok(HostValue::Bool(true))));
}

#[test]
fn deferred_garbage_does_not_consume_a_smaller_request_budget() {
    let runtime = runtime("table Rule { make: fn() -> [int] => { [1,2,3] }; scratch: fn(text: string) -> int => { (text + text).len() }; small: fn() -> int => { 7 }; }".into());
    let held = invoke(&runtime, "make", &[], 1 << 20).unwrap();
    assert!(matches!(invoke(&runtime, "small", &[], 8192), Ok(HostValue::Int(7))));
    for _ in 0..3 {
        invoke(&runtime, "scratch", &[HostValue::String("x".repeat(32768))], 1 << 20).unwrap();
        // 首次调用就必须成功，不能依赖失败清理或宿主显式 collect。
        assert!(matches!(invoke(&runtime, "small", &[], 8192), Ok(HostValue::Int(7))));
    }
    let HostValue::Existing { value, .. } = held else { panic!("数组必须保活") };
    assert!(runtime.value(value).is_ok());
    runtime.release_value(value).unwrap();
}

#[test]
fn nested_arithmetic_compiles_and_executes_on_a_normal_stack() {
    std::thread::Builder::new().stack_size(1024 * 1024).spawn(|| {
        for (body, expected) in [
            (format!("{}a", "-".repeat(128)), 1),
            (format!("{}a{}", "1+(".repeat(100), ")".repeat(100)), 101),
        ] {
            let runtime = runtime(format!("table Rule {{ run: fn(a: int) -> int => {{ {body} }}; }}"));
            assert!(matches!(invoke(&runtime, "run", &[HostValue::Int(1)], 1 << 20), Ok(HostValue::Int(value)) if value == expected));
        }
    }).unwrap().join().unwrap();
}
