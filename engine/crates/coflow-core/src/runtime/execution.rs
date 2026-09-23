//! Runtime 的执行值适配与动态区。固定配置身份和动态返回值共用 Runtime 归属。
use super::fixed::View as Stored;
use super::heap::{Heap, dynamic_bytes};
#[cfg(test)] use crate::vm::bytecode_optimization::{fold_scalar_control_flow, fuse_int_immediates};
use super::*;
mod builtins;
mod comparison;
mod host;
use super::state::{VmState, CachedRegex};
use crate::vm::{
    bytecode::{Constant, FormatPart, FunctionId, Program},
    executor::{self, FunctionBinding, Budget, CallBinding, Callable, ExecutionHost, ExecutionLimits, Slot},
    image::ValidatedProgram,
};
use std::{
    cmp::Ordering as Comparison,
    cell::RefCell,
    collections::{HashSet, HashMap},
};

impl Runtime {
    pub fn invoke(
        &self,
        id: ValueId,
        arguments: &[HostValue],
        limits: ExecutionLimits,
    ) -> Result<HostValue, ExecutionError> {
        let _entry = self.enter()?;
        self.ensure_value(id)?;
        let host = self.execution_host(limits)?;
        // 外部实参窗口也属于本次调用预算，并跨同步重入保持累计占用。
        let (mut imported, _arguments_memory) = host.reserve_values(arguments.len())?;
        for value in arguments {
            imported.push(host.import(value)?);
        }
        let arguments = imported;
        let target = host.callable(Slot::handle(id))?;
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
    pub(super) fn execution_host(
        &self,
        limits: ExecutionLimits,
    ) -> Result<ExecutionContext<'_>, ExecutionError> {
        // 请求间垃圾不能消耗下一次更小的预算；此时尚未安装新预算，也没有新执行根。
        let collect = self.vm.budget.borrow().is_none() && {
            let heap = self.vm.heap.borrow();
            heap.collection_dirty && limits.max_heap_bytes < heap.collection_limit
                // 每个动态槽都被宿主直接保活时，没有可回收垃圾，无需扫描。
                && heap.live_values > heap.pinned.len()
        };
        if collect { self.collect()?; }
        let mut current = self.vm.budget.borrow_mut();
        let top = current.is_none();
        let mut heap = self.vm.heap.borrow_mut();
        let roots_id = heap.next_roots;
        heap.next_roots = roots_id
            .checked_add(1)
            .ok_or_else(|| invalid("执行身份耗尽"))?;
        let budget = current
            .as_ref()
            .cloned()
            .unwrap_or_else(|| Budget::new(limits));
        let mut buffers = self.vm.buffers.borrow_mut();
        let mut roots = std::mem::take(&mut buffers.published_roots);
        roots.shrink_to(4);
        heap.buffer_bytes = buffers.storage_bytes();
        let table_growth = Heap::table_growth::<(u64, Vec<Slot>)>(heap.roots.len(), heap.roots.capacity());
        let mut growth = table_growth.saturating_add(roots.capacity() * size_of::<Slot>());
        if growth > budget.remaining_heap_bytes().saturating_sub(heap.total_bytes()) {
            *buffers = executor::ExecutionBuffers::default();
            roots = Vec::new();
            heap.buffer_bytes = 0;
            growth = table_growth;
        }
        if growth > budget.remaining_heap_bytes().saturating_sub(heap.total_bytes()) {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        drop(buffers);
        heap.roots
            .try_reserve(1)
            .map_err(|_| invalid("执行根索引分配失败"))?;
        heap.roots.insert(roots_id, roots);
        if top {
            *current = Some(budget.clone());
        }
        Ok(ExecutionContext {
            runtime: self,
            budget,
            roots_id,
            top,
        })
    }
    pub(crate) fn execute_check_program(
        &self,
        program: Arc<ValidatedProgram>,
        owner: Option<ValueId>,
        budget: Budget,
    ) -> Result<(), ExecutionError> {
        let _entry = self.enter()?;
        let host = self.execution_host(ExecutionLimits::default())?;
        let binding = FunctionBinding {
            program,
            owner: owner.map_or(Slot::None, Slot::handle),
            captures: Box::default(),
        };
        let result = executor::execute(&host, &binding, &[], budget);
        drop(host);
        self.collect()?;
        result.map(|_| ())
    }
    pub(super) fn execution_equals(
        &self,
        left: ValueId,
        right: ValueId,
    ) -> Result<bool, ExecutionError> {
        self.ensure_value(left)?;
        self.ensure_value(right)?;
        let host = self.execution_host(ExecutionLimits::default())?;
        host.roots(&[Slot::handle(left), Slot::handle(right)])?;
        host.equal(host.slot(left)?, host.slot(right)?)
    }
    pub(super) fn evaluate_text(&self, id: ValueId) -> Result<String, ExecutionError> {
        let host = self.execution_host(ExecutionLimits::default())?;
        let value = if let Some(binding) = host.template(Slot::handle(id))? {
            if let Some(Constant::String(text)) = binding.program.static_text() {
                return Ok(text.clone());
            }
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
// 所有格式片段直接写入同一输出；预算检查先于扩容。
struct FormatOutput<'a> {
    text: String,
    budget: &'a Budget,
    heap: &'a RefCell<Heap>,
    memory: executor::TemporaryBytes,
    error: Option<ExecutionError>,
}
impl std::fmt::Write for FormatOutput<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if let Err(error) = self.budget.charge(text.len() as u64) {
            self.error = Some(error);
            return Err(std::fmt::Error);
        }
        let Some(required) = self
            .text
            .len()
            .checked_add(text.len())
            .and_then(|n| n.checked_add(dynamic_bytes(&Value::String(String::new()), None)))
        else {
            self.error = Some(invalid("文本容量溢出"));
            return Err(std::fmt::Error);
        };
        if let Err(error) = self
            .memory
            .resize(required, self.heap.borrow().total_bytes())
        {
            self.error = Some(error);
            return Err(std::fmt::Error);
        }
        if self.text.try_reserve_exact(text.len()).is_err() {
            self.error = Some(invalid("文本分配失败"));
            return Err(std::fmt::Error);
        }
        self.text.push_str(text);
        Ok(())
    }
}
/// 显式通用值物化；借用访问使用 BorrowedValue，动态值只保留短期共享所有权。
pub(super) enum MaterializedValue {
    Materialized {
        value: Value,
        _memory: Option<executor::TemporaryBytes>,
    },
    Dynamic(Arc<Value>),
    Scalar(Value),
}
impl AsRef<Value> for MaterializedValue {
    fn as_ref(&self) -> &Value {
        match self {
            Self::Materialized { value, .. } => value,
            Self::Dynamic(value) => value.as_ref(),
            Self::Scalar(value) => value,
        }
    }
}
impl std::ops::Deref for MaterializedValue {
    type Target = Value;
    fn deref(&self) -> &Value {
        self.as_ref()
    }
}
impl MaterializedValue {
    pub(super) fn into_arc(self) -> Arc<Value> {
        match self {
            Self::Dynamic(value) => value,
            Self::Materialized { value, .. } => Arc::new(value),
            Self::Scalar(value) => Arc::new(value),
        }
    }
}
/// 固定数据直接借用，动态载荷以短期所有权跨回调保活；不持有堆借用。
enum BorrowedValue<'a> { Fixed(Stored<'a>), Dynamic(Arc<Value>), Scalar(Slot) }
impl BorrowedValue<'_> {
    fn view(&self) -> Stored<'_> {
        match self { Self::Fixed(view) => *view, Self::Dynamic(value) => Stored::dynamic(value), Self::Scalar(value) => Stored::Scalar(*value) }
    }
}
pub(super) struct ExecutionContext<'a> {
    runtime: &'a Runtime,
    pub(super) budget: Budget,
    roots_id: u64,
    top: bool,
}
impl Drop for ExecutionContext<'_> {
    fn drop(&mut self) {
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if let Some(mut roots) = heap.roots.remove(&self.roots_id) {
            roots.clear();
            let mut buffers = self.runtime.vm.buffers.borrow_mut();
            if roots.capacity() > buffers.published_roots.capacity() { buffers.published_roots = roots; }
            heap.buffer_bytes = buffers.storage_bytes();
        }
        drop(heap);
        if self.top {
            // 正则缓存只活在共享执行预算内；结束时连同 hash 容量一起释放。
            *self.runtime.vm.regexes.borrow_mut() = HashMap::new();
            *self.runtime.vm.budget.borrow_mut() = None;
            let collect = {
                let mut heap = self.runtime.vm.heap.borrow_mut();
                heap.collection_limit = self.budget.remaining_heap_bytes();
                // 保活图存在时按压力回收；撤销保活及无保活请求仍及时清理。
                heap.collection_dirty && (heap.collection_released || heap.pinned.is_empty()
                    || heap.live_values >= heap.next_collection
                    || heap.bytes > self.budget.remaining_heap_bytes() / 2)
            };
            if collect { let _ = self.runtime.collect(); }
        }
    }
}
impl ExecutionContext<'_> {
    fn read_value(&self, value: Slot) -> Result<BorrowedValue<'_>, ExecutionError> {
        let Slot::Handle(id) = value else { return Ok(BorrowedValue::Scalar(value)); };
        if let Some(view) = self.runtime.values.view(id.get()) {
            if !matches!(view, Stored::Host) { return Ok(BorrowedValue::Fixed(view)); }
        }
        Ok(BorrowedValue::Dynamic(self.runtime.value(id.get())?))
    }
    fn root(&self, value: Slot) -> Result<(), ExecutionError> {
        let Slot::Handle(id) = value else { return Ok(()); };
        if id.get() < self.runtime.values.len() || Slot::from_scalar_id(id.get()).is_some() { return Ok(()); }
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let required = heap.roots.get(&self.roots_id).ok_or_else(|| invalid("执行根集合不存在"))?.len().saturating_add(1);
        heap.reserve_roots(self.roots_id, required, self.budget.remaining_heap_bytes())?;
        heap.roots.get_mut(&self.roots_id).expect("根集合已验证").push(value);
        Ok(())
    }
    fn builder_append(&self, builder: Slot, value: Slot) -> Result<(), ExecutionError> {
        let Slot::Handle(id) = builder else { return Err(invalid("追加需要构造能力")); };
        let id = id.get();
        let value = self.id(value)?;
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if !heap.builders.contains(&id) { return Err(invalid("构造能力已经消费")); }
        let remaining = self.budget.remaining_heap_bytes().saturating_sub(heap.total_bytes());
        let entry = heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_deref());
        let Value::Array(values) = Arc::get_mut(&mut entry.value).ok_or_else(|| invalid("构造缓冲存在非法可写别名"))? else { return Err(invalid("追加需要数组")); };
        if values.len() == values.capacity() {
            // 先检查容量增量再分配；独占缓冲禁止隐式写时复制。
            let capacity = values.capacity().checked_mul(2).unwrap_or(usize::MAX).max(4);
            let additional = capacity.checked_sub(values.len()).ok_or_else(|| invalid("集合容量溢出"))?;
            let bytes = additional.checked_mul(values.element_bytes()).ok_or_else(|| invalid("集合容量溢出"))?;
            if bytes > remaining { return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory)); }
            values.reserve(additional).map_err(|error| invalid(&error))?;
        }
        let result = values.push(value).map_err(|error| invalid(&error));
        let after = dynamic_bytes(&entry.value, entry.callable.as_deref());
        heap.bytes = heap.bytes - before + after;
        result
    }
    fn builder_edit(&self, builder: Slot, key: Slot, replacement: Option<Slot>) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = builder else { return Err(invalid("集合修改需要构造能力")); };
        let id = id.get();
        let scalar = self.scalar_key(key)?;
        let replacement = replacement.map(|value| self.id(value)).transpose()?;
        let key_id = if replacement.is_some() { Some(self.id(key)?) } else { None };
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if !heap.builders.contains(&id) { return Err(invalid("构造能力已经消费")); }
        let limit = self.budget.remaining_heap_bytes();
        let remaining = limit.saturating_sub(heap.total_bytes());
        let entry = heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_deref());
        let value = Arc::get_mut(&mut entry.value).ok_or_else(|| invalid("构造缓冲存在非法可写别名"))?;
        match value {
            Value::Array(values) => {
                let index = index(key)?;
                if index >= values.len() { return Err(invalid("数组索引越界")); }
                if let Some(value) = replacement { values.set(index, value).map_err(|error| invalid(&error))?; } else { values.remove(index); }
            }
            Value::Dict(values) => {
                if let Some(value) = replacement {
                    let key_bytes = if values.contains_key(&scalar) { 0 } else { match &scalar {
                        ScalarKey::String(text) => text.capacity(),
                        ScalarKey::Enum { type_name, .. } => type_name.capacity(), _ => 0,
                    }};
                    if key_bytes > remaining { return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory)); }
                    if !values.contains_key(&scalar) && values.len() == values.capacity() {
                        // 在扩容前保守预留哈希索引、键和条目空间，避免先分配后报告超限。
                        let required = values.capacity().max(4).checked_mul(4 * (size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>())).ok_or_else(|| invalid("集合容量溢出"))?;
                        if required > remaining - key_bytes { return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory)); }
                        values.try_reserve(1).map_err(|_| invalid("字典构造分配失败"))?;
                    }
                    values.insert(scalar, (key_id.unwrap(), value));
                } else { values.shift_remove(&scalar); }
            }
            _ => return Err(invalid("构造修改需要集合")),
        }
        let after = dynamic_bytes(&entry.value, entry.callable.as_deref());
        heap.bytes = heap.bytes - before + after;
        Ok(Slot::Unit)
    }
    fn stored_value<'a>(
        &'a self,
        heap: Option<&'a Heap>,
        id: ValueId,
    ) -> Result<Stored<'a>, ExecutionError> {
        if let Some(value) = Slot::from_scalar_id(id) {
            return Ok(Stored::Scalar(value));
        }
        if id < self.runtime.values.len() {
            return self
                .runtime
                .values
                .view(id)
                .ok_or(ExecutionError::InvalidHandle);
        }
        heap.and_then(|heap| heap.get(id))
            .map(|entry| Stored::dynamic(entry.value.as_ref()))
            .ok_or(ExecutionError::InvalidHandle)
    }
    fn slot_in_heap(
        &self,
        heap: Option<&Heap>,
        id: ValueId,
    ) -> Result<Option<Slot>, ExecutionError> {
        Ok(match self.stored_value(heap, id)? {
            Stored::Scalar(value) => Some(value),
            Stored::Host => None,
            _ => Some(Slot::handle(id)),
        })
    }
    fn read_array_slot(&self, heap: Option<&Heap>, value: Slot) -> Result<Option<Slot>, ExecutionError> {
        match value { Slot::Handle(id) => self.slot_in_heap(heap, id.get()), scalar => Ok(Some(scalar)) }
    }
    fn slot(&self, id: ValueId) -> Result<Slot, ExecutionError> {
        if let Some(view) = self.runtime.values.view(id) {
            match view {
                Stored::Scalar(value) => return Ok(value),
                Stored::Host => {}
                _ => return Ok(Slot::handle(id)),
            }
        }
        Ok(match self.value(Slot::handle(id))?.as_ref() {
            Value::None => Slot::None,
            Value::Bool(v) => Slot::Bool(*v),
            Value::Int(v) => Slot::Int(*v),
            Value::Float(v) => Slot::Float(*v),
            _ => Slot::handle(id),
        })
    }
    /// 归一化字典键；仅 string/enum 需要读堆内容。
    fn scalar_key(&self, slot: Slot) -> Result<ScalarKey, ExecutionError> {
        match slot {
            Slot::Bool(value) => Ok(ScalarKey::Bool(value)),
            Slot::Int(value) => Ok(ScalarKey::Int(value)),
            Slot::Handle(id) => self.scalar_key_for_id(id.get()),
            _ => Err(invalid("无效的字典 key 类型")),
        }
    }
    fn temporary_key(
        &self,
        slot: Slot,
    ) -> Result<(ScalarKey, executor::TemporaryBytes), ExecutionError> {
        let key = self.scalar_key(slot)?;
        let bytes = match &key {
            ScalarKey::String(text) => text.capacity(),
            ScalarKey::Enum { type_name, .. } => type_name.capacity(),
            _ => 0,
        };
        // copy_text 在复制前检查剩余额度，此处将复制结果转为集合整个生命周期的累计预留。
        self.reclaim_idle_buffers(bytes);
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        Ok((key, memory))
    }
    fn temporary_key_set(
        &self,
        count: usize,
    ) -> Result<(HashSet<ScalarKey>, executor::TemporaryBytes), ExecutionError> {
        // 哈希表容量按装载因子和二次幂取整，保守覆盖桶及控制字节。
        let bytes = count
            .checked_add(1)
            .and_then(|n| n.checked_mul(4 * (size_of::<ScalarKey>() + 1)))
            .ok_or_else(|| invalid("集合容量溢出"))?;
        self.reclaim_idle_buffers(bytes);
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut keys = HashSet::new();
        keys.try_reserve(count)
            .map_err(|_| invalid("集合索引分配失败"))?;
        Ok((keys, memory))
    }
    fn scalar_key_for_id(&self, id: ValueId) -> Result<ScalarKey, ExecutionError> {
        if let Some(Stored::String(value)) = self.runtime.values.view(id) {
            return Ok(ScalarKey::String(self.copy_text(value)?));
        }
        match self.value(Slot::handle(id))?.as_ref() {
            Value::Bool(value) => Ok(ScalarKey::Bool(*value)),
            Value::Int(value) => Ok(ScalarKey::Int(*value)),
            Value::String(value) => Ok(ScalarKey::String(self.copy_text(value)?)),
            Value::Enum { type_name, value } => Ok(ScalarKey::Enum {
                type_name: self.copy_text(type_name)?,
                value: *value,
            }),
            _ => Err(invalid("无效的字典 key 类型")),
        }
    }
    pub(super) fn value(&self, slot: Slot) -> Result<MaterializedValue, ExecutionError> {
        Ok(match slot {
            Slot::None | Slot::Unit => MaterializedValue::Scalar(Value::None),
            Slot::Bool(v) => MaterializedValue::Scalar(Value::Bool(v)),
            Slot::Int(v) => MaterializedValue::Scalar(Value::Int(v)),
            Slot::Float(v) => MaterializedValue::Scalar(Value::Float(v)),
            Slot::Handle(id) => {
                let id = id.get();
                if self.runtime.values.is_host(id) {
                    MaterializedValue::Dynamic(self.runtime.value(id)?)
                } else if let Some(bytes) = self.runtime.values.materialized_bytes(id) {
                    let memory = self
                        .budget
                        .reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
                    let value = self
                        .runtime
                        .values
                        .materialize(id)
                        .ok_or(ExecutionError::InvalidHandle)?;
                    MaterializedValue::Materialized {
                        value,
                        _memory: Some(memory),
                    }
                } else {
                    MaterializedValue::Dynamic(self.runtime.vm.value(id)?)
                }
            }
            Slot::Empty => return Err(invalid("空寄存器不是语言值")),
        })
    }
    /// 外部输入的长度不能先变成分配；临时缓冲同样先检查剩余内存预算。
    fn reserve_values<T>(
        &self,
        count: usize,
    ) -> Result<(Vec<T>, executor::TemporaryBytes), ExecutionError> {
        let bytes = count
            .checked_mul(size_of::<T>())
            .ok_or_else(|| ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory))?;
        self.reclaim_idle_buffers(bytes);
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(count)
            .map_err(|_| invalid("动态缓冲分配失败"))?;
        Ok((values, memory))
    }
    fn reserve_temporary_vec<T>(
        &self,
        values: &mut Vec<T>,
        memory: &mut executor::TemporaryBytes,
        additional: usize,
    ) -> Result<(), ExecutionError> {
        let required = values
            .len()
            .checked_add(additional)
            .ok_or_else(|| invalid("临时缓冲容量溢出"))?;
        if required <= values.capacity() {
            return Ok(());
        }
        let capacity = values.capacity().saturating_mul(2).max(required).max(4);
        memory.resize(
            capacity
                .checked_mul(size_of::<T>())
                .ok_or_else(|| invalid("临时缓冲容量溢出"))?,
            self.runtime.vm.heap.borrow().total_bytes(),
        )?;
        values
            .try_reserve_exact(capacity - values.len())
            .map_err(|_| invalid("临时缓冲分配失败"))
    }
    fn reclaim_idle_buffers(&self, required: usize) {
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if required > self.budget.remaining_heap_bytes().saturating_sub(heap.total_bytes()) && heap.buffer_bytes != 0 {
            *self.runtime.vm.buffers.borrow_mut() = executor::ExecutionBuffers::default();
            heap.buffer_bytes = 0;
        }
    }
    fn preflight_bytes(&self, bytes: usize) -> Result<(), ExecutionError> {
        if bytes
            > self
                .budget
                .remaining_heap_bytes()
                .saturating_sub(self.runtime.vm.heap.borrow().total_bytes())
        {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        Ok(())
    }
    fn copy_text(&self, text: &str) -> Result<String, ExecutionError> {
        self.preflight_bytes(
            text.len()
                .saturating_add(dynamic_bytes(&Value::String(String::new()), None)),
        )?;
        let mut result = String::new();
        result
            .try_reserve_exact(text.len())
            .map_err(|_| invalid("文本分配失败"))?;
        result.push_str(text);
        Ok(result)
    }
    fn allocate(&self, value: Value) -> Result<Slot, ExecutionError> {
        self.allocate_bound(value, None)
    }
    fn allocate_bound(
        &self,
        value: Value,
        binding: Option<FunctionBinding>,
    ) -> Result<Slot, ExecutionError> {
        self.budget.charge(1)?;
        // 分配与根登记共用短期堆借用，回收前释放借用。
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if heap.live_values >= heap.next_collection {
            drop(heap);
            self.runtime.collect()?;
            heap = self.runtime.vm.heap.borrow_mut();
        }
        let root_count = heap.roots.get(&self.roots_id).map_or(0, Vec::len);
        heap.reserve_roots(
            self.roots_id,
            root_count.saturating_add(1),
            self.budget.remaining_heap_bytes(),
        )?;
        let slot =
            VmState::allocate_in_heap(&mut heap, value, binding, self.budget.remaining_heap_bytes())?;
        heap.roots
            .get_mut(&self.roots_id)
            .expect("执行根已预留")
            .push(slot);
        Ok(slot)
    }
    pub(super) fn id(&self, value: Slot) -> Result<ValueId, ExecutionError> {
        if let Some(id) = value.scalar_id() {
            return Ok(id);
        }
        if let Slot::Handle(id) = value {
            return Ok(id.get());
        }
        Err(invalid("空寄存器没有值身份"))
    }
    pub(super) fn import(&self, value: &HostValue) -> Result<Slot, ExecutionError> {
        self.import_depth(value, 0)
    }
    fn import_depth(&self, value: &HostValue, depth: usize) -> Result<Slot, ExecutionError> {
        if depth >= 128 {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::ValueDepth));
        }
        self.budget.charge(1)?;
        Ok(match value {
            HostValue::Array(values) => {
                let (mut imported, _memory) = self.reserve_values(values.len())?;
                for value in values {
                    imported.push(self.import_depth(value, depth + 1)?);
                }
                self.array(imported)?
            }
            HostValue::Dictionary(values) => {
                let (mut imported, _memory) = self.reserve_values(values.len())?;
                for (key, value) in values {
                    imported.push((
                        self.import_depth(key, depth + 1)?,
                        self.import_depth(value, depth + 1)?,
                    ));
                }
                self.dictionary(imported)?
            }
            HostValue::Data { type_name, fields } => {
                let meta = self
                    .runtime
                    .contract
                    .schema()
                    .resolve_type(type_name)
                    .ok_or_else(|| invalid("导入 data 类型不存在"))?;
                if meta.kind != coflow_language::cft::syntax::ast::TypeKind::Data
                    || meta.is_abstract
                {
                    return Err(invalid("只能导入具体 data"));
                }
                if fields.len() != meta.all_fields().count() {
                    return Err(invalid("导入 data 字段数量不匹配"));
                }
                let (mut names, _names_memory) = self.reserve_values(fields.len())?;
                names.extend(fields.iter().map(|(name, _)| name.as_str()));
                names.sort_unstable();
                if names.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(invalid("导入 data 字段重复"));
                }
                let (mut imported, _memory) = self.reserve_values(fields.len())?;
                for (name, value) in fields {
                    let field = meta
                        .field(name)
                        .ok_or_else(|| invalid("导入 data 字段不存在"))?;
                    let value = self.import_depth(value, depth + 1)?;
                    if !self.matches(value, &field.value_type)? {
                        return Err(invalid("导入 data 字段类型不匹配"));
                    }
                    imported.push((name.as_str(), value));
                }
                self.object(type_name, imported)?
            }
            HostValue::None => Slot::None,
            HostValue::Bool(v) => Slot::Bool(*v),
            HostValue::Int(v) => Slot::Int(*v),
            HostValue::Float(v) => Slot::Float(*v),
            HostValue::String(v) => self.allocate(Value::String(self.copy_text(v)?))?,
            HostValue::Enum { type_name, value } => self.allocate(Value::Enum {
                type_name: self.copy_text(type_name)?,
                value: *value,
            })?,
            HostValue::Existing { runtime, value } => {
                if *runtime != self.runtime.identity {
                    return Err(ExecutionError::ForeignRuntime);
                }
                self.runtime.ensure_value(*value)?;
                if self.runtime.values.is_host(*value) {
                    return Err(invalid("Host must return a concrete value"));
                }
                // 导入后续参数可能触发 GC；已有动态值也必须先登记当前请求根。
                self.root(Slot::handle(*value))?;
                self.slot(*value)?
            }
        })
    }
    pub(super) fn matches(&self, value: Slot, ty: &CftValueType) -> Result<bool, ExecutionError> {
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
        if let Slot::Handle(id) = value {
            if let Some((actual, record)) = self.runtime.values.object_identity(id.get()) {
                // 固定对象的类型判定只读布局和记录标记，不复制字段及名称。
                return Ok(match ty {
                    CftValueType::Object(expected) => {
                        !record
                            && self
                                .runtime
                                .contract
                                .schema()
                                .is_assignable(actual, expected)
                    }
                    CftValueType::RecordRef(expected) => {
                        record
                            && self
                                .runtime
                                .contract
                                .schema()
                                .is_assignable(actual, expected)
                    }
                    _ => false,
                });
            }
            if matches!(ty, CftValueType::String | CftValueType::FString)
                && matches!(self.runtime.values.view(id.get()), Some(Stored::String(_)))
            {
                return Ok(true);
            }
        }
        // 集合中的闭包按已验证程序签名校验，不能重新解析共享的外层源码。
        if let CftValueType::Array(inner) = ty {
            let check = |values: &ArrayValue| -> Result<bool, ExecutionError> {
                self.budget.charge(values.len() as u64)?;
                for value in values {
                    if !self.matches(self.slot(value)?, inner)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            };
            // 固定集合校验直接借用连续负载，递归 Host 读取不持有动态堆借用。
            if let Slot::Handle(id) = value {
                if let Some(Stored::Array(values)) = self.runtime.values.view(id.get()) {
                    return check(values);
                }
            }
            let stored = self.value(value)?;
            let Value::Array(values) = stored.as_ref() else {
                return Ok(false);
            };
            return check(values);
        }
        if let CftValueType::Dict(key, inner) = ty {
            let check = |values: &DictionaryValue| -> Result<bool, ExecutionError> {
                self.budget.charge(values.len() as u64)?;
                for (k, v) in values.values() {
                    if !self.matches(self.slot(*k)?, key)? || !self.matches(self.slot(*v)?, inner)? { return Ok(false); }
                }
                Ok(true)
            };
            if let Slot::Handle(id) = value {
                if let Some(Stored::Dict(values)) = self.runtime.values.view(id.get()) {
                    return check(values);
                }
            }
            let stored = self.value(value)?;
            let Value::Dict(values) = stored.as_ref() else {
                return Ok(false);
            };
            return check(values);
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
    pub(super) fn export(&self, slot: Slot) -> Result<HostValue, ExecutionError> {
        self.export_value(slot, true)
    }
    fn export_value(&self, slot: Slot, retain: bool) -> Result<HostValue, ExecutionError> {
        if let Slot::Handle(id) = slot {
            let id = id.get();
            if let Some(Stored::String(text)) = self.runtime.values.view(id) {
                return Ok(HostValue::String(self.copy_text(text)?));
            }
            if let Some((name, value)) = self.runtime.values.enum_value(id) {
                return Ok(HostValue::Enum { type_name: self.copy_text(name)?, value });
            }
            if id < self.runtime.values.len() && !self.runtime.values.is_host(id) {
                return Ok(HostValue::Existing { runtime: self.runtime.identity, value: id });
            }
        }
        Ok(match slot {
            Slot::Unit | Slot::None => HostValue::None,
            Slot::Bool(v) => HostValue::Bool(v),
            Slot::Int(v) => HostValue::Int(v),
            Slot::Float(v) => HostValue::Float(v),
            Slot::Handle(id) => match self.value(Slot::Handle(id))?.as_ref() {
                Value::String(v) => HostValue::String(self.copy_text(v)?),
                Value::Enum { type_name, value } => HostValue::Enum {
                    type_name: self.copy_text(type_name)?,
                    value: *value,
                },
                _ => {
                    let id = id.get();
                    if retain && id >= self.runtime.values.len() {
                        self.runtime.retain_value(id)?;
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
        // 固定文本直接借用 UTF-8；数值格式化的最大输出先计入预算。
        if let Slot::Handle(id) = value {
            if let Some(Stored::String(text)) = self.runtime.values.view(id.get()) {
                return self.copy_text(text);
            }
        }
        self.preflight_bytes(64)?;
        match self.value(value)?.as_ref() {
            Value::String(v) => self.copy_text(v),
            Value::Int(v) => Ok(v.to_string()),
            Value::Float(v) => Ok(v.to_string()),
            Value::Bool(v) => Ok(v.to_string()),
            Value::Enum { type_name, value } => Ok(self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .and_then(|meta| {
                    // 使用 schema 预建的按值索引，避免每次格式化线性扫描变体。
                    meta.variant_by_value
                        .get(&i64::from(*value))
                        .and_then(|index| meta.variants.get(*index))
                })
                .map_or_else(
                    || Ok(value.to_string()),
                    |variant| self.copy_text(&variant.name),
                )?),
            _ => Err(invalid("值不能转换为文本")),
        }
    }

}
pub(super) fn invalid(message: &str) -> ExecutionError {
    ExecutionError::InvalidAccess(message.into())
}
fn index(value: Slot) -> Result<usize, ExecutionError> {
    if let Slot::Int(value) = value {
        usize::try_from(value).map_err(|_| invalid("负索引"))
    } else {
        Err(invalid("索引需要 int"))
    }
}
impl ExecutionContext<'_> {
    /// 局部构造与 Host 对象导入共用字段缺省规则，默认函数始终绑定当前对象。
    fn field_default(&self, field: &crate::schema::CftField, owner: Slot) -> Result<Slot, ExecutionError> {
        if let Some(default) = &field.default {
            let module = &self.runtime.contract.schema()
                .resolve_type(&field.declaring_type)
                .ok_or_else(|| invalid("默认字段声明不存在"))?.module;
            return self.default_value(default, owner, module);
        }
        match field.value_type {
            CftValueType::Option(_) => Ok(Slot::None),
            CftValueType::Array(_) => self.array(Vec::new()),
            CftValueType::Dict(..) => self.dictionary(Vec::new()),
            _ => Err(invalid("必填构造字段没有默认值")),
        }
    }

    fn default_value(
        &self,
        value: &crate::schema::CftStaticValue,
        owner: Slot,
        module: &crate::schema::ModuleId,
    ) -> Result<Slot, ExecutionError> {
        use crate::schema::CftStaticValue as D;
        match value {
            D::OptionNone => Ok(Slot::None),
            D::OptionSome(value) => self.default_value(value, owner, module),
            D::Int(value) => Ok(Slot::Int(
                i32::try_from(*value).map_err(|_| invalid("默认 int 越界"))?,
            )),
            D::Float(value) => Ok(Slot::Float(*value as f32)),
            D::Bool(value) => Ok(Slot::Bool(*value)),
            D::String(value) => self.allocate(Value::String(self.copy_text(value)?)),
            D::Enum {
                enum_name, value, ..
            } => self.allocate(Value::Enum {
                type_name: self.copy_text(enum_name)?,
                value: *value as u32,
            }),
            D::Array(values) => {
                let (mut items, _memory) = self.reserve_values(values.len())?;
                for value in values {
                    items.push(self.default_value(value, owner, module)?);
                }
                self.array(items)
            }
            D::Dictionary(values) => {
                let (mut entries, _memory) = self.reserve_values(values.len())?;
                for (key, value) in values {
                    entries.push((
                        self.default_value(key, owner, module)?,
                        self.default_value(value, owner, module)?,
                    ));
                }
                self.dictionary(entries)
            }
            D::Object { type_name, fields } => {
                let target = self.reserve_object(type_name)?;
                let (mut initialized, _memory) = self.reserve_values(fields.len())?;
                for (name, value) in fields {
                    initialized.push((name.as_str(), self.default_value(value, target, module)?));
                }
                self.initialize_object(target, type_name, initialized)
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
                        offset: source.span.start,
                    };
                    if let Some(program) = self.runtime.code().programs.functions.get(&key) {
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
                    FunctionBinding {
                        program,
                        owner,
                        captures: Box::default(),
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
        self.root(result)?;
        Ok(result)
    }

}

#[cfg(test)]
mod tests;
