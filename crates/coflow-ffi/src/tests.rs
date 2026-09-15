use super::*;
use coflow_core::schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};

struct Handle(u64);
impl Drop for Handle {
    fn drop(&mut self) {
        coflow_release(self.0);
    }
}
fn request(op: u32, handle: u64, key: &[u8], data: &[u8], index: u64) -> Response {
    let mut result = Response::default();
    // 测试通过真实 ABI 传递有效切片，覆盖输出初始化和错误缓冲区所有权。
    unsafe {
        coflow_request(
            op,
            handle,
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
    let contract =
        Contract::new(build_schema(&modules, &CftDimensionInputs::default()).expect("schema"))
            .expect("contract");
    let result = request(1, 0, &[], &contract.to_bytes().expect("bytes"), 0);
    assert_eq!(result.error, 0);
    Handle(result.handle)
}

#[test]
fn abi_load_read_retain_and_release() {
    let contract = contract("table Item { value: int; }");
    let builder = Handle(request(10, contract.0, &[], &[], 0).handle);
    assert_eq!(
        request(11, builder.0, b"data.cfd", b"a: Item { value: 42 }", 0).error,
        0
    );
    let built = request(12, builder.0, &[], &[], 0);
    assert_eq!(built.error, 0);
    let runtime = Handle(built.handle);
    let record = Handle(request(20, runtime.0, b"Item", b"a", 0).handle);
    let field = Handle(request(23, record.0, b"value", &[], 0).handle);
    let retained = Handle(request(33, field.0, &[], &[], 0).handle);
    drop(field);
    let value = request(24, retained.0, &[], &[], 0);
    assert_eq!((value.error, value.tag, value.integer), (0, 2, 42));
    drop(runtime);
    let failure = request(24, retained.0, &[], &[], 0);
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
