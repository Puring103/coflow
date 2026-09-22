use coflow_core::{
    contract::Contract,
    runtime::{HostService, HostValue, OptimizationProfile, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftFile, ModuleId},
    vm::{ExecutionLimits, ExecutionError},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Weak,
};

fn contract(sources: &[&str]) -> Arc<Contract> {
    let modules =
        parse_modules(sources.iter().enumerate().map(|(i, source)| {
            CftFile::from_source(ModuleId::from(format!("module{i}")), *source)
        }));
    let schema = build_schema(&modules).expect("schema");
    let contract = Contract::new(schema).expect("compile");
    Arc::new(Contract::from_bytes(&contract.to_bytes().expect("serialize")).expect("deserialize"))
}
fn build(sources: &[&str], data: &str) -> Arc<Runtime> {
    let mut builder = RuntimeBuilder::new(contract(sources));
    builder.add_text(data, Some("integration.cfd"));
    let build = builder.build();
    build
        .runtime
        .unwrap_or_else(|error| panic!("{error}: {:?}", build.diagnostics))
}
fn invoke(runtime: &Runtime, ty: &str, id: &str, field: &str) -> HostValue {
    let function = runtime
        .field(runtime.record(ty, id).expect("record"), field)
        .expect("field");
    runtime
        .invoke(function, &[], ExecutionLimits::default())
        .expect("call")
}
fn int(value: HostValue) -> i32 {
    match value {
        HostValue::Int(value) => value,
        value => panic!("expected int: {value:?}"),
    }
}

#[test]
fn range_proven_hoisting_preserves_empty_loop_and_checked_faults() {
    let contract = contract(&["table Rule { run: fn(x: int, count: int) -> int => { var sum: int = 0; for i in 0..count { sum += (x & 255) + 1; sum += x + 1; } sum }; }"]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(contract.clone()); builder.optimization_profile(profile);
        builder.add_text("r: Rule {}", None); let runtime = builder.build().runtime.unwrap();
        let function = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
        let call = |x, count| runtime.invoke(function, &[HostValue::Int(x), HostValue::Int(count)], ExecutionLimits::default());
        assert_eq!(int(call(i32::MAX, 0).unwrap()), 0);
        assert_eq!(int(call(i32::MIN, -1).unwrap()), 0);
        assert_eq!(int(call(3, 5).unwrap()), 40);
        assert!(call(i32::MAX, 1).is_err());
    }
}

#[test]
fn streaming_value_graph_keeps_pending_children_alive_during_reentrant_collection() {
    let runtime = build(&["table Rule { run: fn() -> [int] => { [1, 2, 3] }; }"], "r: Rule {}");
    let HostValue::Existing { value: root, .. } = invoke(&runtime, "Rule", "r", "run") else { panic!("array result"); };
    runtime.release_value(root).unwrap();
    let mut sum = 0;
    let count = runtime.visit_value_graph(Some(root), |_, value| {
        runtime.collect()?;
        if let coflow_core::runtime::Value::Int(value) = value { sum += value; }
        Ok(())
    }).unwrap();
    assert_eq!(count, 4); assert_eq!(sum, 6);
    runtime.collect().unwrap();
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}

#[test]
fn module_local_constants_and_cfd_imports_keep_their_own_scope() {
    let runtime=build(&[
        "namespace one; const value: int = 11; const read: fn() -> int = fn() -> int { value }; table Item { run: fn() -> int => { read() }; }",
        "namespace two; const value: int = 22; const read: fn() -> int = fn() -> int { value }; table Item { run: fn() -> int => { read() }; }",
    ],"use one::read; a: one::Item {} b: two::Item {} c: two::Item { run: fn() -> int { read() } }");
    assert_eq!(int(invoke(&runtime, "one::Item", "a", "run")), 11);
    assert_eq!(int(invoke(&runtime, "two::Item", "b", "run")), 22);
    assert_eq!(int(invoke(&runtime, "two::Item", "c", "run")), 11);
}
#[test]
fn constructed_objects_bind_direct_functions_and_nested_defaults() {
    let runtime=build(&["data Inner { value: int = 3; read: fn() -> int => { self.value }; } data Outer { value: int; nested: Inner = Inner {}; read: fn() -> int => { self.value }; } table Rule { run: fn() -> int => { var object: Outer = Outer { value: 9, read: fn() -> int { self.value + self.nested.read() } }; object.read() }; }"],"r: Rule {}");
    assert_eq!(int(invoke(&runtime, "Rule", "r", "run")), 12);
}
#[test]
fn grandparent_captures_survive_multiple_returns_and_collection() {
    let runtime=build(&["table Rule { run: fn() -> fn() -> fn() -> int => { var number: int = 42; fn() -> fn() -> int { fn() -> int { number } } }; }"],"r: Rule {}");
    let HostValue::Existing { value: middle, .. } = invoke(&runtime, "Rule", "r", "run") else {
        panic!("closure")
    };
    let HostValue::Existing { value: last, .. } = runtime
        .invoke(middle, &[], ExecutionLimits::default())
        .expect("middle")
    else {
        panic!("closure")
    };
    runtime.release_value(middle).expect("release");
    runtime.collect().expect("collect");
    assert_eq!(
        int(runtime
            .invoke(last, &[], ExecutionLimits::default())
            .expect("last")),
        42
    );
    runtime.release_value(last).expect("release");
    runtime.collect().expect("collect");
    assert_eq!(runtime.dynamic_value_count().expect("heap"), 0);
}
#[test]
fn missing_reference_in_unreachable_nested_code_prevents_publication() {
    let mut builder=RuntimeBuilder::new(contract(&["table Rule { run: fn() -> fn() -> Rule => { fn() -> Rule { if false { &missing } else { self } } }; }"]));
    builder.add_text("r: Rule {}", Some("missing.cfd"));
    assert!(builder.build().runtime.is_err());
}
#[test]
fn execution_limits_cover_iterations_memory_and_recursive_frames() {
    for (body, limits, part) in [
        (
            "for x in 0..100 {} 1",
            ExecutionLimits {
                max_iterations: 2,
                ..ExecutionLimits::default()
            },
            "迭代",
        ),
        (
            "var text: string = \"hello\"; while text.len() < 10000 { text = text + text; } 1",
            ExecutionLimits {
                max_heap_bytes: 1000,
                ..ExecutionLimits::default()
            },
            "内存",
        ),
        (
            "self.run() + 1",
            ExecutionLimits {
                max_depth: 8,
                ..ExecutionLimits::default()
            },
            "深度",
        ),
    ] {
        let runtime = build(
            &[&format!(
                "table Rule {{ run: fn() -> int => {{ {body} }}; }}"
            )],
            "r: Rule {}",
        );
        let target = runtime
            .field(runtime.record("Rule", "r").unwrap(), "run")
            .unwrap();
        let error = runtime.invoke(target, &[], limits).expect_err("limited");
        assert!(error.to_string().contains(part), "{error}");
        assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
    }
}

thread_local! {
    // 单线程 Runtime：重入测试通过线程局部传递 Runtime 弱引用。
    static REENTER_RUNTIME: std::cell::RefCell<Option<Weak<Runtime>>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Debug, Default)]
struct Service {
    calls: AtomicUsize,
}
impl HostService for Service {
    fn has_member(
        &self,
        _: &str,
        _: &coflow_core::schema::CftValueType,
        _: &coflow_core::schema::CftSchema,
    ) -> bool {
        true
    }
    fn read(&self, _: &str) -> Result<HostValue, ExecutionError> {
        Ok(HostValue::Int(
            self.calls.fetch_add(1, Ordering::SeqCst) as i32
        ))
    }
    fn call(&self, field: &str, args: &[HostValue]) -> Result<HostValue, ExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match field {
            "twice" => {
                let [HostValue::Int(value)] = args else {
                    panic!("args")
                };
                Ok(HostValue::Int(value * 2))
            }
            "panic" => panic!("intentional Host panic"),
            "wrong" => Ok(HostValue::String("wrong type".into())),
            "collect" => {
                let runtime = REENTER_RUNTIME.with(|cell| cell.borrow().as_ref().unwrap().upgrade().unwrap());
                runtime.collect()?;
                let function = runtime.field(runtime.record("Rule", "r")?, "inner")?;
                let result = runtime.invoke(function, &[], ExecutionLimits::default())?;
                runtime.collect()?;
                Ok(result)
            }
            "reenter" => {
                let runtime = REENTER_RUNTIME.with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .expect("runtime")
                        .upgrade()
                        .expect("runtime alive")
                });
                let function = runtime.field(runtime.record("Rule", "r")?, "plain")?;
                runtime.invoke(function, &[], ExecutionLimits::default())
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn host_arguments_reentry_panic_and_return_validation_use_real_calls() {
    let service = Arc::new(Service::default());
    let mut builder=RuntimeBuilder::new(contract(&["@Host singleton Service { twice: fn(int) -> int; panic: fn() -> int; wrong: fn() -> int; reenter: fn() -> int; } table Rule { plain: fn() -> int => { 21 }; run: fn() -> int => { Service.twice(Service.reenter()) }; crash: fn() -> int => { Service.panic() }; bad: fn() -> int => { Service.wrong() }; }"]));
    builder
        .bind("Service".into(), service.clone())
        .expect("bind");
    builder.add_text("r: Rule {}", None);
    let runtime = builder.build().runtime.expect("runtime");
    REENTER_RUNTIME.with(|cell| *cell.borrow_mut() = Some(Arc::downgrade(&runtime)));
    assert_eq!(int(invoke(&runtime, "Rule", "r", "run")), 42);
    for field in ["crash", "bad"] {
        let target = runtime
            .field(runtime.record("Rule", "r").unwrap(), field)
            .unwrap();
        assert!(runtime
            .invoke(target, &[], ExecutionLimits::default())
            .is_err());
        assert_eq!(int(invoke(&runtime, "Rule", "r", "plain")), 21);
    }
}

#[test]
fn cft_and_cfd_faults_point_to_original_utf8_expressions() {
    let cft="namespace game; data Arg { value: int; } table Rule { run: fn(arg: Arg) -> int => { arg.value // 0 }; start: fn() -> int => { self.run(Arg { value: 1 }) }; }";
    let runtime = build(&[cft], "r: game::Rule {}");
    let function = runtime
        .field(runtime.record("game::Rule", "r").unwrap(), "start")
        .unwrap();
    let ExecutionError::Fault { span, .. } = runtime
        .invoke(function, &[], ExecutionLimits::default())
        .unwrap_err()
    else {
        panic!("fault")
    };
    assert_eq!(&cft[span.start..span.end], "arg.value // 0");
    // 同一文件中的第二个实现通过解析器 span 定位，不按文本搜索第一处。
    let make = build(
        &["table Rule { run: fn() -> int => { 100 // 0 }; }"],
        "r: Rule {}",
    );
    let function2 = make
        .field(make.record("Rule", "r").unwrap(), "run")
        .unwrap();
    let ExecutionError::Fault { span, module, .. } = make
        .invoke(function2, &[], ExecutionLimits::default())
        .unwrap_err()
    else {
        panic!("fault")
    };
    let schema_source = "table Rule { run: fn() -> int => { 100 // 0 }; }";
    assert_eq!(&schema_source[span.start..span.end], "100 // 0");
    assert!(module.is_some());
    let source = "r: Rule { run: fn() -> int { 7 } } s: Rule { run: fn() -> int { 100 // 0 } }";
    let runtime = build(&[schema_source], source);
    let function = runtime
        .field(runtime.record("Rule", "s").unwrap(), "run")
        .unwrap();
    let ExecutionError::Fault { span, path, .. } = runtime
        .invoke(function, &[], ExecutionLimits::default())
        .unwrap_err()
    else {
        panic!("fault")
    };
    assert_eq!(&source[span.start..span.end], "100 // 0");
    assert_eq!(path.as_deref(), Some("integration.cfd"));
    assert!(span.start > source.find("s: Rule").unwrap());
}
#[test]
fn invalid_cfd_function_has_structured_file_and_span_diagnostic() {
    let mut builder = RuntimeBuilder::new(contract(&["table Rule { run: fn() -> int; }"]));
    let source = "r: Rule { run: fn() -> int { \"错误\" } }";
    builder.add_text(source, Some("invalid.cfd"));
    let result = builder.build();
    assert!(result.runtime.is_err());
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.code == "FUNCTION")
        .expect("function diagnostic");
    assert_eq!(diagnostic.source, "invalid.cfd");
    let (start, end) = diagnostic.span.unwrap();
    assert_eq!(&source[start..end], "\"错误\"");
}
#[test]
fn deep_dynamic_objects_compare_without_recursing_the_host_stack() {
    let runtime=build(&["data Node { next: Node?; } table Rule { run: fn() -> bool => { var left: Node? = None; var right: Node? = None; for i in 0..2000 { left = Node { next: left }; right = Node { next: right }; } left == right }; }"],"r: Rule {}");
    assert!(matches!(
        invoke(&runtime, "Rule", "r", "run"),
        HostValue::Bool(true)
    ));
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}

#[test]
fn template_collections_preserve_filter_storage_and_short_circuit_reads() {
    let service = Arc::new(Service::default());
    let mut builder=RuntimeBuilder::new(contract(&["@Host singleton Service { counter: int; } table Rule { texts: [fstring] = [f\"{Service.counter}\", f\"{Service.counter}\"]; run: fn() -> int => { var filtered: [fstring] = self.texts.filter(fn(value: string) -> bool { true }); var first: string = filtered[0]; Service.counter }; short: fn() -> bool => { self.texts.any(fn(value: string) -> bool { true }) }; }"]));
    builder.bind("Service".into(), service.clone()).unwrap();
    builder.add_text("r: Rule {}", None);
    let runtime = builder.build().runtime.expect("runtime");
    assert_eq!(service.calls.load(Ordering::SeqCst), 0);
    assert_eq!(int(invoke(&runtime, "Rule", "r", "run")), 3);
    assert_eq!(service.calls.load(Ordering::SeqCst), 4);
    assert!(matches!(
        invoke(&runtime, "Rule", "r", "short"),
        HostValue::Bool(true)
    ));
    assert_eq!(service.calls.load(Ordering::SeqCst), 5);
}
#[test]
fn dimensions_execute_default_for_and_variants_in_declaration_order() {
    let source="table Item { value: int = 7; @localized name: fstring; run: fn() -> string => { self.name.default() + self.name.for(\"zh\") + self.name.variants()[\"en\"] }; }";
    let modules = parse_modules([CftFile::from_source(ModuleId::from("dimension"), source)]);
    let schema = build_schema(&modules).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.add_text(
        "a: Item { name: dimension { default: f\"base{self.value}\", zh: f\"中{self.value}\" } } b: Item { name: dimension { default: f\"base{self.value}\", en: f\"english{self.value}\" } }",
        Some("dimensions.cfd"),
    );
    let runtime = builder.build().runtime.expect("dimension runtime");
    assert!(
        matches!(invoke(&runtime,"Item","a","run"),HostValue::String(text)if text=="base7中7base7")
    );
}

#[test]
fn enum_construction_and_flags_validate_ranges_without_implicit_integer_coercion() {
    let runtime=build(&["enum Kind { One = 1 } @flag enum Access { Read = 1, Write = 2 } table Rule { run: fn() -> bool => { Kind(7) == Kind(7) && (Access(1) | Access::Write) == Access(3) && ~Access::Read == Access::Write }; invalid: fn() -> Access => { Access(4) }; }"],"r: Rule {}");
    assert!(matches!(
        invoke(&runtime, "Rule", "r", "run"),
        HostValue::Bool(true)
    ));
    let function = runtime
        .field(runtime.record("Rule", "r").unwrap(), "invalid")
        .unwrap();
    assert!(runtime
        .invoke(function, &[], ExecutionLimits::default())
        .is_err());
}

#[test]
fn identical_default_bodies_keep_distinct_source_locations() {
    let source =
        "table Rule { first: fn() -> int => { 100 // 0 }; second: fn() -> int => { 100 // 0 }; }";
    let runtime = build(&[source], "r: Rule {}");
    let record = runtime.record("Rule", "r").unwrap();
    let mut spans = Vec::new();
    for name in ["first", "second"] {
        let function = runtime.field(record, name).unwrap();
        let ExecutionError::Fault { span, .. } = runtime
            .invoke(function, &[], ExecutionLimits::default())
            .unwrap_err()
        else {
            panic!("fault")
        };
        assert_eq!(&source[span.start..span.end], "100 // 0");
        spans.push(span);
    }
    assert!(spans[1].start > spans[0].end);
}

#[test]
fn function_type_alias_default_executes_with_named_parameters() {
    let runtime=build(&["type Calculation = fn(value: int) -> int; table Rule { run: Calculation => { value * 2 }; start: fn() -> int => { self.run(21) }; }"],"r: Rule {}");
    assert_eq!(int(invoke(&runtime, "Rule", "r", "start")), 42);
}

#[test]
fn retained_child_outlives_released_parent_and_old_ids_never_alias_new_values() {
    let runtime = build(
        &["table Rule { run: fn() -> [[int]] => { [[1,2,3]] }; }"],
        "r: Rule {}",
    );
    let HostValue::Existing { value: parent, .. } = invoke(&runtime, "Rule", "r", "run") else {
        panic!("array")
    };
    let parent_value = runtime.value(parent).unwrap();
    let coflow_core::runtime::Value::Array(values) = parent_value.as_ref() else {
        panic!("array")
    };
    let child = values.get(0).unwrap();
    drop(parent_value);
    runtime.retain_value(child).unwrap();
    runtime.release_value(parent).unwrap();
    runtime.collect().unwrap();
    assert!(runtime.value(parent).is_err());
    assert!(
        matches!(runtime.value(child).unwrap().as_ref(),coflow_core::runtime::Value::Array(values)if values.len()==3)
    );
    runtime.release_value(child).unwrap();
    runtime.collect().unwrap();
    assert!(runtime.value(child).is_err());
    let HostValue::Existing { value: next, .. } = invoke(&runtime, "Rule", "r", "run") else {
        panic!("array")
    };
    assert!(next > child && next > parent);
    runtime.release_value(next).unwrap();
    runtime.collect().unwrap();
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}

#[test]
fn cfd_optional_values_use_none_or_bare_values_and_reject_constructors() {
    let contract = contract(&["table Rule { value: int?; }"]);
    for source in ["r: Rule { value: None }", "r: Rule { value: 3 }"] {
        let mut builder = RuntimeBuilder::new(contract.clone());
        builder.add_text(source, None);
        assert!(builder.build().runtime.is_ok());
    }
    let mut builder = RuntimeBuilder::new(contract);
    builder.add_text("r: Rule { value: Some(3) }", None);
    assert!(builder.build().runtime.is_err());
}

#[test]
fn contract_compilation_errors_keep_module_and_exact_expression_span() {
    let source = "table Rule { run: fn() -> int => { \"错误\" }; }";
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("bad.cft"),
        source,
    )]))
    .unwrap();
    let coflow_core::contract::ContractError::Semantic(error) = Contract::new(schema).expect_err("invalid unused CFT function") else {
        panic!("expected semantic error");
    };
    assert_eq!(error.path.as_deref(), Some("bad.cft"));
    let (start, end) = (error.span.start, error.span.end);
    assert_eq!(&source[start..end], "\"错误\"");
}

#[test]
fn runtime_profiles_link_each_cfd_snapshot_without_changing_success_results() {
    let contract = contract(&[
        "table Rule { value: int; run: fn() -> int => { &Rule::target.value + self.value }; }",
    ]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        for (target, expected) in [(7, 10), (11, 14)] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text(
                &format!("target: Rule {{ value: {target} }} caller: Rule {{ value: 3 }}"),
                Some("snapshot.cfd"),
            );
            let runtime = builder.build().runtime.expect("linked runtime");
            assert_eq!(runtime.optimization_profile(), profile);
            assert!(matches!(
                invoke(&runtime, "Rule", "caller", "run"),
                HostValue::Int(value) if value == expected
            ));
        }
    }
}

#[test]
fn nested_ranges_and_empty_ranges_preserve_profile_results_after_fusion() {
    let contract = contract(&[
        "table Rule { run: fn() -> int => { var result: int = 0; for i in 0..3 { for j in 0..4 { result += 1; } } for unused in 2..2 { result += 100; } for unused in 4..1 { result += 1000; } result }; }",
    ]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(contract.clone());
        builder.optimization_profile(profile);
        builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.expect("nested range snapshot");
        assert_eq!(int(invoke(&runtime, "Rule", "r", "run")), 12);
    }
}

#[test]
fn fixed_owner_specialization_and_static_templates_are_snapshot_local() {
    let compiled = contract(&[r#"table Item { name: string; number: int; text: fstring = f"{self.name}:{self.number}"; run: fn() -> int => { self.number }; }"#]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut images = Vec::new();
        for (name, number) in [("first", 11), ("second", 22)] {
            let mut builder = RuntimeBuilder::new(compiled.clone());
            builder.optimization_profile(profile);
            builder.add_text(&format!("a: Item {{ name: \"{name}\", number: {number} }}"), None);
            images.push(builder.build().runtime.unwrap());
        }
        for (runtime, expected, number) in [(&images[0], "first:11", 11), (&images[1], "second:22", 22)] {
            assert_eq!(int(invoke(runtime, "Item", "a", "run")), number);
            let text = runtime.field(runtime.record("Item", "a").unwrap(), "text").unwrap();
            runtime.collect().unwrap();
            let before = runtime.dynamic_value_count().unwrap();
            for _ in 0..20 { assert_eq!(runtime.read_text(text).unwrap(), expected); }
            if profile == OptimizationProfile::Release {
                assert_eq!(runtime.dynamic_value_count().unwrap(), before, "静态模板读取不执行或分配动态文本");
            }
        }
    }
}

#[test]
fn unused_direct_calls_preserve_faults_and_divergence() {
    let compiled = contract(&[r#"table Rule {
        leaf: fn() -> int => { 7 };
        middle: fn() -> int => { self.leaf() };
        pure: fn() -> int => { self.middle(); 9 };
        fail: fn() -> int => { 2147483647 + 1 };
        fault: fn() -> int => { self.fail(); 9 };
        recur: fn() -> int => { self.recur() };
        diverge: fn() -> int => { self.recur(); 9 };
    }"#]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(compiled.clone());
        builder.optimization_profile(profile); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        assert_eq!(int(invoke(&runtime, "Rule", "r", "pure")), 9);
        let record = runtime.record("Rule", "r").unwrap();
        for name in ["fault", "diverge"] {
            let function = runtime.field(record, name).unwrap();
            assert!(runtime.invoke(function, &[], ExecutionLimits { max_depth: 8, max_work: 2000, ..ExecutionLimits::default() }).is_err(), "{name} 的未使用调用不能删除");
        }
        if profile == OptimizationProfile::Release {
            let function = runtime.field(record, "pure").unwrap();
            assert_eq!(int(runtime.invoke(function, &[], ExecutionLimits { max_depth: 1, ..ExecutionLimits::default() }).unwrap()), 9, "跨调用图证明纯且有限的未使用调用已删除");
        }
    }
}

#[test]
fn release_tail_calls_reuse_argument_windows_and_debug_keeps_frames() {
    let compiled = contract(&["table Rule { run: fn(count: int, left: int, right: int) -> int => { if count == 0 { left } else { self.run(count - 1, right, left) } }; }"]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(compiled.clone()); builder.optimization_profile(profile); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let function = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
        let result = runtime.invoke(function, &[HostValue::Int(1001), HostValue::Int(7), HostValue::Int(9)], ExecutionLimits { max_depth: 4, ..ExecutionLimits::default() });
        if profile == OptimizationProfile::Release { assert_eq!(int(result.unwrap()), 9); }
        else { assert!(result.is_err()); }
    }
}

#[test]
fn concatenation_preserves_host_order_in_both_profiles() {
    #[derive(Debug, Default)]
    struct Trace(std::sync::Mutex<Vec<String>>);
    impl HostService for Trace {
        fn read(&self, _: &str) -> Result<HostValue, ExecutionError> { Err(ExecutionError::InvalidHandle) }
        fn has_member(&self, _: &str, _: &coflow_core::schema::CftValueType, _: &coflow_core::schema::CftSchema) -> bool { true }
        fn call(&self, _: &str, args: &[HostValue]) -> Result<HostValue, ExecutionError> {
            let HostValue::String(value) = &args[0] else { return Err(ExecutionError::InvalidHandle); };
            self.0.lock().unwrap().push(value.clone()); Ok(HostValue::String(value.clone()))
        }
    }
    let compiled = contract(&[r#"@Host singleton Trace { emit: fn(string) -> string; } table Rule { run: fn() -> string => { Trace.emit("a") + Trace.emit("b") + Trace.emit("c") }; }"#]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let trace = Arc::new(Trace::default());
        let mut builder = RuntimeBuilder::new(compiled.clone()); builder.optimization_profile(profile);
        builder.bind("Trace".into(), trace.clone()).unwrap(); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        assert!(trace.0.lock().unwrap().is_empty());
        assert!(matches!(invoke(&runtime, "Rule", "r", "run"), HostValue::String(value) if value == "abc"));
        assert_eq!(*trace.0.lock().unwrap(), ["a", "b", "c"]);
    }
}

#[test]
fn bounded_scalar_inlining_preserves_checked_results_without_extra_frames() {
    let compiled = contract(&["table Rule { twice: fn(value: int) -> int => { value * 2 }; run: fn(value: int) -> int => { self.twice(value) + 1 }; }"]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(compiled.clone()); builder.optimization_profile(profile); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let target = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
        let limits = ExecutionLimits { max_depth: 1, ..ExecutionLimits::default() };
        let result = runtime.invoke(target, &[HostValue::Int(21)], limits);
        if profile == OptimizationProfile::Release { assert_eq!(int(result.unwrap()), 43); } else { assert!(result.is_err()); }
        assert!(runtime.invoke(target, &[HostValue::Int(i32::MAX)], ExecutionLimits::default()).unwrap_err().to_string().contains("溢出"));
    }
}

#[derive(Debug)]
struct RecursiveHost;
impl HostService for RecursiveHost {
    fn has_member(&self, _: &str, _: &coflow_core::schema::CftValueType, _: &coflow_core::schema::CftSchema) -> bool { true }
    fn read(&self, _: &str) -> Result<HostValue, ExecutionError> {
        let runtime = REENTER_RUNTIME.with(|cell| cell.borrow().as_ref().unwrap().upgrade().unwrap());
        runtime.read_host("Recursive", "value", &coflow_core::schema::CftValueType::Int)?;
        Ok(HostValue::Int(0))
    }
    fn call(&self, _: &str, _: &[HostValue]) -> Result<HostValue, ExecutionError> {
        let runtime = REENTER_RUNTIME.with(|cell| cell.borrow().as_ref().unwrap().upgrade().unwrap());
        let function = runtime.field(runtime.singleton("Recursive")?, "run")?;
        runtime.invoke(function, &[], ExecutionLimits::default())
    }
}
#[test]
fn host_only_reentry_shares_depth_and_work_and_releases_failed_boundaries() {
    let mut builder = RuntimeBuilder::new(contract(&["@Host singleton Recursive { value: int; run: fn() -> int; } table Rule { read: fn() -> int => { Recursive.value }; plain: fn() -> int => { 7 }; }"]));
    builder.bind("Recursive".into(), Arc::new(RecursiveHost)).unwrap();
    builder.add_text("r: Rule {}", None);
    let runtime = builder.build().runtime.unwrap();
    REENTER_RUNTIME.with(|cell| *cell.borrow_mut() = Some(Arc::downgrade(&runtime)));
    let read = runtime.field(runtime.record("Rule", "r").unwrap(), "read").unwrap();
    let call = runtime.field(runtime.singleton("Recursive").unwrap(), "run").unwrap();
    for target in [read, call] {
        for (limits, expected) in [
            (ExecutionLimits { max_depth: 8, ..ExecutionLimits::default() }, "深度"),
            (ExecutionLimits { max_work: 4, ..ExecutionLimits::default() }, "工作量"),
        ] {
            let error = runtime.invoke(target, &[], limits).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert_eq!(int(invoke(&runtime, "Rule", "r", "plain")), 7);
        }
    }
}

#[test]
fn fused_total_map_filter_stages_preserve_values_and_empty_inputs() {
    let schema = contract(&["table Rule { run: fn(values: [int]) -> [int] => { values.map(fn(x: int) -> int { x & 255 }).filter(fn(x: int) -> bool { x > 2 }) }; }"]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(schema.clone()); builder.optimization_profile(profile); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let target = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
        for (input, expected) in [(vec![], vec![]), (vec![i32::MIN, -1, 2, 259, i32::MAX], vec![255, 3, 255])] {
            let HostValue::Existing { value, .. } = runtime.invoke(target, &[HostValue::Array(input.into_iter().map(HostValue::Int).collect())], ExecutionLimits::default()).unwrap() else { panic!("array"); };
            let root = runtime.value(value).unwrap();
            let coflow_core::runtime::Value::Array(items) = root.as_ref() else { panic!("array"); };
            let actual = items.iter().map(|id| match runtime.value(id).unwrap().as_ref() { coflow_core::runtime::Value::Int(value) => *value, _ => panic!("int") }).collect::<Vec<_>>();
            assert_eq!(actual, expected);
            runtime.release_value(value).unwrap();
        }
    }
}

#[test]
fn existing_argument_is_rooted_before_later_imports_trigger_collection() {
    let runtime = build(&["table Rule { make: fn() -> [int] => { [42] }; consume: fn(first: [int], allocations: [string]) -> int => { first[0] }; }"], "r: Rule {}");
    let value = invoke(&runtime, "Rule", "r", "make");
    let HostValue::Existing { value: id, .. } = &value else { panic!("array"); };
    runtime.release_value(*id).unwrap();
    let target = runtime.field(runtime.record("Rule", "r").unwrap(), "consume").unwrap();
    let pressure = HostValue::Array((0..2048).map(|i| HostValue::String(i.to_string())).collect());
    assert_eq!(int(runtime.invoke(target, &[value, pressure], ExecutionLimits::default()).unwrap()), 42);
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}

#[test]
fn repeated_scalar_root_maps_release_temporary_strings_in_long_loops() {
    let runtime = build(&[r#"table Rule { run: fn() -> int => { var total: int = 0; for i in 0..20000 { total += i.string().len(); } total }; }"#], "r: Rule {}");
    let target = runtime.field(runtime.record("Rule", "r").unwrap(), "run").unwrap();
    let result = runtime.invoke(target, &[], ExecutionLimits { max_heap_bytes: 512 * 1024, ..ExecutionLimits::default() }).unwrap();
    assert_eq!(int(result), 88890);
    assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
}
#[test]
fn out_of_range_external_ids_are_errors_before_compact_encoding() {
    let runtime = build(&["table Rule {}"], "r: Rule {}");
    assert!(runtime.invoke(u64::MAX, &[], ExecutionLimits::default()).is_err());
    assert!(runtime.equals(u64::MAX, u64::MAX).is_err());
}

#[test]
fn paused_outer_frames_keep_arrays_and_closure_captures_across_host_gc_and_reentry() {
    let schema = contract(&[r#"@Host singleton Service { collect: fn() -> int; }
        table Rule {
            inner: fn() -> int => { var count: int = 0; for i in 0..4000 { count += i.string().len(); } count };
            run: fn() -> int => { var values: [string] = ["alive", "root"]; var saved: fn() -> int = fn() -> int { values[0].len() }; Service.collect(); saved() + values[1].len() };
        }"#]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(schema.clone()); builder.optimization_profile(profile);
        builder.bind("Service".into(), Arc::new(Service::default())).unwrap(); builder.add_text("r: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        REENTER_RUNTIME.with(|cell| *cell.borrow_mut() = Some(Arc::downgrade(&runtime)));
        assert_eq!(int(invoke(&runtime, "Rule", "r", "run")), 9);
        assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
    }
}

#[test]
fn temporary_set_keys_share_budget_and_failed_calls_release_reservations() {
    let keys = (0..64).map(|index| format!("\"{index}{}\"", "x".repeat(1024))).collect::<Vec<_>>().join(",");
    let runtime = build(&["table Rule { items: [string]; run: fn() -> bool => { self.items.isUnique() }; }"], &format!("rule: Rule {{ items: [{keys}] }}"));
    let run = runtime.field(runtime.record("Rule", "rule").unwrap(), "run").unwrap();
    let limits = ExecutionLimits { max_heap_bytes: 32 * 1024, ..ExecutionLimits::default() };
    for _ in 0..3 {
        let error = runtime.invoke(run, &[], limits).unwrap_err().to_string();
        assert!(error.contains("内存预算"), "{error}");
        assert!(matches!(runtime.invoke(run, &[], ExecutionLimits::default()).unwrap(), HostValue::Bool(true)));
    }
}

#[test]
fn repeated_captureless_closure_creation_keeps_identity_in_both_profiles() {
    let contract = contract(&["table Rule { make: fn() -> fn() -> int => { fn() -> int { 7 } }; run: fn() -> int => { var first: fn() -> int = self.make(); var second: fn() -> int = self.make(); var copied: fn() -> int = first; if first == second || first != copied { 0 } else { first() + second() } }; }"]);
    for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
        let mut builder = RuntimeBuilder::new(contract.clone());
        builder.optimization_profile(profile);
        builder.add_text("rule: Rule {}", Some("identity.cfd"));
        let runtime = builder.build().runtime.unwrap();
        assert_eq!(int(invoke(&runtime, "Rule", "rule", "run")), 14);
    }
}
