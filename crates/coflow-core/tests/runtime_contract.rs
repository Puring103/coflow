use coflow_core::{
    contract::Contract,
    loading::SourceInput,
    runtime::{HostService, HostValue, Runtime, RuntimeBuilder, Value},
    schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftValueType, ModuleId},
    vm::ExecutionError,
};
use std::sync::Arc;

fn contract(source: &str) -> Arc<Contract> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    Arc::new(
        Contract::new(
            build_schema(&modules, &CftDimensionInputs::default()).expect("valid declarations"),
        )
        .expect("serializable contract"),
    )
}
fn runtime(source: &str, data: &str) -> Arc<Runtime> {
    let mut builder = RuntimeBuilder::new(contract(source));
    builder.add_source(SourceInput::new("data.cfd", data));
    builder.build().runtime.expect("valid data")
}

#[test]
fn contract_roundtrip_preserves_aliases_and_detects_corruption() {
    let contract = contract("namespace game; type Count = int; table Item { count: Count; }");
    let bytes = contract.to_bytes().expect("serialize");
    let decoded = Contract::from_bytes(&bytes).expect("load contract without recompilation");
    assert_eq!(contract.identity(), decoded.identity());
    let alias =
        coflow_language::cft::syntax::parser::parse_type("game::Count").expect("alias syntax");
    assert_eq!(
        decoded
            .schema()
            .resolve_type_ref(&alias)
            .expect("alias retained"),
        CftValueType::Int
    );
    let mut corrupted = bytes;
    *corrupted.last_mut().expect("nonempty") ^= 1;
    assert!(Contract::from_bytes(&corrupted).is_err());
}

#[test]
fn namespaces_records_data_and_boolean_dictionary_keys_load_together() {
    let runtime = runtime(
        "namespace game; data Stats { hp: int; } table Item { stats: Stats; link: Item? = None; labels: {bool: string}; }",
        "use game::Item; use game::Stats; a: Item { stats: Stats { hp: 7 }, link: &Item::b, labels: { true: \"yes\", false: \"no\" } } b: Item { stats: Stats { hp: 9 }, labels: {} }",
    );
    let a = runtime.record("game::Item", "a").expect("record a");
    let b = runtime.record("game::Item", "b").expect("record b");
    assert!(runtime
        .equals(runtime.field(a, "link").expect("link"), b)
        .expect("identity comparison"));
    let stats = runtime.field(a, "stats").expect("stats");
    assert!(matches!(
        runtime
            .value(runtime.field(stats, "hp").expect("hp"))
            .expect("value")
            .as_ref(),
        Value::Int(7)
    ));
    assert!(runtime.records("game::Stats").is_err());
    runtime.release();
    assert_eq!(
        runtime.value(a).expect_err("released"),
        ExecutionError::Released
    );
}

#[test]
fn record_identity_and_data_content_are_distinct() {
    let runtime = runtime(
        "data Stats { hp: int; } table Item { stats: Stats; }",
        "a: Item { stats: Stats { hp: 1 } } b: Item { stats: Stats { hp: 1 } }",
    );
    let a = runtime.record("Item", "a").expect("a");
    let b = runtime.record("Item", "b").expect("b");
    assert!(!runtime.equals(a, b).expect("different records"));
    assert!(runtime
        .equals(
            runtime.field(a, "stats").expect("stats"),
            runtime.field(b, "stats").expect("stats")
        )
        .expect("equal content"));
}

#[test]
fn functions_and_templates_are_compiled_and_read_explicitly() {
    let runtime = runtime("table Item { name: string; text: fstring = f\"{self.name}\"; run: fn() -> int => { 42 }; }", "a: Item { name: \"sword\" }");
    let a = runtime.record("Item", "a").expect("a");
    let text = runtime.field(a, "text").expect("template");
    assert!(
        matches!(runtime.value(text).expect("stored template").as_ref(), Value::Template { owner: Some(owner), .. } if *owner == a)
    );
    assert_eq!(runtime.read_text(text).expect("execute template"), "sword");
    let result = runtime.invoke(runtime.field(a,"run").expect("function"), &[], coflow_core::vm::executor::ExecutionLimits::default()).expect("execute function");
    assert!(matches!(result, HostValue::Int(42)));
}

#[derive(Debug)]
struct Environment;
impl HostService for Environment {
    fn read(&self, _: &str) -> Result<HostValue, ExecutionError> {
        Ok(HostValue::String("Unity".into()))
    }
    fn has_member(
        &self,
        field: &str,
        ty: &CftValueType,
        _: &coflow_core::schema::CftSchema,
    ) -> bool {
        field == "name" && ty == &CftValueType::String
    }
}
#[test]
fn missing_host_is_lazy_and_bound_data_is_readable() {
    let contract = contract("@Host singleton Services { name: string; }");
    let missing = RuntimeBuilder::new(contract.clone())
        .build()
        .runtime
        .expect("binding may be absent");
    let service = missing
        .record("Services", "Services")
        .expect("service identity");
    assert!(matches!(
        missing.read_text(missing.field(service, "name").expect("member")),
        Err(ExecutionError::MissingHostBinding(_))
    ));
    let mut builder = RuntimeBuilder::new(contract);
    builder
        .bind("Services".into(), Arc::new(Environment))
        .expect("binding");
    let bound = builder.build().runtime.expect("runtime");
    let service = bound
        .record("Services", "Services")
        .expect("service identity");
    assert_eq!(
        bound
            .read_text(bound.field(service, "name").expect("member"))
            .expect("Host data"),
        "Unity"
    );
}

#[test]
fn data_cannot_be_loaded_as_a_record_and_record_keys_span_inheritance() {
    let contract = contract("data Stats { hp: int; } table Base {} table Child: Base {}");
    for source in ["a: Stats { hp: 1 }", "a: Base {} a: Child {}"] {
        let mut builder = RuntimeBuilder::new(contract.clone());
        builder.add_source(SourceInput::new("bad.cfd", source));
        assert!(builder.build().runtime.is_err());
    }
}

#[test]
fn dimension_records_exist_without_overrides_and_keep_business_template_owner() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "table Item { @localized name: fstring; }",
    )]);
    let dimensions = CftDimensionInputs::try_new([("language", vec!["zh".into(), "en".into()])])
        .expect("dimensions");
    let contract = Arc::new(
        Contract::new(build_schema(&modules, &dimensions).expect("schema")).expect("contract"),
    );
    let mut builder = RuntimeBuilder::new(contract);
    builder.add_source(SourceInput::new("data.cfd", "a: Item { name: f\"base\" } b: Item { name: f\"base\" } a: Item_name_language { zh: f\"覆盖\", unknown: \"ignored\" }"));
    let runtime = builder.build().runtime.expect("dimension data");
    for key in ["a", "b"] {
        let business = runtime.record("Item", key).expect("business");
        let dimension = runtime.field(business, "name").expect("dimension");
        let base = runtime.dimension_default(dimension).expect("base");
        assert!(
            matches!(runtime.value(base).expect("template").as_ref(), Value::Template { owner: Some(owner), .. } if *owner == business)
        );
        assert_eq!(
            runtime
                .read_text(runtime.field(dimension, "id").expect("generated id"))
                .expect("id"),
            key
        );
    }
}

#[derive(Debug)]
struct BlockingHost {
    entered: Arc<std::sync::Barrier>,
    resume: Arc<std::sync::Barrier>,
}
impl HostService for BlockingHost {
    fn read(&self, _: &str) -> Result<HostValue, ExecutionError> {
        self.entered.wait();
        self.resume.wait();
        Ok(HostValue::Int(1))
    }
    fn has_member(&self, _: &str, ty: &CftValueType, _: &coflow_core::schema::CftSchema) -> bool {
        ty == &CftValueType::Int
    }
}
#[test]
fn concurrent_host_access_reports_busy_and_runtime_can_change_threads() {
    let entered = Arc::new(std::sync::Barrier::new(2));
    let resume = Arc::new(std::sync::Barrier::new(2));
    let mut builder = RuntimeBuilder::new(contract("@Host singleton Services { value: int; }"));
    builder
        .bind(
            "Services".into(),
            Arc::new(BlockingHost {
                entered: entered.clone(),
                resume: resume.clone(),
            }),
        )
        .expect("bind");
    let runtime = builder.build().runtime.expect("runtime");
    let field = runtime
        .field(
            runtime.record("Services", "Services").expect("service"),
            "value",
        )
        .expect("field");
    let worker_runtime = runtime.clone();
    let worker = std::thread::spawn(move || worker_runtime.value(field));
    entered.wait();
    let busy = runtime.value(field);
    resume.wait();
    assert_eq!(
        busy.expect_err("second execution must not block"),
        ExecutionError::RuntimeBusy
    );
    assert!(matches!(
        worker.join().expect("worker").expect("Host read").as_ref(),
        Value::Int(1)
    ));
    assert!(runtime.record("Services", "Services").is_ok());
}

#[test]
fn copied_constant_callables_keep_creation_identity_and_binding() {
    let runtime = runtime(
        r#"data Payload { name: string; text: fstring; run: fn() -> int; }
        const PAYLOAD: Payload = Payload { name: "constant", text: f"{self.name}", run: fn() -> int { 1 } };
        table Item { payload: Payload = PAYLOAD; }"#,
        "a: Item {} b: Item {}",
    );
    let a = runtime
        .field(runtime.record("Item", "a").expect("a"), "payload")
        .expect("payload");
    let b = runtime
        .field(runtime.record("Item", "b").expect("b"), "payload")
        .expect("payload");
    let first = runtime.field(a, "run").expect("run");
    let second = runtime.field(b, "run").expect("run");
    assert!(runtime
        .equals(first, second)
        .expect("shared function identity"));
    let text = runtime.field(a, "text").expect("text");
    assert_eq!(text, runtime.field(b, "text").expect("text"));
    let stored = runtime.value(text).expect("template");
    let Value::Template {
        owner: Some(owner), ..
    } = stored.as_ref()
    else {
        panic!("constant object template")
    };
    assert_ne!(*owner, a);
    assert_ne!(*owner, b);
    assert_eq!(
        runtime
            .read_text(runtime.field(*owner, "name").expect("constant name"))
            .expect("name"),
        "constant"
    );
}

#[test]
fn special_floats_survive_editor_wire_roundtrip() {
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -0.0] {
        let encoded =
            serde_json::to_string(&coflow_core::CfdValue::Float(value)).expect("serialize");
        assert!(!encoded.contains("null"));
        let decoded: coflow_core::CfdValue = serde_json::from_str(&encoded).expect("deserialize");
        let coflow_core::CfdValue::Float(decoded) = decoded else {
            panic!("float variant")
        };
        if value.is_nan() {
            assert!(decoded.is_nan());
        } else {
            assert_eq!(value.to_bits(), decoded.to_bits());
        }
    }
}

#[test]
fn dimension_metadata_does_not_shadow_variant_names_and_unknown_variants_fall_back() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "table Item { @localized name: string; }",
    )]);
    let dimensions =
        CftDimensionInputs::try_new([("language", vec!["record".into()])]).expect("dimension");
    let schema = build_schema(&modules, &dimensions).expect("schema");
    let field = schema
        .resolve_type("Item")
        .expect("Item")
        .field("name")
        .expect("name");
    assert!(matches!(
        field.runtime_value_type(),
        CftValueType::RecordRef(_)
    ));
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).expect("contract")));
    builder.add_source(SourceInput::new(
        "data.cfd",
        r#"a: Item { name: "base" } a: Item_name_language { record: "override" }"#,
    ));
    let runtime = builder.build().runtime.expect("data");
    let dimension = runtime
        .field(runtime.record("Item", "a").expect("a"), "name")
        .expect("dimension");
    assert_eq!(
        runtime
            .read_text(
                runtime
                    .dimension_variant(dimension, "record")
                    .expect("variant")
            )
            .expect("text"),
        "override"
    );
    assert_eq!(
        runtime
            .read_text(
                runtime
                    .dimension_variant(dimension, "missing")
                    .expect("fallback")
            )
            .expect("text"),
        "base"
    );
}
