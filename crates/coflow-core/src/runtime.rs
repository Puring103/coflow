//! 宿主运行时：只读配置、字节码执行、动态值保活与显式生命周期。
use crate::{
    contract::Contract,
    loading::{self, SourceAnalysis, SourceInput},
    schema::CftValueType,
    vm::ExecutionError,
    CfdDataModel, CfdDictKey, CfdValue,
};
use std::{
    collections::{BTreeMap,BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::ThreadId,
};

mod execution;
mod checking;
pub use checking::CheckSelection;

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
pub type ValueId = usize;

#[derive(Debug, Clone)]
pub enum Value {
    None,
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
    Enum {
        type_name: String,
        value: u32,
    },
    Object {
        type_name: String,
        key: Option<String>,
        fields: Vec<(String, ValueId)>,
        bases: Vec<(String, ValueId)>,
        dimension: Option<(ValueId, String)>,
    },
    Array(Vec<ValueId>),
    Dict(Vec<(ValueId, ValueId)>),
    Function {
        source: String,
        owner: Option<ValueId>,
        host: Option<(String, String)>,
    },
    Template {
        source: String,
        owner: Option<ValueId>,
    },
    HostData {
        service: String,
        field: String,
        value_type: CftValueType,
    },
}

/// 标量按值返回；对象、集合、函数和模板必须使用同一 Runtime 的现有身份。
#[derive(Debug, Clone)]
pub enum HostValue {
    None,
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
    Enum { type_name: String, value: u32 },
    Existing { runtime: u64, value: ValueId },
}

/// Host 服务整对象绑定；数据读取和函数调用使用同一 Runtime 生命周期。
pub trait HostService: std::fmt::Debug + Send + Sync {
    fn call(&self, field: &str, _arguments: &[HostValue]) -> Result<HostValue, ExecutionError> {
        Err(ExecutionError::InvalidAccess(format!("Host 函数未实现：{field}")))
    }
    fn read(&self, field: &str) -> Result<HostValue, ExecutionError>;
    fn has_member(
        &self,
        field: &str,
        value_type: &CftValueType,
        schema: &crate::schema::CftSchema,
    ) -> bool;
}
pub type HostBindings = BTreeMap<String, Arc<dyn HostService>>;

#[derive(Debug)]
pub struct Runtime {
    identity: u64,
    contract: Arc<Contract>,
    values: Vec<Arc<Value>>,
    records: BTreeMap<(String, String), ValueId>,
    table_values: BTreeMap<String, Vec<ValueId>>,
    check_record_ids: BTreeMap<ValueId, crate::CfdRecordId>,
    record_lookup: BTreeMap<(String, String), ValueId>,
    bindings: HostBindings,
    released: AtomicBool,
    execution: Mutex<Option<(ThreadId, usize)>>,
    vm: execution::VmState,
    contract_values: BTreeSet<ValueId>,
    constants: BTreeMap<String,ValueId>,
    function_imports: BTreeMap<ValueId,BTreeMap<String,String>>,
    function_locations: BTreeMap<ValueId,crate::ingest::CallableLocation>,
    check_reporter: Arc<checking::CheckReporter>,
}

#[derive(Debug)]
pub struct RuntimeBuild {
    pub analyses: Vec<SourceAnalysis>,
    pub diagnostics: Vec<BuildDiagnostic>,
    pub runtime: Result<Arc<Runtime>, String>,
}

#[derive(Debug, Clone)]
pub struct BuildDiagnostic {
    pub code: String,
    pub source: String,
    pub message: String,
    pub span: Option<(usize, usize)>,
}

impl From<String> for BuildDiagnostic {
    fn from(message:String)->Self {Self{code:"BUILD".into(),source:String::new(),message,span:None}}
}
impl From<&str> for BuildDiagnostic {
    fn from(message:&str)->Self {message.to_string().into()}
}
#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    contract: Arc<Contract>,
    sources: Vec<SourceInput>,
    bindings: HostBindings,
}
impl RuntimeBuilder {
    pub fn new(contract: Arc<Contract>) -> Self {
        Self {
            contract,
            sources: Vec::new(),
            bindings: BTreeMap::new(),
        }
    }
    pub fn add_source(&mut self, input: SourceInput) {
        self.sources.push(input);
    }
    /// 来源名称只用于诊断，同名输入仍逐次追加；匿名来源由核心稳定编号。
    pub fn add_text(&mut self, source: &str, name: Option<&str>) {
        let name = name
            .map(str::to_string)
            .unwrap_or_else(|| format!("source-{}", self.sources.len() + 1));
        self.add_source(SourceInput::new(name, source));
    }
    pub fn bind(&mut self, name: String, service: Arc<dyn HostService>) -> Result<(), String> {
        if name == "Coflow::Check" {
            if self.bindings.contains_key(&name) {return Err("duplicate Host binding Coflow::Check".into());}
            self.bindings.insert(name,service);return Ok(());
        }
        let ty = self
            .contract
            .schema()
            .resolve_type(&name)
            .ok_or_else(|| format!("unknown Host type {name}"))?;
        if !ty.is_host {
            return Err(format!("{name} is not a Host service"));
        }
        if self.bindings.contains_key(&name) {
            return Err(format!("duplicate Host binding {name}"));
        }
        for field in ty.all_fields() {
            let present = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                service.has_member(
                    field.name.as_str(),
                    &field.value_type,
                    self.contract.schema(),
                )
            }))
            .map_err(|_| format!("Host binding validation panicked: {name}.{}", field.name))?;
            if !present {
                return Err(format!(
                    "Host binding does not implement {name}.{}",
                    field.name
                ));
            }
        }
        self.bindings.insert(name, service);
        Ok(())
    }
    pub fn build(self) -> RuntimeBuild {
        let (analyses, model) = loading::load(self.contract.schema(), self.sources);
        let mut diagnostics: Vec<_> = analyses
            .iter()
            .flat_map(|source| {
                source.diagnostics.iter().map(|error| BuildDiagnostic {
                    code: format!("{:?}", error.code),
                    source: source.input.path.to_string_lossy().into_owned(),
                    message: error.message.clone(),
                    span: Some((error.span.start, error.span.end)),
                })
            })
            .collect();
        if let Err(loading::CfdTextLoadError::DataModel {
            diagnostics: errors,
            origins,
        }) = &model
        {
            for error in &errors.diagnostics {
                let origin = error.primary.as_ref().and_then(|label| {
                    label
                        .origin
                        .as_ref()
                        .or_else(|| label.record.and_then(|id| origins.get(id.index())))
                });
                let source = match origin {
                    Some(crate::RecordOrigin::File { path, .. }) => {
                        path.to_string_lossy().into_owned()
                    }
                    _ => String::new(),
                };
                diagnostics.push(BuildDiagnostic {
                    code: error.code.to_string(),
                    source,
                    message: error.message.clone(),
                    span: None,
                });
            }
        }
        let runtime = model
            .map_err(|e| e.to_string())
            .and_then(|model| Runtime::from_model(self.contract, model, self.bindings).map_err(|diagnostic| {let message=diagnostic.message.clone();diagnostics.push(diagnostic);message}))
            .map(Arc::new);
        if let Err(message) = &runtime {
            if diagnostics.is_empty() {
                diagnostics.push(BuildDiagnostic {
                    code: "BUILD".into(),
                    source: String::new(),
                    message: message.clone(),
                    span: None,
                });
            }
        }
        RuntimeBuild {
            analyses,
            diagnostics,
            runtime,
        }
    }
}

impl Runtime {
    pub(crate) fn from_model(
        contract: Arc<Contract>,
        model: CfdDataModel,
        bindings: HostBindings,
    ) -> Result<Self, BuildDiagnostic> {
        let mut arena = Arena {
            contract: &contract,
            values: Vec::new(),
            records: BTreeMap::new(),
            constant_callables: BTreeMap::new(),
            contract_values: BTreeSet::new(),
            function_imports: BTreeMap::new(),
            function_locations: BTreeMap::new(),
        };
        // 先分配所有记录身份，再连接字段，允许自引用及跨记录循环。
        for (_, record) in model.records() {
            arena.record(record.actual_type(), record.key());
        }
        for ty in contract.schema().singleton_types().filter(|ty| ty.is_host) {
            arena.record(
                ty.name.as_str(),
                ty.name.rsplit("::").next().unwrap_or(ty.name.as_str()),
            );
        }
        // 常量先建立独立存储，后续字段复用其中的函数与模板身份。
        let check_record_ids=model.records().map(|(id,record)|(arena.records[&(record.actual_type().to_string(),record.key().to_string())],id)).collect();
        let mut constants=BTreeMap::new();
        for constant in contract.schema().all_consts() {
            constants.insert(constant.name.to_string(),arena.constant(&constant.value, None)?);
        }
        for (_, record) in model.records() {
            let id = arena.records[&(record.actual_type().to_string(), record.key().to_string())];
            let ty = contract
                .schema()
                .resolve_type(record.actual_type())
                .ok_or("missing record schema")?;
            if ty.is_host {
                continue;
            }
            let mut fields = Vec::new();
            let mut bases = Vec::new();
            let key = arena.push(Value::String(record.key().to_string()));
            fields.push(("id".into(), key));
            for field in ty.all_fields() {
                let value = record
                    .field(field.name.as_str())
                    .ok_or_else(|| format!("missing field {}", field.name))?;
                let base = arena.value(value, &field.value_type, Some(id))?;
                let value_id = if let Some(binding) = &field.dimension {
                    let generated = crate::schema::dimension_record_type(
                        binding.dimension.as_str(),
                        field.declaring_type.as_str(),
                        field.name.as_str(),
                    );
                    let dimension_id = arena.record(&generated, record.key());
                    let mut variants = vec![("id".into(), key)];
                    bases.push((field.name.to_string(), base));
                    let meta = contract
                        .schema()
                        .resolve_dimension(binding.dimension.as_str())
                        .ok_or("missing dimension schema")?;
                    for variant in &meta.variants {
                        let stored = record
                            .dimension_field(field.name.as_str())
                            .and_then(|d| d.variants.get(variant));
                        let override_id = match stored {
                            Some(value) if !matches!(value.value, CfdValue::OptionNone) => {
                                arena.value(&value.value, &field.value_type, Some(id))?
                            }
                            _ => arena.push(Value::None),
                        };
                        variants.push((variant.to_string(), override_id));
                    }
                    arena.values[dimension_id] = Value::Object {
                        type_name: generated,
                        key: Some(record.key().into()),
                        fields: variants,
                        bases: Vec::new(),
                        dimension: Some((id, field.name.to_string())),
                    };
                    dimension_id
                } else {
                    base
                };
                fields.push((field.name.to_string(), value_id));
            }
            arena.values[id] = Value::Object {
                type_name: record.actual_type().into(),
                key: Some(record.key().into()),
                fields,
                bases,
                dimension: None,
            };
        }
        for ty in contract.schema().singleton_types().filter(|ty| ty.is_host) {
            let key = ty.name.rsplit("::").next().unwrap_or(ty.name.as_str());
            let id = arena.records[&(ty.name.to_string(), key.into())];
            let mut fields = Vec::new();
            let key_id = arena.push(Value::String(key.into()));
            fields.push(("id".into(), key_id));
            for field in ty.all_fields() {
                let value = if matches!(field.value_type, CftValueType::Function(..)) {
                    Value::Function {
                        source: String::new(),
                        owner: Some(id),
                        host: Some((ty.name.to_string(), field.name.to_string())),
                    }
                } else {
                    Value::HostData {
                        service: ty.name.to_string(),
                        field: field.name.to_string(),
                        value_type: field.value_type.clone(),
                    }
                };
                let value_id = arena.push(value);
                fields.push((field.name.to_string(), value_id));
            }
            arena.values[id] = Value::Object {
                type_name: ty.name.to_string(),
                key: Some(key.into()),
                fields,
                bases: Vec::new(),
                dimension: None,
            };
        }
        let Arena {
            values, records, contract_values, function_imports, function_locations, ..
        } = arena;
        let identity = NEXT_RUNTIME
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| "Runtime identity exhausted")?;
        // 按静态查询类型建立一次索引，宿主枚举不重复扫描或复制整张表。
        let mut table_values = BTreeMap::new();
        let mut record_lookup = BTreeMap::new();
        for ty in contract
            .schema()
            .all_types()
            .filter(|ty| ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data)
        {
            let mut ids = Vec::new();
            for ((actual, key), id) in &records {
                if contract.schema().is_assignable(actual, &ty.name) {
                    ids.push(*id);
                    record_lookup.insert((ty.name.to_string(), key.clone()), *id);
                }
            }
            table_values.insert(ty.name.to_string(), ids);
        }
        let check_reporter=Arc::new(checking::CheckReporter::default());
        let mut bindings=bindings;
        bindings.entry("Coflow::Check".into()).or_insert_with(||check_reporter.clone());
        let mut runtime = Self {
            identity,
            contract,
            values: values.into_iter().map(Arc::new).collect(),
            records,
            table_values,
            check_record_ids,
            record_lookup,
            bindings,
            released: AtomicBool::new(false),
            execution: Mutex::new(None),
            vm: execution::VmState::default(),
            contract_values,
            constants,
            function_imports,
            function_locations,
            check_reporter,
        };
        runtime.vm = execution::VmState::build(&runtime)?;
        Ok(runtime)
    }
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn contract(&self) -> &Contract {
        &self.contract
    }
    pub fn release(&self) {
        self.released.store(true, Ordering::Release);
    }
    pub fn ensure_alive(&self) -> Result<(), ExecutionError> {
        if self.released.load(Ordering::Acquire) {
            Err(ExecutionError::Released)
        } else {
            Ok(())
        }
    }
    pub fn ensure_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        if id < self.values.len() { Ok(()) } else { self.vm.value(id).map(|_| ()) }
    }
    /// 数据与集合按读取值比较；记录、函数按创建身份比较。
    pub fn equals(&self, left: ValueId, right: ValueId) -> Result<bool, ExecutionError> {
        let _entry=self.enter()?;
        self.execution_equals(left,right)
    }
    pub fn value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        self.ensure_alive()?;
        let value = if let Some(value) = self.values.get(id) { value.clone() } else { self.vm.value(id)? };
        if let Value::HostData {
            service,
            field,
            value_type,
        } = value.as_ref()
        {
            self.read_host(service, field, value_type)
        } else {
            Ok(value)
        }
    }
    pub fn field(&self, id: ValueId, name: &str) -> Result<ValueId, ExecutionError> {
        match self.value(id)?.as_ref() {
            Value::Object { fields, .. } => fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| *value)
                .ok_or_else(|| ExecutionError::InvalidAccess(format!("unknown field {name}"))),
            _ => Err(ExecutionError::InvalidAccess("expected object".into())),
        }
    }
    /// 维度回退沿隐藏回链读取业务对象内部基础值，不占用用户变体名。
    pub fn dimension_default(&self, id: ValueId) -> Result<ValueId, ExecutionError> {
        let value = self.value(id)?;
        let Value::Object {
            dimension: Some((owner, field)),
            ..
        } = value.as_ref()
        else {
            return Err(ExecutionError::InvalidAccess(
                "expected dimension record".into(),
            ));
        };
        let owner = self.value(*owner)?;
        let Value::Object { bases, .. } = owner.as_ref() else {
            return Err(ExecutionError::InvalidHandle);
        };
        bases
            .iter()
            .find(|(name, _)| name == field)
            .map(|(_, id)| *id)
            .ok_or(ExecutionError::InvalidHandle)
    }
    pub fn dimension_variant(&self, id: ValueId, variant: &str) -> Result<ValueId, ExecutionError> {
        let base = self.dimension_default(id)?;
        let value = self.value(id)?;
        let Value::Object {
            fields, type_name, ..
        } = value.as_ref()
        else {
            return Err(ExecutionError::InvalidHandle);
        };
        let (dimension, _) = loading::dimension_source(self.contract.schema(), type_name)
            .ok_or(ExecutionError::InvalidHandle)?;
        if !dimension
            .variants
            .iter()
            .any(|name| name.as_str() == variant)
        {
            return Ok(base);
        }
        let selected = fields
            .iter()
            .find(|(name, _)| name == variant)
            .map(|(_, id)| *id)
            .ok_or(ExecutionError::InvalidHandle)?;
        if matches!(self.value(selected)?.as_ref(), Value::None) {
            Ok(base)
        } else {
            Ok(selected)
        }
    }
    pub fn record(&self, type_name: &str, key: &str) -> Result<ValueId, ExecutionError> {
        self.ensure_alive()?;
        self.records
            .iter()
            .find(|((actual, k), _)| {
                k == key && self.contract.schema().is_assignable(actual, type_name)
            })
            .map(|(_, id)| *id)
            .ok_or_else(|| {
                ExecutionError::InvalidAccess(format!("record {type_name}::{key} not found"))
            })
    }
    pub fn require_record_kind(
        &self,
        type_name: &str,
        singleton: bool,
    ) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        let ty = self
            .contract
            .schema()
            .resolve_type(type_name)
            .ok_or_else(|| ExecutionError::InvalidAccess(format!("unknown type {type_name}")))?;
        let expected = if singleton {
            coflow_language::cft::syntax::ast::TypeKind::Singleton
        } else {
            coflow_language::cft::syntax::ast::TypeKind::Table
        };
        if ty.kind != expected {
            return Err(ExecutionError::InvalidAccess(format!(
                "{type_name} is not a {}",
                if singleton { "singleton" } else { "table" }
            )));
        }
        Ok(())
    }
    pub fn find_record(
        &self,
        type_name: &str,
        key: &str,
    ) -> Result<Option<ValueId>, ExecutionError> {
        self.require_record_kind(type_name, false)?;
        Ok(self
            .record_lookup
            .get(&(type_name.to_string(), key.to_string()))
            .copied())
    }
    pub fn table_values(&self, type_name: &str) -> Result<&[ValueId], ExecutionError> {
        self.require_record_kind(type_name, false)?;
        self.table_values
            .get(type_name)
            .map(Vec::as_slice)
            .ok_or_else(|| ExecutionError::InvalidAccess("unknown table".into()))
    }
    pub fn singleton(&self, type_name: &str) -> Result<ValueId, ExecutionError> {
        self.require_record_kind(type_name, true)?;
        self.record(
            type_name,
            type_name.rsplit("::").next().unwrap_or(type_name),
        )
    }
    /// Host 返回已有对象时归一到原对象 ID，确保记录包装保留同一身份。
    pub fn canonical_value(&self, id: ValueId) -> Result<ValueId, ExecutionError> {
        self.ensure_value(id)?;
        if !matches!(self.values[id].as_ref(), Value::HostData { .. }) {
            return Ok(id);
        }
        let resolved = self.value(id)?;
        self.values
            .iter()
            .position(|value| Arc::ptr_eq(value, &resolved))
            .ok_or_else(|| {
                ExecutionError::InvalidAccess("expected an existing object value".into())
            })
    }
    /// 字典查找在核心执行，宿主只传输已声明键类型的标量。
    pub fn dictionary_find(
        &self,
        id: ValueId,
        key: HostValue,
    ) -> Result<Option<ValueId>, ExecutionError> {
        let dictionary = self.value(id)?;
        let Value::Dict(entries) = dictionary.as_ref() else {
            return Err(ExecutionError::InvalidAccess("expected dictionary".into()));
        };
        for (candidate, value) in entries {
            let candidate = self.value(*candidate)?;
            let equal = match (candidate.as_ref(), &key) {
                (Value::Bool(a), HostValue::Bool(b)) => a == b,
                (Value::Int(a), HostValue::Int(b)) => a == b,
                (Value::String(a), HostValue::String(b)) => a == b,
                (
                    Value::Enum {
                        type_name: a,
                        value: av,
                    },
                    HostValue::Enum {
                        type_name: b,
                        value: bv,
                    },
                ) => a == b && av == bv,
                _ => false,
            };
            if equal {
                return Ok(Some(*value));
            }
        }
        Ok(None)
    }
    pub fn records(&self, type_name: &str) -> Result<Vec<ValueId>, ExecutionError> {
        self.ensure_alive()?;
        let ty = self
            .contract
            .schema()
            .resolve_type(type_name)
            .ok_or_else(|| ExecutionError::InvalidAccess("unknown type".into()))?;
        if ty.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
            return Err(ExecutionError::InvalidAccess(
                "data is not a record type".into(),
            ));
        }
        Ok(self
            .records
            .iter()
            .filter(|((actual, _), _)| self.contract.schema().is_assignable(actual, type_name))
            .map(|(_, id)| *id)
            .collect())
    }
    pub fn read_text(&self, id: ValueId) -> Result<String, ExecutionError> {
        let _entry = self.enter()?;
        match self.value(id)?.as_ref() {
            Value::String(value) => Ok(value.clone()),
            Value::Template { .. } => self.evaluate_text(id),
            _ => Err(ExecutionError::InvalidAccess("expected string".into())),
        }
    }
    pub fn read_host(
        &self,
        service: &str,
        field: &str,
        value_type: &CftValueType,
    ) -> Result<Arc<Value>, ExecutionError> {
        let _entry = self.enter()?;
        let bound = self
            .bindings
            .get(service)
            .ok_or_else(|| ExecutionError::MissingHostBinding(service.into()))?;
        let returned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bound.read(field)))
            .map_err(|_| ExecutionError::InvalidAccess("Host callback panicked".into()))??;
        let value = match returned {
            HostValue::None => Arc::new(Value::None),
            HostValue::Bool(value) => Arc::new(Value::Bool(value)),
            HostValue::Int(value) => Arc::new(Value::Int(value)),
            HostValue::Float(value) => Arc::new(Value::Float(value)),
            HostValue::String(value) => Arc::new(Value::String(value)),
            HostValue::Enum { type_name, value } => Arc::new(Value::Enum { type_name, value }),
            HostValue::Existing { runtime, value } => {
                if runtime != self.identity {
                    return Err(ExecutionError::ForeignRuntime);
                }
                let value = self.value(value)?;
                // 不允许用 Host 字段占位符递归触发自身；返回实际数据句柄。
                if matches!(value.as_ref(), Value::HostData { .. }) {
                    return Err(ExecutionError::InvalidAccess(
                        "Host must return a concrete value".into(),
                    ));
                }
                value.clone()
            }
        };
        if !self.matches_type(value.as_ref(), value_type)? {
            return Err(ExecutionError::InvalidAccess(
                "Host data does not match declared type".into(),
            ));
        }
        Ok(value)
    }
    fn matches_type(&self, value: &Value, ty: &CftValueType) -> Result<bool, ExecutionError> {
        if let CftValueType::Option(inner) = ty {
            return if matches!(value, Value::None) {
                Ok(true)
            } else {
                self.matches_type(value, inner)
            };
        }
        Ok(match (value, ty) {
            (Value::Bool(_), CftValueType::Bool)
            | (Value::Int(_), CftValueType::Int)
            | (Value::Float(_), CftValueType::Float)
            | (Value::String(_), CftValueType::String)
            | (Value::Template { .. }, CftValueType::FString) => true,
            (Value::Enum { type_name, value }, CftValueType::Enum(expected)) => {
                type_name == expected.as_str()
                    && self
                        .contract
                        .schema()
                        .resolve_enum(type_name)
                        .is_some_and(|meta| {
                            if meta.is_flag {
                                let mask = meta
                                    .variants
                                    .iter()
                                    .fold(0u32, |mask, variant| mask | variant.value as u32);
                                *value & !mask == 0
                            } else {
                                *value <= i32::MAX as u32
                            }
                        })
            }
            (Value::Object { type_name, key, .. }, CftValueType::RecordRef(expected)) => {
                key.is_some() && self.contract.schema().is_assignable(type_name, expected)
            }
            (Value::Object { type_name, key, .. }, CftValueType::Object(expected)) => {
                key.is_none() && self.contract.schema().is_assignable(type_name, expected)
            }
            (Value::Array(values), CftValueType::Array(inner)) => {
                for id in values {
                    if !self.matches_type(self.value(*id)?.as_ref(), inner)? {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Dict(values), CftValueType::Dict(key, inner)) => {
                for (k, v) in values {
                    if !self.matches_type(self.value(*k)?.as_ref(), key)?
                        || !self.matches_type(self.value(*v)?.as_ref(), inner)?
                    {
                        return Ok(false);
                    }
                }
                true
            }
            (Value::Function { source, host, .. }, CftValueType::Function(..)) => {
                if let Some((service, field)) = host {
                    self.contract
                        .schema()
                        .resolve_type(service)
                        .and_then(|meta| meta.field(field))
                        .is_some_and(|meta| &meta.value_type == ty)
                } else {
                    coflow_language::cft::syntax::parser::parse_type_prefix(source)
                        .ok()
                        .and_then(|signature| {
                            self.contract.schema().resolve_type_ref(&signature).ok()
                        })
                        .is_some_and(|actual| &actual == ty)
                }
            }
            _ => false,
        })
    }
    fn enter(&self) -> Result<ExecutionGuard<'_>, ExecutionError> {
        self.ensure_alive()?;
        let thread = std::thread::current().id();
        let mut gate = self
            .execution
            .lock()
            .map_err(|_| ExecutionError::RuntimeBusy)?;
        match &mut *gate {
            Some((owner, depth)) if *owner == thread => *depth += 1,
            Some(_) => return Err(ExecutionError::RuntimeBusy),
            None => *gate = Some((thread, 1)),
        }
        Ok(ExecutionGuard(self))
    }
}
struct ExecutionGuard<'a>(&'a Runtime);
impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut gate) = self.0.execution.lock() {
            if let Some((_, depth)) = &mut *gate {
                *depth -= 1;
                if *depth == 0 {
                    *gate = None;
                }
            }
        }
    }
}

struct Arena<'a> {
    contract: &'a Contract,
    values: Vec<Value>,
    records: BTreeMap<(String, String), ValueId>,
    constant_callables: BTreeMap<String, ValueId>,
    contract_values: BTreeSet<ValueId>,
    function_imports: BTreeMap<ValueId,BTreeMap<String,String>>,
    function_locations: BTreeMap<ValueId,crate::ingest::CallableLocation>,
}
impl Arena<'_> {
    fn constant(
        &mut self,
        value: &crate::schema::CftConstValue,
        owner: Option<ValueId>,
    ) -> Result<ValueId, String> {
        use crate::schema::CftConstValue as C;
        let next = match value {
            C::Int(value) => {
                Value::Int(i32::try_from(*value).map_err(|_| "constant int outside i32")?)
            }
            C::Float(value) => Value::Float(*value as f32),
            C::Bool(value) => Value::Bool(*value),
            C::String(value) => Value::String(value.clone()),
            C::Enum {
                enum_name, value, ..
            } => Value::Enum {
                type_name: enum_name.to_string(),
                value: u32::try_from(*value).map_err(|_| "invalid constant enum")?,
            },
            C::OptionNone => Value::None,
            C::OptionSome(value) => return self.constant(value, owner),
            C::Function(source) | C::FormattedString(source) => {
                let origin = source
                    .constant_origin
                    .as_ref()
                    .ok_or("constant callable has no origin")?;
                if let Some(id) = self.constant_callables.get(origin) {
                    return Ok(*id);
                }
                let value = if matches!(value, C::Function(_)) {
                    Value::Function {
                        source: source.source.clone(),
                        owner,
                        host: None,
                    }
                } else {
                    Value::Template {
                        source: source.source.clone(),
                        owner,
                    }
                };
                let id = self.push(value);
                self.constant_callables.insert(origin.clone(), id);
                self.function_locations.insert(id,source.into());
                self.contract_values.insert(id);
                return Ok(id);
            }
            C::Array(values) => Value::Array(
                values
                    .iter()
                    .map(|value| self.constant(value, owner))
                    .collect::<Result<_, _>>()?,
            ),
            C::Dictionary(values) => {
                let mut entries = Vec::with_capacity(values.len());
                for (key, value) in values {
                    entries.push((self.constant(key, owner)?, self.constant(value, owner)?));
                }
                Value::Dict(entries)
            }
            C::Object { type_name, fields } => {
                let id = self.push(Value::None);
                let mut values = Vec::with_capacity(fields.len());
                for (name, value) in fields {
                    values.push((name.to_string(), self.constant(value, Some(id))?));
                }
                self.values[id] = Value::Object {
                    type_name: type_name.to_string(),
                    key: None,
                    fields: values,
                    bases: Vec::new(),
                    dimension: None,
                };
                return Ok(id);
            }
            C::RecordReference { type_name, key } => {
                return self
                    .records
                    .iter()
                    .find(|((actual, record_key), _)| {
                        record_key == key && self.contract.schema().is_assignable(actual, type_name)
                    })
                    .map(|(_, id)| *id)
                    .ok_or_else(|| format!("missing constant reference {type_name}::{key}"))
            }
        };
        Ok(self.push(next))
    }

    fn push(&mut self, value: Value) -> ValueId {
        let id = self.values.len();
        self.values.push(value);
        id
    }
    fn record(&mut self, ty: &str, key: &str) -> ValueId {
        let coordinate = (ty.into(), key.into());
        if let Some(id) = self.records.get(&coordinate) {
            return *id;
        }
        let id = self.push(Value::None);
        self.records.insert(coordinate, id);
        id
    }
    fn value(
        &mut self,
        value: &CfdValue,
        ty: &CftValueType,
        owner: Option<ValueId>,
    ) -> Result<ValueId, String> {
        let origin = match value {
            CfdValue::Function(value) => value.constant_origin.as_ref(),
            CfdValue::FormattedString(value) => value.constant_origin.as_ref(),
            _ => None,
        };
        if let Some(origin) = origin {
            return self
                .constant_callables
                .get(origin)
                .copied()
                .ok_or_else(|| format!("missing constant callable {origin}"));
        }
        let ty = if let CftValueType::Option(inner) = ty {
            inner.as_ref()
        } else {
            ty
        };
        let next = match value {
            CfdValue::OptionNone => Value::None,
            CfdValue::OptionSome(value) => return self.value(value, ty, owner),
            CfdValue::Bool(v) => Value::Bool(*v),
            CfdValue::Int(v) => Value::Int(i32::try_from(*v).map_err(|_| "int outside i32 range")?),
            CfdValue::Float(v) => Value::Float(*v as f32),
            CfdValue::String(v) => Value::String(v.clone()),
            CfdValue::FormattedString(v) => Value::Template {
                source: v.source.clone(),
                owner,
            },
            CfdValue::Function(v) => Value::Function {
                source: v.source.clone(),
                owner,
                host: None,
            },
            CfdValue::Enum(v) => Value::Enum {
                type_name: v.enum_name.to_string(),
                value: u32::try_from(v.value).map_err(|_| "enum outside u32 range")?,
            },
            CfdValue::Ref(key) => {
                let CftValueType::RecordRef(ty) = ty else {
                    return Err("reference type mismatch".into());
                };
                return self
                    .records
                    .iter()
                    .find(|((actual, k), _)| {
                        k == key.as_str() && self.contract.schema().is_assignable(actual, ty)
                    })
                    .map(|(_, id)| *id)
                    .ok_or_else(|| format!("missing reference {ty}::{key}"));
            }
            CfdValue::Object(object) => {
                let id = self.push(Value::None);
                let meta = self
                    .contract
                    .schema()
                    .resolve_type(object.actual_type())
                    .ok_or("unknown data type")?;
                let field_types: Vec<_> = meta
                    .all_fields()
                    .map(|f| (f.name.to_string(), f.value_type.clone()))
                    .collect();
                let mut fields = Vec::new();
                for (name, ty) in field_types {
                    let v = object.field(&name).ok_or("missing data field")?;
                    fields.push((name, self.value(v, &ty, Some(id))?));
                }
                self.values[id] = Value::Object {
                    type_name: object.actual_type().into(),
                    key: None,
                    fields,
                    bases: Vec::new(),
                    dimension: None,
                };
                return Ok(id);
            }
            CfdValue::Array(items) => {
                let CftValueType::Array(inner) = ty else {
                    return Err("array type mismatch".into());
                };
                Value::Array(
                    items
                        .iter()
                        .map(|v| self.value(v, inner, owner))
                        .collect::<Result<_, _>>()?,
                )
            }
            CfdValue::Dict(items) => {
                let CftValueType::Dict(_, inner) = ty else {
                    return Err("dictionary type mismatch".into());
                };
                let mut entries = Vec::new();
                for (k, v) in items {
                    let key = match k {
                        CfdDictKey::Bool(v) => Value::Bool(*v),
                        CfdDictKey::String(v) => Value::String(v.clone()),
                        CfdDictKey::Int(v) => Value::Int(
                            i32::try_from(*v).map_err(|_| "dictionary key outside i32 range")?,
                        ),
                        CfdDictKey::Enum(v) => Value::Enum {
                            type_name: v.enum_name.to_string(),
                            value: u32::try_from(v.value).map_err(|_| "invalid enum key")?,
                        },
                    };
                    let key = self.push(key);
                    let value = self.value(v, inner, owner)?;
                    entries.push((key, value));
                }
                Value::Dict(entries)
            }
        };
        let id=self.push(next);
        match value{CfdValue::Function(function)=>{self.function_imports.insert(id,function.imports.clone()); if let Some(location)=&function.location{self.function_locations.insert(id,location.clone());}},CfdValue::FormattedString(template)=>{self.function_imports.insert(id,template.imports.clone()); if let Some(location)=&template.location{self.function_locations.insert(id,location.clone());}},_=>{}}
        if matches!(value,CfdValue::Function(function) if function.from_default) || matches!(value,CfdValue::FormattedString(template) if template.from_default) {self.contract_values.insert(id);}
        Ok(id)
    }
}
