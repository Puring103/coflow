use coflow_core::{
    contract::Contract,
    runtime::{HostService, HostValue, Runtime, RuntimeBuilder},
    schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId},
    vm::{executor::ExecutionLimits, ExecutionError},
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, Weak,
};

fn contract(sources: &[&str]) -> Arc<Contract> {
    let modules =
        parse_modules(sources.iter().enumerate().map(|(i, source)| {
            CftFile::from_source(ModuleId::from(format!("module{i}")), *source)
        }));
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
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
            "self.run()",
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

#[derive(Debug, Default)]
struct Service {
    runtime: Mutex<Weak<Runtime>>,
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
            "reenter" => {
                let runtime = self.runtime.lock().unwrap().upgrade().unwrap();
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
    *service.runtime.lock().unwrap() = Arc::downgrade(&runtime);
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
    let dimensions =
        CftDimensionInputs::try_new([("language", vec!["zh".into(), "en".into()])]).unwrap();
    let schema = build_schema(&modules, &dimensions).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.add_text(
        "a: Item { name: f\"base{self.value}\" } a: Item_name_language { zh: f\"中{self.value}\" }",
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
    let child = values[0];
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
    let schema = build_schema(
        &parse_modules([CftFile::from_source(ModuleId::from("bad.cft"), source)]),
        &CftDimensionInputs::default(),
    )
    .unwrap();
    let coflow_core::contract::ContractError::Compilation(error) =
        Contract::new(schema).unwrap_err()
    else {
        panic!("compile diagnostic")
    };
    assert_eq!(error.module.as_str(), "bad.cft");
    assert_eq!(&source[error.span.start..error.span.end], "\"错误\"");
}
