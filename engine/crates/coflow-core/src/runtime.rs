//! 宿主运行时：只读配置、字节码执行、动态值保活与显式生命周期。
use crate::{
    contract::Contract,
    schema::CftValueType,
    vm::ExecutionError,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::AtomicU64,
        Arc,
    },
};

mod build;
pub use build::{BuildDiagnostic, RuntimeBuild, RuntimeBuilder};
mod array;
mod dictionary;
pub use dictionary::DictionaryValue;
mod checking;
pub use array::ArrayValue;
mod execution;
mod heap;
mod state;
mod image;
mod fixed;
pub use checking::CheckSelection;

static NEXT_RUNTIME: AtomicU64 = AtomicU64::new(1);
pub type ValueId = u64;

fn inline_value(id: ValueId) -> Option<Value> {
    use crate::vm::executor::Slot;
    Some(match Slot::from_scalar_id(id)? {
        Slot::None => Value::None,
        Slot::Bool(value) => Value::Bool(value),
        Slot::Int(value) => Value::Int(value),
        Slot::Float(value) => Value::Float(value),
        _ => unreachable!("标量身份解码只产生标量"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptimizationProfile {
    Debug,
    #[default]
    Release,
}

/// 字典键的拥有型归一化表示。键类型限制为 bool/int/string/enum，
/// 查询借用键内容，跨类型键不会命中同一条目。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ScalarKey {
    Bool(bool),
    Int(i32),
    String(String),
    Enum { type_name: String, value: u32 },
}

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
    },
    Array(ArrayValue),
    /// 插入序字典：条目顺序独立于按键类型和分布选择的查询索引。
    Dict(DictionaryValue),
    Dimension {
        default: ValueId,
        variants: BTreeMap<String, ValueId>,
        explicit: BTreeSet<String>,
    },
    Function {
        source: Arc<str>,
        owner: Option<ValueId>,
        host: Option<(String, String)>,
    },
    Template {
        source: Arc<str>,
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
    Enum {
        type_name: String,
        value: u32,
    },
    Existing {
        runtime: u64,
        value: ValueId,
    },
    Data {
        type_name: String,
        fields: Vec<(String, HostValue)>,
    },
    Array(Vec<HostValue>),
    Dictionary(Vec<(HostValue, HostValue)>),
}

/// Host 服务整对象绑定；数据读取和函数调用使用同一 Runtime 生命周期。
/// Runtime 为单线程设计（见 VM 设计文档），服务回调只会从执行线程调用。
pub trait HostService: std::fmt::Debug {
    fn call(&self, field: &str, _arguments: &[HostValue]) -> Result<HostValue, ExecutionError> {
        Err(ExecutionError::InvalidAccess(format!(
            "Host 函数未实现：{field}"
        )))
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

/// 构建成功后不可修改的配置与程序映像，可跨线程共享给多个执行实例。
#[derive(Debug)]
pub struct RuntimeImage {
    profile: OptimizationProfile,
    contract: Arc<Contract>,
    values: fixed::FixedValues,
    records: BTreeMap<(String, String), ValueId>,
    table_values: BTreeMap<String, Vec<ValueId>>,
    check_record_ids: BTreeMap<ValueId, crate::CfdRecordId>,
    record_lookup: BTreeMap<String, BTreeMap<String, ValueId>>,
    constants: BTreeMap<String, ValueId>,
    programs: image::ProgramImage,
}

#[derive(Debug)]
pub struct Runtime {
    identity: u64,
    image: Arc<RuntimeImage>,
    bindings: HostBindings,
    released: std::cell::Cell<bool>,
    creator_thread: std::thread::ThreadId,
    execution: std::cell::Cell<u32>,
    vm: state::VmState,
    check_reporter: Arc<checking::CheckReporter>,
}
impl std::ops::Deref for Runtime {
    type Target = RuntimeImage;
    fn deref(&self) -> &Self::Target {
        &self.image
    }
}

impl Runtime {
    fn code(&self) -> &image::ProgramImage { &self.image.programs }
    pub fn image(&self) -> Arc<RuntimeImage> {
        self.image.clone()
    }
    pub fn identity(&self) -> u64 {
        self.identity
    }
    pub fn optimization_profile(&self) -> OptimizationProfile {
        self.profile
    }
    pub fn contract(&self) -> &Contract {
        &self.contract
    }
    /// 只有没有活动执行边界时才能使实例失效。
    ///
    /// Host 同步重入期间外层帧仍持有本实例的寄存器与根，不能在回调中提前释放。
    pub fn release(&self) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        if self.execution.get() != 0 {
            return Err(ExecutionError::RuntimeBusy);
        }
        self.released.set(true);
        Ok(())
    }
    pub fn is_executing(&self) -> bool {
        self.execution.get() != 0
    }
    pub fn ensure_alive(&self) -> Result<(), ExecutionError> {
        if self.creator_thread != std::thread::current().id() {
            return Err(ExecutionError::InvalidAccess(
                "Runtime 只能在创建线程访问".into(),
            ));
        }
        if self.released.get() {
            Err(ExecutionError::Released)
        } else {
            Ok(())
        }
    }
    pub fn ensure_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        if id < self.values.len() || inline_value(id).is_some() {
            Ok(())
        } else {
            self.vm.value(id).map(|_| ())
        }
    }
    /// 数据与集合按读取值比较；记录、函数按创建身份比较。
    pub fn equals(&self, left: ValueId, right: ValueId) -> Result<bool, ExecutionError> {
        let _entry = self.enter()?;
        self.execution_equals(left, right)
    }
    /// 投影逐节点访问原始存储，不调用 Host 或模板，也不保留整张通用值副本。
    pub fn visit_value_graph(
        &self,
        root: Option<ValueId>,
        mut visit: impl FnMut(ValueId, &Value) -> Result<(), ExecutionError>,
    ) -> Result<usize, ExecutionError> {
        let _entry = self.enter()?;
        // 动态根的借用同时保活整张返回图，访问回调同步重入 GC 也不会丢失待访问子节点。
        let _root = root
            .filter(|id| *id >= self.values.len())
            .map(|id| self.vm.value(id))
            .transpose()?;
        let mut pending = root.map_or_else(|| (0..self.values.len()).collect(), |id| vec![id]);
        let mut visited = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            self.ensure_alive()?;
            let value = if let Some(value) = self.values.materialize(id) {
                execution::MaterializedValue::Materialized {
                    value,
                    _memory: None,
                }
            } else {
                execution::MaterializedValue::Dynamic(self.vm.value(id)?)
            };
            match value.as_ref() {
                Value::Object { fields, bases, .. } => {
                    pending.extend(fields.iter().chain(bases).map(|(_, id)| *id))
                }
                Value::Array(items) => pending.extend(items),
                Value::Dict(items) => {
                    pending.extend(items.values().flat_map(|(key, value)| [*key, *value]))
                }
                Value::Dimension {
                    default, variants, ..
                } => {
                    pending.push(*default);
                    pending.extend(variants.values());
                }
                _ => {}
            }
            visit(id, value.as_ref())?;
        }
        Ok(visited.len())
    }
    pub fn value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        self.ensure_alive()?;
        let value = if let Some(value) = self.values.materialize(id) {
            Arc::new(value)
        } else {
            self.vm.value(id)?
        };
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

    /// 为批量传输读取原始节点；HostData 保持惰性，不在序列化记录时触发回调。
    pub fn stored_value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        self.ensure_alive()?;
        if let Some(value) = self.values.materialize(id) {
            Ok(Arc::new(value))
        } else {
            self.vm.value(id)
        }
    }
    pub fn field(&self, id: ValueId, name: &str) -> Result<ValueId, ExecutionError> {
        self.ensure_alive()?;
        if let Some(value) = self.values.named_field(id, name) {
            return Ok(value);
        }
        match self.value(id)?.as_ref() {
            Value::Object { fields, .. } => fields
                .iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| *value)
                .ok_or_else(|| ExecutionError::InvalidAccess(format!("unknown field {name}"))),
            _ => Err(ExecutionError::InvalidAccess("expected object".into())),
        }
    }
    pub fn dimension_default(&self, id: ValueId) -> Result<ValueId, ExecutionError> {
        match self.value(id)?.as_ref() {
            Value::Dimension { default, .. } => Ok(*default),
            _ => Err(ExecutionError::InvalidAccess(
                "expected dimension value".into(),
            )),
        }
    }
    pub fn dimension_variant(&self, id: ValueId, variant: &str) -> Result<ValueId, ExecutionError> {
        let base = self.dimension_default(id)?;
        let value = self.value(id)?;
        let Value::Dimension { variants, .. } = value.as_ref() else {
            return Err(ExecutionError::InvalidHandle);
        };
        let Some(selected) = variants.get(variant).copied() else {
            return Ok(base);
        };
        // 覆盖存在性独立于值；显式 None 不能与缺失混淆。
        Ok(selected)
    }
    pub fn record(&self, type_name: &str, key: &str) -> Result<ValueId, ExecutionError> {
        self.ensure_alive()?;
        // Reference 热路径：编译期引用已是精确的 Type::key，先零分配精确查找；
        // 子类型引用（如通过父类型名访问）才走可赋值扫描兑底。
        if let Some(exact) = self
            .record_lookup
            .get(type_name)
            .and_then(|keys| keys.get(key))
        {
            return Ok(*exact);
        }
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
            .get(type_name)
            .and_then(|keys| keys.get(key))
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
    /// 一次 Host 读取归一为拥有型具体值；记录保留原身份，标量也不重复触发回调。
    pub fn canonical_value(&self, id: ValueId) -> Result<ValueId, ExecutionError> {
        self.ensure_value(id)?;
        let Some((service, field, value_type)) = self.values.host(id) else {
            return Ok(id);
        };
        let bound = self
            .bindings
            .get(service)
            .ok_or_else(|| ExecutionError::MissingHostBinding(service.into()))?;
        // 在回调前建立共享预算，直接 Host 读取重入也不能绕过深度限制。
        let host = self.execution_host(crate::vm::ExecutionLimits::default())?;
        let _boundary = host.budget.enter_host()?;
        let returned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bound.read(field)))
            .map_err(|_| ExecutionError::InvalidAccess("Host callback panicked".into()))??;
        let slot = host.import(&returned)?;
        if !host.matches(slot, value_type)? {
            return Err(ExecutionError::InvalidAccess(
                "Host data does not match declared type".into(),
            ));
        }
        let value = host.id(slot)?;
        self.retain_value(value)?;
        Ok(value)
    }

    /// 字典查找在核心执行，宿主只传输已声明键类型的标量。
    pub fn dictionary_find(
        &self,
        id: ValueId,
        key: HostValue,
    ) -> Result<Option<ValueId>, ExecutionError> {
        self.ensure_alive()?;
        // 查询只借用固定字典；动态字典借出的 Arc 保持载荷存活。
        let scalar = match key {
            HostValue::Bool(value) => ScalarKey::Bool(value),
            HostValue::Int(value) => ScalarKey::Int(value),
            HostValue::String(value) => ScalarKey::String(value),
            HostValue::Enum { type_name, value } => ScalarKey::Enum { type_name, value },
            _ => return Ok(None),
        };
        if let Some(fixed::View::Dict(entries)) = self.values.view(id) {
            return Ok(entries.get(&scalar).map(|(_, value)| *value));
        }
        let dictionary = self.value(id)?;
        let Value::Dict(entries) = dictionary.as_ref() else {
            return Err(ExecutionError::InvalidAccess("expected dictionary".into()));
        };
        Ok(entries.get(&scalar).map(|(_, value)| *value))
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
        // 在回调前建立共享预算，直接 Host 读取重入也不能绕过深度限制。
        let host = self.execution_host(crate::vm::ExecutionLimits::default())?;
        let _boundary = host.budget.enter_host()?;
        let returned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| bound.read(field)))
            .map_err(|_| ExecutionError::InvalidAccess("Host callback panicked".into()))??;
        if let HostValue::Existing { runtime, value } = &returned {
            if *runtime != self.identity {
                return Err(ExecutionError::ForeignRuntime);
            }
            // 原始占位符不能作为 Host 返回值再次触发同一读取。
            if self.values.is_host(*value) {
                return Err(ExecutionError::InvalidAccess(
                    "Host must return a concrete value".into(),
                ));
            }
        }
        let slot = host.import(&returned)?;
        if !host.matches(slot, value_type)? {
            return Err(ExecutionError::InvalidAccess(
                "Host data does not match declared type".into(),
            ));
        }
        Ok(host.value(slot)?.into_arc())
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
            | (Value::Template { .. }, CftValueType::FString)
            | (Value::String(_), CftValueType::FString) => true,
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
            (Value::Function { host, .. }, CftValueType::Function(..)) => {
                if let Some((service, field)) = host {
                    self.contract
                        .schema()
                        .resolve_type(service)
                        .and_then(|meta| meta.field(field))
                        .is_some_and(|meta| &meta.value_type == ty)
                } else {
                    false
                }
            }
            _ => false,
        })
    }
    fn enter(&self) -> Result<ExecutionGuard<'_>, ExecutionError> {
        self.ensure_alive()?;
        self.execution.set(self.execution.get() + 1);
        Ok(ExecutionGuard(self))
    }
}
struct ExecutionGuard<'a>(&'a Runtime);
impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.execution.set(self.0.execution.get() - 1);
    }
}
