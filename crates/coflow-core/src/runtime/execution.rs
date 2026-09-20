//! Runtime 的执行值适配与动态区。固定配置身份和动态返回值共用 Runtime 归属。
use super::*;
use super::fixed::View as Stored;
use crate::vm::{
    bytecode::{Constant, FormatPart, FunctionId, Program},
    compiler::{self, CompileContext},
    image::ValidatedProgram,
    executor::{self, Binding, Budget, Callable, ExecutionHost, ExecutionLimits, Slot},
};
use std::{
    cell::RefCell,
    cmp::Ordering as Comparison,
    collections::{BTreeMap, HashMap, HashSet},
    sync::OnceLock,
};

fn float_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"^[+-]?(?:[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?|inf|NaN)$")
            .expect("固定浮点语法正则必须有效")
    })
}

#[derive(Debug)]
struct DynamicValue {
    identity: ValueId,
    value: Arc<Value>,
    callable: Option<Binding>,
}
#[derive(Debug, Default)]
struct Heap {
    #[cfg(test)]
    metrics: HeapMetrics,
    next: ValueId,
    bytes: usize,
    /// 语言身份不复用，存储槽独立回收；早期长寿命值不会阻止临时槽复用。
    values: Vec<Option<DynamicValue>>,
    locations: HashMap<ValueId, usize>,
    free: Vec<usize>,
    live_values: usize,
    pinned: HashMap<ValueId, usize>,
    roots: HashMap<u64, Vec<Slot>>,
    next_roots: u64,
    next_collection: usize,
    builders: HashSet<ValueId>,
}
#[cfg(test)]
#[derive(Debug, Default)]
struct HeapMetrics { dispatches: u64, calls: u64, closures: u64, allocations: u64, allocated_payload_bytes: u64, peak_heap_bytes: usize, collections: u64, marked_values: u64, visited_edges: u64, gc_nanoseconds: u128, max_gc_nanoseconds: u128 }
impl Heap {
    fn table_bytes<T>(capacity: usize) -> usize {
        if capacity == 0 { 0 } else { capacity.saturating_mul(2 * (size_of::<T>() + 1)).saturating_add(16) }
    }
    /// 空闲槽、索引及根缓冲仍由实例持有，不能在回收值载荷后从预算中消失。
    fn total_bytes(&self) -> usize {
        self.bytes
            .saturating_add(self.values.capacity().saturating_mul(size_of::<Option<DynamicValue>>()))
            .saturating_add(Self::table_bytes::<(ValueId, usize)>(self.locations.capacity()))
            .saturating_add(Self::table_bytes::<ValueId>(self.builders.capacity()))
            .saturating_add(self.free.capacity().saturating_mul(size_of::<usize>()))
            .saturating_add(Self::table_bytes::<(ValueId, usize)>(self.pinned.capacity()))
            .saturating_add(Self::table_bytes::<(u64, Vec<Slot>)>(self.roots.capacity()))
            .saturating_add(self.roots.values().map(|roots| roots.capacity().saturating_mul(size_of::<Slot>())).sum::<usize>())
    }
    fn table_growth<T>(len: usize, capacity: usize) -> usize {
        if len < capacity { 0 } else {
            Self::table_bytes::<T>(len.saturating_add(1).saturating_mul(2).max(4)).saturating_sub(Self::table_bytes::<T>(capacity))
        }
    }
    fn reserve_roots(&mut self, id: u64, required: usize, limit: usize) -> Result<(), ExecutionError> {
        let roots = self.roots.get(&id).ok_or_else(|| invalid("执行根集合不存在"))?;
        if required <= roots.capacity() { return Ok(()); }
        let capacity = required.max(roots.capacity().saturating_mul(2)).max(4);
        let bytes = capacity.checked_sub(roots.capacity()).and_then(|n| n.checked_mul(size_of::<Slot>())).ok_or_else(|| invalid("执行根容量溢出"))?;
        if bytes > limit.saturating_sub(self.total_bytes()) { return Err(invalid("动态内存预算耗尽")); }
        let roots = self.roots.get_mut(&id).expect("根集合已验证");
        roots.try_reserve_exact(capacity - roots.len()).map_err(|_| invalid("执行根分配失败"))
    }
    fn index(&self, id: ValueId) -> Option<usize> {
        self.locations.get(&id).copied()
    }

    fn get(&self, id: ValueId) -> Option<&DynamicValue> {
        self.values.get(self.index(id)?)?.as_ref()
    }

    fn get_mut(&mut self, id: ValueId) -> Option<&mut DynamicValue> {
        let index = self.index(id)?;
        self.values.get_mut(index)?.as_mut()
    }
}
/// 程序区只保存相对固定区的绑定，不包含实例堆或 Host 地址。
#[derive(Debug, Default)]
pub(super) struct ImagePrograms {
    functions: BTreeMap<ValueId, FunctionId>,
    direct: Vec<Binding>,
    programs: crate::vm::contract_programs::ContractPrograms<ValidatedProgram>,
}
/// 每个实例独立拥有堆、执行预算和动态缓存。
#[derive(Debug, Default)]
pub(super) struct VmState {
    heap: RefCell<Heap>,
    budget: RefCell<Option<Budget>>,
    regexes: RefCell<HashMap<String, CachedRegex>>,
}
#[derive(Debug)]
struct CachedRegex {
    program: regex_automata::nfa::thompson::pikevm::PikeVM,
    cache: regex_automata::nfa::thompson::pikevm::Cache,
    _memory: executor::TemporaryBytes,
}
impl ImagePrograms {
    pub(super) fn build(runtime: &Runtime) -> Result<Self, BuildDiagnostic> {
        let mut state = Self::default();
        let mut bindings = BTreeMap::new();
        let function_ids = runtime.values.iter().enumerate().filter_map(|(id, value)| {
            matches!(value.as_ref(), Value::Function { host: None, .. } | Value::Template { .. }).then_some(id as ValueId)
        }).enumerate().map(|(index, value)| {
            u32::try_from(index).map(|index| (value, FunctionId(index))).map_err(|_| "程序区过大")
        }).collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut unlinked = runtime.contract.ir().lower(runtime.profile == OptimizationProfile::Release)
        .map_err(|error| BuildDiagnostic {
            code: "FUNCTION".into(),
            source: error.path.unwrap_or_else(|| error.module.to_string()),
            message: error.message,
            span: Some((error.span.start, error.span.end)),
        })?;
        // Contract 程序只包含符号引用；发布 Runtime 前统一绑定到当前不可变快照。
        for program in unlinked.functions.values_mut() {
            link_program(runtime, Arc::make_mut(program), &function_ids)?;
        }
        for check in &mut unlinked.checks {
            link_program(runtime, Arc::make_mut(&mut check.program), &function_ids)?;
        }
        state.programs = unlinked.publish().map_err(BuildDiagnostic::from)?;
        let mut programs: BTreeMap<
            (String, Option<String>, bool, BTreeMap<String, String>),
            Arc<ValidatedProgram>,
        > = BTreeMap::new();
        for (id, value) in runtime.values.iter().enumerate() {
            let id = id as ValueId;
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
                owner.and_then(|id| runtime.values.object_type(id)).map(str::to_owned);
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
                        offset: location.span.start,
                    };
                    if let Some(program) = state.programs.functions.get(&key) {
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
                bindings.insert(
                    id,
                    Binding {
                        program,
                        owner: owner.map_or(Slot::None, Slot::handle),
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
            let source = location.map_or(source.as_ref(), |location| location.source.as_str());
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
                    compiler::analyze_template(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                } else {
                    compiler::analyze(
                        runtime.contract.schema(),
                        source,
                        &format!("value#{id}"),
                        context,
                    )
                }
                .and_then(|function| function.lower_optimized(runtime.profile == OptimizationProfile::Release).map_err(|message| compiler::CompileError {
                    span: function.body.first().map_or(crate::source::Span::default(), |node| node.span), message,
                }))
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
                link_program(runtime, &mut program, &function_ids)?;
                let program = Arc::new(ValidatedProgram::new(program).map_err(BuildDiagnostic::from)?);
                programs.insert(key, program.clone());
                program
            };
            bindings.insert(
                id,
                Binding {
                    program,
                    owner: owner.map_or(Slot::None, Slot::handle),
                    captures: Arc::from([]),
                },
            );
        }
        // 固定 owner 的专化只属于当前映像；代码预算耗尽后保留通用程序。
        // 不改写嵌套闭包，它们可能显式绑定新构造对象的 self。
        if runtime.profile == OptimizationProfile::Release {
            let mut remaining = 65_536usize;
            for binding in bindings.values_mut() {
                use crate::vm::bytecode::Opcode;
                let Slot::Handle(owner) = binding.owner else { continue; };
        let owner = owner.get();
                let count = binding.program.instructions.len();
                if count > remaining || !binding.program.instructions.iter().any(|i| matches!(i.opcode(), Some(Opcode::SelfValue | Opcode::SelfField))) { continue; }
                let mut specialized = binding.program.to_editable();
                let mut changed = false;
                for instruction in &mut specialized.instructions {
                    let id = match instruction.opcode() {
                        Some(Opcode::SelfValue) => Some(owner),
                        Some(Opcode::SelfField) => runtime.fixed_field(owner, instruction.c()),
                        _ => None,
                    };
                    if let Some(id) = id {
                        *instruction = fixed_instruction(id, instruction.a())?;
                        changed = true;
                    }
                }
                if changed {
                    fold_fixed_reads(runtime, &mut specialized)?;
                    fold_scalar_control_flow(&mut specialized)?;
                    fold_format_plans(runtime, &mut specialized)?;
                    link_direct_calls(&mut specialized, &function_ids)?;
                    binding.program = Arc::new(ValidatedProgram::new(specialized)?);
                    remaining -= count;
                }
            }
        }
        // 编号在链接前统一分配，实际绑定在全部程序完成后一次发布；递归不形成 Arc 环。
        if !function_ids.keys().eq(bindings.keys()) { return Err("程序编号没有对应绑定".into()); }
        state.direct = bindings.into_values().collect();
        if runtime.profile == OptimizationProfile::Release {
            let effects = crate::vm::optimization::call_effects(&state.direct)?;
            let callees = state.direct.iter().map(|binding| binding.program.clone()).collect::<Vec<_>>();
            let mut inline_budget = 65_536;
            for binding in &mut state.direct {
                use crate::vm::bytecode::Opcode;
                let mut program = binding.program.to_editable();
                let inlined = crate::vm::optimization::inline_scalar_calls(&mut program, &callees, &mut inline_budget)?;
                if inlined { fold_scalar_control_flow(&mut program)?; fold_format_plans(runtime, &mut program)?; }
                program.build_liveness()?;
                let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
                    instruction.opcode() == Some(Opcode::CallDirect)
                        && effects[program.direct_calls[instruction.index() as usize].function.0 as usize].discardable()
                        && program.live.get(pc + 1).is_some_and(|live| !live.contains(&instruction.a()))
                }).collect::<Vec<_>>();
                if inlined || remove.iter().any(|removed| *removed) {
                    compact_instructions(&mut program, &remove)?;
                    binding.program = Arc::new(ValidatedProgram::new(program)?);
                }
            }
        }
        state.functions = function_ids;
        for binding in &state.direct {
            state.validate_direct_calls(runtime.contract.schema(), &binding.program)?;
        }
        for program in state.programs.functions.values() {
            state.validate_direct_calls(runtime.contract.schema(), program)?;
        }
        for check in &state.programs.checks {
            state.validate_direct_calls(runtime.contract.schema(), &check.program)?;
        }
        Ok(state)
    }

    fn validate_direct_calls(&self, schema: &crate::schema::CftSchema, program: &Program) -> Result<(), String> {
        use crate::vm::bytecode::Opcode;
        for instruction in &program.instructions {
            if instruction.opcode() != Some(Opcode::CallDirect) { continue; }
            let site = program.direct_calls.get(instruction.index() as usize).ok_or("直接调用附表越界")?;
            let target = &self.direct.get(site.function.0 as usize).ok_or("直接调用程序编号越界")?.program;
            let arguments = program.operands(site.arguments_start, site.arguments_len).ok_or("直接调用参数越界")?;
            if arguments.len() != target.parameters.len() || program.registers[instruction.a() as usize] != target.result {
                return Err("直接调用签名不匹配".into());
            }
            for (argument, expected) in arguments.iter().zip(&target.parameters) {
                if !schema.value_type_assignable(&program.registers[*argument as usize], expected) {
                    return Err("直接调用参数类型不匹配".into());
                }
            }
        }
        for closure in &program.closures { self.validate_direct_calls(schema, &closure.program)?; }
        Ok(())
    }

    pub(super) fn checks(&self) -> &[crate::vm::contract_programs::CheckProgram<ValidatedProgram>] {
        &self.programs.checks
    }
}
impl VmState {
    pub(super) fn new(fixed_count: ValueId) -> Self {
        let state = Self::default();
        {
            let mut heap = state.heap.borrow_mut();
            heap.next = fixed_count;
            heap.next_collection = 1024;
        }
        state
    }
    pub(super) fn value(&self, id: ValueId) -> Result<Arc<Value>, ExecutionError> {
        if let Some(value) = inline_value(id) { return Ok(Arc::new(value)); }
        self.heap
            .borrow()
            .get(id)
            .map(|entry| entry.value.clone())
            .ok_or(ExecutionError::InvalidHandle)
    }
    /// 在已持有的堆锁内分配；调用方负责先做 GC 阈值检查，一次锁完成全部分配。
    fn allocate_locked(
        heap: &mut Heap,
        value: Value,
        callable: Option<Binding>,
        limit: usize,
    ) -> Result<Slot, ExecutionError> {
        if heap.live_values >= 1_000_000 {
            return Err(invalid("动态值数量超限"));
        }
        let bytes = dynamic_bytes(&value, callable.as_ref());
        let mut metadata = 0usize;
        if heap.locations.len() == heap.locations.capacity() {
            let capacity = heap.locations.len().saturating_add(1).saturating_mul(2).max(4);
            metadata = metadata.saturating_add(Heap::table_bytes::<(ValueId, usize)>(capacity).saturating_sub(Heap::table_bytes::<(ValueId, usize)>(heap.locations.capacity())));
        }
        let slot_capacity = heap.values.len().saturating_add(1).max(heap.values.capacity().saturating_mul(2)).max(4);
        if heap.free.is_empty() && heap.values.len() == heap.values.capacity() {
            metadata = metadata.saturating_add(slot_capacity.saturating_sub(heap.values.capacity()).saturating_mul(size_of::<Option<DynamicValue>>()));
            metadata = metadata.saturating_add(slot_capacity.saturating_sub(heap.free.capacity()).saturating_mul(size_of::<usize>()));
        }
        if bytes.saturating_add(metadata) > limit.saturating_sub(heap.total_bytes()) {
            return Err(invalid("动态内存预算耗尽"));
        }
        let id = heap.next;
        if id > Slot::MAX_HEAP_HANDLE { return Err(invalid("动态值身份耗尽")); }
        let next = id.checked_add(1).ok_or_else(|| invalid("动态值身份耗尽"))?;
        // 先完成所有可失败的容量检查，再发布身份和计数；失败不能留下半个堆条目。
        heap.locations.try_reserve(1).map_err(|_| invalid("动态索引分配失败"))?;
        if heap.free.is_empty() && heap.values.len() == heap.values.capacity() {
            heap.values.try_reserve_exact(slot_capacity - heap.values.len()).map_err(|_| invalid("动态槽位分配失败"))?;
            heap.free.try_reserve_exact(slot_capacity - heap.free.len()).map_err(|_| invalid("回收槽索引分配失败"))?;
        }
        let entry = Some(DynamicValue { identity: id, value: Arc::new(value), callable });
        let index = if let Some(index) = heap.free.pop() {
            heap.values[index] = entry;
            index
        } else {
            let index = heap.values.len();
            heap.values.push(entry);
            index
        };
        heap.locations.insert(id, index);
        heap.next = next;
        heap.bytes += bytes;
        heap.live_values += 1;
        #[cfg(test)] {
            heap.metrics.allocations += 1; heap.metrics.allocated_payload_bytes += bytes as u64;
            heap.metrics.peak_heap_bytes = heap.metrics.peak_heap_bytes.max(heap.total_bytes());
        }
        Ok(Slot::handle(id))
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
        self.ensure_value(id)?;
        let host = self.execution_host(limits)?;
        // 外部实参窗口也属于本次调用预算，并跨同步重入保持累计占用。
        let (mut imported, _arguments_memory) = host.reserve_values(arguments.len())?;
        for value in arguments { imported.push(host.import(value)?); }
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
    /// 宿主把借用的子值保存到父值生命周期之外时，显式增加独立保活。
    pub fn retain_value(&self, id: ValueId) -> Result<(), ExecutionError> {
        let _entry = self.enter()?;
        self.ensure_value(id)?;
        if id < self.values.len() || Slot::from_scalar_id(id).is_some() {
            return Ok(());
        }
        let mut heap = self.vm.heap.borrow_mut();
        let limit = self.vm.budget.borrow().as_ref().map_or(usize::MAX, Budget::max_heap_bytes);
        if !heap.pinned.contains_key(&id) {
            let growth = Heap::table_growth::<(ValueId, usize)>(heap.pinned.len(), heap.pinned.capacity());
            if growth > limit.saturating_sub(heap.total_bytes()) { return Err(invalid("动态内存预算耗尽")); }
            heap.pinned.try_reserve(1).map_err(|_| invalid("保活索引分配失败"))?;
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
        for entry in heap.values.iter().flatten().filter(|entry| Arc::strong_count(&entry.value) > 1) { mark(entry.identity, &mut live, &mut pending); }
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
                let entry = heap.values[index].take().expect("已检查动态槽");
                heap.builders.remove(&entry.identity);
                heap.locations.remove(&entry.identity);
                heap.free.push(index);
                removed += 1;
            }
        }
        heap.live_values -= removed;
        heap.bytes = heap
            .values
            .iter()
            .flatten()
            .map(|entry| dynamic_bytes(&entry.value, entry.callable.as_ref()))
            .sum();
        #[cfg(test)] {
            let elapsed = gc_started.elapsed().as_nanos();
            heap.metrics.collections += 1; heap.metrics.marked_values += marked_values as u64;
            heap.metrics.visited_edges += visited_edges.get(); heap.metrics.gc_nanoseconds += elapsed;
            heap.metrics.max_gc_nanoseconds = heap.metrics.max_gc_nanoseconds.max(elapsed);
        }
        Ok(before - heap.live_values)
    }
    pub fn dynamic_value_count(&self) -> Result<usize, ExecutionError> {
        Ok(self.vm.heap.borrow().live_values)
    }
    pub(super) fn execution_host(
        &self,
        limits: ExecutionLimits,
    ) -> Result<RuntimeHost<'_>, ExecutionError> {
        let mut current = self.vm.budget.borrow_mut();
        let top = current.is_none();
        let mut heap = self.vm.heap.borrow_mut();
        let roots_id = heap.next_roots;
        heap.next_roots = roots_id
            .checked_add(1)
            .ok_or_else(|| invalid("执行身份耗尽"))?;
        let budget = current.as_ref().cloned().unwrap_or_else(|| Budget::new(limits));
        let growth = Heap::table_growth::<(u64, Vec<Slot>)>(heap.roots.len(), heap.roots.capacity());
        if growth > budget.max_heap_bytes().saturating_sub(heap.total_bytes()) { return Err(invalid("动态内存预算耗尽")); }
        heap.roots.try_reserve(1).map_err(|_| invalid("执行根索引分配失败"))?;
        heap.roots.insert(roots_id, Vec::new());
        if top { *current = Some(budget.clone()); }
        Ok(RuntimeHost {
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
        let result = executor::execute(
            &host,
            Binding {
                program,
                owner: owner.map_or(Slot::None, Slot::handle),
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
        self.ensure_value(left)?;
        self.ensure_value(right)?;
        let host = self.execution_host(ExecutionLimits::default())?;
        host.roots(&[Slot::handle(left), Slot::handle(right)])?;
        host.equal(host.slot(left)?, host.slot(right)?)
    }
    pub(super) fn evaluate_text(&self, id: ValueId) -> Result<String, ExecutionError> {
        let host = self.execution_host(ExecutionLimits::default())?;
        let value = if let Some(binding) = host.template(Slot::handle(id))? {
            if let Some(Constant::String(text)) = binding.program.static_text() { return Ok(text.clone()); }
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
struct FormatOutput<'a> { text: String, budget: &'a Budget, heap: &'a RefCell<Heap>, memory: executor::TemporaryBytes, error: Option<ExecutionError> }
impl std::fmt::Write for FormatOutput<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        if let Err(error) = self.budget.charge(text.len() as u64) { self.error = Some(error); return Err(std::fmt::Error); }
        let Some(required) = self.text.len().checked_add(text.len()).and_then(|n| n.checked_add(dynamic_bytes(&Value::String(String::new()), None))) else { self.error = Some(invalid("文本容量溢出")); return Err(std::fmt::Error); };
        if let Err(error) = self.memory.resize(required, self.heap.borrow().total_bytes()) { self.error = Some(error); return Err(std::fmt::Error); }
        if self.text.try_reserve_exact(text.len()).is_err() { self.error = Some(invalid("文本分配失败")); return Err(std::fmt::Error); }
        self.text.push_str(text); Ok(())
    }
}
/// 固定读取借用映像；动态读取持有短期 Arc，标量直接在栈上传递。
pub(super) enum ValueAccess<'a> {
    Fixed { value: std::borrow::Cow<'a, Value>, _memory: Option<executor::TemporaryBytes> },
    Dynamic(Arc<Value>),
    Scalar(Value),
}
impl AsRef<Value> for ValueAccess<'_> {
    fn as_ref(&self) -> &Value { match self { Self::Fixed { value, .. } => value.as_ref(), Self::Dynamic(value) => value.as_ref(), Self::Scalar(value) => value } }
}
impl std::ops::Deref for ValueAccess<'_> {
    type Target = Value;
    fn deref(&self) -> &Value { self.as_ref() }
}
impl ValueAccess<'_> {
    pub(super) fn into_arc(self) -> Arc<Value> { match self { Self::Dynamic(value) => value, Self::Fixed { value, .. } => Arc::new(value.into_owned()), Self::Scalar(value) => Arc::new(value) } }
}
pub(super) struct RuntimeHost<'a> {
    runtime: &'a Runtime,
    pub(super) budget: Budget,
    roots_id: u64,
    top: bool,
}
impl Drop for RuntimeHost<'_> {
    fn drop(&mut self) {
        self.runtime
            .vm
            .heap
            .borrow_mut()
            .roots
            .remove(&self.roots_id);
        if self.top {
            // 正则缓存只活在共享执行预算内；结束时连同 hash 容量一起释放。
            *self.runtime.vm.regexes.borrow_mut() = HashMap::new();
            *self.runtime.vm.budget.borrow_mut() = None;
            let _ = self.runtime.collect();
        }
    }
}
impl RuntimeHost<'_> {
    fn root(&self, value: Slot) -> Result<(), ExecutionError> {
        let Slot::Handle(id) = value else { return Ok(()); };
        if id.get() < self.runtime.values.len() || Slot::from_scalar_id(id.get()).is_some() { return Ok(()); }
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let required = heap.roots.get(&self.roots_id).ok_or_else(|| invalid("执行根集合不存在"))?.len().saturating_add(1);
        heap.reserve_roots(self.roots_id, required, self.budget.max_heap_bytes())?;
        heap.roots.get_mut(&self.roots_id).expect("根集合已验证").push(value);
        Ok(())
    }
    fn builder_append(&self, builder: Slot, value: Slot) -> Result<(), ExecutionError> {
        let Slot::Handle(id) = builder else { return Err(invalid("追加需要构造能力")); };
        let id = id.get();
        let value = self.id(value)?;
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if !heap.builders.contains(&id) { return Err(invalid("构造能力已经消费")); }
        let remaining = self.budget.max_heap_bytes().saturating_sub(heap.total_bytes());
        let entry = heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_ref());
        let Value::Array(values) = Arc::get_mut(&mut entry.value).ok_or_else(|| invalid("构造缓冲存在非法可写别名"))? else { return Err(invalid("追加需要数组")); };
        if values.len() == values.capacity() {
            // 先检查容量增量再分配；独占缓冲禁止隐式写时复制。
            let capacity = values.capacity().checked_mul(2).unwrap_or(usize::MAX).max(4);
            let additional = capacity.checked_sub(values.len()).ok_or_else(|| invalid("集合容量溢出"))?;
            let bytes = additional.checked_mul(values.element_bytes()).ok_or_else(|| invalid("集合容量溢出"))?;
            if bytes > remaining { return Err(invalid("动态内存预算耗尽")); }
            values.reserve(additional).map_err(|error| invalid(&error))?;
        }
        let result = values.push(value).map_err(|error| invalid(&error));
        let after = dynamic_bytes(&entry.value, entry.callable.as_ref());
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
        let limit = self.budget.max_heap_bytes();
        let remaining = limit.saturating_sub(heap.total_bytes());
        let entry = heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_ref());
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
                    if key_bytes > remaining { return Err(invalid("动态内存预算耗尽")); }
                    if !values.contains_key(&scalar) && values.len() == values.capacity() {
                        // 在扩容前保守预留哈希索引、键和条目空间，避免先分配后报告超限。
                        let required = values.capacity().max(4).checked_mul(4 * (size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>())).ok_or_else(|| invalid("集合容量溢出"))?;
                        if required > remaining - key_bytes { return Err(invalid("动态内存预算耗尽")); }
                        values.try_reserve(1).map_err(|_| invalid("字典构造分配失败"))?;
                    }
                    values.insert(scalar, (key_id.unwrap(), value));
                } else { values.shift_remove(&scalar); }
            }
            _ => return Err(invalid("构造修改需要集合")),
        }
        let after = dynamic_bytes(&entry.value, entry.callable.as_ref());
        heap.bytes = heap.bytes - before + after;
        Ok(Slot::Unit)
    }
    fn stored_value<'a>(
        &'a self,
        heap: Option<&'a Heap>,
        id: ValueId,
    ) -> Result<Stored<'a>, ExecutionError> {
        if let Some(value) = Slot::from_scalar_id(id) { return Ok(Stored::Scalar(value)); }
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
    fn slot_in_heap(&self, heap: Option<&Heap>, id: ValueId) -> Result<Option<Slot>, ExecutionError> {
        Ok(match self.stored_value(heap, id)? {
            Stored::Scalar(value) => Some(value), Stored::Host => None, _ => Some(Slot::handle(id)),
        })
    }
    fn slot(&self, id: ValueId) -> Result<Slot, ExecutionError> {
        if let Some(view) = self.runtime.values.view(id) {
            match view {
                Stored::Scalar(value) => return Ok(value), Stored::Host => {}, _ => return Ok(Slot::handle(id)),
            }
        }
        Ok(match self.value(Slot::handle(id))?.as_ref() {
            Value::None => Slot::None, Value::Bool(v) => Slot::Bool(*v), Value::Int(v) => Slot::Int(*v),
            Value::Float(v) => Slot::Float(*v), _ => Slot::handle(id),
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
    fn temporary_key(&self, slot: Slot) -> Result<(ScalarKey, executor::TemporaryBytes), ExecutionError> {
        let key = self.scalar_key(slot)?;
        let bytes = match &key { ScalarKey::String(text) => text.capacity(), ScalarKey::Enum { type_name, .. } => type_name.capacity(), _ => 0 };
        // copy_text 在复制前检查剩余额度，此处将复制结果转为集合整个生命周期的累计预留。
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        Ok((key, memory))
    }
    fn temporary_key_set(&self, count: usize) -> Result<(HashSet<ScalarKey>, executor::TemporaryBytes), ExecutionError> {
        // 哈希表容量按装载因子和二次幂取整，保守覆盖桶及控制字节。
        let bytes = count.checked_add(1).and_then(|n| n.checked_mul(4 * (size_of::<ScalarKey>() + 1))).ok_or_else(|| invalid("集合容量溢出"))?;
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut keys = HashSet::new();
        keys.try_reserve(count).map_err(|_| invalid("集合索引分配失败"))?;
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
    pub(super) fn value(&self, slot: Slot) -> Result<ValueAccess<'_>, ExecutionError> {
        Ok(match slot {
            Slot::None | Slot::Unit => ValueAccess::Scalar(Value::None),
            Slot::Bool(v) => ValueAccess::Scalar(Value::Bool(v)),
            Slot::Int(v) => ValueAccess::Scalar(Value::Int(v)),
            Slot::Float(v) => ValueAccess::Scalar(Value::Float(v)),
            Slot::Handle(id) => {
                let id = id.get();
                if self.runtime.values.is_host(id) { ValueAccess::Dynamic(self.runtime.value(id)?) }
                else if let Some(bytes) = self.runtime.values.materialized_bytes(id) {
                    let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
                    let value = self.runtime.values.get(id).ok_or(ExecutionError::InvalidHandle)?;
                    ValueAccess::Fixed { value, _memory: Some(memory) }
                } else { ValueAccess::Dynamic(self.runtime.vm.value(id)?) }
            }
            Slot::Empty => return Err(invalid("空寄存器不是语言值")),
        })
    }
    /// 外部输入的长度不能先变成分配；临时缓冲同样先检查剩余内存预算。
    fn reserve_values<T>(&self, count: usize) -> Result<(Vec<T>, executor::TemporaryBytes), ExecutionError> {
        let bytes = count.checked_mul(size_of::<T>()).ok_or_else(|| invalid("动态内存预算耗尽"))?;
        let memory = self.budget.reserve_temporary(bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| invalid("动态缓冲分配失败"))?;
        Ok((values, memory))
    }
    fn reserve_temporary_vec<T>(&self, values: &mut Vec<T>, memory: &mut executor::TemporaryBytes, additional: usize) -> Result<(), ExecutionError> {
        let required = values.len().checked_add(additional).ok_or_else(|| invalid("临时缓冲容量溢出"))?;
        if required <= values.capacity() { return Ok(()); }
        let capacity = values.capacity().saturating_mul(2).max(required).max(4);
        memory.resize(capacity.checked_mul(size_of::<T>()).ok_or_else(|| invalid("临时缓冲容量溢出"))?, self.runtime.vm.heap.borrow().total_bytes())?;
        values.try_reserve_exact(capacity - values.len()).map_err(|_| invalid("临时缓冲分配失败"))
    }
    fn preflight_bytes(&self, bytes: usize) -> Result<(), ExecutionError> {
        if bytes > self.budget.max_heap_bytes().saturating_sub(self.runtime.vm.heap.borrow().total_bytes()) {
            return Err(invalid("动态内存预算耗尽"));
        }
        Ok(())
    }
    fn copy_text(&self, text: &str) -> Result<String, ExecutionError> {
        self.preflight_bytes(text.len().saturating_add(dynamic_bytes(&Value::String(String::new()), None)))?;
        let mut result = String::new();
        result.try_reserve_exact(text.len()).map_err(|_| invalid("文本分配失败"))?;
        result.push_str(text);
        Ok(result)
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
        // 单次锁内完成 GC 阈值检查、分配与根登记，避免一次分配四次锁。
        let mut heap = self.runtime.vm.heap.borrow_mut();
        if heap.live_values >= heap.next_collection {
            drop(heap);
            self.runtime.collect()?;
            heap = self.runtime.vm.heap.borrow_mut();
            heap.next_collection = 1024.max(heap.live_values.saturating_mul(2));
        }
        let root_count = heap.roots.get(&self.roots_id).map_or(0, Vec::len);
        heap.reserve_roots(self.roots_id, root_count.saturating_add(1), self.budget.max_heap_bytes())?;
        let slot = VmState::allocate_locked(&mut heap, value, binding, self.budget.max_heap_bytes())?;
        heap.roots.get_mut(&self.roots_id).expect("执行根已预留").push(slot);
        Ok(slot)
    }
    pub(super) fn id(&self, value: Slot) -> Result<ValueId, ExecutionError> {
        if let Some(id) = value.scalar_id() { return Ok(id); }
        if let Slot::Handle(id) = value {
            return Ok(id.get());
        }
        Err(invalid("空寄存器没有值身份"))
    }
    pub(super) fn import(&self, value: &HostValue) -> Result<Slot, ExecutionError> {
        self.import_depth(value, 0)
    }
    fn import_depth(&self, value: &HostValue, depth: usize) -> Result<Slot, ExecutionError> {
        if depth >= 128 { return Err(invalid("导入值嵌套深度超限")); }
        self.budget.charge(1)?;
        Ok(match value {
            HostValue::Array(values) => {
                let (mut imported, _memory) = self.reserve_values(values.len())?;
                for value in values { imported.push(self.import_depth(value, depth + 1)?); }
                self.array(imported)?
            }
            HostValue::Dictionary(values) => {
                let (mut imported, _memory) = self.reserve_values(values.len())?;
                for (key, value) in values { imported.push((self.import_depth(key, depth + 1)?, self.import_depth(value, depth + 1)?)); }
                self.dictionary(imported)?
            }
            HostValue::Data { type_name, fields } => {
                let meta = self.runtime.contract.schema().resolve_type(type_name).ok_or_else(|| invalid("导入 data 类型不存在"))?;
                if meta.kind != coflow_language::cft::syntax::ast::TypeKind::Data || meta.is_abstract { return Err(invalid("只能导入具体 data")); }
                if fields.len() != meta.all_fields().count() { return Err(invalid("导入 data 字段数量不匹配")); }
                let (mut names, _names_memory) = self.reserve_values(fields.len())?;
                names.extend(fields.iter().map(|(name, _)| name.as_str())); names.sort_unstable();
                if names.windows(2).any(|pair| pair[0] == pair[1]) { return Err(invalid("导入 data 字段重复")); }
                let (mut imported, _memory) = self.reserve_values(fields.len())?;
                for (name, value) in fields {
                    let field = meta.field(name).ok_or_else(|| invalid("导入 data 字段不存在"))?;
                    let value = self.import_depth(value, depth + 1)?;
                    if !self.matches(value, &field.value_type)? { return Err(invalid("导入 data 字段类型不匹配")); }
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
                if self.runtime.values.is_host(*value) { return Err(invalid("Host must return a concrete value")); }
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
                    CftValueType::Object(expected) => !record && self.runtime.contract.schema().is_assignable(actual, expected),
                    CftValueType::RecordRef(expected) => record && self.runtime.contract.schema().is_assignable(actual, expected),
                    _ => false,
                });
            }
            if matches!(ty, CftValueType::String | CftValueType::FString)
                && matches!(self.runtime.values.view(id.get()), Some(Stored::String(_))) { return Ok(true); }
        }
        // 集合中的闭包按已验证程序签名校验，不能重新解析共享的外层源码。
        if let CftValueType::Array(inner) = ty {
            let check = |values: &ArrayValue| -> Result<bool, ExecutionError> {
                self.budget.charge(values.len() as u64)?;
                for value in values { if !self.matches(self.slot(value)?, inner)? { return Ok(false); } }
                Ok(true)
            };
            // 固定集合校验直接借用连续负载，递归 Host 读取不持有动态堆借用。
            if let Slot::Handle(id) = value {
                if let Some(Stored::Array(values)) = self.runtime.values.view(id.get()) { return check(values); }
            }
            let stored = self.value(value)?;
            let Value::Array(values) = stored.as_ref() else { return Ok(false); };
            return check(values);
        }
        if let CftValueType::Dict(key, inner) = ty {
            let check = |values: &indexmap::IndexMap<ScalarKey, (ValueId, ValueId)>| -> Result<bool, ExecutionError> {
                self.budget.charge(values.len() as u64)?;
                for (k, v) in values.values() {
                    if !self.matches(self.slot(*k)?, key)? || !self.matches(self.slot(*v)?, inner)? { return Ok(false); }
                }
                Ok(true)
            };
            if let Slot::Handle(id) = value {
                if let Some(Stored::Dict(values)) = self.runtime.values.view(id.get()) { return check(values); }
            }
            let stored = self.value(value)?;
            let Value::Dict(values) = stored.as_ref() else { return Ok(false); };
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
                .map_or_else(|| Ok(value.to_string()), |variant| self.copy_text(&variant.name))?),
            _ => Err(invalid("值不能转换为文本")),
        }
    }
    fn equal(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError> {
        // 用户可以逐次构造很深的不可变数据链；结构比较使用显式工作栈。
        self.root(left)?; self.root(right)?;
        let (mut pending, mut pending_memory) = self.reserve_values(1)?;
        pending.push((left, right));
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
                    self.root(left)?;
                    let right = read(right)?;
                    self.root(right)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, 1)?;
                    pending.push((left, right));
                    true
                }
                (Value::Array(a), Value::Array(b)) => {
                    if a.len() != b.len() {
                        return Ok(false);
                    }
                    self.budget.charge(a.len() as u64)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, a.len())?;
                    for (a, b) in a.iter().zip(b).rev() {
                        pending.push((self.slot(a)?, self.slot(b)?));
                    }
                    true
                }
                (Value::Dict(a), Value::Dict(b)) => {
                    if a.len() != b.len() {
                        return Ok(false);
                    }
                    self.budget.charge(a.len() as u64)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, a.len().checked_mul(2).ok_or_else(|| invalid("比较工作栈溢出"))?)?;
                    // 键已归一化为 ScalarKey：直接按键查表，值递归比较。
                    for (scalar, (key, value)) in a.iter() {
                        let Some((other_key, other_value)) = b.get(scalar) else {
                            return Ok(false);
                        };
                        pending.push((self.slot(*key)?, self.slot(*other_key)?));
                        pending.push((self.slot(*value)?, self.slot(*other_value)?));
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
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, af.len())?;
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
    #[cfg(test)]
    fn observe_instruction(&self, opcode: crate::vm::bytecode::Opcode) {
        use crate::vm::bytecode::Opcode;
        let mut heap = self.runtime.vm.heap.borrow_mut();
        heap.metrics.dispatches += 1;
        heap.metrics.calls += u64::from(matches!(opcode, Opcode::Call | Opcode::CallDirect));
        heap.metrics.closures += u64::from(matches!(opcode, Opcode::Closure));
    }

    fn heap_bytes(&self) -> usize { self.runtime.vm.heap.borrow().total_bytes() }
    fn build(&self, operation: &crate::vm::construction::BuildOp<Slot>, ty: &CftValueType) -> Result<Slot, ExecutionError> {
        use crate::vm::construction::BuildOp as B;
        if !matches!(operation, B::Drop { .. }) { self.budget.charge(1)?; }
        match operation {
            B::Drop { builder } => {
                let Slot::Handle(id) = builder else { return Err(invalid("清理需要构造能力")); };
        let id = id.get();
                let mut heap = self.runtime.vm.heap.borrow_mut();
                if heap.builders.remove(&id) {
                    if let Some(index) = heap.locations.remove(&id) {
                        if let Some(entry) = heap.values[index].take() {
                            heap.bytes -= dynamic_bytes(&entry.value, entry.callable.as_ref());
                            heap.live_values -= 1;
                            heap.free.push(index);
                        }
                    }
                }
                Ok(Slot::Unit)
            }
            B::Start { source } => {
                let result = match ty {
                    CftValueType::Object(name) => self.reserve_object(name)?,
                    CftValueType::Array(element) => {
                        let mut result = ArrayValue::empty(element);
                        if let Some(source) = source {
                            let Slot::Handle(source) = source else { return Err(invalid("数组构造来源无效")); };
                            let heap = self.runtime.vm.heap.borrow();
                            let Stored::Array(values) = self.stored_value(Some(&heap), source.get())? else { return Err(invalid("数组构造来源无效")); };
                            // 直接借用固定或动态负载，先预留预算再复制，避免物化和二次 clone。
                            let bytes = values.len().checked_mul(result.element_bytes()).ok_or_else(|| invalid("集合容量溢出"))?;
                            let memory = self.budget.reserve_temporary(bytes, heap.total_bytes())?;
                            result.reserve(values.len()).map_err(|error| invalid(&error))?;
                            for value in values { result.push(value).map_err(|error| invalid(&error))?; }
                            drop(memory);
                        }
                        self.allocate(Value::Array(result))?
                    }
                    CftValueType::Dict(..) => {
                        let mut result = indexmap::IndexMap::new();
                        if let Some(source) = source {
                            let Slot::Handle(source) = source else { return Err(invalid("字典构造来源无效")); };
                            let heap = self.runtime.vm.heap.borrow();
                            let Stored::Dict(values) = self.stored_value(Some(&heap), source.get())? else { return Err(invalid("字典构造来源无效")); };
                            let entry_bytes = size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>() + 32;
                            let mut bytes = values.len().checked_mul(entry_bytes).ok_or_else(|| invalid("集合容量溢出"))?;
                            for key in values.keys() {
                                let text = match key { ScalarKey::String(text) => text.len(), ScalarKey::Enum { type_name, .. } => type_name.len(), _ => 0 };
                                bytes = bytes.checked_add(text).ok_or_else(|| invalid("集合容量溢出"))?;
                            }
                            // 同时预留索引和所有字符串 key，固定字典无需先物化通用 Value。
                            let memory = self.budget.reserve_temporary(bytes, heap.total_bytes())?;
                            result.try_reserve(values.len()).map_err(|_| invalid("字典缓冲分配失败"))?;
                            for (key, value) in values {
                                let copy_text = |source: &str| -> Result<String, ExecutionError> {
                                    let mut text = String::new();
                                    text.try_reserve_exact(source.len()).map_err(|_| invalid("字典键分配失败"))?;
                                    text.push_str(source);
                                    Ok(text)
                                };
                                let key = match key {
                                    ScalarKey::String(text) => ScalarKey::String(copy_text(text)?),
                                    ScalarKey::Enum { type_name, value } => ScalarKey::Enum { type_name: copy_text(type_name)?, value: *value },
                                    ScalarKey::Int(value) => ScalarKey::Int(*value),
                                    ScalarKey::Bool(value) => ScalarKey::Bool(*value),
                                };
                                result.insert(key, *value);
                            }
                            drop(memory);
                        }
                        self.allocate(Value::Dict(result))?
                    }
                    _ => return Err(invalid("无效的局部构造类型")),
                };
                let Slot::Handle(id) = result else { return Err(invalid("构造缓冲必须具有身份")); };
        let id = id.get();
                let mut heap = self.runtime.vm.heap.borrow_mut();
                let growth = Heap::table_growth::<ValueId>(heap.builders.len(), heap.builders.capacity());
                if growth > self.budget.max_heap_bytes().saturating_sub(heap.total_bytes()) { return Err(invalid("动态内存预算耗尽")); }
                heap.builders.try_reserve(1).map_err(|_| invalid("构造能力索引分配失败"))?;
                heap.builders.insert(id);
                Ok(result)
            }
            B::DefaultField { owner, field } => {
                let object = self.value(*owner)?;
                let Value::Object { type_name, .. } = object.as_ref() else { return Err(invalid("字段默认值需要对象")); };
                let schema = self.runtime.contract.schema();
                let field = schema.resolve_type(type_name).and_then(|meta| meta.all_fields().nth(field.0 as usize)).ok_or_else(|| invalid("构造字段不存在"))?;
                if let Some(default) = &field.default {
                    let module = &schema.resolve_type(&field.declaring_type).ok_or_else(|| invalid("默认字段声明不存在"))?.module;
                    self.default_value(default, *owner, module)
                } else {
                    match field.value_type {
                        CftValueType::Option(_) => Ok(Slot::None),
                        CftValueType::Array(_) => self.array(Vec::new()),
                        CftValueType::Dict(..) => self.dictionary(Vec::new()),
                        _ => Err(invalid("必填构造字段没有默认值")),
                    }
                }
            }
            B::Freeze { builder } => {
                let Slot::Handle(id) = builder else { return Err(invalid("冻结需要构造能力")); };
        let id = id.get();
                if !self.runtime.vm.heap.borrow_mut().builders.remove(&id) { return Err(invalid("构造能力已经消费")); }
                // 冻结只移除写权限，缓冲与语言身份原位转交给不可变结果。
                Ok(*builder)
            }
            B::Append { builder, value } => {
                let Slot::Handle(id) = builder else { return Err(invalid("追加需要构造能力")); };
        let id = id.get();
                if !self.runtime.vm.heap.borrow().builders.contains(&id) { return Err(invalid("构造能力已经消费")); }
                self.builder_append(*builder, *value)?;
                Ok(Slot::Unit)
            }
            B::Set { builder, key, value } => self.builder_edit(*builder, *key, Some(*value)),
            B::Remove { builder, key } => self.builder_edit(*builder, *key, None),
        }
    }
    fn needs_roots(&self) -> bool {
        let heap = self.runtime.vm.heap.borrow();
        heap.live_values >= heap.next_collection
    }
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
            Constant::String(v) => self.allocate(Value::String(self.copy_text(v)?))?,
            Constant::Enum { name, value } => self.allocate(Value::Enum {
                type_name: self.copy_text(name)?,
                value: *value,
            })?,
        })
    }
    fn field(&self, receiver: Slot, slot: u16) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = receiver else {
            return Err(invalid("字段读取需要对象"));
        };
        let id = id.get();
        // 固定引用不能指向动态区；纯固定读取不借用实例堆。
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        let field = {
            let Stored::Object(fields) = self.stored_value(heap.as_deref(), id)? else {
                return Err(invalid("字段读取需要对象"));
            };
            fields
                .get(usize::from(slot))
                .ok_or_else(|| invalid("字段槽越界"))?
        };
        if let Some(value) = self.slot_in_heap(heap.as_deref(), field)? {
            Ok(value)
        } else {
            drop(heap);
            self.slot(field)
        }
    }
    fn index(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = receiver else {
            return Err(invalid("索引需要集合或字符串"));
        };
        let id = id.get();
        if let Some(value) = self.runtime.values.dictionary_index(id, key) {
            return self.slot(value.ok_or_else(|| invalid("字典 key 不存在"))?);
        }
        let scalar = self.scalar_key(key).ok();
        // 固定引用不能指向动态区；纯固定读取不借用实例堆。
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        match self.stored_value(heap.as_deref(), id)? {
            Stored::Array(values) => {
                let index = index(key)?;
                let id = values.get(index).ok_or_else(|| invalid("数组索引越界"))?;
                if let Some(value) = self.slot_in_heap(heap.as_deref(), id)? {
                    Ok(value)
                } else {
                    drop(heap);
                    self.slot(id)
                }
            }
            Stored::String(value) => {
                let index = index(key)?;
                self.budget.charge(index as u64 + 1)?;
                let ch = value
                    .chars()
                    .nth(index)
                    .ok_or_else(|| invalid("字符串索引越界"))?;
                drop(heap);
                self.allocate(Value::String(ch.to_string()))
            }
            Stored::Dict(values) => {
                let scalar = scalar.ok_or_else(|| invalid("无效的字典 key 类型"))?;
                let id = *values
                        .get(&scalar)
                        .map(|(_, value)| value)
                        .ok_or_else(|| invalid("字典 key 不存在"))?;
                if let Some(value) = self.slot_in_heap(heap.as_deref(), id)? {
                    Ok(value)
                } else {
                    drop(heap);
                    self.slot(id)
                }
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
                source: "".into(),
                owner: None,
                host: Some((service.into(), field.into())),
            });
        }
        let (ty, key) = name
            .rsplit_once("::")
            .ok_or_else(|| invalid("无效记录引用"))?;
        self.slot(self.runtime.record(ty, key)?)
    }
    fn fixed(&self, index: u32) -> Result<Slot, ExecutionError> {
        self.slot(u64::from(index))
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
        let (Slot::Handle(left), Slot::Handle(right)) = (left, right) else {
            return Err(invalid("连接需要 string"));
        };
        let (left, right) = (left.get(), right.get());
        let text = {
            let heap = (left >= self.runtime.values.len() || right >= self.runtime.values.len())
                .then(|| self.runtime.vm.heap.borrow());
            let (Stored::String(left), Stored::String(right)) = (
                self.stored_value(heap.as_deref(), left)?,
                self.stored_value(heap.as_deref(), right)?,
            ) else {
                return Err(invalid("连接需要 string"));
            };
            let length = left.len().checked_add(right.len()).ok_or_else(|| invalid("动态内存预算耗尽"))?;
            self.preflight_bytes(length.saturating_add(dynamic_bytes(&Value::String(String::new()), None)))?;
            self.budget.charge(length as u64)?;
            let mut text = String::new();
            text.try_reserve_exact(length).map_err(|_| invalid("文本分配失败"))?;
            text.push_str(left);
            text.push_str(right);
            text
        };
        self.allocate(Value::String(text))
    }
    fn enum_unary(&self, value: Slot) -> Result<Slot, ExecutionError> {
        if let Value::Enum { type_name, value } = self.value(value)?.as_ref() {
            let meta = self
                .runtime
                .contract
                .schema()
                .resolve_enum(type_name)
                .ok_or_else(|| invalid("未知 enum"))?;
            let mask = meta.flag_mask;
            self.allocate(Value::Enum {
                type_name: self.copy_text(type_name)?,
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
                type_name: self.copy_text(a)?,
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
        if let Slot::Handle(id) = value {
            if let Some((actual, _)) = self.runtime.values.object_identity(id.get()) {
                return Ok(self.runtime.contract.schema().is_assignable(actual, type_name));
            }
        }
        Ok(
            matches!(self.value(value)?.as_ref(),Value::Object{type_name:actual,..}if self.runtime.contract.schema().is_assignable(actual,type_name)),
        )
    }
    fn direct_callable(&self, function: FunctionId) -> Result<Binding, ExecutionError> {
        self.runtime.code().direct.get(function.0 as usize).cloned()
            .ok_or_else(|| invalid("直接调用程序编号越界"))
    }
    fn callable(&self, value: Slot) -> Result<Callable, ExecutionError> {
        let Slot::Handle(id) = value else {
            return Err(invalid("需要函数"));
        };
        let id = id.get();
        if let Some(function) = self.runtime.code().functions.get(&id) {
            return Ok(Callable::Program(self.direct_callable(*function)?));
        }
        if matches!(
            self.value(Slot::handle(id))?.as_ref(),
            Value::Function { host: Some(_), .. }
        ) {
            return Ok(Callable::Host(value));
        }
        self.runtime
            .vm
            .heap
            .borrow()
            .get(id)
            .and_then(|entry| entry.callable.clone())
            .map(Callable::Program)
            .ok_or_else(|| invalid("函数没有实现"))
    }
    fn call_host(&self, target: Slot, args: &[Slot]) -> Result<Slot, ExecutionError> {
        let _boundary = self.budget.enter_host()?;
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
        // 签名属于不可变契约；Host 调用借用它，不复制整棵嵌套参数类型。
        let check_parameters = [crate::schema::CftFunctionParameter::unnamed(CftValueType::Bool), crate::schema::CftFunctionParameter::unnamed(CftValueType::String)];
        let check_result = CftValueType::Unit;
        let (parameters, result_type): (&[crate::schema::CftFunctionParameter], &CftValueType) = if service == "Coflow::Check" && field == "require" {
            (&check_parameters, &check_result)
        } else {
            let signature = &self.runtime.contract.schema().field(service, field).ok_or_else(|| invalid("未知 Host 函数"))?.value_type;
            let CftValueType::Function(parameters, result) = signature else { return Err(invalid("Host 成员不是函数")); };
            (parameters, result)
        };
        if parameters.len() != args.len() {
            return Err(invalid("Host 参数数量不匹配"));
        }
        for (arg, parameter) in args.iter().zip(parameters) {
            if !self.matches(*arg, &parameter.value_type)? {
                return Err(invalid("Host 参数类型不匹配"));
            }
        }
        let (mut exported, _args_memory) = self.reserve_values(args.len())?;
        let mut payload_memory = self.budget.reserve_temporary(0, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut payload_bytes = 0usize;
        for value in args {
            let value = self.export_value(*value, false)?;
            let bytes = match &value { HostValue::String(text) => text.capacity(), HostValue::Enum { type_name, .. } => type_name.capacity(), _ => 0 };
            // 已导出的文本一直计费到同步 Host 回调及其重入全部结束。
            payload_bytes = payload_bytes.checked_add(bytes).ok_or_else(|| invalid("Host 参数容量溢出"))?;
            payload_memory.resize(payload_bytes, self.runtime.vm.heap.borrow().total_bytes())?;
            exported.push(value);
        }
        let args = exported;
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
            Some(id.get())
        } else {
            None
        };
        // 程序源码共享为 Arc<str>：闭包创建只支付引用计数，不再复制整段源文本。
        let source = Arc::clone(&binding.program.source);
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
    fn array(&self, values: Vec<Slot>) -> Result<Slot, ExecutionError> {
        let (mut stored, memory) = self.reserve_values(values.len())?;
        for value in values { stored.push(self.id(value)?); }
        let packed_memory = self.budget.reserve_temporary(ArrayValue::packing_bytes(&stored), self.runtime.vm.heap.borrow().total_bytes())?;
        let stored = ArrayValue::pack(stored).map_err(|error| invalid(&error))?;
        // 数值转换完成后转交连续缓冲，输入向量的临时预留不再占用预算。
        drop(memory); drop(packed_memory);
        self.allocate(Value::Array(stored))
    }
    fn dictionary(&self, values: Vec<(Slot, Slot)>) -> Result<Slot, ExecutionError> {
        // 键先归一化，再在插入时以 O(n) 检出重复键，替代旧的全量结构比较。
        let (mut keys, keys_memory) = self.reserve_values(values.len())?;
        let (mut key_guards, guards_memory) = self.reserve_values(values.len())?;
        for (key, _) in &values {
            let (key, memory) = self.temporary_key(*key)?;
            keys.push(key); key_guards.push(memory);
        }
        let entries_bytes = values.len().checked_mul(size_of::<(ScalarKey, (ValueId, ValueId))>() + 32).ok_or_else(|| invalid("动态内存预算耗尽"))?;
        let entries_memory = self.budget.reserve_temporary(entries_bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut entries = indexmap::IndexMap::new();
        entries.try_reserve(values.len()).map_err(|_| invalid("字典分配失败"))?;
        for ((key, value), scalar) in values.into_iter().zip(keys) {
            if entries
                .insert(scalar, (self.id(key)?, self.id(value)?))
                .is_some()
            {
                return Err(invalid("字典 key 重复"));
            }
        }
        drop(keys_memory); drop(key_guards); drop(guards_memory);
        drop(entries_memory);
        self.allocate(Value::Dict(entries))
    }
    fn reserve_object(&self, type_name: &str) -> Result<Slot, ExecutionError> {
        self.allocate(Value::Object {
            type_name: self.copy_text(type_name)?,
            key: None,
            fields: Vec::new(),
            bases: Vec::new(),
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
        let id = id.get();
        let meta = self
            .runtime
            .contract
            .schema()
            .resolve_type(type_name)
            .ok_or_else(|| invalid("未知 data 类型"))?;
        // 已求值的字段原地排序后二分查询，不额外分配临时树索引；默认值仍按声明顺序求值。
        let mut provided = fields;
        provided.sort_unstable_by(|left, right| left.0.cmp(right.0));
        if provided.windows(2).any(|pair| pair[0].0 == pair[1].0) { return Err(invalid("data 字段重复")); }
        let (mut stored, stored_memory) = self.reserve_values(meta.all_fields().count())?;
        let text_bytes = meta.all_fields().try_fold(type_name.len().saturating_add(size_of::<Value>() + 2 * size_of::<usize>()), |bytes, field| bytes.checked_add(field.name.len())).ok_or_else(|| invalid("对象载荷容量溢出"))?;
        let text_memory = self.budget.reserve_temporary(text_bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let copy_reserved = |source: &str| -> Result<String, ExecutionError> {
            let mut text = String::new();
            text.try_reserve_exact(source.len()).map_err(|_| invalid("对象文本分配失败"))?;
            text.push_str(source); Ok(text)
        };
        for field in meta.all_fields() {
            let value = if let Ok(index) = provided.binary_search_by(|(name, _)| name.cmp(&field.name.as_str())) {
                provided[index].1
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
            stored.push((copy_reserved(field.name.as_str())?, self.id(value)?));
        }
        let value = Value::Object {
            type_name: copy_reserved(type_name)?,
            key: None,
            fields: stored,
            bases: Vec::new(),
        };
        drop(stored_memory); drop(text_memory);
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let entry = heap.get(id).ok_or(ExecutionError::InvalidHandle)?;
        let bytes = heap.bytes - dynamic_bytes(&entry.value, entry.callable.as_ref())
            + dynamic_bytes(&value, None);
        if bytes.saturating_add(heap.total_bytes().saturating_sub(heap.bytes)) > self.budget.max_heap_bytes() {
            return Err(invalid("动态内存预算耗尽"));
        }
        heap.get_mut(id)
            .ok_or(ExecutionError::InvalidHandle)?
            .value = Arc::new(value);
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
    fn format(&self, parts: &[FormatPart], values: &[Slot]) -> Result<Slot, ExecutionError> {
        use std::fmt::Write;
        // 输出与片段物化、Host 重入共享累计预留，不能各自使用同一份剩余额度。
        let memory = self.budget.reserve_temporary(0, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut output = FormatOutput { text: String::new(), budget: &self.budget, heap: &self.runtime.vm.heap, memory, error: None };
        for part in parts {
            let written = match part {
                FormatPart::Text(text) => output.write_str(text),
                FormatPart::Value(register) => {
                    let slot = *values.get(*register as usize).ok_or_else(|| invalid("格式输入越界"))?;
                    if let Slot::Handle(id) = slot {
                        if let Some(Stored::String(text)) = self.runtime.values.view(id.get()) {
                            if output.write_str(text).is_err() { return Err(output.error.take().unwrap_or_else(|| invalid("格式化失败"))); }
                            continue;
                        }
                    }
                    match self.value(slot)?.as_ref() {
                        Value::String(value) => output.write_str(value),
                        Value::Int(value) => write!(output, "{value}"),
                        Value::Float(value) => write!(output, "{value}"),
                        Value::Bool(value) => write!(output, "{value}"),
                        Value::Enum { type_name, value } => {
                            let variant = self.runtime.contract.schema().resolve_enum(type_name).and_then(|meta| meta.variant_by_value.get(&i64::from(*value)).and_then(|index| meta.variants.get(*index)));
                            if let Some(variant) = variant { output.write_str(&variant.name) } else { write!(output, "{value}") }
                        }
                        _ => return Err(invalid("值不能转换为文本")),
                    }
                }
            };
            if written.is_err() { return Err(output.error.take().unwrap_or_else(|| invalid("格式化失败"))); }
        }
        drop(output.memory);
        self.allocate(Value::String(output.text))
    }
    fn length(&self, value: Slot) -> Result<usize, ExecutionError> {
        let Slot::Handle(id) = value else {
            return Err(invalid("值没有长度"));
        };
        let id = id.get();
        // 固定引用不能指向动态区；纯固定读取不借用实例堆。
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        Ok(match self.stored_value(heap.as_deref(), id)? {
            Stored::Array(v) => v.len(),
            Stored::Dict(v) => v.len(),
            Stored::String(v) => {
                self.budget.charge(v.len() as u64)?;
                v.chars().count()
            }
            _ => return Err(invalid("值没有长度")),
        })
    }
    fn iterator(&self, value: Slot, index: usize) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = value else {
            return Err(invalid("值不能迭代"));
        };
        let id = id.get();
        // 固定引用不能指向动态区；纯固定读取不借用实例堆。
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        match self.stored_value(heap.as_deref(), id)? {
            Stored::Array(values) => {
                let id = values.get(index).ok_or_else(|| invalid("索引越界"))?;
                if let Some(value) = self.slot_in_heap(heap.as_deref(), id)? {
                    Ok(value)
                } else {
                    drop(heap);
                    self.slot(id)
                }
            }
            Stored::Dict(values) => {
                let (_, (k, _)) = values.get_index(index).ok_or_else(|| invalid("索引越界"))?;
                if let Some(value) = self.slot_in_heap(heap.as_deref(), *k)? {
                    Ok(value)
                } else {
                    let id = *k;
                    drop(heap);
                    self.slot(id)
                }
            }
            _ => Err(invalid("值不能迭代")),
        }
    }
    fn iter_next(&self, value: Slot, index: usize) -> Result<(Slot, Slot), ExecutionError> {
        // 单次堆读取同时产出键与值；数组键是寄存器内的 int，不触碰堆。
        let Slot::Handle(id) = value else {
            return Err(invalid("值不能迭代"));
        };
        let id = id.get();
        // 固定引用不能指向动态区；纯固定读取不借用实例堆。
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        match self.stored_value(heap.as_deref(), id)? {
            Stored::Array(values) => {
                let stored = values.get(index).ok_or_else(|| invalid("索引越界"))?;
                let key = i32::try_from(index).map_err(|_| invalid("索引超出 int"))?;
                let value = if let Some(value) = self.slot_in_heap(heap.as_deref(), stored)? {
                    value
                } else {
                    let id = stored;
                    drop(heap);
                    return Ok((Slot::Int(key), self.slot(id)?));
                };
                Ok((Slot::Int(key), value))
            }
            Stored::Dict(values) => {
                let (_, (k, v)) = values.get_index(index).ok_or_else(|| invalid("索引越界"))?;
                let (key, value) = (*k, *v);
                let key_slot = self.slot_in_heap(heap.as_deref(), key)?;
                let value_slot = self.slot_in_heap(heap.as_deref(), value)?;
                if let (Some(key), Some(value)) = (key_slot, value_slot) {
                    Ok((key, value))
                } else {
                    drop(heap);
                    Ok((
                        key_slot.map_or_else(|| self.slot(key), Ok)?,
                        value_slot.map_or_else(|| self.slot(value), Ok)?,
                    ))
                }
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
        // 复用既有缓冲容量，避免每次根发布重新分配。
        let mut heap = self.runtime.vm.heap.borrow_mut();
        heap.reserve_roots(self.roots_id, roots.len(), self.budget.max_heap_bytes())?;
        let entry = heap.roots.get_mut(&self.roots_id).expect("执行根已预留");
        entry.clear();
        entry.extend_from_slice(roots);
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
            D::String(value) => self.allocate(Value::String(self.copy_text(value)?)),
            D::Enum {
                enum_name, value, ..
            } => self.allocate(Value::Enum {
                type_name: self.copy_text(enum_name)?,
                value: *value as u32,
            }),
            D::EmptyArray => self.array(Vec::new()),
            D::EmptyObject => self.dictionary(Vec::new()),
            D::Array(values) => {
                let (mut items, _memory) = self.reserve_values(values.len())?;
                for value in values { items.push(self.default_value(value, owner, module)?); }
                self.array(items)
            }
            D::Dictionary(values) => {
                let (mut entries, _memory) = self.reserve_values(values.len())?;
                for (key, value) in values {
                    entries.push((self.default_value(key, owner, module)?, self.default_value(value, owner, module)?));
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
        self.root(result)?;
        Ok(result)
    }
    fn regex_match(&self, pattern: &str, text: &str) -> Result<bool, ExecutionError> {
        use regex_automata::nfa::thompson::{pikevm::PikeVM, WhichCaptures, NFA};
        let mut regexes = self.runtime.vm.regexes.borrow_mut();
        if !regexes.contains_key(pattern) {
            let heap_bytes = self.runtime.vm.heap.borrow().total_bytes();
            let remaining = self.budget.max_heap_bytes().saturating_sub(heap_bytes);
            let syntax_bytes = pattern.len().checked_mul(256).and_then(|n| n.checked_add(4096)).ok_or_else(|| invalid("正则容量溢出"))?;
            let nfa_limit = remaining.saturating_sub(syntax_bytes) / 64;
            if nfa_limit < 1024 { return Err(invalid("动态内存预算耗尽")); }
            // 无捕获 Pike VM 没有随输入增长的 DFA 缓存；编译和 NFA 状态空间先保守预留。
            let compile_memory = self.budget.reserve_temporary(syntax_bytes + nfa_limit * 64, heap_bytes)?;
            self.budget.charge(pattern.len() as u64)?;
            let program = PikeVM::builder().thompson(NFA::config().which_captures(WhichCaptures::None).nfa_size_limit(Some(nfa_limit)))
                .build(pattern).map_err(|error| invalid(&format!("正则编译失败：{error}")))?;
            let required = program.get_nfa().memory_usage().checked_mul(16)
                .and_then(|n| n.checked_add(program.get_nfa().states().len().saturating_mul(64)))
                .and_then(|n| n.checked_add(pattern.len() + size_of::<CachedRegex>() + 1024))
                .and_then(|n| n.checked_add(Heap::table_growth::<(String, CachedRegex)>(regexes.len(), regexes.capacity())))
                .ok_or_else(|| invalid("正则容量溢出"))?;
            drop(compile_memory);
            let memory = self.budget.reserve_temporary(required, heap_bytes)?;
            let cache = program.create_cache();
            let mut key = String::new(); key.try_reserve_exact(pattern.len()).map_err(|_| invalid("正则键分配失败"))?; key.push_str(pattern);
            regexes.try_reserve(1).map_err(|_| invalid("正则索引分配失败"))?;
            regexes.insert(key, CachedRegex { program, cache, _memory: memory });
        }
        let entry = regexes.get_mut(pattern).ok_or_else(|| invalid("正则缓存缺失"))?;
        let work = (entry.program.get_nfa().states().len() as u64).saturating_mul(text.len().saturating_add(1) as u64);
        self.budget.charge(work)?;
        Ok(entry.program.is_match(&mut entry.cache, text))
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
                type_name: self.copy_text(type_name)?,
                value,
            });
        }
        if matches!(name, "for" | "default" | "variants") {
            let Slot::Handle(id) = receiver else {
                return Err(invalid("维度方法需要记录"));
            };
        let id = id.get();
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
            let value = self.value(Slot::handle(id))?;
            let Value::Dimension { variants, .. } = value.as_ref() else {
                return Err(invalid("维度方法需要维度值"));
            };
            let (mut values, _memory) = self.reserve_values(variants.len())?;
            for variant in variants.keys() {
                self.budget.charge(1)?;
                let key = self.allocate(Value::String(self.copy_text(variant)?))?;
                let value =
                    self.read_slot(self.slot(self.runtime.dimension_variant(id, variant)?)?)?;
                values.push((key, value));
            }
            return self.dictionary(values);
        }
        if let Some(type_name) = name.strip_prefix("$records::") {
            // 记录目录属于映像，直接借用遍历，避免先复制未计费的目录。
            let records = &self.runtime.records;
            self.budget.charge(records.len() as u64)?;
            let count = records.keys().filter(|(actual, _)| self.runtime.contract.schema().is_assignable(actual, type_name)).count();
            let (mut values, _memory) = self.reserve_values(count)?;
            for ((actual, _), id) in records.iter() {
                if self.runtime.contract.schema().is_assignable(actual, type_name) { values.push(Slot::handle(*id)); }
            }
            return self.array(values);
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
                    return Ok(if float_pattern().is_match(value) {
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
                    "matches" => self.regex_match(other, value)?,
                    _ => return Err(invalid("未知 string 内建")),
                }))
            }
            Value::Array(values) => {
                self.budget.charge(values.len() as u64)?;
                if name == "contains" {
                    for value in values {
                        if self.equal(self.slot(value)?, argument(0)?)? {
                            return Ok(Slot::Bool(true));
                        }
                    }
                    return Ok(Slot::Bool(false));
                }
                if name == "isUnique" {
                    let (mut seen, _set_memory) = self.temporary_key_set(values.len())?;
                    let (mut key_memory, _guards_memory) = self.reserve_values(values.len())?;
                    for value in values {
                        let (key, memory) = self.temporary_key(self.slot(value)?)?;
                        if !seen.insert(key) { return Ok(Slot::Bool(false)); }
                        key_memory.push(memory);
                    }
                    return Ok(Slot::Bool(true));
                }
                if name == "isSorted" || name == "isStrictlySorted" {
                    for value in values {
                        if matches!(self.slot(value)?,Slot::Float(v)if v.is_nan()) {
                            return Ok(Slot::Bool(false));
                        }
                    }
                    for (left, right) in values.iter().zip(values.iter().skip(1)) {
                        let order = self.compare(self.slot(left)?, self.slot(right)?)?;
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
                    let mut result = self.slot(values.get(0).expect("非空数组"))?;
                    for id in values.iter().skip(1) {
                        let next = self.slot(id)?;
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
                    let (mut right_keys, _set_memory) = self.temporary_key_set(right.len())?;
                    let (mut key_memory, _guards_memory) = self.reserve_values(right.len())?;
                    for value in right {
                        let (key, memory) = self.temporary_key(self.slot(value)?)?;
                        if right_keys.insert(key) { key_memory.push(memory); }
                    }
                    let right = right_keys;
                    for value in left {
                        let found = right.contains(&self.scalar_key(self.slot(value)?)?);
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
                    let (mut items, _memory) = self.reserve_values(values.len())?;
                    for (key, value) in values.values() {
                        items.push(self.slot(if name == "keys" { *key } else { *value })?);
                    }
                    return self.array(items);
                }
                if matches!(name, "contains" | "containsKey" | "containsValue") {
                    let argument = argument(0)?;
                    if name != "containsValue" {
                        return Ok(Slot::Bool(values.contains_key(&self.scalar_key(argument)?)));
                    }
                    for (_, (_, value)) in values {
                        if self.equal(
                            self.slot(*value)?,
                            argument,
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

fn fixed_instruction(id: ValueId, destination: u16) -> Result<crate::vm::bytecode::Instruction, String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    Ok(match Slot::from_scalar_id(id) {
        Some(Slot::None) => Instruction::new(Opcode::Constant, destination, 1, 0, 3),
        Some(Slot::Bool(value)) => Instruction::new(Opcode::Constant, destination, if value { 3 } else { 2 }, 0, 3),
        Some(Slot::Int(value)) => Instruction::indexed(Opcode::Constant, destination, value as u32).with_flags(1),
        Some(Slot::Float(value)) => Instruction::indexed(Opcode::Constant, destination, value.to_bits()).with_flags(2),
        _ => Instruction::indexed(Opcode::LoadFixed, destination, u32::try_from(id).map_err(|_| "固定值槽超限")?),
    })
}

fn link_program(runtime: &Runtime, program: &mut Program, functions: &BTreeMap<ValueId, FunctionId>) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    program.tail_calls = runtime.profile == OptimizationProfile::Release;

    for instruction in &mut program.instructions {
        if instruction.opcode() != Some(Opcode::Reference) {
            continue;
        }
        let name = program
            .names
            .get(instruction.index() as usize)
            .ok_or_else(|| format!("{}: 引用索引越界", program.name))?;
        if name.starts_with("$host::") {
            *instruction = Instruction::indexed(
                Opcode::LoadHost,
                instruction.a(),
                instruction.index(),
            );
            continue;
        }
        let id = if let Some(name) = name.strip_prefix("$const::") {
            *runtime
                .constants
                .get(name)
                .ok_or_else(|| format!("{}: 未链接常量 {name}", program.name))?
        } else {
            let (ty, key) = name
                .rsplit_once("::")
                .ok_or_else(|| format!("{}: 无效记录引用 {name}", program.name))?;
            runtime
                .record(ty, key)
                .map_err(|error| format!("{}: {name}: {error}", program.name))?
        };
        *instruction = fixed_instruction(id, instruction.a())?;
    }
    for closure in &mut program.closures {
        link_program(runtime, Arc::make_mut(&mut closure.program), functions)?;
    }
    if runtime.profile == OptimizationProfile::Release {
        fold_fixed_reads(runtime, program)?;
        fold_scalar_control_flow(program)?;
        fold_format_plans(runtime, program)?;
        fuse_int_immediates(program)?;
    }
    link_direct_calls(program, functions)?;
    if program
        .instructions
        .iter()
        .any(|instruction| instruction.opcode() == Some(Opcode::Reference))
    {
        return Err(format!("{}: Runtime 映像仍包含未链接引用", program.name));
    }
    program.validate()
}

/// 仅沿同一基本块传播已链接函数身份，分支入口与所有写入均杀死旧事实。
fn link_direct_calls(program: &mut Program, functions: &BTreeMap<ValueId, FunctionId>) -> Result<(), String> {
    use crate::vm::bytecode::{DirectCallSite, Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let writes = program.instructions.iter().copied().map(|instruction| program.written_registers(instruction))
        .collect::<Result<Vec<_>, _>>()?;
    let mut known = HashMap::new();
    for (pc, instruction) in program.instructions.iter_mut().enumerate() {
        if leaders.contains(&pc) { known.clear(); }
        let original = *instruction;
        let linked = match original.opcode() {
            Some(Opcode::LoadFixed) => functions.get(&u64::from(original.index())).copied(),
            Some(Opcode::Move) => known.get(&original.b()).copied(),
            _ => None,
        };
        if original.opcode() == Some(Opcode::Call) {
            let site = program.calls.get(original.index() as usize).ok_or("调用附表越界")?;
            if let Some(function) = known.get(&site.target).copied() {
                let index = u32::try_from(program.direct_calls.len()).map_err(|_| "直接调用附表过大")?;
                program.direct_calls.push(DirectCallSite { function, arguments_start: site.arguments_start, arguments_len: site.arguments_len });
                *instruction = Instruction::indexed(Opcode::CallDirect, original.a(), index);
            }
        }
        for register in &writes[pc] { known.remove(register); }
        if let Some(function) = linked { known.insert(original.a(), function); }
    }
    program.build_liveness()?;
    // 直接调用不再需要函数值载入；只删除已证明无 Host/求值行为且结果不活跃的节点。
    let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
        let removable = instruction.opcode() == Some(Opcode::Move)
            || (instruction.opcode() == Some(Opcode::LoadFixed)
                && functions.contains_key(&u64::from(instruction.index())));
        removable && program.live.get(pc + 1).is_none_or(|live| !live.contains(&instruction.a()))
    }).collect::<Vec<_>>();
    compact_instructions(program, &remove)?;
    let old_calls = std::mem::take(&mut program.calls);
    for instruction in &mut program.instructions {
        if instruction.opcode() == Some(Opcode::Call) {
            let site = old_calls.get(instruction.index() as usize).ok_or("调用附表越界")?.clone();
            let index = u32::try_from(program.calls.len()).map_err(|_| "调用附表过大")?;
            program.calls.push(site);
            *instruction = Instruction::indexed(Opcode::Call, instruction.a(), index);
        }
    }
    Ok(())
}

fn fuse_int_immediates(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};

    // 其他前驱可能绕过常量写入；只在同一基本块中融合。
    program.build_liveness()?;
    let leaders = program.block_leaders()?;
    let mut remove = vec![false; program.instructions.len()];
    for pc in 0..program.instructions.len().saturating_sub(1) {
        if leaders.contains(&(pc + 1)) {
            continue;
        }
        let constant = program.instructions[pc];
        let binary = program.instructions[pc + 1];
        if constant.opcode() != Some(Opcode::Constant)
            || constant.flags() != 1
            || binary.opcode() != Some(Opcode::IntBinary)
        {
            continue;
        }
        let temporary = constant.a();
        let (current, immediate_left) = if binary.b() == temporary && binary.a() == binary.c() {
            (binary.c(), true)
        } else if binary.c() == temporary && binary.a() == binary.b() {
            (binary.b(), false)
        } else {
            continue;
        };
        if current == temporary {
            continue;
        }
        if program
            .live
            .get(pc + 2)
            .is_some_and(|live| live.contains(&temporary))
        {
            continue;
        }
        let flags = binary.flags() | if immediate_left { 0x80 } else { 0 };
        program.instructions[pc + 1] = Instruction::indexed(
            Opcode::IntBinaryImmediate,
            current,
            constant.index(),
        )
        .with_flags(flags);
        remove[pc] = true;
    }
    compact_instructions(program, &remove)
}

fn compact_instructions(program: &mut Program, remove: &[bool]) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};

    if !remove.iter().any(|remove| *remove) {
        return Ok(());
    }
    let mut old_to_new = vec![0usize; remove.len() + 1];
    let mut next = 0usize;
    for (pc, removed) in remove.iter().copied().enumerate() {
        old_to_new[pc] = next;
        if !removed {
            next += 1;
        }
    }
    old_to_new[remove.len()] = next;

    let mut instructions = Vec::with_capacity(next);
    let mut spans = Vec::with_capacity(next);
    for (pc, (instruction, span)) in program
        .instructions
        .iter()
        .copied()
        .zip(program.spans.iter().copied())
        .enumerate()
    {
        if remove[pc] {
            continue;
        }
        let instruction = match instruction.opcode() {
            Some(opcode @ (Opcode::Jump | Opcode::JumpFalse)) => {
                let target = *old_to_new
                    .get(instruction.index() as usize)
                    .ok_or("跳转目标越界")?;
                Instruction::indexed(
                    opcode,
                    instruction.a(),
                    u32::try_from(target).map_err(|_| "程序过大")?,
                )
                .with_flags(instruction.flags())
            }
            _ => instruction,
        };
        instructions.push(instruction);
        spans.push(span);
    }
    for site in &mut program.for_sites {
        site.target = u32::try_from(
            *old_to_new
                .get(site.target as usize)
                .ok_or("循环回边越界")?,
        )
        .map_err(|_| "程序过大")?;
    }
    program.instructions = instructions;
    program.spans = spans;
    program.build_liveness()
}

/// 只折叠实际成功的标量计算，随后按真实跳转边删除不可达节点。
/// 调用者须先链接所有符号，未执行分支里的非法引用仍然阻止发布。
fn fold_scalar_control_flow(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let mut known = HashMap::new();
    let mut remove = vec![false; program.instructions.len()];
    for pc in 0..program.instructions.len() {
        if leaders.contains(&pc) { known.clear(); }
        let instruction = program.instructions[pc];
        let a = instruction.a();
        let input = |register| known.get(&register).copied();
        let result = match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Constant => match instruction.flags() {
                1 => Some(Slot::Int(instruction.index() as i32)),
                2 => Some(Slot::Float(f32::from_bits(instruction.index()))),
                3 => Some(match instruction.b() { 0 => Slot::Unit, 1 => Slot::None, 2 => Slot::Bool(false), _ => Slot::Bool(true) }),
                0 => match program.constants.get(instruction.index() as usize) {
                    Some(Constant::Int(value)) => Some(Slot::Int(*value)), Some(Constant::Float(value)) => Some(Slot::Float(*value)),
                    Some(Constant::Bool(value)) => Some(Slot::Bool(*value)), Some(Constant::None) => Some(Slot::None), Some(Constant::Unit) => Some(Slot::Unit), _ => None,
                }, _ => None,
            },
            Opcode::Move => input(instruction.b()),
            Opcode::Binary | Opcode::IntBinary | Opcode::FloatBinary => input(instruction.b()).zip(input(instruction.c()))
                .and_then(|(left, right)| executor::scalar_binary(instruction.flags(), left, right)).and_then(Result::ok),
            Opcode::IntBinaryImmediate => input(a).and_then(|value| {
                let immediate = Slot::Int(instruction.index() as i32);
                let (left, right) = if instruction.flags() & 0x80 != 0 { (immediate, value) } else { (value, immediate) };
                executor::scalar_binary(instruction.flags() & 0x7f, left, right)?.ok()
            }),
            Opcode::ConvertFloat => input(instruction.b()).and_then(|value| if let Slot::Int(value) = value { Some(Slot::Float(value as f32)) } else { None }),
            Opcode::IsSome => input(instruction.b()).map(|value| Slot::Bool(value != Slot::None)),
            Opcode::Unary => input(instruction.b()).and_then(|value| match (instruction.flags(), value) {
                (0, Slot::Int(value)) => value.checked_neg().map(Slot::Int), (0, Slot::Float(value)) => Some(Slot::Float(-value)),
                (1, Slot::Bool(value)) => Some(Slot::Bool(!value)), (2, Slot::Int(value)) => Some(Slot::Int(!value)), _ => None,
            }),
            Opcode::JumpFalse => {
                if let Some(Slot::Bool(condition)) = input(a) {
                    if condition { remove[pc] = true; }
                    else { program.instructions[pc] = Instruction::indexed(Opcode::Jump, 0, instruction.index()); }
                }
                None
            }
            _ => None,
        };
        for register in program.written_registers(instruction)? { known.remove(&register); }
        if let Some(value) = result {
            let replacement = match value {
                Slot::Int(value) => Instruction::indexed(Opcode::Constant, a, value as u32).with_flags(1),
                Slot::Float(value) => Instruction::indexed(Opcode::Constant, a, value.to_bits()).with_flags(2),
                Slot::Unit => Instruction::new(Opcode::Constant, a, 0, 0, 3),
                Slot::None => Instruction::new(Opcode::Constant, a, 1, 0, 3),
                Slot::Bool(value) => Instruction::new(Opcode::Constant, a, if value { 3 } else { 2 }, 0, 3),
                _ => continue,
            };
            program.instructions[pc] = replacement; known.insert(a, value);
        }
    }
    compact_instructions(program, &remove)?;
    let mut reachable = vec![false; program.instructions.len()];
    let mut pending = vec![0];
    while let Some(pc) = pending.pop() {
        if reachable[pc] { continue; } reachable[pc] = true;
        let instruction = program.instructions[pc];
        if let Some(target) = program.branch_target(instruction)? { pending.push(target); }
        if !matches!(instruction.opcode(), Some(Opcode::Jump | Opcode::Return)) && pc + 1 < reachable.len() { pending.push(pc + 1); }
    }
    let remove = reachable.iter().map(|reachable| !reachable).collect::<Vec<_>>();
    compact_instructions(program, &remove)
}

fn fold_format_plans(runtime: &Runtime, program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let mut known = HashMap::new();
    for pc in 0..program.instructions.len() {
        if leaders.contains(&pc) { known.clear(); }
        let instruction = program.instructions[pc];
        let opcode = instruction.opcode().ok_or("未知操作码")?;
        let constant = match opcode {
            Opcode::Constant => match instruction.flags() {
                0 => program.constants.get(instruction.index() as usize).cloned(),
                1 => Some(Constant::Int(instruction.index() as i32)),
                2 => Some(Constant::Float(f32::from_bits(instruction.index()))),
                3 => match instruction.b() { 0 => Some(Constant::Unit), 1 => Some(Constant::None), 2 => Some(Constant::Bool(false)), _ => Some(Constant::Bool(true)) },
                _ => None,
            },
            Opcode::Move => known.get(&instruction.b()).cloned(),
            Opcode::LoadFixed => match runtime.values.get(u64::from(instruction.index())).as_deref() {
                Some(Value::String(text)) => Some(Constant::String(text.clone())),
                Some(Value::Enum { type_name, value }) => Some(Constant::Enum { name: type_name.clone(), value: *value }),
                _ => None,
            },
            _ => None,
        };
        if opcode == Opcode::Format {
            let plan = program.formats.get_mut(instruction.index() as usize).ok_or("格式计划越界")?;
            let mut merged = Vec::new();
            for part in std::mem::take(plan) {
                let part = match part {
                    FormatPart::Value(register) => {
                        let text = match known.get(&register) {
                            Some(Constant::String(text)) => Some(text.clone()),
                            Some(Constant::Int(value)) => Some(value.to_string()),
                            Some(Constant::Float(value)) => Some(value.to_string()),
                            Some(Constant::Bool(value)) => Some(value.to_string()),
                            Some(Constant::Enum { name, value }) => Some(runtime.contract.schema().resolve_enum(name).and_then(|meta| meta.variant_by_value.get(&i64::from(*value)).and_then(|index| meta.variants.get(*index))).map_or_else(|| value.to_string(), |variant| variant.name.to_string())),
                            _ => None,
                        };
                        text.map_or(FormatPart::Value(register), FormatPart::Text)
                    }
                    part => part,
                };
                if let (Some(FormatPart::Text(previous)), FormatPart::Text(text)) = (merged.last_mut(), &part) { previous.push_str(text); }
                else { merged.push(part); }
            }
            *plan = merged;
            let text = match plan.as_slice() { [] => Some(String::new()), [FormatPart::Text(text)] => Some(text.clone()), _ => None };
            if let Some(text) = text {
                let index = u32::try_from(program.constants.len()).map_err(|_| "常量区超限")?;
                program.constants.push(Constant::String(text));
                program.instructions[pc] = Instruction::indexed(Opcode::Constant, instruction.a(), index);
            }
        }
        for register in program.written_registers(instruction)? { known.remove(&register); }
        if let Some(constant) = constant { known.insert(instruction.a(), constant); }
    }
    // 静态片段不再占用寄存器；清除失去使用者的常量装载并统一重定位分支。
    program.build_liveness()?;
    let remove = program.instructions.iter().enumerate().map(|(pc, instruction)| {
        (matches!(instruction.opcode(), Some(Opcode::Constant | Opcode::Move))
            || (instruction.opcode() == Some(Opcode::LoadFixed)
                && !runtime.values.is_host(u64::from(instruction.index()))))
            && program.live.get(pc + 1).is_some_and(|live| !live.contains(&instruction.a()))
    }).collect::<Vec<_>>();
    compact_instructions(program, &remove)
}

fn fold_fixed_reads(runtime: &Runtime, program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};

    let leaders = program.block_leaders()?;
    let writes = program.instructions.iter().copied()
        .map(|instruction| program.written_registers(instruction))
        .collect::<Result<Vec<_>, _>>()?;

    let mut fixed = HashMap::<crate::vm::bytecode::Register, ValueId>::new();
    let mut keys = HashMap::<crate::vm::bytecode::Register, ScalarKey>::new();
    for (pc, instruction) in program.instructions.iter_mut().enumerate() {
        if leaders.contains(&pc) {
            fixed.clear();
            keys.clear();
        }
        let original = *instruction;
        let opcode = original.opcode().ok_or("未知操作码")?;
        let target = original.a();
        let key = match opcode {
            Opcode::Constant => match original.flags() {
                1 => Some(ScalarKey::Int(original.index() as i32)),
                3 if original.b() >= 2 => Some(ScalarKey::Bool(original.b() == 3)),
                0 => match program.constants.get(original.index() as usize) {
                    Some(Constant::Int(value)) => Some(ScalarKey::Int(*value)),
                    Some(Constant::Bool(value)) => Some(ScalarKey::Bool(*value)),
                    Some(Constant::String(value)) => Some(ScalarKey::String(value.clone())),
                    Some(Constant::Enum { name, value }) => Some(ScalarKey::Enum { type_name: name.clone(), value: *value }),
                    _ => None,
                }, _ => None,
            },
            Opcode::Move => keys.get(&original.b()).cloned(),
            _ => None,
        };
        let linked = match opcode {
            Opcode::LoadFixed => Some(u64::from(original.index())),
            Opcode::Move => fixed.get(&original.b()).copied(),
            Opcode::Field => fixed
                .get(&original.b())
                .copied()
                .and_then(|owner| runtime.fixed_field(owner, original.c())),
            Opcode::Index => {
                let (receiver, key) = if original.flags() == 1 {
                    let site = program.index_consts.get(original.index() as usize).ok_or("索引附表越界")?;
                    let key = match site.key { Constant::Int(value) => Some(ScalarKey::Int(value)), _ => None };
                    (site.receiver, key)
                } else { (original.b(), keys.get(&original.c()).cloned()) };
                fixed.get(&receiver).and_then(|id| runtime.values.view(*id)).and_then(|value| match (value, key.as_ref()) {
                    (Stored::Array(values), Some(ScalarKey::Int(index))) => usize::try_from(*index).ok().and_then(|index| values.get(index)),
                    (Stored::Dict(values), Some(key)) => values.get(key).map(|(_, value)| *value),
                    _ => None,
                })
            }
            _ => None,
        };
        for register in &writes[pc] {
            fixed.remove(register);
            keys.remove(register);
        }
        if let Some(key) = key { keys.insert(target, key); }
        if let Some(id) = linked {
            // Host 占位符的读取有可观察行为，不能传播为可重复使用的静态事实。
            if runtime.values.is_host(id) { continue; }
            fixed.insert(target, id);
            let key = match runtime.values.get(id).as_deref() {
                Some(Value::Int(value)) => Some(ScalarKey::Int(*value)),
                Some(Value::Bool(value)) => Some(ScalarKey::Bool(*value)),
                Some(Value::String(value)) => Some(ScalarKey::String(value.clone())),
                Some(Value::Enum { type_name, value }) => Some(ScalarKey::Enum { type_name: type_name.clone(), value: *value }),
                _ => None,
            };
            if let Some(key) = key { keys.insert(target, key); }
            *instruction = match runtime.values.get(id).as_deref() {
                Some(Value::None) => Instruction::new(Opcode::Constant, target, 1, 0, 3),
                Some(Value::Bool(false)) => Instruction::new(Opcode::Constant, target, 2, 0, 3),
                Some(Value::Bool(true)) => Instruction::new(Opcode::Constant, target, 3, 0, 3),
                Some(Value::Int(value)) => {
                    Instruction::indexed(Opcode::Constant, target, *value as u32)
                        .with_flags(1)
                }
                Some(Value::Float(value)) => {
                    Instruction::indexed(Opcode::Constant, target, value.to_bits())
                        .with_flags(2)
                }
                _ => Instruction::indexed(
                    Opcode::LoadFixed,
                    target,
                    u32::try_from(id).map_err(|_| "固定值槽超限")?,
                ),
            };
        }
    }
    program.build_liveness()
}

/// 计入值头、字符串容量、集合容量与捕获槽；共享程序归 Runtime 执行映像管理。
fn dynamic_bytes(value: &Value, binding: Option<&Binding>) -> usize {
    use std::mem::size_of;
    let payload = match value {
        Value::String(text) => text.capacity(),
        Value::Enum { type_name, .. } => type_name.capacity(),
        Value::Array(values) => values.heap_bytes(),
        Value::Dict(values) => {
            // 有序条目、哈希索引及归一化 key 的独立字符串均属于该字典。
            values.capacity() * (size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>() + 32)
                + values.keys().map(|key| match key {
                    ScalarKey::String(text) => text.capacity(),
                    ScalarKey::Enum { type_name, .. } => type_name.capacity(),
                    _ => 0,
                }).sum::<usize>()
        }
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
        Value::Function { source, host, .. } => {
            source.len()
                + host
                    .as_ref()
                    .map_or(0, |(service, field)| service.capacity() + field.capacity())
        }
        Value::Template { source, .. } => source.len(),
        Value::HostData { service, field, .. } => service.capacity() + field.capacity(),
        _ => 0,
    };
    size_of::<DynamicValue>()
        + size_of::<Value>()
        + 2 * size_of::<usize>()
        + payload
        + binding.map_or(0, |binding| binding.captures.len() * size_of::<Slot>())
}

#[cfg(test)]
mod control_flow_tests {
    use super::*;
    use crate::source::Span;
    use crate::vm::bytecode::{ForSite, Instruction as I, Opcode as O};

    #[test]
    fn failed_heap_allocation_does_not_publish_identity_or_accounting() {
        let mut heap = Heap { next: u64::MAX, ..Heap::default() };
        assert!(VmState::allocate_locked(&mut heap, Value::Int(1), None, usize::MAX).is_err());
        assert_eq!(heap.bytes, 0);
        assert_eq!(heap.next, u64::MAX);
        assert_eq!(heap.live_values, 0);
        assert!(heap.values.is_empty());
        assert!(heap.locations.is_empty());
        heap.next = 17;
        assert!(VmState::allocate_locked(&mut heap, Value::String("budget".into()), None, 0).is_err());
        assert_eq!(heap.next, 17);
        assert_eq!(heap.bytes, 0);
        assert!(heap.locations.is_empty());
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn regex_cache_obeys_shared_budget_and_releases_after_failure() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("regex"), "table Rule { value: int = 1; }")])).unwrap();
        let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
        builder.add_text("rule: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        {
            let host = runtime.execution_host(ExecutionLimits { max_heap_bytes: 1024, ..ExecutionLimits::default() }).unwrap();
            assert!(host.regex_match("a+", "aaa").is_err());
            assert!(runtime.vm.regexes.borrow().is_empty());
        }
        {
            let host = runtime.execution_host(ExecutionLimits::default()).unwrap();
            assert!(host.regex_match(r"^\p{Greek}+$", "αβγ").unwrap());
            assert!(!host.regex_match(r"^\p{Greek}+$", "abc").unwrap());
            assert!(host.regex_match("a+", "aaa").unwrap());
            assert!(host.regex_match("[", "a").is_err());
            assert!(host.regex_match("b+", "bbb").unwrap());
            assert_eq!(runtime.vm.regexes.borrow().len(), 3);
        }
        assert_eq!(runtime.vm.regexes.borrow().capacity(), 0);
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    #[ignore = "原生分配与 GC 测量，release 单测试线程运行"]
    fn runtime_memory_probe() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        use crate::allocation_probe;
        use std::time::Instant;
        let workloads = [
            ("transient_strings", "int", "var total: int = 0; for i in 0..20000 { total += i.string().len(); } total"),
            ("numeric_array", "[int]", "build [int] as b { for i in 0..20000 { b.append(i); } }"),
            ("optional_array", "[int?]", "build [int?] as b { for i in 0..20000 { if i % 2 == 0 { b.append(None); } else { b.append(i); } } }"),
            ("data_chain", "Node", "var node: Node = Node { value: 0 }; for i in 0..2000 { node = Node { value: i, next: node }; } node"),
            ("closure_escape", "fn() -> int", "var values: [int] = build [int] as b { for i in 0..2000 { b.append(i); } }; fn() -> int { values.sum() }"),
            ("dictionary", "{int: int}", "build {int: int} as b { for i in 0..2000 { b[i] = i; } }"),
            ("map_filter", "int", "var values: [int] = [0,1,2,3,4,5,6,7,8,9]; var total: int = 0; for i in 0..2000 { total += values.map(fn(x: int) -> int { x % 7 }).filter(fn(x: int) -> bool { x > 2 }).sum(); } total"),
            ("template", "int", "var total: int = 0; for i in 0..2000 { var value: fstring = f\"item-{i}\"; total += value.len(); } total"),
        ];
        for (workload, result_type, body) in workloads {
          for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let name = format!("{workload}/{profile:?}");
            let started = Instant::now();
            let ((contract, contract_bytes), compiled) = allocation_probe::measure(|| {
                let source = format!("data Node {{ value: int; next: Node?; }} table Rule {{ run: fn() -> {result_type} => {{ {body} }}; }}");
                let modules = parse_modules([CftFile::from_source(ModuleId::from("memory"), source)]);
                let contract = Arc::new(Contract::new(build_schema(&modules).unwrap()).unwrap());
                let contract_bytes = contract.to_bytes().unwrap().len();
                (contract, contract_bytes)
            });
            println!("compile workload={name},ns={},allocations={compiled:?},contract_bytes={contract_bytes}", started.elapsed().as_nanos());
            let started = Instant::now();
            let (runtime, linked) = allocation_probe::measure(|| { let mut builder = RuntimeBuilder::new(contract); builder.optimization_profile(profile); builder.add_text("rule: Rule {}", None); builder.build().runtime.unwrap() });
            println!("link workload={name},ns={},allocations={linked:?},fixed_regions={:?}", started.elapsed().as_nanos(), runtime.values.storage_sizes());
            let programs = &runtime.code().direct;
            println!("image workload={name},programs={},instruction_bytes={},source_map_bytes={},gc_map_bytes={},operand_bytes={}", programs.len(), programs.iter().map(|binding| binding.program.instructions.capacity()*size_of::<crate::vm::bytecode::Instruction>()).sum::<usize>(), programs.iter().map(|binding| binding.program.spans.storage_bytes()).sum::<usize>(), programs.iter().map(|binding| binding.program.live.capacity()*size_of::<Vec<u16>>() + binding.program.live.iter().map(|live| live.capacity()*size_of::<u16>()).sum::<usize>()).sum::<usize>(), programs.iter().map(|binding| binding.program.operands.capacity()*size_of::<u16>()).sum::<usize>());
            let function = runtime.field(runtime.record("Rule", "rule").unwrap(), "run").unwrap();
            let (result, executed) = allocation_probe::measure(|| runtime.invoke(function, &[], ExecutionLimits::default()).unwrap());
            println!("execute workload={name},allocations={executed:?},heap={:?}", runtime.vm.heap.borrow().metrics);
            if let HostValue::Existing { value, .. } = result { runtime.release_value(value).unwrap(); }
            runtime.collect().unwrap();
            println!("released workload={name},live_values={},retained_heap_budget_bytes={}", runtime.dynamic_value_count().unwrap(), runtime.vm.heap.borrow().total_bytes());
            assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
          }
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    #[ignore = "与 C# 快照探针使用相同 1000 条记录，测量原生存活和峰值"]
    fn snapshot_native_memory_probe() {
        use crate::allocation_probe;
        let source = (0..1000).map(|i| format!("h{i}: Hero {{ name: \"Hero {i}\", stats: Stats {{ health: {}, weights: [1, 2, 3] }} }}\n", i + 1)).collect::<String>() + "RuntimeSettings: RuntimeSettings {}";
        let (contract, loaded) = allocation_probe::measure(|| Arc::new(Contract::from_bytes(include_bytes!("../../../../tests/csharp-runtime-integration/generated/coflow.contract")).unwrap()));
        let (runtime, linked) = allocation_probe::measure(|| {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.add_text(&source, None);
            builder.build().runtime.unwrap()
        });
        assert_eq!(runtime.records("Character").unwrap().len(), 1000);
        println!("snapshot_native records=1000,contract={loaded:?},image_and_instance={linked:?},fixed_regions={:?}", runtime.values.storage_sizes());
        let (_, released) = allocation_probe::measure(|| { drop(runtime); drop(contract); });
        println!("snapshot_native released={released:?}");
        assert_eq!(loaded.bytes_current + linked.bytes_current + released.bytes_current, 0);
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn direct_calls_preserve_recursion_and_branch_reassigned_function_values() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("calls"), r#"
            const inc: fn(int) -> int = fn(x: int) -> int { x + 1 };
            const recur: fn(int) -> int = fn(n: int) -> int { if n == 0 { 0 } else { recur(n - 1) + 1 } };
            table Rule {
                run: fn(n: int) -> int => {
                    var selected: fn(int) -> int = inc;
                    if n > 0 { selected = fn(x: int) -> int { x + 10 }; }
                    selected(n) + inc(n) + recur(n)
                };
            }
        "#)])).unwrap();
        let contract = Arc::new(Contract::new(schema).unwrap());
        for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text("a: Rule {}", None);
            let runtime = builder.build().runtime.unwrap();
            assert!(runtime.code().direct.iter().any(|binding| binding.program.instructions.iter()
                .any(|instruction| instruction.opcode() == Some(O::CallDirect))));
            let function = runtime.field(runtime.record("Rule", "a").unwrap(), "run").unwrap();
            for (input, expected) in [(0, 2), (2, 17), (8, 35)] {
                assert!(matches!(runtime.invoke(function, &[HostValue::Int(input)], ExecutionLimits::default()).unwrap(),
                    HostValue::Int(value) if value == expected));
            }
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn call_windows_preserve_permuted_repeated_arguments_and_loop_targets() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("windows"), r#"
            const encode: fn(int, int, int) -> int = fn(a: int, b: int, c: int) -> int { a * 100 + b * 10 + c };
            table Rule {
                run: fn(a: int, b: int, c: int) -> int => {
                    var total: int = 0;
                    for index in 0..3 {
                        if index == 1 { continue; }
                        total += encode(c, b, a) + encode(a, a, c);
                    }
                    total
                };
            }
        "#)])).unwrap();
        let contract = Arc::new(Contract::new(schema).unwrap());
        for profile in [OptimizationProfile::Debug, OptimizationProfile::Release] {
            let mut builder = RuntimeBuilder::new(contract.clone());
            builder.optimization_profile(profile);
            builder.add_text("a: Rule {}", None);
            let runtime = builder.build().runtime.unwrap();
            let function = runtime.field(runtime.record("Rule", "a").unwrap(), "run").unwrap();
            assert!(matches!(runtime.invoke(function, &[HostValue::Int(1), HostValue::Int(2), HostValue::Int(3)], ExecutionLimits::default()).unwrap(),
                HostValue::Int(868)));
        }
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn fixed_field_and_collection_reads_do_not_borrow_dynamic_heap() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("fixed"),
            "table Rule { values: [int] = [4, 5]; mapping: {int: int} = {1: 7}; number: int = 9; }")])).unwrap();
        let mut builder = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap()));
        builder.add_text("a: Rule {}", None);
        let runtime = builder.build().runtime.unwrap();
        let record = runtime.record("Rule", "a").unwrap();
        let array = Slot::handle(runtime.field(record, "values").unwrap());
        let dictionary = Slot::handle(runtime.field(record, "mapping").unwrap());
        let host = runtime.execution_host(ExecutionLimits::default()).unwrap();
        // 故意占用动态堆写借用；任何固定读误入堆都会触发 RefCell 冲突。
        let _heap = runtime.vm.heap.borrow_mut();
        assert_eq!(host.field(Slot::handle(record), 3).unwrap(), Slot::Int(9));
        assert_eq!(host.length(array).unwrap(), 2);
        assert_eq!(host.index(array, Slot::Int(1)).unwrap(), Slot::Int(5));
        assert_eq!(host.iterator(array, 0).unwrap(), Slot::Int(4));
        assert_eq!(host.iter_next(array, 1).unwrap(), (Slot::Int(1), Slot::Int(5)));
        assert_eq!(host.length(dictionary).unwrap(), 1);
        assert_eq!(host.index(dictionary, Slot::Int(1)).unwrap(), Slot::Int(7));
        assert_eq!(host.iterator(dictionary, 0).unwrap(), Slot::Int(1));
        assert_eq!(host.iter_next(dictionary, 0).unwrap(), (Slot::Int(1), Slot::Int(7)));
    }

    #[cfg(feature = "cft-compiler")]
    #[test]
    fn pinned_early_value_does_not_accumulate_dead_heap_slots() {
        use crate::schema::{build_schema, parse_modules, CftFile, ModuleId};
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("gc"), "table Item {}")])).unwrap();
        let runtime = RuntimeBuilder::new(Arc::new(Contract::new(schema).unwrap())).build().runtime.unwrap();
        let allocate = || {
            let Slot::Handle(id) = VmState::allocate_locked(&mut runtime.vm.heap.borrow_mut(), Value::String("live".into()), None, 1024 * 1024).unwrap() else { panic!("heap handle"); };
        let id = id.get();
            id
        };
        let pinned = allocate();
        runtime.retain_value(pinned).unwrap();
        let stale = allocate();
        runtime.collect().unwrap();
        for _ in 0..5000 {
            let current = allocate();
            assert_ne!(current, stale);
            assert!(runtime.vm.value(stale).is_err());
            runtime.collect().unwrap();
        }
        assert!(runtime.vm.heap.borrow().values.len() <= 2);
        assert_eq!(runtime.dynamic_value_count().unwrap(), 1);
        runtime.release_value(pinned).unwrap();
        runtime.collect().unwrap();
        assert_eq!(runtime.dynamic_value_count().unwrap(), 0);
    }

    fn program(instructions: Vec<I>) -> Program {
        let mut p = Program::new("fusion".into(), String::new(), vec![CftValueType::Int; 2], CftValueType::Int);
        p.spans = vec![Span { start: 0, end: 0 }; instructions.len()];
        p.instructions = instructions;
        p.build_liveness().unwrap();
        p
    }

    #[test]
    fn scalar_folding_uses_inline_integer_payload_and_keeps_overflow() {
        let mut p = program(vec![
            I::indexed(O::Constant, 0, 10).with_flags(1),
            I::indexed(O::IntBinaryImmediate, 0, (-3i32) as u32),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        fold_scalar_control_flow(&mut p).unwrap();
        assert_eq!(p.instructions[1].opcode(), Some(O::Constant));
        assert_eq!(p.instructions[1].index(), 7);
        let mut p = program(vec![
            I::indexed(O::Constant, 0, i32::MAX as u32).with_flags(1),
            I::indexed(O::IntBinaryImmediate, 0, 1),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        fold_scalar_control_flow(&mut p).unwrap();
        assert_eq!(p.instructions[1].opcode(), Some(O::IntBinaryImmediate));
    }

    #[test]
    fn fusion_does_not_replace_a_constant_bypassed_by_another_predecessor() {
        let mut p = program(vec![
            I::indexed(O::Jump, 0, 2),
            I::indexed(O::Constant, 1, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 1, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        let original = p.instructions.clone();
        fuse_int_immediates(&mut p).unwrap();
        assert_eq!(p.instructions, original);
    }

    #[test]
    fn fusion_does_not_remove_the_current_value_assignment() {
        let mut p = program(vec![
            I::indexed(O::Constant, 0, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 0, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        let original = p.instructions.clone();
        fuse_int_immediates(&mut p).unwrap();
        assert_eq!(p.instructions, original);
    }

    #[test]
    fn compaction_relocates_loop_targets_without_rewriting_descriptor_ids() {
        let mut p = program(vec![
            I::indexed(O::Constant, 1, 7).with_flags(1),
            I::new(O::IntBinary, 0, 0, 1, 0),
            I::new(O::Return, 0, 0, 0, 0),
        ]);
        p.instructions.insert(0, I::indexed(O::ForPrep, 0, 0));
        p.spans.insert(0, Span { start: 0, end: 0 });
        p.for_sites.push(ForSite { limit: 1, target: 3, exclusive: true });
        fuse_int_immediates(&mut p).unwrap();
        p.validate().unwrap();
        assert_eq!(p.instructions.len(), 3);
        assert_eq!(p.instructions[0].index(), 0);
        assert_eq!(p.for_sites[0].target, 2);
        assert_eq!(p.instructions[1].opcode(), Some(O::IntBinaryImmediate));
    }
}
