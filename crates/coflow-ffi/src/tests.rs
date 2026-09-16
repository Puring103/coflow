use super::*;
use coflow_core::schema::{build_schema, parse_modules, CftFile, ModuleId};

#[test]
fn operation_codes_match_the_public_header_and_csharp_runtime() {
    let header = include_str!("../include/coflow.h");
    let csharp = include_str!("../../../runtimes/csharp/Coflow.Runtime/src/Native.cs");
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
