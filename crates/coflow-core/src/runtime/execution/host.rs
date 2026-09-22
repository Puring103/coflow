//! VM 指令与 Runtime 值、构造器和 Host 服务之间的适配。
use super::*;
impl ExecutionHost for ExecutionContext<'_> {
    fn take_buffers(&self) -> executor::ExecutionBuffers {
        let buffers = std::mem::take(&mut *self.runtime.vm.buffers.borrow_mut());
        self.runtime.vm.heap.borrow_mut().buffer_bytes = 0;
        buffers
    }
    fn return_buffers(&self, mut buffers: executor::ExecutionBuffers) {
        // 同步重入拥有独立缓冲；空闲池只保留一个较大的窗口，不累积每层容量。
        let mut cached = self.runtime.vm.buffers.borrow_mut();
        if buffers.storage_bytes() > cached.storage_bytes() {
            buffers.published_roots = std::mem::take(&mut cached.published_roots);
            *cached = buffers;
        }
        self.runtime.vm.heap.borrow_mut().buffer_bytes = cached.storage_bytes();
    }

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
                    if let Some(index) = heap.index(id) {
                        if let Some(entry) = heap.remove_slot(index) {
                            heap.bytes -= dynamic_bytes(&entry.value, entry.callable.as_deref());
                            heap.live_values -= 1;
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
                            let Slot::Handle(source) = source else {
                                return Err(invalid("数组构造来源无效"));
                            };
                            let heap = self.runtime.vm.heap.borrow();
                            let Stored::Array(values) =
                                self.stored_value(Some(&heap), source.get())?
                            else {
                                return Err(invalid("数组构造来源无效"));
                            };
                            // 直接借用固定或动态负载，先预留预算再复制，避免物化和二次 clone。
                            let bytes = values
                                .len()
                                .checked_mul(result.element_bytes())
                                .ok_or_else(|| invalid("集合容量溢出"))?;
                            let memory =
                                self.budget.reserve_temporary(bytes, heap.total_bytes())?;
                            result
                                .reserve(values.len())
                                .map_err(|error| invalid(&error))?;
                            for value in values {
                                result.push(value).map_err(|error| invalid(&error))?;
                            }
                            drop(memory);
                        }
                        self.allocate(Value::Array(result))?
                    }
                    CftValueType::Dict(..) => {
                        let mut result = DictionaryValue::new();
                        if let Some(source) = source {
                            let Slot::Handle(source) = source else {
                                return Err(invalid("字典构造来源无效"));
                            };
                            let heap = self.runtime.vm.heap.borrow();
                            let Stored::Dict(values) =
                                self.stored_value(Some(&heap), source.get())?
                            else {
                                return Err(invalid("字典构造来源无效"));
                            };
                            let entry_bytes =
                                size_of::<ScalarKey>() + size_of::<(ValueId, ValueId)>() + 32;
                            let mut bytes = values
                                .len()
                                .checked_mul(entry_bytes)
                                .ok_or_else(|| invalid("集合容量溢出"))?;
                            for key in values.keys() {
                                let text = match key {
                                    ScalarKey::String(text) => text.len(),
                                    ScalarKey::Enum { type_name, .. } => type_name.len(),
                                    _ => 0,
                                };
                                bytes = bytes
                                    .checked_add(text)
                                    .ok_or_else(|| invalid("集合容量溢出"))?;
                            }
                            // 同时预留索引和所有字符串 key，固定字典无需先物化通用 Value。
                            let memory =
                                self.budget.reserve_temporary(bytes, heap.total_bytes())?;
                            result
                                .try_reserve(values.len())
                                .map_err(|_| invalid("字典缓冲分配失败"))?;
                            for (key, value) in values {
                                let copy_text = |source: &str| -> Result<String, ExecutionError> {
                                    let mut text = String::new();
                                    text.try_reserve_exact(source.len())
                                        .map_err(|_| invalid("字典键分配失败"))?;
                                    text.push_str(source);
                                    Ok(text)
                                };
                                let key = match key {
                                    ScalarKey::String(text) => ScalarKey::String(copy_text(text)?),
                                    ScalarKey::Enum { type_name, value } => ScalarKey::Enum {
                                        type_name: copy_text(type_name)?,
                                        value: *value,
                                    },
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
                if growth > self.budget.remaining_heap_bytes().saturating_sub(heap.total_bytes()) { return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory)); }
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
                let mut heap = self.runtime.vm.heap.borrow_mut();
                if !heap.builders.contains(&id) { return Err(invalid("构造能力已经消费")); }
                let spare = self.budget.remaining_heap_bytes().saturating_sub(heap.total_bytes());
                let entry = heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?;
                let before = dynamic_bytes(&entry.value, entry.callable.as_deref());
                if let Some(Value::Dict(values)) = Arc::get_mut(&mut entry.value) {
                    values.optimize_index(spare).map_err(|message| invalid(&message))?;
                }
                let after = dynamic_bytes(&entry.value, entry.callable.as_deref());
                heap.bytes = heap.bytes - before + after;
                heap.builders.remove(&id);
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
        if !self.runtime.check_reporter.messages.borrow().is_empty() {
            let mut location = self.runtime.check_reporter.location.borrow_mut();
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
            Stored::Dict(_) => {
                drop(heap);
                self.index_dict(receiver, key)
            }
            _ => Err(invalid("值不支持索引")),
        }
    }
    fn index_array(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = receiver else { return Err(invalid("索引需要集合或字符串")); };
        let id = id.get();
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());

        let Stored::Array(values) = self.stored_value(heap.as_deref(), id)? else {
            return Err(invalid("数组索引需要 array"));
        };
        let value = values.get_slot(index(key)?).ok_or_else(|| invalid("数组索引越界"))?;
        if let Some(slot) = self.read_array_slot(heap.as_deref(), value)? { Ok(slot) }
        else { let Slot::Handle(id) = value else { unreachable!() }; drop(heap); self.slot(id.get()) }
    }
    fn index_dict(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = receiver else { return Err(invalid("索引需要集合或字符串")); };
        let id = id.get();
        if id < self.runtime.values.len() {
            if let Some(value) = self.runtime.values.dictionary_index(id, key) {
                return self.slot(value.ok_or_else(|| invalid("字典 key 不存在"))?);
            }
        }
        // 键借用覆盖整个查询；动态字符串只借用内容，不复制 UTF-8 或枚举类型名。
        let owned;
        let scalar = if let Some(key) = self.runtime.values.key(key) { key } else {
            owned = self.value(key)?;
            use super::dictionary::KeyRef;
            match owned.as_ref() {
                Value::Bool(value) => KeyRef::Bool(*value), Value::Int(value) => KeyRef::Int(*value),
                Value::String(value) => KeyRef::String(value),
                Value::Enum { type_name, value } => KeyRef::Enum { type_name, value: *value },
                _ => return Err(invalid("无效的字典 key 类型")),
            }
        };
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());
        let Stored::Dict(values) = self.stored_value(heap.as_deref(), id)? else {
            return Err(invalid("字典索引需要 dict"));
        };
        let value = *values.get_ref(scalar).map(|(_, value)| value)
            .ok_or_else(|| invalid("字典 key 不存在"))?;
        if let Some(slot) = self.slot_in_heap(heap.as_deref(), value)? { Ok(slot) }
        else { drop(heap); self.slot(value) }
    }
    fn index_string(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError> {
        let Slot::Handle(id) = receiver else { return Err(invalid("索引需要集合或字符串")); };
        let id = id.get();
        let heap = (id >= self.runtime.values.len()).then(|| self.runtime.vm.heap.borrow());

        let Stored::String(value) = self.stored_value(heap.as_deref(), id)? else {
            return Err(invalid("字符串索引需要 string"));
        };
        let offset = index(key)?;
        self.budget.charge(offset as u64 + 1)?;
        let ch = value.chars().nth(offset).ok_or_else(|| invalid("字符串索引越界"))?;
        drop(heap);
        self.allocate(Value::String(ch.to_string()))
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
        let left_read = self.read_value(left)?;
        let right_read = self.read_value(right)?;
        Ok(match (left_read.view(), right_read.view()) {
            (Stored::Scalar(Slot::Int(a)), Stored::Scalar(Slot::Int(b))) => Some(a.cmp(&b)),
            (Stored::Scalar(Slot::Float(a)), Stored::Scalar(Slot::Float(b))) => a.partial_cmp(&b),
            (Stored::String(a), Stored::String(b)) => {
                self.budget.charge(a.len().min(b.len()) as u64)?;
                Some(a.cmp(b))
            }
            (Stored::Enum(a, av), Stored::Enum(b, bv)) if a == b => Some(av.cmp(&bv)),
            _ => return Err(invalid("值不支持顺序比较")),
        })
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
            let length = left
                .len()
                .checked_add(right.len())
                .ok_or_else(|| ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory))?;
            self.preflight_bytes(
                length.saturating_add(dynamic_bytes(&Value::String(String::new()), None)),
            )?;
            self.budget.charge(length as u64)?;
            let mut text = String::new();
            text.try_reserve_exact(length)
                .map_err(|_| invalid("文本分配失败"))?;
            text.push_str(left);
            text.push_str(right);
            text
        };
        self.allocate(Value::String(text))
    }
    fn accumulate_text(&self, left: Slot, right: Slot) -> Result<Slot, ExecutionError> {
        let (Slot::Handle(left), Slot::Handle(right)) = (left, right) else {
            return Err(invalid("文本累积需要 string"));
        };
        let (left, right) = (left.get(), right.get());
        // 固定映像字符串不可写；第一次追加创建动态缓冲，后续回边复用该缓冲。
        if left < self.runtime.values.len() || left == right {
            return self.concatenate(Slot::handle(left), Slot::handle(right));
        }
        let right_dynamic = if right >= self.runtime.values.len() {
            Some(
                self.runtime
                    .vm
                    .heap
                    .borrow()
                    .get(right)
                    .ok_or(ExecutionError::InvalidHandle)?
                    .value
                    .clone(),
            )
        } else {
            None
        };
        let right_text = if let Some(value) = right_dynamic.as_deref() {
            let Value::String(text) = value else {
                return Err(invalid("文本累积需要 string"));
            };
            text.as_str()
        } else {
            let Some(Stored::String(text)) = self.runtime.values.view(right) else {
                return Err(invalid("文本累积需要 string"));
            };
            text
        };
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let limit = self.budget.remaining_heap_bytes();
        let total = heap.total_bytes();
        let entry = heap.get_mut(left).ok_or(ExecutionError::InvalidHandle)?;
        let before = dynamic_bytes(&entry.value, entry.callable.as_deref());
        let value = Arc::get_mut(&mut entry.value)
            .ok_or_else(|| invalid("文本累积缓冲存在非法可写别名"))?;
        let Value::String(text) = value else {
            return Err(invalid("文本累积需要 string"));
        };
        let required = text
            .len()
            .checked_add(right_text.len())
            .ok_or_else(|| invalid("文本容量溢出"))?;
        if required > text.capacity() {
            let remaining = limit.saturating_sub(total);
            let old_capacity = text.capacity();
            let target = required.max(old_capacity.saturating_mul(2)).max(16);
            let additional = target.saturating_sub(old_capacity);
            if additional > remaining {
                return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
            }
            text.try_reserve_exact(additional)
                .map_err(|_| invalid("文本分配失败"))?;
            if text.capacity().saturating_sub(old_capacity) > remaining {
                let after = dynamic_bytes(&entry.value, entry.callable.as_deref());
                heap.bytes = heap.bytes - before + after;
                return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
            }
        }
        self.budget.charge(right_text.len() as u64)?;
        text.push_str(right_text);
        let after = dynamic_bytes(&entry.value, entry.callable.as_deref());
        heap.bytes = heap.bytes - before + after;
        Ok(Slot::handle(left))
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
                return Ok(self
                    .runtime
                    .contract
                    .schema()
                    .is_assignable(actual, type_name));
            }
        }
        Ok(
            matches!(self.value(value)?.as_ref(),Value::Object{type_name:actual,..}if self.runtime.contract.schema().is_assignable(actual,type_name)),
        )
    }
    fn direct_callable(&self, function: FunctionId) -> Result<&FunctionBinding, ExecutionError> {
        self.runtime
            .code()
            .direct
            .get(function.0 as usize)
            .ok_or_else(|| invalid("直接调用程序编号越界"))
    }
    fn callable(&self, value: Slot) -> Result<Callable<'_>, ExecutionError> {
        let Slot::Handle(id) = value else {
            return Err(invalid("需要函数"));
        };
        let id = id.get();
        if let Some(function) = self.runtime.code().functions.get(&id) {
            return Ok(Callable::Program(self.direct_callable(*function)?.into()));
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
            .map(|binding| Callable::Program(binding.into()))
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
        let mut payload_memory = self
            .budget
            .reserve_temporary(0, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut payload_bytes = 0usize;
        for value in args {
            let value = self.export_value(*value, false)?;
            let bytes = match &value {
                HostValue::String(text) => text.capacity(),
                HostValue::Enum { type_name, .. } => type_name.capacity(),
                _ => 0,
            };
            // 已导出的文本一直计费到同步 Host 回调及其重入全部结束。
            payload_bytes = payload_bytes
                .checked_add(bytes)
                .ok_or_else(|| invalid("Host 参数容量溢出"))?;
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
    fn closure(&self, binding: FunctionBinding, template: bool) -> Result<Slot, ExecutionError> {
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
        for value in values {
            stored.push(self.id(value)?);
        }
        let packed_memory = self.budget.reserve_temporary(
            ArrayValue::packing_bytes(&stored),
            self.runtime.vm.heap.borrow().total_bytes(),
        )?;
        let stored = ArrayValue::pack(stored).map_err(|error| invalid(&error))?;
        // 数值转换完成后转交连续缓冲，输入向量的临时预留不再占用预算。
        drop(memory);
        drop(packed_memory);
        self.allocate(Value::Array(stored))
    }
    fn dictionary(&self, values: Vec<(Slot, Slot)>) -> Result<Slot, ExecutionError> {
        // 键先归一化，再在插入时以 O(n) 检出重复键，替代旧的全量结构比较。
        let (mut keys, keys_memory) = self.reserve_values(values.len())?;
        let (mut key_guards, guards_memory) = self.reserve_values(values.len())?;
        for (key, _) in &values {
            let (key, memory) = self.temporary_key(*key)?;
            keys.push(key);
            key_guards.push(memory);
        }
        let entries_bytes = values
            .len()
            .checked_mul(size_of::<(ScalarKey, (ValueId, ValueId))>() + 32)
            .ok_or_else(|| ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory))?;
        let entries_memory = self
            .budget
            .reserve_temporary(entries_bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut entries = DictionaryValue::new();
        entries
            .try_reserve(values.len())
            .map_err(|_| invalid("字典分配失败"))?;
        for ((key, value), scalar) in values.into_iter().zip(keys) {
            if entries
                .insert(scalar, (self.id(key)?, self.id(value)?))
                .is_some()
            {
                return Err(invalid("字典 key 重复"));
            }
        }
        drop(keys_memory);
        drop(key_guards);
        drop(guards_memory);
        entries.optimize_index(self.budget.remaining_heap_bytes().saturating_sub(self.runtime.vm.heap.borrow().total_bytes())).map_err(|message| invalid(&message))?;
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
        if provided.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(invalid("data 字段重复"));
        }
        let (mut stored, stored_memory) = self.reserve_values(meta.all_fields().count())?;
        let text_bytes = meta
            .all_fields()
            .try_fold(
                type_name
                    .len()
                    .saturating_add(size_of::<Value>() + 2 * size_of::<usize>()),
                |bytes, field| bytes.checked_add(field.name.len()),
            )
            .ok_or_else(|| invalid("对象载荷容量溢出"))?;
        let text_memory = self
            .budget
            .reserve_temporary(text_bytes, self.runtime.vm.heap.borrow().total_bytes())?;
        let copy_reserved = |source: &str| -> Result<String, ExecutionError> {
            let mut text = String::new();
            text.try_reserve_exact(source.len())
                .map_err(|_| invalid("对象文本分配失败"))?;
            text.push_str(source);
            Ok(text)
        };
        for field in meta.all_fields() {
            let value = if let Ok(index) =
                provided.binary_search_by(|(name, _)| name.cmp(&field.name.as_str()))
            {
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
        drop(stored_memory);
        drop(text_memory);
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let entry = heap.get(id).ok_or(ExecutionError::InvalidHandle)?;
        let bytes = heap.bytes - dynamic_bytes(&entry.value, entry.callable.as_deref())
            + dynamic_bytes(&value, None);
        if bytes.saturating_add(heap.total_bytes().saturating_sub(heap.bytes))
            > self.budget.remaining_heap_bytes()
        {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        heap.get_mut(id).ok_or(ExecutionError::InvalidHandle)?.value = Arc::new(value);
        heap.bytes = bytes;
        Ok(target)
    }
    fn object(&self, type_name: &str, fields: Vec<(&str, Slot)>) -> Result<Slot, ExecutionError> {
        let target = self.reserve_object(type_name)?;
        self.initialize_object(target, type_name, fields)
    }
    fn template(&self, value: Slot) -> Result<Option<CallBinding<'_>>, ExecutionError> {
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
        let memory = self
            .budget
            .reserve_temporary(0, self.runtime.vm.heap.borrow().total_bytes())?;
        let mut output = FormatOutput {
            text: String::new(),
            budget: &self.budget,
            heap: &self.runtime.vm.heap,
            memory,
            error: None,
        };
        for part in parts {
            let written = match part {
                FormatPart::Text(text) => output.write_str(text),
                FormatPart::Value(register) => {
                    let slot = *values
                        .get(*register as usize)
                        .ok_or_else(|| invalid("格式输入越界"))?;
                    if let Slot::Handle(id) = slot {
                        if let Some(Stored::String(text)) = self.runtime.values.view(id.get()) {
                            if output.write_str(text).is_err() {
                                return Err(output
                                    .error
                                    .take()
                                    .unwrap_or_else(|| invalid("格式化失败")));
                            }
                            continue;
                        }
                    }
                    match self.value(slot)?.as_ref() {
                        Value::String(value) => output.write_str(value),
                        Value::Int(value) => write!(output, "{value}"),
                        Value::Float(value) => write!(output, "{value}"),
                        Value::Bool(value) => write!(output, "{value}"),
                        Value::Enum { type_name, value } => {
                            let variant = self
                                .runtime
                                .contract
                                .schema()
                                .resolve_enum(type_name)
                                .and_then(|meta| {
                                    meta.variant_by_value
                                        .get(&i64::from(*value))
                                        .and_then(|index| meta.variants.get(*index))
                                });
                            if let Some(variant) = variant {
                                output.write_str(&variant.name)
                            } else {
                                write!(output, "{value}")
                            }
                        }
                        _ => return Err(invalid("值不能转换为文本")),
                    }
                }
            };
            if written.is_err() {
                return Err(output.error.take().unwrap_or_else(|| invalid("格式化失败")));
            }
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
                let stored = values.get_slot(index).ok_or_else(|| invalid("索引越界"))?;
                if let Some(value) = self.read_array_slot(heap.as_deref(), stored)? {
                    Ok(value)
                } else {
                    let Slot::Handle(id) = stored else { unreachable!() };
                    drop(heap);
                    self.slot(id.get())
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
                let stored = values.get_slot(index).ok_or_else(|| invalid("索引越界"))?;
                let key = i32::try_from(index).map_err(|_| invalid("索引超出 int"))?;
                let value = if let Some(value) = self.read_array_slot(heap.as_deref(), stored)? {
                    value
                } else {
                    let Slot::Handle(id) = stored else { unreachable!() };
                    drop(heap);
                    return Ok((Slot::Int(key), self.slot(id.get())?));
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
    fn publish_roots(&self, roots: &mut Vec<Slot>) -> Result<(), ExecutionError> {
        // 发布整组根直接交换所有权，避免把草稿再次逐元素复制到 Heap。
        let mut heap = self.runtime.vm.heap.borrow_mut();
        let published = heap.roots.get_mut(&self.roots_id).ok_or_else(|| invalid("执行根集合不存在"))?;
        std::mem::swap(published, roots);
        Ok(())
    }
    fn roots(&self, roots: &[Slot]) -> Result<(), ExecutionError> {
        // 复用既有缓冲容量，避免每次根发布重新分配。
        let mut heap = self.runtime.vm.heap.borrow_mut();
        heap.reserve_roots(self.roots_id, roots.len(), self.budget.remaining_heap_bytes())?;
        let entry = heap.roots.get_mut(&self.roots_id).expect("执行根已预留");
        entry.clear();
        entry.extend_from_slice(roots);
        Ok(())
    }
}
