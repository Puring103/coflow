//! Unity/IL2CPP C ABI。句柄为不复用的整数，不暴露 Rust 对象地址。
#[cfg(test)]
mod tests;
use coflow_core::{
    contract::Contract,
    loading::SourceInput,
    runtime::{Runtime, RuntimeBuilder, Value, ValueId},
};
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
};

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Response {
    pub handle: u64,
    pub integer: i64,
    pub number: f64,
    pub length: u64,
    pub tag: u32,
    pub error: u32,
}

#[derive(Debug, Clone)]
enum Entry {
    Contract(Arc<Contract>),
    Builder(Arc<Mutex<Option<RuntimeBuilder>>>),
    Runtime(Arc<Runtime>),
    Value(Arc<Runtime>, ValueId),
    Buffer(Arc<Vec<u8>>),
    #[cfg(feature = "cft-compiler")]
    Compiler(Arc<Mutex<Compilation>>),
}
#[cfg(feature = "cft-compiler")]
#[derive(Debug, Default)]
struct Compilation {
    sources: Vec<coflow_core::schema::CftFile>,
    dimensions: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Default)]
struct Registry {
    next: u64,
    entries: BTreeMap<u64, Entry>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}
fn insert(entry: Entry) -> Result<u64, String> {
    let mut reg = registry()
        .lock()
        .map_err(|_| "handle registry unavailable")?;
    reg.next = reg.next.checked_add(1).ok_or("handle identity exhausted")?;
    let id = reg.next;
    reg.entries.insert(id, entry);
    Ok(id)
}
fn get(id: u64) -> Result<Entry, String> {
    registry()
        .lock()
        .map_err(|_| "handle registry unavailable")?
        .entries
        .get(&id)
        .cloned()
        .ok_or_else(|| "invalid or released handle".into())
}
fn buffer(bytes: Vec<u8>) -> Result<Response, String> {
    let length = bytes.len() as u64;
    Ok(Response {
        handle: insert(Entry::Buffer(Arc::new(bytes)))?,
        length,
        ..Response::default()
    })
}
fn value(runtime: Arc<Runtime>, id: ValueId) -> Result<Response, String> {
    runtime.ensure_value(id).map_err(|e| e.to_string())?;
    Ok(Response {
        handle: insert(Entry::Value(runtime, id))?,
        ..Response::default()
    })
}
fn target(handle: u64) -> Result<(Arc<Runtime>, ValueId), String> {
    match get(handle)? {
        Entry::Value(runtime, id) => Ok((runtime, id)),
        _ => Err("expected value handle".into()),
    }
}
fn text(bytes: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(bytes).map_err(|e| e.to_string())
}

pub type HostCallback = unsafe extern "C" fn(u64, u32, *const u8, usize, *mut Response);
pub type HostRelease = unsafe extern "C" fn(u64);
#[derive(Debug)]
struct NativeService {
    context: u64,
    callback: HostCallback,
    release: HostRelease,
}
impl NativeService {
    fn request(&self, op: u32, field: &str) -> Result<Response, String> {
        let mut result = Response::default();
        // 回调必须同步返回；不持有注册表锁，允许同线程重入。
        unsafe {
            (self.callback)(self.context, op, field.as_ptr(), field.len(), &mut result);
        }
        if result.error != 0 {
            let message = take_buffer(result.handle)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_else(|| "Host callback failed".into());
            return Err(message);
        }
        Ok(result)
    }
}
impl Drop for NativeService {
    fn drop(&mut self) {
        unsafe {
            (self.release)(self.context);
        }
    }
}
fn take_buffer(handle: u64) -> Result<Vec<u8>, String> {
    let entry = registry()
        .lock()
        .map_err(|_| "handle registry unavailable")?
        .entries
        .remove(&handle);
    match entry {
        Some(Entry::Buffer(bytes)) => Ok(bytes.as_ref().clone()),
        _ => Err("expected returned Host buffer".into()),
    }
}
impl coflow_core::runtime::HostService for NativeService {
    fn has_member(
        &self,
        field: &str,
        ty: &coflow_core::schema::CftValueType,
        schema: &coflow_core::schema::CftSchema,
    ) -> bool {
        self.request(0, field)
            .ok()
            .and_then(|r| take_buffer(r.handle).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|source| coflow_core::schema::syntax::parser::parse_type(&source).ok())
            .and_then(|syntax| schema.resolve_type_ref(&syntax).ok())
            .is_some_and(|actual| &actual == ty)
    }
    fn read(
        &self,
        field: &str,
    ) -> Result<coflow_core::runtime::HostValue, coflow_core::vm::ExecutionError> {
        use coflow_core::{runtime::HostValue, vm::ExecutionError};
        let result = self
            .request(1, field)
            .map_err(ExecutionError::InvalidAccess)?;
        match result.tag {
            0 => Ok(HostValue::None),
            1 => Ok(HostValue::Bool(result.integer != 0)),
            2 => i32::try_from(result.integer)
                .map(HostValue::Int)
                .map_err(|_| ExecutionError::InvalidAccess("Host int outside i32 range".into())),
            3 => Ok(HostValue::Float(result.number as f32)),
            4 => take_buffer(result.handle)
                .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
                .map(HostValue::String)
                .map_err(ExecutionError::InvalidAccess),
            11 => {
                // 回调提交一个独立保留句柄，读取后立即释放该传输句柄。
                let entry = registry()
                    .lock()
                    .map_err(|_| ExecutionError::InvalidHandle)?
                    .entries
                    .remove(&result.handle);
                match entry {
                    Some(Entry::Value(runtime, value)) => {
                        runtime.ensure_alive()?;
                        Ok(HostValue::Existing {
                            runtime: runtime.identity(),
                            value,
                        })
                    }
                    _ => Err(ExecutionError::InvalidHandle),
                }
            }
            _ => Err(ExecutionError::InvalidAccess(
                "invalid Host data tag".into(),
            )),
        }
    }
}

/// # Safety
/// service 必须为有效 UTF-8 切片；回调及 context 必须存活至 release 回调。
/// 有效 callback/release 一经提交，context 的所有权交给本接口，包括绑定失败路径。
#[no_mangle]
pub unsafe extern "C" fn coflow_bind_host(
    builder: u64,
    service: *const u8,
    length: usize,
    context: u64,
    callback: Option<HostCallback>,
    release: Option<HostRelease>,
) -> u32 {
    let (Some(callback), Some(release)) = (callback, release) else {
        return 1;
    };
    let service_object = Arc::new(NativeService {
        context,
        callback,
        release,
    });
    catch_unwind(AssertUnwindSafe(|| {
        if length > isize::MAX as usize || (service.is_null() && length != 0) {
            return Err("null service name".to_string());
        }
        let name = if length == 0 {
            ""
        } else {
            text(unsafe { std::slice::from_raw_parts(service, length) })?
        };
        let Entry::Builder(builder) = get(builder)? else {
            return Err("expected builder".into());
        };
        let mut guard = builder.try_lock().map_err(|_| "builder busy")?;
        guard
            .as_mut()
            .ok_or("builder already consumed")?
            .bind(name.into(), service_object)
    }))
    .ok()
    .and_then(Result::ok)
    .map_or(1, |()| 0)
}

/// 输入切片只在本次调用中借用，所有返回内容由独立句柄持有。
/// # Safety
/// 非空输入必须指向有效的连续字节；out 必须指向可写 Response。
#[no_mangle]
pub unsafe extern "C" fn coflow_request(
    op: u32,
    handle: u64,
    key: *const u8,
    key_len: usize,
    data: *const u8,
    data_len: usize,
    index: u64,
    out: *mut Response,
) -> u32 {
    if out.is_null() {
        return 1;
    }
    // 即使参数错误也初始化输出，避免托管侧读取未初始化字段。
    unsafe {
        out.write(Response::default());
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        if key_len > isize::MAX as usize
            || data_len > isize::MAX as usize
            || (key.is_null() && key_len != 0)
            || (data.is_null() && data_len != 0)
        {
            return Err("null input buffer".into());
        }
        let key = if key_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(key, key_len) }
        };
        let data = if data_len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(data, data_len) }
        };
        dispatch(op, handle, key, data, index)
    }))
    .unwrap_or_else(|_| Err("native boundary panic".into()));
    let response = match result {
        Ok(value) => value,
        Err(error) => {
            let mut response = buffer(error.into_bytes()).unwrap_or_default();
            response.error = 1;
            response
        }
    };
    unsafe {
        out.write(response);
    }
    response.error
}

fn dispatch(op: u32, handle: u64, key: &[u8], data: &[u8], index: u64) -> Result<Response, String> {
    match op {
        35 => {
            let (runtime, left) = target(handle)?;
            let (other, right) = target(index)?;
            if runtime.identity() != other.identity() {
                return Err("values belong to different Runtime instances".into());
            }
            let equal = runtime
                .equals(left, right)
                .map_err(|error| error.to_string())?;
            Ok(Response {
                tag: 1,
                integer: i64::from(equal),
                ..Response::default()
            })
        }
        9 => {
            let runtime = match get(handle)? {
                Entry::Runtime(runtime) | Entry::Value(runtime, _) => runtime,
                _ => return Err("expected Runtime or value".into()),
            };
            runtime.ensure_alive().map_err(|error| error.to_string())?;
            buffer(runtime.contract().identity().to_vec())
        }
        1 => Ok(Response {
            handle: insert(Entry::Contract(Arc::new(
                Contract::from_bytes(data).map_err(|e| e.to_string())?,
            )))?,
            ..Response::default()
        }),
        #[cfg(feature = "cft-compiler")]
        3 => Ok(Response {
            handle: insert(Entry::Compiler(Arc::new(
                Mutex::new(Compilation::default()),
            )))?,
            ..Response::default()
        }),
        #[cfg(feature = "cft-compiler")]
        4 | 5 | 6 => {
            let Entry::Compiler(compiler) = get(handle)? else {
                return Err("expected CFT compilation".into());
            };
            let mut compiler = compiler.try_lock().map_err(|_| "CFT compilation busy")?;
            if op == 4 {
                compiler
                    .sources
                    .push(coflow_core::schema::CftFile::from_source(
                        coflow_core::schema::ModuleId::from(text(key)?),
                        text(data)?,
                    ));
                return Ok(Response::default());
            }
            if op == 5 {
                let variants = text(data)?.split('\n').map(str::to_string).collect();
                if compiler
                    .dimensions
                    .insert(text(key)?.into(), variants)
                    .is_some()
                {
                    return Err("duplicate dimension".into());
                }
                return Ok(Response::default());
            }
            let modules = coflow_core::schema::parse_modules(compiler.sources.clone());
            let dimensions =
                coflow_core::schema::CftDimensionInputs::try_new(compiler.dimensions.clone())
                    .map_err(|e| e.to_string())?;
            let schema = coflow_core::schema::build_schema(&modules, &dimensions)
                .map_err(|e| format!("{e:?}"))?;
            let contract = Contract::new(schema).map_err(|e| e.to_string())?;
            Ok(Response {
                handle: insert(Entry::Contract(Arc::new(contract)))?,
                ..Response::default()
            })
        }
        7 | 8 => {
            let Entry::Contract(contract) = get(handle)? else {
                return Err("expected contract".into());
            };
            buffer(if op == 7 {
                contract.to_bytes().map_err(|e| e.to_string())?
            } else {
                contract.identity().to_vec()
            })
        }
        10 => {
            let Entry::Contract(contract) = get(handle)? else {
                return Err("expected contract".into());
            };
            Ok(Response {
                handle: insert(Entry::Builder(Arc::new(Mutex::new(Some(
                    RuntimeBuilder::new(contract),
                )))))?,
                ..Response::default()
            })
        }
        11 | 12 => {
            let Entry::Builder(builder) = get(handle)? else {
                return Err("expected builder".into());
            };
            let mut builder = builder.try_lock().map_err(|_| "builder busy")?;
            if op == 11 {
                builder
                    .as_mut()
                    .ok_or("builder already consumed")?
                    .add_source(SourceInput::new(text(key)?, text(data)?));
                return Ok(Response::default());
            }
            let runtime = builder
                .take()
                .ok_or("builder already consumed")?
                .build()
                .runtime?;
            Ok(Response {
                handle: insert(Entry::Runtime(runtime))?,
                ..Response::default()
            })
        }
        20 => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let id = runtime
                .record(text(key)?, text(data)?)
                .map_err(|e| e.to_string())?;
            value(runtime, id)
        }
        21 => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let ids = runtime.records(text(key)?).map_err(|e| e.to_string())?;
            Ok(Response {
                length: ids.len() as u64,
                ..Response::default()
            })
        }
        22 => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let ids = runtime.records(text(key)?).map_err(|e| e.to_string())?;
            let id = *ids
                .get(usize::try_from(index).map_err(|_| "index overflow")?)
                .ok_or("record index out of range")?;
            value(runtime, id)
        }
        23 => {
            let (runtime, id) = target(handle)?;
            let field = runtime.field(id, text(key)?).map_err(|e| e.to_string())?;
            value(runtime, field)
        }
        24 => {
            let (runtime, id) = target(handle)?;
            let value = runtime.value(id).map_err(|e| e.to_string())?;
            let mut response = Response::default();
            match value.as_ref() {
                Value::None => response.tag = 0,
                Value::Bool(v) => {
                    response.tag = 1;
                    response.integer = i64::from(*v);
                }
                Value::Int(v) => {
                    response.tag = 2;
                    response.integer = i64::from(*v);
                }
                Value::Float(v) => {
                    response.tag = 3;
                    response.number = f64::from(*v);
                }
                Value::String(_) => response.tag = 4,
                Value::Enum { value, .. } => {
                    response.tag = 5;
                    response.integer = i64::from(*value);
                }
                Value::Object { fields, .. } => {
                    response.tag = 6;
                    response.length = fields.len() as u64;
                }
                Value::Array(items) => {
                    response.tag = 7;
                    response.length = items.len() as u64;
                }
                Value::Dict(items) => {
                    response.tag = 8;
                    response.length = items.len() as u64;
                }
                Value::Function { .. } => response.tag = 9,
                Value::Template { .. } => response.tag = 10,
                Value::HostData { .. } => return Err("unresolved Host data".into()),
            }
            Ok(response)
        }
        25 => {
            let (runtime, id) = target(handle)?;
            buffer(
                runtime
                    .read_text(id)
                    .map_err(|e| e.to_string())?
                    .into_bytes(),
            )
        }
        26 | 27 | 28 => {
            let (runtime, id) = target(handle)?;
            let value_ref = runtime.value(id).map_err(|e| e.to_string())?;
            let index = usize::try_from(index).map_err(|_| "index overflow")?;
            let child = match (value_ref.as_ref(), op) {
                (Value::Array(items), 26) => *items.get(index).ok_or("array index out of range")?,
                (Value::Dict(items), 27) => {
                    items.get(index).ok_or("dictionary index out of range")?.0
                }
                (Value::Dict(items), 28) => {
                    items.get(index).ok_or("dictionary index out of range")?.1
                }
                _ => return Err("container operation mismatch".into()),
            };
            value(runtime, child)
        }
        29 => {
            let (runtime, id) = target(handle)?;
            runtime.call(id).map_err(|e| e.to_string())?;
            Ok(Response::default())
        }
        30 => {
            let (runtime, id) = target(handle)?;
            match runtime.value(id).map_err(|e| e.to_string())?.as_ref() {
                Value::Object { type_name, .. } | Value::Enum { type_name, .. } => {
                    buffer(type_name.as_bytes().to_vec())
                }
                _ => Err("value has no named type".into()),
            }
        }
        31 => {
            let (runtime, id) = target(handle)?;
            match runtime.value(id).map_err(|e| e.to_string())?.as_ref() {
                Value::Template { source, .. } | Value::Function { source, .. } => {
                    buffer(source.as_bytes().to_vec())
                }
                _ => Err("value has no program source".into()),
            }
        }
        40 => {
            let Entry::Buffer(bytes) = get(handle)? else {
                return Err("expected buffer".into());
            };
            Ok(Response {
                length: bytes.len() as u64,
                ..Response::default()
            })
        }
        41 => buffer(data.to_vec()),
        33 => {
            let (runtime, id) = target(handle)?;
            value(runtime, id)
        }
        34 => {
            let (runtime, id) = target(handle)?;
            let selected = runtime
                .dimension_variant(id, text(key)?)
                .map_err(|e| e.to_string())?;
            value(runtime, selected)
        }
        36 => {
            let (runtime, id) = target(handle)?;
            let base = runtime.dimension_default(id).map_err(|e| e.to_string())?;
            value(runtime, base)
        }
        _ => Err("operation unavailable in this build".into()),
    }
}

/// # Safety
/// destination 必须具有 capacity 个可写字节；capacity 为零时允许为空。
#[no_mangle]
pub unsafe extern "C" fn coflow_buffer_copy(
    handle: u64,
    destination: *mut u8,
    capacity: usize,
) -> u32 {
    let Ok(Entry::Buffer(bytes)) = get(handle) else {
        return 1;
    };
    if bytes.len() > capacity || (destination.is_null() && !bytes.is_empty()) {
        return 1;
    }
    if !bytes.is_empty() {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        }
    }
    0
}
#[no_mangle]
pub extern "C" fn coflow_release(handle: u64) {
    let removed = registry()
        .lock()
        .ok()
        .and_then(|mut registry| registry.entries.remove(&handle));
    // 不在注册表锁内释放宿主资源，释放回调可以安全地再次进入边界。
    if let Some(Entry::Runtime(runtime)) = &removed {
        runtime.release();
    }
    drop(removed);
}
