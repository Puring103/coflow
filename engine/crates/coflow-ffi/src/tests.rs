use super::*;
use std::sync::Mutex;
use coflow_core::schema::{build_schema, parse_modules, CftFile, ModuleId};

#[derive(Debug)]
struct ThreadProbe(Arc<Mutex<Vec<std::thread::ThreadId>>>);
impl Drop for ThreadProbe {
    fn drop(&mut self) {
        self.0.lock().unwrap().push(std::thread::current().id());
    }
}
impl coflow_core::runtime::HostService for ThreadProbe {
    fn read(
        &self,
        _: &str,
    ) -> Result<coflow_core::runtime::HostValue, coflow_core::vm::ExecutionError> {
        Ok(coflow_core::runtime::HostValue::Int(1))
    }
    fn has_member(
        &self,
        _: &str,
        _: &coflow_core::schema::CftValueType,
        _: &coflow_core::schema::CftSchema,
    ) -> bool {
        true
    }
}
fn thread_runtime(drops: Arc<Mutex<Vec<std::thread::ThreadId>>>) -> u64 {
    let schema = build_schema(&parse_modules([CftFile::from_source(
        ModuleId::from("thread"),
        "@Host singleton Service { value: int; }",
    )]))
    .unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder
        .bind("Service".into(), Arc::new(ThreadProbe(drops)))
        .unwrap();
    let runtime = Arc::try_unwrap(builder.build().runtime.unwrap()).unwrap();
    insert(Entry::Runtime(ThreadBound::new(runtime))).unwrap()
}

#[test]
fn foreign_access_and_release_leave_creator_resources_untouched() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let observed = drops.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let id = thread_runtime(observed);
        ready_tx.send((id, std::thread::current().id())).unwrap();
        resume_rx.recv().unwrap();
        assert!(get(id).is_ok());
        assert_eq!(coflow_dispose(id), 0);
    });
    let (id, creator) = ready_rx.recv().unwrap();
    assert!(get(id).unwrap_err().contains("creating thread"));
    coflow_release(id);
    assert!(drops.lock().unwrap().is_empty());
    resume_tx.send(()).unwrap();
    thread.join().unwrap();
    assert_eq!(*drops.lock().unwrap(), [creator]);
    assert!(get(id).is_err());
}

#[test]
fn creator_thread_exit_releases_unclaimed_runtime_without_leaking_registry_entry() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let observed = drops.clone();
    let (id, creator) =
        std::thread::spawn(move || (thread_runtime(observed), std::thread::current().id()))
            .join()
            .unwrap();
    assert_eq!(*drops.lock().unwrap(), [creator]);
    assert!(get(id).is_err());
}

#[test]
fn thread_shutdown_releases_all_local_resources_and_is_idempotent() {
    let drops = Arc::new(Mutex::new(Vec::new()));
    let first = thread_runtime(drops.clone());
    let second = thread_runtime(drops.clone());
    assert_eq!(coflow_thread_shutdown(), 0);
    assert_eq!(drops.lock().unwrap().len(), 2);
    assert!(get(first).is_err());
    assert!(get(second).is_err());
    assert_eq!(coflow_thread_shutdown(), 0);
}

static ACTIVE_RUNTIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ACTIVE_DISPOSE_STATUS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static ACTIVE_SHUTDOWN_STATUS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

unsafe extern "C" fn disposing_host(
    _context: u64,
    op: u32,
    data: *const u8,
    length: usize,
    out: *mut Response,
) {
    if op == 0 {
        unsafe {
            out.write(buffer(b"fn() -> int".to_vec()).unwrap());
        }
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, length) };
    let name_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    assert_eq!(&bytes[4..4 + name_len], b"dispose");
    let runtime = ACTIVE_RUNTIME.load(std::sync::atomic::Ordering::SeqCst);
    ACTIVE_DISPOSE_STATUS.store(coflow_dispose(runtime), std::sync::atomic::Ordering::SeqCst);
    ACTIVE_SHUTDOWN_STATUS.store(
        coflow_thread_shutdown(),
        std::sync::atomic::Ordering::SeqCst,
    );
    unsafe {
        out.write(Response {
            tag: 2,
            integer: 7,
            ..Response::default()
        });
    }
}

#[test]
fn active_host_callback_cannot_dispose_or_shutdown_runtime() {
    let contract = contract("@Host singleton Service { dispose: fn() -> int; } table Rule { run: fn() -> int => { Service.dispose() }; }");
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        unsafe {
            coflow_bind_host(
                builder.0,
                b"Service".as_ptr(),
                7,
                0,
                Some(disposing_host),
                Some(release_function_host),
            )
        },
        0
    );
    request(11, builder.0, b"data.cfd", b"r: Rule {}", 0);
    let built = request(12, builder.0, &[], &[], 0);
    assert_eq!(built.error, 0);
    let runtime_id = built.handle;
    ACTIVE_RUNTIME.store(runtime_id, std::sync::atomic::Ordering::SeqCst);
    let record = request(20, runtime_id, b"Rule", b"r", 0).handle;
    let function = value_request(23, runtime_id, record, b"run", &[], 0).handle;
    let result = value_request(29, runtime_id, function, &[], &[], 0);
    assert_eq!((result.error, result.tag, result.integer), (0, 2, 7));
    assert_eq!(
        ACTIVE_DISPOSE_STATUS.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(
        ACTIVE_SHUTDOWN_STATUS.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert_eq!(coflow_dispose(runtime_id), 0);
    assert!(get(runtime_id).is_err());
}

#[test]
fn operation_codes_match_the_public_header_and_csharp_runtime() {
    let header = include_str!("../include/coflow.h");
    let csharp = include_str!("../../../runtimes/csharp/src/Coflow.Runtime/src/Native.cs");
    let operations = [
        (
            "COFLOW_LOAD_CONTRACT",
            "LoadContract",
            Operation::LoadContract,
        ),
        (
            "COFLOW_CONTRACT_IDENTITY",
            "ContractIdentity",
            Operation::ContractIdentity,
        ),
        (
            "COFLOW_NEW_BUILDER",
            "CreateBuilder",
            Operation::CreateBuilder,
        ),
        ("COFLOW_ADD_CFD", "AddDataSource", Operation::AddDataSource),
        (
            "COFLOW_BUILD_RUNTIME",
            "BuildRuntime",
            Operation::BuildRuntime,
        ),
        (
            "COFLOW_READ_DYNAMIC_VALUE",
            "ReadDynamicValue",
            Operation::ReadDynamicValue,
        ),
        (
            "COFLOW_CREATE_VALUE_LEASE",
            "CreateValueLease",
            Operation::CreateValueLease,
        ),
        ("COFLOW_READ_RECORD", "ReadRecord", Operation::ReadRecord),
        ("COFLOW_RECORD_COUNT", "TableLength", Operation::TableLength),
        ("COFLOW_RECORD_AT", "TableValue", Operation::TableValue),
        ("COFLOW_FIELD", "ReadField", Operation::ReadField),
        ("COFLOW_DESCRIBE", "InspectValue", Operation::InspectValue),
        ("COFLOW_TEXT", "ReadText", Operation::ReadText),
        ("COFLOW_ARRAY_AT", "ArrayValue", Operation::ArrayValue),
        (
            "COFLOW_DICT_KEY_AT",
            "DictionaryKey",
            Operation::DictionaryKey,
        ),
        (
            "COFLOW_DICT_VALUE_AT",
            "DictionaryValue",
            Operation::DictionaryValue,
        ),
        ("COFLOW_CALL", "Invoke", Operation::Invoke),
        ("COFLOW_TYPE_NAME", "TypeName", Operation::TypeName),
        (
            "COFLOW_PROGRAM_SOURCE",
            "ProgramSource",
            Operation::ProgramSource,
        ),
        (
            "COFLOW_TRY_RECORD",
            "TryFindRecord",
            Operation::TryFindRecord,
        ),
        (
            "COFLOW_DIMENSION_VALUE",
            "DimensionVariant",
            Operation::DimensionVariant,
        ),
        ("COFLOW_VALUE_EQUALS", "ValueEquals", Operation::ValueEquals),
        (
            "COFLOW_DIMENSION_DEFAULT",
            "DimensionDefault",
            Operation::DimensionDefault,
        ),
        (
            "COFLOW_DIMENSION_VARIANT_KEY",
            "DimensionVariantKey",
            Operation::DimensionVariantKey,
        ),
        ("COFLOW_SINGLETON", "Singleton", Operation::Singleton),
        (
            "COFLOW_DICT_FIND",
            "DictionaryFind",
            Operation::DictionaryFind,
        ),
        (
            "COFLOW_CANONICAL_VALUE",
            "CanonicalValue",
            Operation::CanonicalValue,
        ),
        ("COFLOW_NEW_BUFFER", "CreateBuffer", Operation::CreateBuffer),
        ("COFLOW_RUN_CHECKS", "RunChecks", Operation::RunChecks),
    ];
    for (header_name, csharp_name, operation) in operations {
        let value = operation as u32;
        assert!(header.contains(&format!("{header_name} = {value}")));
        assert!(csharp.contains(&format!("{csharp_name} = {value}")));
    }
}

struct Handle(u64);
impl Drop for Handle {
    fn drop(&mut self) {
        coflow_release(self.0);
    }
}
fn request(op: u32, handle: u64, key: &[u8], data: &[u8], index: u64) -> Response {
    value_request(op, handle, 0, key, data, index)
}
fn value_request(
    op: u32,
    handle: u64,
    value: u64,
    key: &[u8],
    data: &[u8],
    index: u64,
) -> Response {
    let mut result = Response::default();
    // 测试通过真实 ABI 传递有效切片，覆盖输出初始化和错误缓冲区所有权。
    unsafe {
        coflow_request(
            op,
            handle,
            value,
            key.as_ptr(),
            key.len(),
            data.as_ptr(),
            data.len(),
            index,
            &mut result,
        );
    }
    result
}
fn contract(source: &str) -> Handle {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("test"), source)]);
    let contract = Contract::new(build_schema(&modules).expect("schema")).expect("contract");
    let result = request(1, 0, &[], &contract.to_bytes().expect("bytes"), 0);
    assert_eq!(result.error, 0);
    Handle(result.handle)
}

#[test]
fn abi_values_share_runtime_lifetime() {
    let contract = contract("table Item { value: int; }");
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        request(11, builder.0, b"data.cfd", b"a: Item { value: 42 }", 0).error,
        0
    );
    let built = request(12, builder.0, &[], &[], 0);
    assert_eq!(built.error, 0);
    let runtime = Handle(built.handle);
    let record = request(20, runtime.0, b"Item", b"a", 0).handle;
    let field = value_request(23, runtime.0, record, b"value", &[], 0).handle;
    let value = value_request(24, runtime.0, field, &[], &[], 0);
    assert_eq!((value.error, value.tag, value.integer), (0, 2, 42));
    let runtime_id = runtime.0;
    drop(runtime);
    let failure = value_request(24, runtime_id, field, &[], &[], 0);
    assert_ne!(failure.error, 0);
    assert!(!take_buffer(failure.handle).expect("error text").is_empty());
}

#[test]
fn invalid_input_returns_owned_error_and_never_reuses_handle() {
    let first = Handle(buffer(b"first".to_vec()).expect("buffer").handle);
    let id = first.0;
    drop(first);
    let next = Handle(buffer(b"next".to_vec()).expect("buffer").handle);
    assert!(next.0 > id);
    let mut result = Response {
        integer: 123,
        ..Response::default()
    };
    let code = unsafe {
        coflow_request(
            1,
            0,
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            1,
            0,
            &mut result,
        )
    };
    assert_ne!(code, 0);
    assert_eq!(result.integer, 0);
    assert!(
        String::from_utf8(take_buffer(result.handle).expect("owned error"))
            .expect("UTF-8")
            .contains("null")
    );
    let error = request(24, id, &[], &[], 0);
    assert_ne!(error.error, 0);
    coflow_release(error.handle);
}

#[test]
fn abi_invokes_functions_with_arguments_and_releases_returned_closures() {
    use coflow_core::runtime::HostValue;
    let contract = contract(
        "table Rule { run: fn(value: int) -> fn() -> int => { fn() -> int { value + 1 } }; }",
    );
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        request(11, builder.0, b"data.cfd", b"r: Rule {}", 0).error,
        0
    );
    let built = request(12, builder.0, &[], &[], 0);
    assert_eq!(built.error, 0);
    let runtime = Handle(built.handle);
    let record = request(20, runtime.0, b"Rule", b"r", 0).handle;
    let function = value_request(23, runtime.0, record, b"run", &[], 0).handle;
    let mut arguments = Vec::new();
    invocation::encode_arguments_into(&mut arguments, &[HostValue::Int(41)]).unwrap();
    let closure = value_request(29, runtime.0, function, &[], &arguments, 0);
    assert_eq!((closure.error, closure.tag), (0, 11));
    assert_eq!(closure.handle, runtime.0);
    assert_eq!(request(43, runtime.0, &[], &[], 0).error, 0);
    let result = value_request(29, runtime.0, closure.length, &[], &[], 0);
    assert_eq!((result.error, result.tag, result.integer), (0, 2, 42));
    assert_eq!(
        value_request(42, runtime.0, closure.length, &[], &[], 0).error,
        0
    );
    assert_eq!(request(43, runtime.0, &[], &[], 0).error, 0);
    let result = value_request(29, runtime.0, closure.length, &[], &[], 0);
    assert_ne!(result.error, 0);
    take_buffer(result.handle).unwrap();
    for end in 1..arguments.len() {
        let result = value_request(29, runtime.0, function, &[], &arguments[..end], 0);
        assert_ne!(result.error, 0);
        take_buffer(result.handle).unwrap();
    }
}
unsafe extern "C" fn function_host(
    _context: u64,
    op: u32,
    data: *const u8,
    length: usize,
    out: *mut Response,
) {
    let bytes = unsafe { std::slice::from_raw_parts(data, length) };
    let result = if op == 0 {
        buffer(b"fn(int) -> int".to_vec()).unwrap()
    } else {
        assert_eq!(op, 2);
        let name_len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert_eq!(&bytes[4..4 + name_len], b"twice");
        let arguments = invocation::decode_arguments(&bytes[4 + name_len..]).unwrap();
        let [coflow_core::runtime::HostValue::Int(value)] = arguments.as_slice() else {
            panic!("arguments")
        };
        Response {
            tag: 2,
            integer: i64::from(*value) * 2,
            ..Response::default()
        }
    };
    unsafe {
        out.write(result);
    }
}
unsafe extern "C" fn release_function_host(_context: u64) {}
#[test]
fn abi_host_function_uses_synchronous_typed_callback() {
    let contract=contract("@Host singleton Service { twice: fn(int) -> int; } table Rule { run: fn() -> int => { Service.twice(21) }; }");
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        unsafe {
            coflow_bind_host(
                builder.0,
                b"Service".as_ptr(),
                7,
                0,
                Some(function_host),
                Some(release_function_host),
            )
        },
        0
    );
    request(11, builder.0, b"data.cfd", b"r: Rule {}", 0);
    let built = request(12, builder.0, &[], &[], 0);
    assert_eq!(built.error, 0);
    let runtime = Handle(built.handle);
    let record = request(20, runtime.0, b"Rule", b"r", 0).handle;
    let function = value_request(23, runtime.0, record, b"run", &[], 0).handle;
    let result = value_request(29, runtime.0, function, &[], &[], 0);
    assert_eq!((result.error, result.tag, result.integer), (0, 2, 42));
}

#[test]
fn value_image_and_creator_thread_lease_release_preserve_dynamic_graphs() {
    let contract = contract(
        "table Rule { run: fn() -> fn() -> int => { var n: int = 42; fn() -> int { n } }; }",
    );
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        request(11, builder.0, b"data.cfd", b"r: Rule {}", 0).error,
        0
    );
    let runtime = Handle(request(12, builder.0, &[], &[], 0).handle);
    let record = request(20, runtime.0, b"Rule", b"r", 0).handle;
    let function = value_request(23, runtime.0, record, b"run", &[], 0).handle;
    let closure = value_request(29, runtime.0, function, &[], &[], 0);
    let projected = value_request(47, runtime.0, closure.length, &[], &[], 0);
    assert_eq!(projected.error, 0);
    let bytes = take_buffer(projected.handle).unwrap();
    assert_eq!(&bytes[..8], b"CFVI\x01\x00\x00\x00");
    let lease = value_request(48, runtime.0, closure.length, &[], &[], 1);
    assert_eq!(lease.error, 0);
    assert_eq!(request(43, runtime.0, &[], &[], 0).error, 0);
    assert_eq!(
        value_request(29, runtime.0, closure.length, &[], &[], 0).integer,
        42
    );
    assert_eq!(coflow_dispose(lease.handle), 0);
    assert_eq!(request(43, runtime.0, &[], &[], 0).error, 0);
    let stale = value_request(29, runtime.0, closure.length, &[], &[], 0);
    assert_ne!(stale.error, 0);
    take_buffer(stale.handle).unwrap();
}

#[test]
fn contract_and_buffer_handles_are_creator_thread_owned() {
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("local"), "table Item { value: int; }")])).unwrap();
    let contract = insert(Entry::Contract(Arc::new(Contract::new(schema).unwrap()))).unwrap();
    let bytes = vec![1, 2, 3, 4];
    let pointer = bytes.as_ptr();
    let buffer = insert(Entry::Buffer(ThreadBound::new(bytes))).unwrap();
    std::thread::spawn(move || {
        assert!(get(contract).is_err());
        assert!(take_buffer(buffer).is_err());
        assert_eq!(coflow_dispose(contract), 1);
        coflow_release(buffer);
    }).join().unwrap();
    assert!(get(contract).is_ok());
    let consumed = take_buffer(buffer).unwrap();
    assert_eq!(consumed.as_ptr(), pointer, "独占缓冲消费不复制载荷");
    assert_eq!(consumed, [1, 2, 3, 4]);
    assert!(get(buffer).is_err());
    assert_eq!(coflow_dispose(contract), 0);
}

#[test]
fn shutdown_blocks_resource_creation_from_host_destructors() {
    #[derive(Debug)]
    struct Reenter;
    impl coflow_core::runtime::HostService for Reenter {
        fn read(&self, _: &str) -> Result<coflow_core::runtime::HostValue, coflow_core::vm::ExecutionError> { unreachable!() }
        fn has_member(&self, _: &str, _: &coflow_core::schema::CftValueType, _: &coflow_core::schema::CftSchema) -> bool { true }
    }
    impl Drop for Reenter {
        fn drop(&mut self) {
            assert!(insert(Entry::Buffer(ThreadBound::new(vec![1]))).is_err());
            assert_eq!(coflow_thread_shutdown(), 1);
        }
    }
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("shutdown"), "@Host singleton Service { value: int; }")])).unwrap();
    let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
    builder.bind("Service".into(), Arc::new(Reenter)).unwrap();
    insert(Entry::Builder(ThreadBound::new(RefCell::new(Some(builder))))).unwrap();
    assert_eq!(coflow_thread_shutdown(), 0);
    let buffer = insert(Entry::Buffer(ThreadBound::new(vec![2]))).unwrap();
    assert_eq!(coflow_dispose(buffer), 0);
}

#[test]
fn busy_builder_cannot_be_disposed_or_shut_down() {
    let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("busy"), "table Item { value: int; }")])).unwrap();
    let builder = ThreadBound::new(RefCell::new(Some(RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap())))));
    let id = insert(Entry::Builder(builder.clone())).unwrap();
    let borrowed = builder.borrow_mut();
    assert_eq!(coflow_dispose(id), 1);
    assert_eq!(coflow_thread_shutdown(), 1);
    assert!(get(id).is_ok());
    drop(borrowed);
    assert_eq!(coflow_dispose(id), 0);
}

#[test]
fn operation_declarations_match_native_and_managed_protocols() {
    fn declarations<'a>(source: &'a str, marker: &str) -> std::collections::BTreeMap<&'a str, u32> {
        source.split_once(marker).unwrap().1.split_once('{').unwrap().1
            .split_once('}').unwrap().0.lines().filter_map(|line| {
                let (name, value) = line.trim().trim_end_matches(',').split_once('=')?;
                Some((name.trim(), value.trim().parse().unwrap()))
            }).collect()
    }
    let header = declarations(include_str!("../include/coflow.h"), "enum CoflowOperation");
    let managed = declarations(include_str!("../../../runtimes/csharp/src/Coflow.Runtime/src/Native.cs"), "enum NativeOperation");
    let expected = super::OPERATIONS.iter().map(|(_, c, value)| (*c, *value)).collect();
    assert_eq!(header, expected);
    for (name, value) in managed {
        assert!(super::OPERATIONS.iter().any(|(rust, _, number)| *rust == name && *number == value), "managed opcode {name}");
    }
    for (_, _, value) in super::OPERATIONS {
        assert_eq!(super::Operation::try_from(*value).unwrap() as u32, *value);
    }
    assert!(super::Operation::try_from(5).is_err());
    assert_eq!(std::mem::size_of::<super::Response>(), 40);
    assert_eq!(std::mem::offset_of!(super::Response, error), 36);
}
