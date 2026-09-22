//! 动态值的代数句柄、独立保活、标记清扫及真实载荷计费。
use super::{inline_value, Runtime, Value, ValueId, state::VmState, execution::invalid};
use crate::vm::{ExecutionError, executor::{FunctionBinding, Budget, ExecutionLimits, Slot}};
use std::{collections::{HashMap, HashSet}, sync::Arc};
#[derive(Debug)]
pub(super) struct DynamicValue {
    pub(super) identity: ValueId,
    pub(super) value: Arc<Value>,
    pub(super) callable: Option<Arc<FunctionBinding>>,
}
#[derive(Debug, Default)]
pub(super) struct Heap {
    #[cfg(test)]
    pub(super) metrics: HeapMetrics,
    pub(super) bytes: usize,
    pub(super) collection_dirty: bool,
    /// 延迟清扫所依据的请求预算；收紧预算前先结清上次请求的垃圾。
    pub(super) collection_limit: usize,
    pub(super) collection_released: bool,
    pub(super) buffer_bytes: usize,
    /// 语言身份不复用，存储槽独立回收；早期长寿命值不会阻止临时槽复用。
    pub(super) values: Vec<Option<DynamicValue>>,
    /// 代数跨回收保留；达到上限的槽永久退役，旧身份永不复用。
    pub(super) generations: Vec<u32>,
    pub(super) free: Vec<usize>,
    pub(super) live_values: usize,
    pub(super) pinned: HashMap<ValueId, usize>,
    pub(super) roots: HashMap<u64, Vec<Slot>>,
    pub(super) next_roots: u64,
    pub(super) next_collection: usize,
    pub(super) builders: HashSet<ValueId>,
}
#[cfg(test)]
#[derive(Debug, Default)]
pub(super) struct HeapMetrics {
    pub(super) dispatches: u64,
    pub(super) calls: u64,
    pub(super) closures: u64,
    pub(super) allocations: u64,
    pub(super) allocated_payload_bytes: u64,
    pub(super) peak_heap_bytes: usize,
    pub(super) collections: u64,
    pub(super) marked_values: u64,
    pub(super) visited_edges: u64,
    pub(super) gc_nanoseconds: u128,
    pub(super) max_gc_nanoseconds: u128,
}
impl Heap {
    pub(super) const DYNAMIC_TAG: u64 = 1 << 43;
    pub(super) const INDEX_BITS: u32 = 20;
    pub(super) const INDEX_MASK: u64 = (1 << Self::INDEX_BITS) - 1;
    pub(super) const MAX_GENERATION: u32 = (1 << (43 - Self::INDEX_BITS)) - 1;
    pub(super) fn table_bytes<T>(capacity: usize) -> usize {
        if capacity == 0 {
            0
        } else {
            capacity
                .saturating_mul(2 * (size_of::<T>() + 1))
                .saturating_add(16)
        }
    }
    /// 空闲槽、索引及根缓冲仍由实例持有，不能在回收值载荷后从预算中消失。
    pub(super) fn total_bytes(&self) -> usize {
        self.bytes.saturating_add(self.buffer_bytes)
            .saturating_add(
                self.values
                    .capacity()
                    .saturating_mul(size_of::<Option<DynamicValue>>()),
            )
            .saturating_add(self.generations.capacity().saturating_mul(size_of::<u32>()))
            .saturating_add(Self::table_bytes::<ValueId>(self.builders.capacity()))
            .saturating_add(self.free.capacity().saturating_mul(size_of::<usize>()))
            .saturating_add(Self::table_bytes::<(ValueId, usize)>(
                self.pinned.capacity(),
            ))
            .saturating_add(Self::table_bytes::<(u64, Vec<Slot>)>(self.roots.capacity()))
            .saturating_add(
                self.roots
                    .values()
                    .map(|roots| roots.capacity().saturating_mul(size_of::<Slot>()))
                    .sum::<usize>(),
            )
    }
    pub(super) fn table_growth<T>(len: usize, capacity: usize) -> usize {
        if len < capacity {
            0
        } else {
            Self::table_bytes::<T>(len.saturating_add(1).saturating_mul(2).max(4))
                .saturating_sub(Self::table_bytes::<T>(capacity))
        }
    }
    pub(super) fn reserve_roots(
        &mut self,
        id: u64,
        required: usize,
        limit: usize,
    ) -> Result<(), ExecutionError> {
        let roots = self
            .roots
            .get(&id)
            .ok_or_else(|| invalid("执行根集合不存在"))?;
        if required <= roots.capacity() {
            return Ok(());
        }
        let capacity = required.max(roots.capacity().saturating_mul(2)).max(4);
        let bytes = capacity
            .checked_sub(roots.capacity())
            .and_then(|n| n.checked_mul(size_of::<Slot>()))
            .ok_or_else(|| invalid("执行根容量溢出"))?;
        if bytes > limit.saturating_sub(self.total_bytes()) {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        let roots = self.roots.get_mut(&id).expect("根集合已验证");
        roots
            .try_reserve_exact(capacity - roots.len())
            .map_err(|_| invalid("执行根分配失败"))
    }
    pub(super) fn index(&self, id: ValueId) -> Option<usize> {
        if id & !Slot::MAX_HEAP_HANDLE != 0 || id & Self::DYNAMIC_TAG == 0 { return None; }
        let index = (id & Self::INDEX_MASK) as usize;
        self.values.get(index)?.as_ref().filter(|entry| entry.identity == id).map(|_| index)
    }

    /// 身份校验由调用方完成；回收和构造器清理共用代数退休规则。
    pub(super) fn remove_slot(&mut self, index: usize) -> Option<DynamicValue> {
        let entry = self.values[index].take()?;
        if self.generations[index] < Self::MAX_GENERATION { self.free.push(index); }
        Some(entry)
    }

    pub(super) fn get(&self, id: ValueId) -> Option<&DynamicValue> {
        self.values.get(self.index(id)?)?.as_ref()
    }

    pub(super) fn get_mut(&mut self, id: ValueId) -> Option<&mut DynamicValue> {
        let index = self.index(id)?;
        self.values.get_mut(index)?.as_mut()
    }
}
impl VmState {
    pub(super) fn new(fixed_count: ValueId) -> Self {
        let state = Self::default();
        {
            let mut heap = state.heap.borrow_mut();
            assert!(fixed_count < Heap::DYNAMIC_TAG, "固定身份不能进入动态句柄空间");
            heap.next_collection = 1024;
        }
        state
    }
    pub(super) fn value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        if let Some(value) = inline_value(id) {
            return Ok(Arc::new(value));
        }
        self.heap
            .borrow()
            .get(id)
            .map(|entry| entry.value.clone())
            .ok_or(ExecutionError::InvalidHandle)
    }
    /// 在已有可变堆借用内分配；调用方负责先完成 GC 阈值检查。
    pub(super) fn allocate_in_heap(
        heap: &mut Heap,
        value: Value,
        callable: Option<FunctionBinding>,
        limit: usize,
    ) -> Result<Slot, ExecutionError> {
        if heap.live_values >= 1_000_000 {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Values));
        }
        let bytes = dynamic_bytes(&value, callable.as_ref());
        let mut metadata = 0usize;
        let slot_capacity = heap
            .values
            .len()
            .saturating_add(1)
            .max(heap.values.capacity().saturating_mul(2))
            .max(4);
        if heap.free.is_empty() && heap.values.len() == heap.values.capacity() {
            metadata = metadata.saturating_add(
                slot_capacity
                    .saturating_sub(heap.values.capacity())
                    .saturating_mul(size_of::<Option<DynamicValue>>()),
            );
            metadata = metadata.saturating_add(
                slot_capacity
                    .saturating_sub(heap.free.capacity())
                    .saturating_mul(size_of::<usize>()),
            );
        }
        if heap.free.is_empty() && heap.generations.len() == heap.generations.capacity() {
            metadata = metadata.saturating_add(slot_capacity.saturating_sub(heap.generations.capacity()).saturating_mul(size_of::<u32>()));
        }
        if bytes.saturating_add(metadata) > limit.saturating_sub(heap.total_bytes()) {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        let index = heap.free.last().copied().unwrap_or(heap.values.len());
        if index as u64 > Heap::INDEX_MASK { return Err(invalid("动态值身份耗尽")); }
        let generation = heap.free.last().map_or(0, |index| heap.generations[*index] + 1);
        let id = Heap::DYNAMIC_TAG | (u64::from(generation) << Heap::INDEX_BITS) | index as u64;
        // 完成所有可失败的预留后才发布新代数，失败不会消耗身份或暴露半个条目。
        if heap.free.is_empty() && heap.generations.len() == heap.generations.capacity() {
            heap.generations.try_reserve_exact(slot_capacity - heap.generations.len())
                .map_err(|_| invalid("动态代数分配失败"))?;
        }
        if heap.free.is_empty() && heap.values.len() == heap.values.capacity() {
            heap.values
                .try_reserve_exact(slot_capacity - heap.values.len())
                .map_err(|_| invalid("动态槽位分配失败"))?;
            heap.free
                .try_reserve_exact(slot_capacity - heap.free.len())
                .map_err(|_| invalid("回收槽索引分配失败"))?;
        }
        let entry = Some(DynamicValue {
            identity: id,
            value: Arc::new(value),
            callable: callable.map(Arc::new),
        });
        if let Some(index) = heap.free.pop() {
            heap.values[index] = entry;
            heap.generations[index] = generation;
        } else {
            heap.values.push(entry);
            heap.generations.push(generation);
        }
        heap.bytes += bytes;
        heap.collection_dirty = true;
        heap.live_values += 1;
        #[cfg(test)]
        {
            heap.metrics.allocations += 1;
            heap.metrics.allocated_payload_bytes += bytes as u64;
            heap.metrics.peak_heap_bytes = heap.metrics.peak_heap_bytes.max(heap.total_bytes());
        }
        Ok(Slot::handle(id))
    }
}
impl Runtime {
    /// 宿主把借用的子值保存到父值生命周期之外时，显式增加独立保活。
    pub fn retain_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        let _entry = self.enter()?;
        self.ensure_value(id)?;
        if id < self.values.len() || Slot::from_scalar_id(id).is_some() {
            return Ok(());
        }
        let mut heap = self.vm.heap.borrow_mut();
        let limit = self
            .vm
            .budget
            .borrow()
            .as_ref()
            .map_or(usize::MAX, Budget::remaining_heap_bytes);
        if !heap.pinned.contains_key(&id) {
            let growth =
                Heap::table_growth::<(ValueId, usize)>(heap.pinned.len(), heap.pinned.capacity());
            if growth > limit.saturating_sub(heap.total_bytes()) {
                return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
            }
            heap.pinned
                .try_reserve(1)
                .map_err(|_| invalid("保活索引分配失败"))?;
        }
        let count = heap.pinned.entry(id).or_default();
        *count = count
            .checked_add(1)
            .ok_or_else(|| invalid("宿主保活计数溢出"))?;
        Ok(())
    }
    /// 宿主显式释放动态返回值的保活。固定配置值随 Runtime 整体管理。
    pub fn release_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        self.ensure_alive()?;
        if id < self.values.len() || Slot::from_scalar_id(id).is_some() {
            return Ok(());
        }
        let mut heap = self.vm.heap.borrow_mut();
        let count = heap
            .pinned
            .get_mut(&id)
            .ok_or(ExecutionError::InvalidHandle)?;
        *count -= 1;
        if *count == 0 {
            heap.pinned.remove(&id);
            heap.collection_dirty = true;
            heap.collection_released = true;
        }
        Ok(())
    }
    pub fn collect(&self) -> Result<usize, ExecutionError> {
        let _entry = self.enter()?;
        let mut heap = self.vm.heap.borrow_mut();
        #[cfg(test)] { heap.metrics.peak_heap_bytes = heap.metrics.peak_heap_bytes.max(heap.total_bytes()); }
        #[cfg(test)] let gc_started = std::time::Instant::now();
        #[cfg(test)] let visited_edges = std::cell::Cell::new(0u64);
        // 标记在入栈时发生，因此每个槽最多入栈一次，工作栈严格有界于槽数。
        let scratch_bytes = heap.values.len().checked_mul(size_of::<usize>() + 1).ok_or_else(|| invalid("GC 工作区容量溢出"))?;
        let budget = self.vm.budget.borrow().as_ref().cloned().unwrap_or_else(|| Budget::new(ExecutionLimits { max_heap_bytes: usize::MAX, ..ExecutionLimits::default() }));
        let _scratch = budget.reserve_temporary(scratch_bytes, heap.total_bytes())?;
        let mut pending = Vec::<usize>::new();
        pending.try_reserve_exact(heap.values.len()).map_err(|_| invalid("GC 工作栈分配失败"))?;
        let mut live = Vec::<u8>::new();
        live.try_reserve_exact(heap.values.len()).map_err(|_| invalid("GC 标记区分配失败"))?;
        live.resize(heap.values.len(), 0);
        let mark = |id, live: &mut [u8], pending: &mut Vec<usize>| {
            #[cfg(test)] visited_edges.set(visited_edges.get() + 1);
            if let Some(index) = heap.index(id) {
                if live[index] == 0 { live[index] = 1; pending.push(index); }
            }
        };
        for id in heap.pinned.keys().chain(heap.builders.iter()) { mark(*id, &mut live, &mut pending); }
        for root in heap.roots.values().flatten() {
            if let Slot::Handle(id) = root { mark(id.get(), &mut live, &mut pending); }
        }
        // 借出的 Arc 也是根；仅标记其所属槽，不额外复制整张可达图。
        let mut borrowed_roots = false;
        for entry in heap.values.iter().flatten().filter(|entry| Arc::strong_count(&entry.value) > 1) {
            borrowed_roots = true;
            mark(entry.identity, &mut live, &mut pending);
        }
        while let Some(index) = pending.pop() {
            let Some(entry) = heap.values[index].as_ref() else { continue; };
            match entry.value.as_ref() {
                Value::Object { fields, bases, .. } => { for (_, id) in fields.iter().chain(bases) { mark(*id, &mut live, &mut pending); } }
                Value::Array(values) => { for id in values.references() { mark(*id, &mut live, &mut pending); } }
                Value::Dict(values) => { for (key, value) in values.values() { mark(*key, &mut live, &mut pending); mark(*value, &mut live, &mut pending); } }
                Value::Function { owner, .. } | Value::Template { owner, .. } => { if let Some(owner) = owner { mark(*owner, &mut live, &mut pending); } }
                _ => {}
            }
            if let Some(binding) = &entry.callable {
                for slot in binding.captures.iter().chain(std::iter::once(&binding.owner)) {
                    if let Slot::Handle(id) = slot { mark(id.get(), &mut live, &mut pending); }
                }
            }
        }
        #[cfg(test)] let marked_values = live.iter().filter(|value| **value != 0).count();
        let before = heap.live_values;
        let mut removed = 0;
        for (index, marked) in live.into_iter().enumerate() {
            if marked == 0 && heap.values[index].is_some() {
                let entry = heap.remove_slot(index).expect("已检查动态槽");
                heap.builders.remove(&entry.identity);
                heap.bytes -= dynamic_bytes(&entry.value, entry.callable.as_deref());
                removed += 1;
            }
        }
        heap.live_values -= removed;
        // 活动根与借出的 Arc 稍后会撤销；Arc 的归还没有通知，因此保留下一次扫描。
        heap.collection_dirty = !heap.roots.is_empty() || borrowed_roots;
        heap.collection_released = borrowed_roots;
        heap.next_collection = 1024.max(heap.live_values.saturating_mul(2));
        #[cfg(test)]
        {
            let elapsed = gc_started.elapsed().as_nanos();
            heap.metrics.collections += 1;
            heap.metrics.marked_values += marked_values as u64;
            heap.metrics.visited_edges += visited_edges.get();
            heap.metrics.gc_nanoseconds += elapsed;
            heap.metrics.max_gc_nanoseconds = heap.metrics.max_gc_nanoseconds.max(elapsed);
        }
        Ok(before - heap.live_values)
    }
    pub fn dynamic_value_count(&self) -> Result<usize, ExecutionError> {
        self.ensure_alive()?;
        Ok(self.vm.heap.borrow().live_values)
    }
}
pub(super) fn dynamic_bytes(value: &Value, binding: Option<&FunctionBinding>) -> usize {
    use std::mem::size_of;
    let payload = match value {
        Value::String(text) => text.capacity(),
        Value::Enum { type_name, .. } => type_name.capacity(),
        Value::Array(values) => values.heap_bytes(),
        Value::Dict(values) => values.heap_bytes(),
        Value::Object {
            type_name,
            key,
            fields,
            bases,
        } => {
            type_name.capacity()
                + key.as_ref().map_or(0, String::capacity)
                + (fields.capacity() + bases.capacity()) * size_of::<(String, ValueId)>()
                + fields
                    .iter()
                    .chain(bases)
                    .map(|(name, _)| name.capacity())
                    .sum::<usize>()
        }
        Value::Function { host, .. } => {
            host
                    .as_ref()
                    .map_or(0, |(service, field)| service.capacity() + field.capacity())
        }
        // 源码由程序映像共享，动态闭包只计费独立绑定和捕获。
        Value::Template { .. } => 0,
        Value::HostData { service, field, .. } => service.capacity() + field.capacity(),
        _ => 0,
    };
    size_of::<DynamicValue>()
        + size_of::<Value>()
        + 2 * size_of::<usize>()
        + payload
        + binding.map_or(0, |binding| size_of::<FunctionBinding>() + 2 * size_of::<usize>() + binding.captures.len() * size_of::<Slot>())
}

