//! Unity/IL2CPP C ABI。句柄为不复用的整数，不暴露 Rust 对象地址。
mod invocation;
#[cfg(test)]
mod tests;
use coflow_core::{
    contract::Contract,
    runtime::{Runtime, RuntimeBuilder, Value, ValueId},
};
use std::{
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Operation {
    LoadContract = 1,
    CreateCompiler = 3,
    AddSchemaSource = 4,
    AddDimension = 5,
    CompileContract = 6,
    SerializeContract = 7,
    ContractIdentity = 8,
    RuntimeContractIdentity = 9,
    CreateBuilder = 10,
    AddDataSource = 11,
    BuildRuntime = 12,
    FindRecord = 20,
    TableLength = 21,
    TableValue = 22,
    ReadField = 23,
    InspectValue = 24,
    ReadText = 25,
    ArrayValue = 26,
    DictionaryKey = 27,
    DictionaryValue = 28,
    Invoke = 29,
    TypeName = 30,
    ProgramSource = 31,
    TryFindRecord = 32,
    DimensionVariant = 34,
    ValueEquals = 35,
    DimensionDefault = 36,
    Singleton = 37,
    DictionaryFind = 38,
    CanonicalValue = 39,
    BufferLength = 40,
    CreateBuffer = 41,
    ReleaseValue = 42,
    Collect = 43,
    RetainValue = 44,
    RunChecks = 45,
}

impl TryFrom<u32> for Operation {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::LoadContract,
            3 => Self::CreateCompiler,
            4 => Self::AddSchemaSource,
            5 => Self::AddDimension,
            6 => Self::CompileContract,
            7 => Self::SerializeContract,
            8 => Self::ContractIdentity,
            9 => Self::RuntimeContractIdentity,
            10 => Self::CreateBuilder,
            11 => Self::AddDataSource,
            12 => Self::BuildRuntime,
            20 => Self::FindRecord,
            21 => Self::TableLength,
            22 => Self::TableValue,
            23 => Self::ReadField,
            24 => Self::InspectValue,
            25 => Self::ReadText,
            26 => Self::ArrayValue,
            27 => Self::DictionaryKey,
            28 => Self::DictionaryValue,
            29 => Self::Invoke,
            30 => Self::TypeName,
            31 => Self::ProgramSource,
            32 => Self::TryFindRecord,
            34 => Self::DimensionVariant,
            35 => Self::ValueEquals,
            36 => Self::DimensionDefault,
            37 => Self::Singleton,
            38 => Self::DictionaryFind,
            39 => Self::CanonicalValue,
            40 => Self::BufferLength,
            41 => Self::CreateBuffer,
            42 => Self::ReleaseValue,
            43 => Self::Collect,
            44 => Self::RetainValue,
            45 => Self::RunChecks,
            _ => return Err("operation unavailable in this build".into()),
        })
    }
}

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
        handle: u64::try_from(id)
            .map_err(|_| "value ID overflow")?
            .checked_add(1)
            .ok_or("value ID overflow")?,
        ..Response::default()
    })
}
fn target(handle: u64, raw_value: u64) -> Result<(Arc<Runtime>, ValueId), String> {
    let Entry::Runtime(runtime) = get(handle)? else {
        return Err("expected Runtime".into());
    };
    let id = usize::try_from(raw_value.checked_sub(1).ok_or("missing value ID")?)
        .map_err(|_| "value ID overflow")?;
    runtime.ensure_value(id).map_err(|e| e.to_string())?;
    Ok((runtime, id))
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
        self.request_bytes(op, field.as_bytes())
    }
    fn request_bytes(&self, op: u32, bytes: &[u8]) -> Result<Response, String> {
        let mut result = Response::default();
        // 回调必须同步返回；不持有注册表锁，允许同线程重入。
        unsafe {
            (self.callback)(self.context, op, bytes.as_ptr(), bytes.len(), &mut result);
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
        use coflow_core::vm::ExecutionError;
        let result = self
            .request(1, field)
            .map_err(ExecutionError::InvalidAccess)?;
        invocation::host_result(result)
    }
    fn call(
        &self,
        field: &str,
        args: &[coflow_core::runtime::HostValue],
    ) -> Result<coflow_core::runtime::HostValue, coflow_core::vm::ExecutionError> {
        let bytes = invocation::encode_call(field, args)
            .map_err(coflow_core::vm::ExecutionError::InvalidAccess)?;
        let result = self
            .request_bytes(2, &bytes)
            .map_err(coflow_core::vm::ExecutionError::InvalidAccess)?;
        invocation::host_result(result)
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
    raw_value: u64,
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
        dispatch(op, handle, raw_value, key, data, index)
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

fn dispatch(
    operation: u32,
    handle: u64,
    raw_value: u64,
    key: &[u8],
    data: &[u8],
    index: u64,
) -> Result<Response, String> {
    let op = Operation::try_from(operation)?;
    match op {
        #[cfg(not(feature = "cft-compiler"))]
        Operation::CreateCompiler
        | Operation::AddSchemaSource
        | Operation::AddDimension
        | Operation::CompileContract => Err("operation unavailable in this build".into()),
        Operation::ValueEquals => {
            let (runtime, left) = target(handle, raw_value)?;
            let (other, right) = target(handle, index)?;
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
        Operation::RuntimeContractIdentity => {
            let runtime = match get(handle)? {
                Entry::Runtime(runtime) => runtime,
                _ => return Err("expected Runtime or value".into()),
            };
            runtime.ensure_alive().map_err(|error| error.to_string())?;
            buffer(runtime.contract().identity().to_vec())
        }
        Operation::LoadContract => Ok(Response {
            handle: insert(Entry::Contract(Arc::new(
                Contract::from_bytes(data).map_err(|e| e.to_string())?,
            )))?,
            ..Response::default()
        }),
        #[cfg(feature = "cft-compiler")]
        Operation::CreateCompiler => Ok(Response {
            handle: insert(Entry::Compiler(Arc::new(
                Mutex::new(Compilation::default()),
            )))?,
            ..Response::default()
        }),
        #[cfg(feature = "cft-compiler")]
        Operation::AddSchemaSource | Operation::AddDimension | Operation::CompileContract => {
            let Entry::Compiler(compiler) = get(handle)? else {
                return Err("expected CFT compilation".into());
            };
            let mut compiler = compiler.try_lock().map_err(|_| "CFT compilation busy")?;
            if op == Operation::AddSchemaSource {
                compiler
                    .sources
                    .push(coflow_core::schema::CftFile::from_source(
                        coflow_core::schema::ModuleId::from(text(key)?),
                        text(data)?,
                    ));
                return Ok(Response::default());
            }
            if op == Operation::AddDimension {
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
        Operation::SerializeContract | Operation::ContractIdentity => {
            let Entry::Contract(contract) = get(handle)? else {
                return Err("expected contract".into());
            };
            buffer(if op == Operation::SerializeContract {
                contract.to_bytes().map_err(|e| e.to_string())?
            } else {
                contract.identity().to_vec()
            })
        }
        Operation::CreateBuilder => {
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
        Operation::AddDataSource | Operation::BuildRuntime => {
            let Entry::Builder(builder) = get(handle)? else {
                return Err("expected builder".into());
            };
            let mut builder = builder.try_lock().map_err(|_| "builder busy")?;
            if op == Operation::AddDataSource {
                builder
                    .as_mut()
                    .ok_or("builder already consumed")?
                    .add_text(
                        text(data)?,
                        if key.is_empty() {
                            None
                        } else {
                            Some(text(key)?)
                        },
                    );
                return Ok(Response::default());
            }
            // 构建失败保留候选输入；只有成功后才消耗构建器。
            let built = builder
                .as_ref()
                .ok_or("builder already consumed")?
                .clone()
                .build();
            let runtime = match built.runtime {
                Ok(runtime) => runtime,
                Err(_) => {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(&(built.diagnostics.len() as u32).to_le_bytes());
                    for error in built.diagnostics {
                        for text in [&error.code, &error.source, &error.message] {
                            bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                            bytes.extend_from_slice(text.as_bytes());
                        }
                        bytes.push(u8::from(error.span.is_some()));
                        let (start, end) = error.span.unwrap_or_default();
                        bytes.extend_from_slice(&(start as u64).to_le_bytes());
                        bytes.extend_from_slice(&(end as u64).to_le_bytes());
                    }
                    let mut response = buffer(bytes)?;
                    response.error = 2;
                    return Ok(response);
                }
            };
            builder.take();
            Ok(Response {
                handle: insert(Entry::Runtime(runtime))?,
                ..Response::default()
            })
        }
        Operation::FindRecord | Operation::TryFindRecord => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let id = runtime
                .find_record(text(key)?, text(data)?)
                .map_err(|e| e.to_string())?;
            match id {
                Some(id) => value(runtime, id),
                None if op == Operation::TryFindRecord => Ok(Response::default()),
                None => Err("record not found".into()),
            }
        }
        Operation::TableLength => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            runtime
                .require_record_kind(text(key)?, false)
                .map_err(|e| e.to_string())?;
            let ids = runtime
                .table_values(text(key)?)
                .map_err(|e| e.to_string())?;
            Ok(Response {
                length: ids.len() as u64,
                ..Response::default()
            })
        }
        Operation::TableValue => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            runtime
                .require_record_kind(text(key)?, false)
                .map_err(|e| e.to_string())?;
            let ids = runtime
                .table_values(text(key)?)
                .map_err(|e| e.to_string())?;
            let id = *ids
                .get(usize::try_from(index).map_err(|_| "index overflow")?)
                .ok_or("record index out of range")?;
            value(runtime, id)
        }
        Operation::ReadField => {
            let (runtime, id) = target(handle, raw_value)?;
            let field = runtime.field(id, text(key)?).map_err(|e| e.to_string())?;
            value(runtime, field)
        }
        Operation::InspectValue => {
            let (runtime, id) = target(handle, raw_value)?;
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
        Operation::ReadText => {
            let (runtime, id) = target(handle, raw_value)?;
            buffer(
                runtime
                    .read_text(id)
                    .map_err(|e| e.to_string())?
                    .into_bytes(),
            )
        }
        Operation::ArrayValue | Operation::DictionaryKey | Operation::DictionaryValue => {
            let (runtime, id) = target(handle, raw_value)?;
            let value_ref = runtime.value(id).map_err(|e| e.to_string())?;
            let index = usize::try_from(index).map_err(|_| "index overflow")?;
            let child = match (value_ref.as_ref(), op) {
                (Value::Array(items), Operation::ArrayValue) => {
                    *items.get(index).ok_or("array index out of range")?
                }
                (Value::Dict(items), Operation::DictionaryKey) => {
                    items.get(index).ok_or("dictionary index out of range")?.0
                }
                (Value::Dict(items), Operation::DictionaryValue) => {
                    items.get(index).ok_or("dictionary index out of range")?.1
                }
                _ => return Err("container operation mismatch".into()),
            };
            value(runtime, child)
        }
        Operation::Invoke => {
            let (runtime, id) = target(handle, raw_value)?;
            let args = invocation::decode_arguments(data)?;
            invocation::response(
                runtime
                    .invoke(
                        id,
                        &args,
                        coflow_core::vm::executor::ExecutionLimits::default(),
                    )
                    .map_err(|e| e.to_string())?,
            )
        }
        Operation::ReleaseValue => {
            let (runtime, id) = target(handle, raw_value)?;
            runtime.release_value(id).map_err(|e| e.to_string())?;
            Ok(Response::default())
        }
        Operation::RetainValue => {
            let (runtime, id) = target(handle, raw_value)?;
            runtime.retain_value(id).map_err(|e| e.to_string())?;
            Ok(Response::default())
        }
        Operation::RunChecks => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let mut position = 0usize;
            let take_u32 = |position: &mut usize| -> Result<u32, String> {
                let end = position.checked_add(4).ok_or("check request overflow")?;
                let value = u32::from_le_bytes(
                    data.get(*position..end)
                        .ok_or("truncated check request")?
                        .try_into()
                        .map_err(|_| "invalid check request")?,
                );
                *position = end;
                Ok(value)
            };
            let take_u64 = |position: &mut usize| -> Result<u64, String> {
                let end = position.checked_add(8).ok_or("check request overflow")?;
                let value = u64::from_le_bytes(
                    data.get(*position..end)
                        .ok_or("truncated check request")?
                        .try_into()
                        .map_err(|_| "invalid check request")?,
                );
                *position = end;
                Ok(value)
            };
            let take_text = |position: &mut usize| -> Result<String, String> {
                let length = take_u32(position)? as usize;
                let end = position
                    .checked_add(length)
                    .ok_or("check request overflow")?;
                let value =
                    text(data.get(*position..end).ok_or("truncated check request")?)?.to_string();
                *position = end;
                Ok(value)
            };
            let max_work = take_u64(&mut position)?;
            let max_iterations = take_u64(&mut position)?;
            let include_global = *data.get(position).ok_or("truncated check request")? != 0;
            position += 1;
            let mut names = std::collections::BTreeSet::new();
            for _ in 0..take_u32(&mut position)? {
                names.insert(take_text(&mut position)?);
            }
            let record_count = take_u32(&mut position)?;
            let records = if record_count == u32::MAX {
                None
            } else {
                let mut records = Vec::with_capacity(record_count as usize);
                for _ in 0..record_count {
                    let raw = take_u64(&mut position)?;
                    let id = usize::try_from(raw.checked_sub(1).ok_or("missing value ID")?)
                        .map_err(|_| "value ID overflow")?;
                    runtime.ensure_value(id).map_err(|e| e.to_string())?;
                    records.push(id);
                }
                Some(records)
            };
            if position != data.len() {
                return Err("trailing check request bytes".into());
            }
            let output = runtime.run_checks(
                coflow_core::runtime::CheckSelection {
                    records,
                    names,
                    include_global,
                },
                coflow_core::check::CheckLimits {
                    evaluation: coflow_core::check::EvaluationLimits::new(max_work, max_iterations),
                },
            );
            fn write_text(bytes: &mut Vec<u8>, value: &str) -> Result<(), String> {
                bytes.extend_from_slice(
                    &u32::try_from(value.len())
                        .map_err(|_| "check text too large")?
                        .to_le_bytes(),
                );
                bytes.extend_from_slice(value.as_bytes());
                Ok(())
            }
            let mut bytes = Vec::new();
            bytes.push(u8::from(output.is_success()));
            for value in [
                output.statistics.requested_tasks as u64,
                output.statistics.executed_tasks as u64,
                output.statistics.rejected_tasks as u64,
                output.statistics.work_used,
                output.statistics.dimension_projected_records as u64,
            ] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(
                &u32::try_from(output.request_diagnostics.len())
                    .map_err(|_| "too many check diagnostics")?
                    .to_le_bytes(),
            );
            for diagnostic in output.request_diagnostics {
                write_text(&mut bytes, diagnostic.diagnostic.code.as_str())?;
                write_text(&mut bytes, &diagnostic.diagnostic.message)?;
                if let Some(location) = diagnostic.schema_location {
                    bytes.push(1);
                    write_text(&mut bytes, location.module.as_str())?;
                    bytes.extend_from_slice(&(location.span.start as u64).to_le_bytes());
                    bytes.extend_from_slice(&(location.span.end as u64).to_le_bytes());
                } else {
                    bytes.push(0);
                }
                let names = diagnostic
                    .contexts
                    .into_iter()
                    .filter_map(|context| match context {
                        coflow_core::check::CheckDiagnosticContext::Check { name } => Some(name),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                bytes.extend_from_slice(
                    &u32::try_from(names.len())
                        .map_err(|_| "too many check contexts")?
                        .to_le_bytes(),
                );
                for name in names {
                    write_text(&mut bytes, &name)?;
                }
            }
            buffer(bytes)
        }
        Operation::Collect => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            Ok(Response {
                length: runtime.collect().map_err(|e| e.to_string())? as u64,
                ..Response::default()
            })
        }
        Operation::TypeName => {
            let (runtime, id) = target(handle, raw_value)?;
            match runtime.value(id).map_err(|e| e.to_string())?.as_ref() {
                Value::Object { type_name, .. } | Value::Enum { type_name, .. } => {
                    buffer(type_name.as_bytes().to_vec())
                }
                _ => Err("value has no named type".into()),
            }
        }
        Operation::ProgramSource => {
            let (runtime, id) = target(handle, raw_value)?;
            match runtime.value(id).map_err(|e| e.to_string())?.as_ref() {
                Value::Template { source, .. } | Value::Function { source, .. } => {
                    buffer(source.as_bytes().to_vec())
                }
                _ => Err("value has no program source".into()),
            }
        }
        Operation::Singleton => {
            let Entry::Runtime(runtime) = get(handle)? else {
                return Err("expected Runtime".into());
            };
            let id = runtime.singleton(text(key)?).map_err(|e| e.to_string())?;
            value(runtime, id)
        }
        Operation::DictionaryFind => {
            let (runtime, id) = target(handle, raw_value)?;
            use coflow_core::runtime::HostValue;
            let key = match index {
                1 if data.len() == 1 => HostValue::Bool(data[0] != 0),
                2 if data.len() == 4 => HostValue::Int(i32::from_le_bytes(
                    data.try_into().map_err(|_| "invalid int")?,
                )),
                4 => HostValue::String(text(data)?.into()),
                5 if data.len() == 4 => HostValue::Enum {
                    type_name: text(key)?.into(),
                    value: u32::from_le_bytes(data.try_into().map_err(|_| "invalid enum")?),
                },
                _ => return Err("invalid dictionary key".into()),
            };
            match runtime
                .dictionary_find(id, key)
                .map_err(|e| e.to_string())?
            {
                Some(child) => value(runtime, child),
                None => Ok(Response::default()),
            }
        }
        Operation::CanonicalValue => {
            let (runtime, id) = target(handle, raw_value)?;
            let canonical = runtime.canonical_value(id).map_err(|e| e.to_string())?;
            value(runtime, canonical)
        }
        Operation::BufferLength => {
            let Entry::Buffer(bytes) = get(handle)? else {
                return Err("expected buffer".into());
            };
            Ok(Response {
                length: bytes.len() as u64,
                ..Response::default()
            })
        }
        Operation::CreateBuffer => buffer(data.to_vec()),
        Operation::DimensionVariant => {
            let (runtime, id) = target(handle, raw_value)?;
            let selected = runtime
                .dimension_variant(id, text(key)?)
                .map_err(|e| e.to_string())?;
            value(runtime, selected)
        }
        Operation::DimensionDefault => {
            let (runtime, id) = target(handle, raw_value)?;
            let base = runtime.dimension_default(id).map_err(|e| e.to_string())?;
            value(runtime, base)
        }
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
