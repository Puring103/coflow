//! Runtime 的执行值适配与动态区。固定配置身份和动态返回值共用 Runtime 归属。
use super::*;
use crate::vm::{
    bytecode::{Constant, Program},
    compiler::{self, CompileContext},
    executor::{self, Binding, Budget, Callable, ExecutionHost, ExecutionLimits, Slot},
};
use std::{cmp::Ordering as Comparison, collections::BTreeSet};

#[derive(Debug)]
struct DynamicValue {
    value: Arc<Value>,
    callable: Option<Binding>,
}
#[derive(Debug, Default)]
struct Heap {
    next: usize,
    bytes: usize,
    values: BTreeMap<ValueId, DynamicValue>,
    pinned: BTreeMap<ValueId, usize>,
    roots: BTreeMap<u64, Vec<Slot>>,
    next_roots: u64,
    next_collection: usize,
}
#[derive(Debug, Default)]
pub(super) struct VmState {
    functions: BTreeMap<ValueId, Binding>,
    heap: Mutex<Heap>,
    budget: Mutex<Option<Budget>>,
}
impl VmState {
    pub(super) fn build(runtime: &Runtime) -> Result<Self, BuildDiagnostic> {
        let mut state = Self::default();
        state.heap.get_mut().map_err(|_| "invalid heap")?.next = runtime.values.len();
        state
            .heap
            .get_mut()
            .map_err(|_| "invalid heap")?
            .next_collection = 1024;
        let mut programs: BTreeMap<
            (String, Option<String>, bool, BTreeMap<String, String>),
            Arc<Program>,
        > = BTreeMap::new();
        for (id, value) in runtime.values.iter().enumerate() {
            let (source, owner, template) = match value.as_ref() {
                Value::Function {
                    source,
                    owner,
                    host: None,
                } => (source, owner, false),
                Value::Template { source, owner } => (source, owner, true),
                _ => continue,
            };
            let type_name =
                owner
                    .and_then(|id| runtime.values.get(id))
                    .and_then(|value| match value.as_ref() {
                        Value::Object { type_name, .. } => Some(type_name.clone()),
                        _ => None,
                    });
            if runtime.contract_values.contains(&id) {
                let location = runtime
                    .function_locations
                    .get(&id)
                    .ok_or("missing contract source location")?;
                let constant_module = location.module.clone();
                let mut owner_type = type_name.clone();
                let program = loop {
                    let module = constant_module
                        .clone()
                        .or_else(|| {
                            owner_type
                                .as_ref()
                                .and_then(|name| runtime.contract.schema().resolve_type(name))
                                .map(|meta| meta.module.clone())
                        })
                        .ok_or("missing compiled program module")?;
                    let key = crate::vm::contract_programs::ProgramKey {
                        module,
                        owner: owner_type.clone(),
                        source: source.clone(),
                        offset: location.span.start,
                    };
                    if let Some(program) = runtime.contract.programs().functions.get(&key) {
                        break program.clone();
                    }
                    let previous = owner_type.clone();
                    owner_type = owner_type.and_then(|name| {
                        runtime
                            .contract
                            .schema()
                            .resolve_type(&name)
                            .and_then(|meta| meta.parent.as_ref().map(ToString::to_string))
                    });
                    if owner_type == previous || (owner_type.is_none() && constant_module.is_none())
                    {
                        return Err("missing compiled contract program".into());
                    }
                };
                state.functions.insert(
                    id,
                    Binding {
                        program,
                        owner: owner.map_or(Slot::None, Slot::Handle),
                        captures: Arc::from([]),
                    },
                );
                continue;
            }
            let imports = runtime
                .function_imports
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let location = runtime.function_locations.get(&id);
            let source = location.map_or(source.as_str(), |location| location.source.as_str());
            let key = (
                format!("{source}:{location:?}"),
                type_name.clone(),
                template,
                imports.clone(),
            );
            let program = if let Some(program) = programs.get(&key) {
                program.clone()
            } else {
                let owner_type = type_name
                    .as_ref()
                    .and_then(|name| runtime.contract.schema().resolve_type(name))
                    .map(|meta| {
                        if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                            CftValueType::Object(meta.name.clone())
                        } else {
                            CftValueType::RecordRef(meta.name.clone())
                        }
                    });
                let context = CompileContext {
                    owner: owner_type,
                    imports,
                    ..CompileContext::default()
                };
                let mut program = if template {
                    compiler::compile_template(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                } else {
                    compiler::compile(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                }
                .map_err(|error| BuildDiagnostic {
                    code: "FUNCTION".into(),
                    source: location
                        .and_then(|location| location.path.clone())
                        .unwrap_or_default(),
                    message: error.message,
                    span: Some((
                        error.span.start + location.map_or(0, |location| location.span.start),
                        error.span.end + location.map_or(0, |location| location.span.start),
                    )),
                })?;
                if let Some(location) = location {
                    program.locate(None, location.path.clone(), location.span.start);
                }
                let program = Arc::new(program);
                programs.insert(key, program.clone());
                program
            };
            state.functions.insert(
                id,
                Binding {
                    program,
                    owner: owner.map_or(Slot::None, Slot::Handle),
                    captures: Arc::from([]),
                },
            );
        }
        // 所有程序（含未执行分支和嵌套闭包）的记录引用在发布 Runtime 前完成链接校验。
        for binding in state.functions.values() {
            validate_references(runtime, &binding.program)?;
        }
        for program in runtime.contract.programs().functions.values() {
            validate_references(runtime, program)?;
        }
        for check in &runtime.contract.programs().checks {
            validate_references(runtime, &check.program)?;
        }
        Ok(state)
    }
    pub(super) fn value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        self.heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?
            .values
            .get(&id)
            .map(|entry| entry.value.clone())
            .ok_or(ExecutionError::InvalidHandle)
    }
    fn allocate(
        &self,
        value: Value,
        callable: Option<Binding>,
        limit: usize,
    ) -> Result<Slot, ExecutionError> {
        let mut heap = self
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        if heap.values.len() >= 1_000_000 {
            return Err(invalid("动态值数量超限"));
        }
        let bytes = dynamic_bytes(&value, callable.as_ref());
        if bytes > limit.saturating_sub(heap.bytes) {
            return Err(invalid("动态内存预算耗尽"));
        }
        heap.bytes += bytes;
        let id = heap.next;
        heap.next = id.checked_add(1).ok_or_else(|| invalid("动态值身份耗尽"))?;
        heap.values.insert(
            id,
            DynamicValue {
                value: Arc::new(value),
                callable,
            },
        );
        Ok(Slot::Handle(id))
    }
}
impl Runtime {
    pub fn invoke(
        &self,
        id: ValueId,
        arguments: &[HostValue],
        limits: ExecutionLimits,
    ) -> Result<HostValue, ExecutionError> {
        let _entry = self.enter()?;
        let host = self.execution_host(limits)?;
        let arguments = arguments
            .iter()
            .map(|value| host.import(value))
            .collect::<Result<Vec<_>, _>>()?;
        let target = host.callable(Slot::Handle(id))?;
        let result = match target {
            Callable::Program(binding) => {
                if arguments.len() != binding.program.parameters.len() {
                    return Err(invalid("参数数量不匹配"));
                }
                for (argument, ty) in arguments.iter().zip(&binding.program.parameters) {
                    if !host.matches(*argument, ty)? {
                        return Err(invalid("参数类型不匹配"));
                    }
                }
                executor::execute(&host, binding, &arguments, host.budget.clone())?
            }
            Callable::Host(target) => host.call_host(target, &arguments)?,
        };
        host.export(result)
    }
    /// 宿主把借用的子值保存到父值生命周期之外时，显式增加独立保活。
    pub fn retain_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        let _entry = self.enter()?;
        self.ensure_value(id)?;
        if id < self.values.len() {
            return Ok(());
        }
        let mut heap = self
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let count = heap.pinned.entry(id).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| invalid("宿主保活计数溢出"))?;
        Ok(())
    }
    /// 宿主显式释放动态返回值的保活。固定配置值随 Runtime 整体管理。
    pub fn release_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        if id < self.values.len() {
            return Ok(());
        }
        let mut heap = self
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let count = heap
            .pinned
            .get_mut(&id)
            .ok_or(ExecutionError::InvalidHandle)?;
        *count -= 1;
        if *count == 0 {
            heap.pinned.remove(&id);
        }
        Ok(())
    }
    pub fn collect(&self) -> Result<usize, ExecutionError> {
        let _entry = self.enter()?;
        let mut heap = self
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let mut pending: Vec<_> = heap.pinned.keys().copied().collect();
        pending.extend(heap.roots.values().flatten().filter_map(|slot| {
            if let Slot::Handle(id) = slot {
                Some(*id)
            } else {
                None
            }
        }));
        let mut live = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !live.insert(id) {
                continue;
            }
            let Some(entry) = heap.values.get(&id) else {
                continue;
            };
            match entry.value.as_ref() {
                Value::Object {
                    fields,
                    bases,
                    dimension,
                    ..
                } => {
                    pending.extend(fields.iter().chain(bases).map(|(_, id)| *id));
                    if let Some((owner, _)) = dimension {
                        pending.push(*owner);
                    }
                }
                Value::Array(values) => pending.extend(values),
                Value::Dict(values) => {
                    for (key, value) in values {
                        pending.extend([*key, *value]);
                    }
                }
                Value::Function { owner, .. } | Value::Template { owner, .. } => {
                    if let Some(owner) = owner {
                        pending.push(*owner);
                    }
                }
                _ => {}
            }
            if let Some(binding) = &entry.callable {
                for slot in binding
                    .captures
                    .iter()
                    .chain(std::iter::once(&binding.owner))
                {
                    if let Slot::Handle(id) = slot {
                        pending.push(*id);
                    }
                }
            }
        }
        let before = heap.values.len();
        heap.values.retain(|id, _| live.contains(id));
        heap.bytes = heap
            .values
            .values()
            .map(|entry| dynamic_bytes(&entry.value, entry.callable.as_ref()))
            .sum();
        Ok(before - heap.values.len())
    }
    pub fn dynamic_value_count(&self) -> Result<usize, ExecutionError> {
        Ok(self
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?
            .values
            .len())
    }
    pub(super) fn execution_host(
        &self,
        limits: ExecutionLimits,
    ) -> Result<RuntimeHost<'_>, ExecutionError> {
        let mut current = self
            .vm
            .budget
            .lock()
            .map_err(|_| invalid("budget poisoned"))?;
        let top = current.is_none();
        let budget = current.get_or_insert_with(|| Budget::new(limits)).clone();
        let mut heap = self
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let roots_id = heap.next_roots;
        heap.next_roots = roots_id
            .checked_add(1)
            .ok_or_else(|| invalid("执行身份耗尽"))?;
        heap.roots.insert(roots_id, Vec::new());
        Ok(RuntimeHost {
            runtime: self,
            budget,
            roots_id,
            top,
        })
    }
    pub(crate) fn execute_check_program(
        &self,
        program: Arc<Program>,
        owner: Option<ValueId>,
        budget: Budget,
    ) -> Result<(), ExecutionError> {
        let _entry = self.enter()?;
        let host = self.execution_host(ExecutionLimits::default())?;
        let result = executor::execute(
            &host,
            Binding {
                program,
                owner: owner.map_or(Slot::None, Slot::Handle),
                captures: Arc::from([]),
            },
            &[],
            budget,
        );
        drop(host);
        self.collect()?;
        result.map(|_| ())
    }
    pub(super) fn execution_equals(
        &self,
        left: ValueId,
        right: ValueId,
    ) -> Result<bool, ExecutionError> {
        let host = self.execution_host(ExecutionLimits::default())?;
        host.roots(&[Slot::Handle(left), Slot::Handle(right)])?;
        host.equal(host.slot(left)?, host.slot(right)?)
    }
    pub(super) fn evaluate_text(&self, id: ValueId) -> Result<String, ExecutionError> {
        let host = self.execution_host(ExecutionLimits::default())?;
        let value = if let Some(binding) = host.template(Slot::Handle(id))? {
            executor::execute(&host, binding, &[], host.budget.clone())?
        } else {
            host.slot(id)?
        };
        match host.value(value)?.as_ref() {
            Value::String(value) => Ok(value.clone()),
            _ => Err(invalid("模板没有返回 string")),
        }
    }
}
pub(super) struct RuntimeHost<'a> {
    runtime: &'a Runtime,
    pub(super) budget: Budget,
    roots_id: u64,
    top: bool,
}
impl Drop for RuntimeHost<'_> {
    fn drop(&mut self) {
        if let Ok(mut heap) = self.runtime.vm.heap.lock() {
            heap.roots.remove(&self.roots_id);
        }
        if self.top {
            if let Ok(mut budget) = self.runtime.vm.budget.lock() {
                *budget = None;
            }
            let _ = self.runtime.collect();
        }
    }
}
impl RuntimeHost<'_> {
    fn slot(&self, id: ValueId) -> Result<Slot, ExecutionError> {
        Ok(match self.runtime.value(id)?.as_ref() {
            Value::None => Slot::None,
            Value::Bool(v) => Slot::Bool(*v),
            Value::Int(v) => Slot::Int(*v),
            Value::Float(v) => Slot::Float(*v),
            _ => Slot::Handle(id),
        })
    }
    fn value(&self, slot: Slot) -> Result<Arc<Value>, ExecutionError> {
        Ok(Arc::new(match slot {
            Slot::None | Slot::Unit => Value::None,
            Slot::Bool(v) => Value::Bool(v),
            Slot::Int(v) => Value::Int(v),
            Slot::Float(v) => Value::Float(v),
            Slot::Handle(id) => return self.runtime.value(id),
            Slot::Empty => return Err(invalid("空寄存器不是语言值")),
        }))
    }
    fn allocate(&self, value: Value) -> Result<Slot, ExecutionError> {
        self.allocate_bound(value, None)
    }
    fn allocate_bound(
        &self,
        value: Value,
        binding: Option<Binding>,
    ) -> Result<Slot, ExecutionError> {
        self.budget.charge(1)?;
        let collect = {
            let heap = self
                .runtime
                .vm
                .heap
                .lock()
                .map_err(|_| invalid("dynamic heap poisoned"))?;
            heap.values.len() >= heap.next_collection
        };
        if collect {
            self.runtime.collect()?;
            let mut heap = self
                .runtime
                .vm
                .heap
                .lock()
                .map_err(|_| invalid("dynamic heap poisoned"))?;
            heap.next_collection = 1024.max(heap.values.len().saturating_mul(2));
        }
        let slot = self
            .runtime
            .vm
            .allocate(value, binding, self.budget.max_heap_bytes())?;
        self.runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?
            .roots
            .entry(self.roots_id)
            .or_default()
            .push(slot);
        Ok(slot)
    }
    fn id(&self, value: Slot) -> Result<ValueId, ExecutionError> {
        if let Slot::Handle(id) = value {
            return Ok(id);
        }
        let stored = self.value(value)?;
        match self.allocate((*stored).clone())? {
            Slot::Handle(id) => Ok(id),
            _ => Err(invalid("分配未产生句柄")),
        }
    }
    fn import(&self, value: &HostValue) -> Result<Slot, ExecutionError> {
        Ok(match value {
            HostValue::None => Slot::None,
            HostValue::Bool(v) => Slot::Bool(*v),
            HostValue::Int(v) => Slot::Int(*v),
            HostValue::Float(v) => Slot::Float(*v),
            HostValue::String(v) => self.allocate(Value::String(v.clone()))?,
            HostValue::Enum { type_name, value } => self.allocate(Value::Enum {
                type_name: type_name.clone(),
                value: *value,
            })?,
            HostValue::Existing { runtime, value } => {
                if *runtime != self.runtime.identity {
                    return Err(ExecutionError::ForeignRuntime);
                }
                self.runtime.ensure_value(*value)?;
                self.slot(*value)?
            }
        })
    }
    fn matches(&self, value: Slot, ty: &CftValueType) -> Result<bool, ExecutionError> {
        if *ty == CftValueType::Unit {
            return Ok(matches!(value, Slot::Unit | Slot::None));
        }
        if let CftValueType::Option(inner) = ty {
            return if value == Slot::None {
                Ok(true)
            } else {
                self.matches(value, inner)
            };
        }
        if let CftValueType::Function(parameters, result) = ty {
            if let Callable::Program(binding) = self.callable(value)? {
                return Ok(binding
                    .program
                    .parameters
                    .iter()
                    .eq(parameters.iter().map(|p| &p.value_type))
                    && binding.program.result == **result);
            }
        }
        self.runtime.matches_type(self.value(value)?.as_ref(), ty)
    }
    fn export(&self, slot: Slot) -> Result<HostValue, ExecutionError> {
        self.export_value(slot, true)
    }
    fn export_value(&self, slot: Slot, retain: bool) -> Result<HostValue, ExecutionError> {
        Ok(match slot {
            Slot::Unit | Slot::None => HostValue::None,
            Slot::Bool(v) => HostValue::Bool(v),
            Slot::Int(v) => HostValue::Int(v),
            Slot::Float(v) => HostValue::Float(v),
            Slot::Handle(id) => match self.runtime.value(id)?.as_ref() {
                Value::String(v) => HostValue::String(v.clone()),
                Value::Enum { type_name, value } => HostValue::Enum {
                    type_name: type_name.clone(),
                    value: *value,
                },
                _ => {
                    if retain && id >= self.runtime.values.len() {
                        *self
                            .runtime
                            .vm
                            .heap
                            .lock()
                            .map_err(|_| invalid("dynamic heap poisoned"))?
                            .pinned
                            .entry(id)
                            .or_default() += 1;
                    }
                    HostValue::Existing {
                        runtime: self.runtime.identity,
                        value: id,
                    }
                }
            },
            Slot::Empty => return Err(invalid("函数返回了空寄存器")),
        })
    }
    fn text(&self, value: Slot) -> Result<String, ExecutionError> {
        match self.value(value)?.as_ref() {
            Value::String(v) => Ok(v.clone()),
            Value::Int(v) => Ok(v.to_string()),
            Value::Float(v) => Ok(v.to_string()),
            Value::Bool(v) => Ok(v.to_string()),
            Value::Enum { type_name, value } => Ok(self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .and_then(|meta| {
                    meta.variants
                        .iter()
                        .find(|variant| variant.value == i64::from(*value))
                })
                .map_or_else(|| value.to_string(), |variant| variant.name.to_string())),
            _ => Err(invalid("值不能转换为文本")),
        }
    }
    fn equal(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError> {
        // 用户可以逐次构造很深的不可变数据链；结构比较使用显式工作栈。
        let mut pending = vec![(left, right)];
        while let Some((left, right)) = pending.pop() {
            self.budget.charge(1)?;
            let left_value = self.value(left)?;
            let right_value = self.value(right)?;
            let equal = match (left_value.as_ref(), right_value.as_ref()) {
                (Value::None, Value::None) => true,
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Int(a), Value::Int(b)) => a == b,
                (Value::Float(a), Value::Float(b)) => a == b,
                (Value::Int(a), Value::Float(b)) => *a as f32 == *b,
                (Value::Float(a), Value::Int(b)) => *a == *b as f32,
                (Value::String(a), Value::String(b)) => {
                    self.budget.charge(a.len().min(b.len()) as u64)?;
                    a == b
                }
                (
                    Value::Enum {
                        type_name: a,
                        value: av,
                    },
                    Value::Enum {
                        type_name: b,
                        value: bv,
                    },
                ) => a == b && av == bv,
                (Value::Function { .. }, Value::Function { .. })
                | (Value::Object { key: Some(_), .. }, Value::Object { key: Some(_), .. }) => {
                    left == right
                }
                (Value::Template { .. }, _) | (_, Value::Template { .. }) => {
                    let read = |slot| -> Result<Slot, ExecutionError> {
                        if let Some(binding) = self.template(slot)? {
                            let host = self.runtime.execution_host(ExecutionLimits::default())?;
                            executor::execute(&host, binding, &[], self.budget.clone())
                        } else {
                            Ok(slot)
                        }
                    };
                    let left = read(left)?;
                    self.runtime
                        .vm
                        .heap
                        .lock()
                        .map_err(|_| invalid("dynamic heap poisoned"))?
                        .roots
                        .entry(self.roots_id)
                        .or_default()
                        .push(left);
                    let right = read(right)?;
                    self.runtime
                        .vm
                        .heap
                        .lock()
                        .map_err(|_| invalid("dynamic heap poisoned"))?
                        .roots
                        .entry(self.roots_id)
                        .or_default()
                        .push(right);
                    pending.push((left, right));
                    true
                }
                (Value::Array(a), Value::Array(b)) => {
                    if a.len() != b.len() {
                        return Ok(false);
                    }
                    self.budget.charge(a.len() as u64)?;
                    for (a, b) in a.iter().zip(b).rev() {
                        pending.push((self.slot(*a)?, self.slot(*b)?));
                    }
                    true
                }
                (Value::Dict(a), Value::Dict(b)) => {
                    if a.len() != b.len() {
                        return Ok(false);
                    }
                    for (key, value) in a.iter().rev() {
                        let mut found = None;
                        // 字典键由静态规则限制为标量；这里的比较不会递归数据结构。
                        for (other_key, other_value) in b {
                            if self.equal(self.slot(*key)?, self.slot(*other_key)?)? {
                                found = Some(*other_value);
                                break;
                            }
                        }
                        let Some(other) = found else {
                            return Ok(false);
                        };
                        pending.push((self.slot(*value)?, self.slot(other)?));
                    }
                    true
                }
                (
                    Value::Object {
                        type_name: a,
                        fields: af,
                        key: None,
                        ..
                    },
                    Value::Object {
                        type_name: b,
                        fields: bf,
                        key: None,
                        ..
                    },
                ) => {
                    if a != b || af.len() != bf.len() {
                        return Ok(false);
                    }
                    for ((an, av), (bn, bv)) in af.iter().zip(bf).rev() {
                        if an != bn {
                            return Ok(false);
                        }
                        pending.push((self.slot(*av)?, self.slot(*bv)?));
                    }
                    true
                }
                _ => false,
            };
            if !equal {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
impl ExecutionHost for RuntimeHost<'_> {
    fn location(&self, program: &Program, span: crate::source::Span) {
        if let Ok(mut location) = self.runtime.check_reporter.location.lock() {
            *location = program
                .module
                .as_ref()
                .map(|module| crate::check::CheckSchemaLocation {
                    module: module.clone(),
                    span,
                });
        }
    }
    fn constant(&self, value: &Constant) -> Result<Slot, ExecutionError> {
        Ok(match value {
            Constant::Unit => Slot::Unit,
            Constant::None => Slot::None,
            Constant::Bool(v) => Slot::Bool(*v),
            Constant::Int(v) => Slot::Int(*v),
            Constant::Float(v) => Slot::Float(*v),
            Constant::String(v) => self.allocate(Value::String(v.clone()))?,
            Constant::Enum { name, value } => self.allocate(Value::Enum {
                type_name: name.clone(),
                value: *value,
            })?,
        })
    }
    fn field(&self, receiver: Slot, slot: u16) -> Result<Slot, ExecutionError> {
        let value = self.value(receiver)?;
        if let Value::Object { fields, .. } = value.as_ref() {
            self.slot(
                fields
                    .get(usize::from(slot))
                    .ok_or_else(|| invalid("字段槽越界"))?
                    .1,
            )
        } else {
            Err(invalid("字段读取需要对象"))
        }
    }
    fn index(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError> {
        match self.value(receiver)?.as_ref() {
            Value::Array(values) => {
                let index = index(key)?;
                self.slot(*values.get(index).ok_or_else(|| invalid("数组索引越界"))?)
            }
            Value::String(value) => {
                let index = index(key)?;
                self.budget.charge(index as u64 + 1)?;
                let ch = value
                    .chars()
                    .nth(index)
                    .ok_or_else(|| invalid("字符串索引越界"))?;
                self.allocate(Value::String(ch.to_string()))
            }
            Value::Dict(values) => {
                for (k, v) in values {
                    if self.equal(self.slot(*k)?, key)? {
                        return self.slot(*v);
                    }
                }
                Err(invalid("字典 key 不存在"))
            }
            _ => Err(invalid("值不支持索引")),
        }
    }
    fn reference(&self, name: &str) -> Result<Slot, ExecutionError> {
        if let Some(name) = name.strip_prefix("$const::") {
            return self.slot(
                *self
                    .runtime
                    .constants
                    .get(name)
                    .ok_or_else(|| invalid("未知常量"))?,
            );
        }
        if let Some(host) = name.strip_prefix("$host::") {
            let (service, field) = host
                .rsplit_once("::")
                .ok_or_else(|| invalid("无效 Host 引用"))?;
            return self.allocate(Value::Function {
                source: String::new(),
                owner: None,
                host: Some((service.into(), field.into())),
            });
        }
        let (ty, key) = name
            .rsplit_once("::")
            .ok_or_else(|| invalid("无效记录引用"))?;
        self.slot(self.runtime.record(ty, key)?)
    }
    fn equals(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError> {
        self.equal(left, right)
    }
    fn compare(&self, left: Slot, right: Slot) -> Result<Option<Comparison>, ExecutionError> {
        self.budget.charge(1)?;
        Ok(
            match (self.value(left)?.as_ref(), self.value(right)?.as_ref()) {
                (Value::Int(a), Value::Int(b)) => Some(a.cmp(b)),
                (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
                (Value::String(a), Value::String(b)) => {
                    self.budget.charge(a.len().min(b.len()) as u64)?;
                    Some(a.cmp(b))
                }
                (
                    Value::Enum {
                        type_name: a,
                        value: av,
                    },
                    Value::Enum {
                        type_name: b,
                        value: bv,
                    },
                ) if a == b => Some(av.cmp(bv)),
                _ => return Err(invalid("值不支持顺序比较")),
            },
        )
    }
    fn concatenate(&self, left: Slot, right: Slot) -> Result<Slot, ExecutionError> {
        match (self.value(left)?.as_ref(), self.value(right)?.as_ref()) {
            (Value::String(a), Value::String(b)) => {
                self.budget.charge((a.len() + b.len()) as u64)?;
                self.allocate(Value::String(format!("{a}{b}")))
            }
            _ => Err(invalid("连接需要 string")),
        }
    }
    fn enum_unary(&self, value: Slot) -> Result<Slot, ExecutionError> {
        if let Value::Enum { type_name, value } = self.value(value)?.as_ref() {
            let meta = self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .ok_or_else(|| invalid("未知 enum"))?;
            let mask = meta
                .variants
                .iter()
                .fold(0u32, |mask, v| mask | v.value as u32);
            self.allocate(Value::Enum {
                type_name: type_name.clone(),
                value: !value & mask,
            })
        } else {
            Err(invalid("需要 flag"))
        }
    }
    fn enum_binary(&self, operator: u8, left: Slot, right: Slot) -> Result<Slot, ExecutionError> {
        match (self.value(left)?.as_ref(), self.value(right)?.as_ref()) {
            (
                Value::Enum {
                    type_name: a,
                    value: av,
                },
                Value::Enum {
                    type_name: b,
                    value: bv,
                },
            ) if a == b => self.allocate(Value::Enum {
                type_name: a.clone(),
                value: match operator {
                    15 => av & bv,
                    16 => av | bv,
                    17 => av ^ bv,
                    _ => return Err(invalid("无效位运算")),
                },
            }),
            _ => Err(invalid("需要同一 flag 类型")),
        }
    }
    fn is_type(&self, value: Slot, type_name: &str) -> Result<bool, ExecutionError> {
        Ok(
            matches!(self.value(value)?.as_ref(),Value::Object{type_name:actual,..}if self.runtime.contract.schema().is_assignable(actual,type_name)),
        )
    }
    fn callable(&self, value: Slot) -> Result<Callable, ExecutionError> {
        let Slot::Handle(id) = value else {
            return Err(invalid("需要函数"));
        };
        if matches!(
            self.runtime.value(id)?.as_ref(),
            Value::Function { host: Some(_), .. }
        ) {
            return Ok(Callable::Host(value));
        }
        if let Some(binding) = self.runtime.vm.functions.get(&id) {
            return Ok(Callable::Program(binding.clone()));
        }
        let heap = self
            .runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        heap.values
            .get(&id)
            .and_then(|entry| entry.callable.clone())
            .map(Callable::Program)
            .ok_or_else(|| invalid("函数没有实现"))
    }
    fn call_host(&self, target: Slot, args: &[Slot]) -> Result<Slot, ExecutionError> {
        let value = self.value(target)?;
        let Value::Function {
            host: Some((service, field)),
            ..
        } = value.as_ref()
        else {
            return Err(invalid("需要 Host 函数"));
        };
        let binding = self
            .runtime
            .bindings
            .get(service)
            .ok_or_else(|| ExecutionError::MissingHostBinding(service.clone()))?;
        let signature = if service == "Coflow::Check" && field == "require" {
            CftValueType::Function(
                vec![
                    crate::schema::CftFunctionParameter::unnamed(CftValueType::Bool),
                    crate::schema::CftFunctionParameter::unnamed(CftValueType::String),
                ],
                Box::new(CftValueType::Unit),
            )
        } else {
            self.runtime
                .contract
                .schema()
                .field(service, field)
                .ok_or_else(|| invalid("未知 Host 函数"))?
                .value_type
                .clone()
        };
        let CftValueType::Function(parameters, result_type) = signature else {
            return Err(invalid("Host 成员不是函数"));
        };
        if parameters.len() != args.len() {
            return Err(invalid("Host 参数数量不匹配"));
        }
        for (arg, parameter) in args.iter().zip(&parameters) {
            if !self.matches(*arg, &parameter.value_type)? {
                return Err(invalid("Host 参数类型不匹配"));
            }
        }
        let args = args
            .iter()
            .map(|value| self.export_value(*value, false))
            .collect::<Result<Vec<_>, _>>()?;
        let returned =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| binding.call(field, &args)))
                .map_err(|_| invalid("Host callback panicked"))??;
        let returned = self.import(&returned)?;
        if !self.matches(returned, &result_type)? {
            return Err(invalid("Host 返回值类型不匹配"));
        }
        Ok(if *result_type == CftValueType::Unit {
            Slot::Unit
        } else {
            returned
        })
    }
    fn closure(&self, binding: Binding, template: bool) -> Result<Slot, ExecutionError> {
        let owner = if let Slot::Handle(id) = binding.owner {
            Some(id)
        } else {
            None
        };
        let source = binding.program.source.clone();
        let value = if template {
            Value::Template { source, owner }
        } else {
            Value::Function {
                source,
                owner,
                host: None,
            }
        };
        self.allocate_bound(value, Some(binding))
    }
    fn append(&self, target: Slot, value: Slot, key: Option<Slot>) -> Result<(), ExecutionError> {
        self.budget.charge(1)?;
        let Slot::Handle(target) = target else {
            return Err(invalid("集合构造目标不是句柄"));
        };
        let value = self.id(value)?;
        let key = key.map(|key| self.id(key)).transpose()?;
        let mut heap = self
            .runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let entry = heap
            .values
            .get_mut(&target)
            .ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_ref());
        // Append 仅由高阶方法 lowering 使用；构造中的集合未暴露给用户函数。
        match (Arc::make_mut(&mut entry.value), key) {
            (Value::Array(values), None) => values.push(value),
            (Value::Dict(values), Some(key)) => values.push((key, value)),
            _ => return Err(invalid("集合构造类型不匹配")),
        }
        let after = dynamic_bytes(&entry.value, entry.callable.as_ref());
        heap.bytes = heap.bytes - before + after;
        if heap.bytes > self.budget.max_heap_bytes() {
            return Err(invalid("动态内存预算耗尽"));
        }
        Ok(())
    }
    fn array(&self, values: Vec<Slot>) -> Result<Slot, ExecutionError> {
        let values = values
            .into_iter()
            .map(|value| self.id(value))
            .collect::<Result<_, _>>()?;
        self.allocate(Value::Array(values))
    }
    fn dictionary(&self, values: Vec<(Slot, Slot)>) -> Result<Slot, ExecutionError> {
        for (index, (key, _)) in values.iter().enumerate() {
            for (other, _) in &values[..index] {
                if self.equal(*key, *other)? {
                    return Err(invalid("字典 key 重复"));
                }
            }
        }
        let values = values
            .into_iter()
            .map(|(k, v)| Ok((self.id(k)?, self.id(v)?)))
            .collect::<Result<_, ExecutionError>>()?;
        self.allocate(Value::Dict(values))
    }
    fn reserve_object(&self, type_name: &str) -> Result<Slot, ExecutionError> {
        self.allocate(Value::Object {
            type_name: type_name.into(),
            key: None,
            fields: Vec::new(),
            bases: Vec::new(),
            dimension: None,
        })
    }
    fn initialize_object(
        &self,
        target: Slot,
        type_name: &str,
        fields: Vec<(&str, Slot)>,
    ) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = target else {
            return Err(invalid("对象构造需要预留身份"));
        };
        let meta = self
            .runtime
            .contract
            .schema()
            .resolve_type(type_name)
            .ok_or_else(|| invalid("未知 data 类型"))?;
        let mut stored = Vec::new();
        for field in meta.all_fields() {
            let value = if let Some((_, value)) =
                fields.iter().find(|(name, _)| *name == field.name.as_str())
            {
                *value
            } else if let Some(default) = &field.default {
                let module = &self
                    .runtime
                    .contract
                    .schema()
                    .resolve_type(&field.declaring_type)
                    .ok_or_else(|| invalid("未知字段声明类型"))?
                    .module;
                self.default_value(default, target, module)?
            } else {
                match field.value_type {
                    CftValueType::Option(_) => Slot::None,
                    CftValueType::Array(_) => self.array(Vec::new())?,
                    CftValueType::Dict(..) => self.dictionary(Vec::new())?,
                    _ => return Err(invalid("缺少 data 字段")),
                }
            };
            stored.push((field.name.to_string(), self.id(value)?));
        }
        let value = Arc::new(Value::Object {
            type_name: type_name.into(),
            key: None,
            fields: stored,
            bases: Vec::new(),
            dimension: None,
        });
        let mut heap = self
            .runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?;
        let entry = heap.values.get(&id).ok_or(ExecutionError::InvalidHandle)?;
        let bytes = heap.bytes - dynamic_bytes(&entry.value, entry.callable.as_ref())
            + dynamic_bytes(&value, None);
        if bytes > self.budget.max_heap_bytes() {
            return Err(invalid("动态内存预算耗尽"));
        }
        heap.values
            .get_mut(&id)
            .ok_or(ExecutionError::InvalidHandle)?
            .value = value;
        heap.bytes = bytes;
        Ok(target)
    }
    fn object(&self, type_name: &str, fields: Vec<(&str, Slot)>) -> Result<Slot, ExecutionError> {
        let target = self.reserve_object(type_name)?;
        self.initialize_object(target, type_name, fields)
    }
    fn template(&self, value: Slot) -> Result<Option<Binding>, ExecutionError> {
        if !matches!(self.value(value)?.as_ref(), Value::Template { .. }) {
            return Ok(None);
        }
        match self.callable(value)? {
            Callable::Program(binding) => Ok(Some(binding)),
            _ => Err(invalid("无效模板目标")),
        }
    }
    fn format(&self, values: &[Slot]) -> Result<Slot, ExecutionError> {
        let mut result = String::new();
        for value in values {
            let text = self.text(*value)?;
            self.budget.charge(text.len() as u64)?;
            result.push_str(&text);
        }
        self.allocate(Value::String(result))
    }
    fn length(&self, value: Slot) -> Result<usize, ExecutionError> {
        Ok(match self.value(value)?.as_ref() {
            Value::Array(v) => v.len(),
            Value::Dict(v) => v.len(),
            Value::String(v) => {
                self.budget.charge(v.len() as u64)?;
                v.chars().count()
            }
            _ => return Err(invalid("值没有长度")),
        })
    }
    fn iterator(&self, value: Slot, index: usize, key: bool) -> Result<Slot, ExecutionError> {
        match self.value(value)?.as_ref() {
            Value::Array(values) => {
                if key {
                    Ok(Slot::Int(
                        i32::try_from(index).map_err(|_| invalid("索引超出 int"))?,
                    ))
                } else {
                    self.slot(*values.get(index).ok_or_else(|| invalid("索引越界"))?)
                }
            }
            Value::Dict(values) => {
                let (k, v) = values.get(index).ok_or_else(|| invalid("索引越界"))?;
                self.slot(if key { *k } else { *v })
            }
            _ => Err(invalid("值不能迭代")),
        }
    }
    fn builtin(
        &self,
        name: &str,
        receiver: Slot,
        args: &[Slot],
        result_type: &CftValueType,
    ) -> Result<Slot, ExecutionError> {
        self.builtin_value(name, receiver, args, result_type)
    }
    fn roots(&self, roots: &[Slot]) -> Result<(), ExecutionError> {
        self.runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?
            .roots
            .insert(self.roots_id, roots.to_vec());
        Ok(())
    }
}
fn invalid(message: &str) -> ExecutionError {
    ExecutionError::InvalidAccess(message.into())
}
fn index(value: Slot) -> Result<usize, ExecutionError> {
    if let Slot::Int(value) = value {
        usize::try_from(value).map_err(|_| invalid("负索引"))
    } else {
        Err(invalid("索引需要 int"))
    }
}
impl RuntimeHost<'_> {
    fn default_value(
        &self,
        value: &crate::schema::CftSchemaDefaultValue,
        owner: Slot,
        module: &crate::schema::ModuleId,
    ) -> Result<Slot, ExecutionError> {
        use crate::schema::CftSchemaDefaultValue as D;
        match value {
            D::OptionNone => Ok(Slot::None),
            D::OptionSome(value) => self.default_value(value, owner, module),
            D::Int(value) => Ok(Slot::Int(
                i32::try_from(*value).map_err(|_| invalid("默认 int 越界"))?,
            )),
            D::Float(value) => Ok(Slot::Float(*value as f32)),
            D::Bool(value) => Ok(Slot::Bool(*value)),
            D::String(value) => self.allocate(Value::String(value.clone())),
            D::Enum {
                enum_name, value, ..
            } => self.allocate(Value::Enum {
                type_name: enum_name.to_string(),
                value: *value as u32,
            }),
            D::EmptyArray => self.array(Vec::new()),
            D::EmptyObject => self.dictionary(Vec::new()),
            D::Array(values) => self.array(
                values
                    .iter()
                    .map(|value| self.default_value(value, owner, module))
                    .collect::<Result<_, _>>()?,
            ),
            D::Dictionary(values) => self.dictionary(
                values
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            self.default_value(k, owner, module)?,
                            self.default_value(v, owner, module)?,
                        ))
                    })
                    .collect::<Result<_, ExecutionError>>()?,
            ),
            D::Object { type_name, fields } => {
                let target = self.reserve_object(type_name)?;
                let fields = fields
                    .iter()
                    .map(|(name, value)| {
                        Ok((name.as_str(), self.default_value(value, target, module)?))
                    })
                    .collect::<Result<_, ExecutionError>>()?;
                self.initialize_object(target, type_name, fields)
            }
            D::RecordReference { type_name, key } => self.reference(&format!("{type_name}::{key}")),
            D::Function(source) | D::FormattedString(source) => {
                let template = matches!(value, D::FormattedString(_));
                let owner_type = match self.value(owner)?.as_ref() {
                    Value::Object { type_name, .. } => Some(type_name.clone()),
                    _ => None,
                };
                let mut owner_type = owner_type;
                let program = loop {
                    let key = crate::vm::contract_programs::ProgramKey {
                        module: source.module.clone(),
                        owner: owner_type.clone(),
                        source: source.source.clone(),
                        offset: source.span.start,
                    };
                    if let Some(program) = self.runtime.contract.programs().functions.get(&key) {
                        break program.clone();
                    }
                    let Some(owner) = owner_type else {
                        return Err(invalid("缺少契约默认程序"));
                    };
                    owner_type = self
                        .runtime
                        .contract
                        .schema()
                        .resolve_type(&owner)
                        .and_then(|meta| meta.parent.as_ref().map(ToString::to_string));
                };
                self.closure(
                    Binding {
                        program,
                        owner,
                        captures: Arc::from([]),
                    },
                    template,
                )
            }
        }
    }
    fn read_slot(&self, value: Slot) -> Result<Slot, ExecutionError> {
        let Some(binding) = self.template(value)? else {
            return Ok(value);
        };
        let host = self.runtime.execution_host(ExecutionLimits::default())?;
        let result = executor::execute(&host, binding, &[], self.budget.clone())?;
        self.runtime
            .vm
            .heap
            .lock()
            .map_err(|_| invalid("dynamic heap poisoned"))?
            .roots
            .entry(self.roots_id)
            .or_default()
            .push(result);
        Ok(result)
    }
    fn builtin_value(
        &self,
        name: &str,
        receiver: Slot,
        args: &[Slot],
        result_type: &CftValueType,
    ) -> Result<Slot, ExecutionError> {
        if let Some(type_name) = name.strip_prefix("$enum::") {
            let Slot::Int(value) = receiver else {
                return Err(invalid("enum 构造需要 int"));
            };
            let value = u32::try_from(value).map_err(|_| invalid("enum 值不能为负"))?;
            let meta = self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .ok_or_else(|| invalid("未知 enum"))?;
            if meta.is_flag {
                let mask = meta
                    .variants
                    .iter()
                    .fold(0u32, |mask, variant| mask | variant.value as u32);
                if value & !mask != 0 {
                    return Err(invalid("flag 包含未知位"));
                }
            }
            return self.allocate(Value::Enum {
                type_name: type_name.into(),
                value,
            });
        }
        if matches!(name, "for" | "default" | "variants") {
            let Slot::Handle(id) = receiver else {
                return Err(invalid("维度方法需要记录"));
            };
            if name == "default" {
                return self.read_slot(self.slot(self.runtime.dimension_default(id)?)?);
            }
            if name == "for" {
                let Some(argument) = args.first() else {
                    return Err(invalid("缺少变体名"));
                };
                let text = self.text(*argument)?;
                return self.read_slot(self.slot(self.runtime.dimension_variant(id, &text)?)?);
            }
            let value = self.runtime.value(id)?;
            let Value::Object { type_name, .. } = value.as_ref() else {
                return Err(invalid("维度方法需要记录"));
            };
            let (dimension, _) =
                loading::dimension_source(self.runtime.contract.schema(), type_name)
                    .ok_or_else(|| invalid("需要维度记录"))?;
            let mut values = Vec::new();
            for variant in &dimension.variants {
                self.budget.charge(1)?;
                let key = self.allocate(Value::String(variant.to_string()))?;
                let value =
                    self.read_slot(self.slot(self.runtime.dimension_variant(id, variant)?)?)?;
                values.push((key, value));
            }
            return self.dictionary(values);
        }
        if let Some(type_name) = name.strip_prefix("$records::") {
            return self.array(
                self.runtime
                    .records(type_name)?
                    .into_iter()
                    .map(Slot::Handle)
                    .collect(),
            );
        }
        let argument = |index: usize| {
            args.get(index)
                .copied()
                .ok_or_else(|| invalid("缺少内建参数"))
        };
        if name == "len" {
            return Ok(Slot::Int(
                i32::try_from(self.length(receiver)?).map_err(|_| invalid("长度超出 int"))?,
            ));
        }
        if name == "string" {
            return self.allocate(Value::String(self.text(receiver)?));
        }
        if name == "isSome" || name == "isNone" {
            return Ok(Slot::Bool(if name == "isSome" {
                receiver != Slot::None
            } else {
                receiver == Slot::None
            }));
        }
        match self.value(receiver)?.as_ref() {
            Value::Int(value) => match name {
                "abs" => Ok(Slot::Int(
                    value.checked_abs().ok_or_else(|| invalid("绝对值溢出"))?,
                )),
                "float" => Ok(Slot::Float(*value as f32)),
                _ => Err(invalid("未知 int 内建")),
            },
            Value::Float(value) => match name {
                "abs" => Ok(Slot::Float(value.abs())),
                "isFinite" => Ok(Slot::Bool(value.is_finite())),
                "int" => {
                    if value.is_finite() && *value >= -2147483648.0 && *value < 2147483648.0 {
                        Ok(Slot::Int(value.trunc() as i32))
                    } else {
                        Err(invalid("float 转 int 越界"))
                    }
                }
                "approxEqual" => {
                    let (Slot::Float(other), Slot::Float(epsilon)) = (argument(0)?, argument(1)?)
                    else {
                        return Err(invalid("需要 float 参数"));
                    };
                    if !epsilon.is_finite() || epsilon < 0.0 {
                        return Err(invalid("epsilon 必须有限且非负"));
                    }
                    Ok(Slot::Bool((*value - other).abs() <= epsilon))
                }
                _ => Err(invalid("未知 float 内建")),
            },
            Value::String(value) => {
                self.budget.charge(value.len() as u64)?;
                if name == "isBlank" {
                    return Ok(Slot::Bool(value.chars().all(char::is_whitespace)));
                }
                if name == "parseInt" {
                    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
                    return Ok(
                        if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
                            value.parse::<i32>().map_or(Slot::None, Slot::Int)
                        } else {
                            Slot::None
                        },
                    );
                }
                if name == "parseFloat" {
                    let numeric = regex::Regex::new(
                        r"^[+-]?(?:[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?|inf|NaN)$",
                    )
                    .map_err(|e| invalid(&e.to_string()))?;
                    return Ok(if numeric.is_match(value) {
                        value.parse::<f32>().map_or(Slot::None, Slot::Float)
                    } else {
                        Slot::None
                    });
                }
                let other = self.value(argument(0)?)?;
                let Value::String(other) = other.as_ref() else {
                    return Err(invalid("需要 string 参数"));
                };
                Ok(Slot::Bool(match name {
                    "contains" => value.contains(other),
                    "startsWith" => value.starts_with(other),
                    "endsWith" => value.ends_with(other),
                    "matches" => regex::Regex::new(other)
                        .map_err(|e| invalid(&e.to_string()))?
                        .is_match(value),
                    _ => return Err(invalid("未知 string 内建")),
                }))
            }
            Value::Array(values) => {
                self.budget.charge(values.len() as u64)?;
                if name == "contains" {
                    for value in values {
                        if self.equal(self.slot(*value)?, argument(0)?)? {
                            return Ok(Slot::Bool(true));
                        }
                    }
                    return Ok(Slot::Bool(false));
                }
                if name == "isUnique" {
                    for (index, value) in values.iter().enumerate() {
                        for other in &values[..index] {
                            if self.equal(self.slot(*value)?, self.slot(*other)?)? {
                                return Ok(Slot::Bool(false));
                            }
                        }
                    }
                    return Ok(Slot::Bool(true));
                }
                if name == "isSorted" || name == "isStrictlySorted" {
                    for value in values {
                        if matches!(self.slot(*value)?,Slot::Float(v)if v.is_nan()) {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    for pair in values.windows(2) {
                        let order = self.compare(self.slot(pair[0])?, self.slot(pair[1])?)?;
                        if !(order == Some(Comparison::Less)
                            || (name == "isSorted" && order == Some(Comparison::Equal)))
                        {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    return Ok(Slot::Bool(true));
                }
                if matches!(name, "min" | "max" | "sum") {
                    if values.is_empty() {
                        return if name == "sum" {
                            Ok(if *result_type == CftValueType::Float {
                                Slot::Float(0.0)
                            } else {
                                Slot::Int(0)
                            })
                        } else {
                            Err(invalid("空数组没有极值"))
                        };
                    }
                    let mut result = self.slot(values[0])?;
                    for id in &values[1..] {
                        let next = self.slot(*id)?;
                        result = if name == "sum" {
                            match (result, next) {
                                (Slot::Int(a), Slot::Int(b)) => {
                                    Slot::Int(a.checked_add(b).ok_or_else(|| invalid("求和溢出"))?)
                                }
                                (Slot::Float(a), Slot::Float(b)) => Slot::Float(a + b),
                                _ => return Err(invalid("求和需要数值")),
                            }
                        } else if matches!(result,Slot::Float(v)if v.is_nan()) {
                            result
                        } else if matches!(next,Slot::Float(v)if v.is_nan()) {
                            next
                        } else {
                            let order = self.compare(result, next)?;
                            if (name == "min" && order == Some(Comparison::Greater))
                                || (name == "max" && order == Some(Comparison::Less))
                            {
                                next
                            } else {
                                result
                            }
                        };
                    }
                    return Ok(result);
                }
                if matches!(
                    name,
                    "intersects" | "isDisjoint" | "isSubsetOf" | "isSupersetOf"
                ) {
                    let other = self.value(argument(0)?)?;
                    let Value::Array(other) = other.as_ref() else {
                        return Err(invalid("需要数组"));
                    };
                    let (left, right) = if name == "isSupersetOf" {
                        (other, values)
                    } else {
                        (values, other)
                    };
                    for value in left {
                        let mut found = false;
                        for other in right {
                            if self.equal(self.slot(*value)?, self.slot(*other)?)? {
                                found = true;
                                break;
                            }
                        }
                        if matches!(name, "intersects" | "isDisjoint") && found {
                            return Ok(Slot::Bool(name == "intersects"));
                        }
                        if matches!(name, "isSubsetOf" | "isSupersetOf") && !found {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    return Ok(Slot::Bool(name != "intersects"));
                }
                Err(invalid("未知数组内建"))
            }
            Value::Dict(values) => {
                self.budget.charge(values.len() as u64)?;
                if name == "keys" || name == "values" {
                    return self.array(
                        values
                            .iter()
                            .map(|(key, value)| {
                                self.slot(if name == "keys" { *key } else { *value })
                            })
                            .collect::<Result<_, _>>()?,
                    );
                }
                if matches!(name, "contains" | "containsKey" | "containsValue") {
                    for (key, value) in values {
                        if self.equal(
                            self.slot(if name == "containsValue" {
                                *value
                            } else {
                                *key
                            })?,
                            argument(0)?,
                        )? {
                            return Ok(Slot::Bool(true));
                        }
                    }
                    return Ok(Slot::Bool(false));
                }
                Err(invalid("未知字典内建"))
            }
            _ => Err(invalid("该值不提供内建方法")),
        }
    }
}

fn validate_references(runtime: &Runtime, program: &Program) -> Result<(), String> {
    for instruction in &program.instructions {
        if instruction.opcode() != Some(crate::vm::bytecode::Opcode::Reference) {
            continue;
        }
        let name = &program.names[instruction.index() as usize];
        if name.starts_with("$host::") {
            continue;
        }
        if let Some(name) = name.strip_prefix("$const::") {
            if !runtime.constants.contains_key(name) {
                return Err(format!("{}: 未链接常量 {name}", program.name));
            }
        } else {
            let (ty, key) = name
                .rsplit_once("::")
                .ok_or_else(|| format!("无效记录引用 {name}"))?;
            runtime
                .record(ty, key)
                .map_err(|error| format!("{}: {name}: {error}", program.name))?;
        }
    }
    for closure in &program.closures {
        validate_references(runtime, &closure.program)?;
    }
    Ok(())
}

/// 计入值头、字符串容量、集合容量与捕获槽；共享的只读程序归 Contract 管理。
fn dynamic_bytes(value: &Value, binding: Option<&Binding>) -> usize {
    use std::mem::size_of;
    let payload = match value {
        Value::String(text) => text.capacity(),
        Value::Enum { type_name, .. } => type_name.capacity(),
        Value::Array(values) => values.capacity() * size_of::<ValueId>(),
        Value::Dict(values) => values.capacity() * size_of::<(ValueId, ValueId)>(),
        Value::Object {
            type_name,
            key,
            fields,
            bases,
            dimension,
        } => {
            type_name.capacity()
                + key.as_ref().map_or(0, String::capacity)
                + (fields.capacity() + bases.capacity()) * size_of::<(String, ValueId)>()
                + fields
                    .iter()
                    .chain(bases)
                    .map(|(name, _)| name.capacity())
                    .sum::<usize>()
                + dimension.as_ref().map_or(0, |(_, name)| name.capacity())
        }
        Value::Function { source, host, .. } => {
            source.capacity()
                + host
                    .as_ref()
                    .map_or(0, |(service, field)| service.capacity() + field.capacity())
        }
        Value::Template { source, .. } => source.capacity(),
        Value::HostData { service, field, .. } => service.capacity() + field.capacity(),
        _ => 0,
    };
    size_of::<DynamicValue>()
        + size_of::<Value>()
        + payload
        + binding.map_or(0, |binding| binding.captures.len() * size_of::<Slot>())
}
