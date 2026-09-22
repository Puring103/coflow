//! 显式调用帧解释器。语言递归不会递归 Rust 调用栈，Host 边界只传稳定值。
//!
//! 性能契约：普通寄存器操作（Move、算术、跳转、标量比较）不触碰 Host 与堆，
//! 只有昂贵指令（循环回边、调用、分配、文本操作）才计预算与发布 GC 根。
use super::{bytecode::*, ExecutionError};
use std::{cmp::Ordering, sync::Arc};
use super::{error, scalar::{integer, boolean, int_binary, float_binary}};
pub(crate) use super::{slot::Slot, budget::{Budget, TemporaryBytes}};
pub use super::budget::ExecutionLimits;
pub(crate) use super::scalar::scalar_binary;

const ITERATION_BUDGET_BATCH: u64 = 256;

#[derive(Debug, Clone)]
pub(crate) struct FunctionBinding<P = super::image::ValidatedProgram> {
    pub program: Arc<P>,
    pub owner: Slot,
    /// 动态绑定整体共享，捕获缓冲直接转移所有权；空捕获不分配引用计数头。
    pub captures: Box<[Slot]>,
}
/// 映像绑定借用不可变程序表；只有动态闭包携带独立的捕获所有权。
#[derive(Debug, Clone)]
pub(crate) enum CallBinding<'a> {
    Fixed(&'a FunctionBinding),
    Closure(Arc<FunctionBinding>),
}
impl std::ops::Deref for CallBinding<'_> {
    type Target = FunctionBinding;
    fn deref(&self) -> &FunctionBinding { match self { Self::Fixed(value) => value, Self::Closure(value) => value } }
}
impl<'a> From<&'a FunctionBinding> for CallBinding<'a> {
    fn from(value: &'a FunctionBinding) -> Self { Self::Fixed(value) }
}
impl From<Arc<FunctionBinding>> for CallBinding<'_> {
    fn from(value: Arc<FunctionBinding>) -> Self { Self::Closure(value) }
}
enum ActiveProgram<'a> {
    Fixed(&'a super::image::ValidatedProgram),
    Closure(Arc<super::image::ValidatedProgram>),
}
impl std::ops::Deref for ActiveProgram<'_> {
    type Target = super::image::ValidatedProgram;
    fn deref(&self) -> &Self::Target { match self { Self::Fixed(value) => value, Self::Closure(value) => value } }
}
#[derive(Debug, Clone)]
pub(crate) enum Callable<'a> {
    Program(CallBinding<'a>),
    Host(Slot),
}

/// 值存储和宿主适配由 Runtime 提供；解释器只负责指令、调用和错误栈。
#[derive(Debug, Default)]
pub(crate) struct ExecutionBuffers {
    registers: Vec<Slot>,
    roots: Vec<Slot>,
    operands: Vec<Slot>,
    frames: Vec<Frame<'static>>,
    pub(crate) published_roots: Vec<Slot>,
}
impl ExecutionBuffers {
    pub(crate) fn storage_bytes(&self) -> usize {
        (self.registers.capacity() + self.roots.capacity() + self.operands.capacity() + self.published_roots.capacity()) * size_of::<Slot>()
            + self.frames.capacity() * size_of::<Frame<'_>>()
    }
}
pub(crate) trait ExecutionHost {
    fn take_buffers(&self) -> ExecutionBuffers { ExecutionBuffers::default() }
    fn return_buffers(&self, _buffers: ExecutionBuffers) {}
    /// 测试观测不进入生产派发循环。
    #[cfg(test)]
    fn observe_instruction(&self, _opcode: Opcode) {}

    fn heap_bytes(&self) -> usize {
        0
    }
    /// 仅在下一次分配可能触发 GC 时请求执行器发布寄存器根。
    fn needs_roots(&self) -> bool {
        true
    }
    fn location(&self, _program: &Program, _span: crate::source::Span) {}
    fn constant(&self, value: &Constant) -> Result<Slot, ExecutionError>;
    fn field(&self, receiver: Slot, slot: u16) -> Result<Slot, ExecutionError>;
    fn index(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError>;
    fn index_array(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError>;
    fn index_dict(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError>;
    fn index_string(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError>;
    fn reference(&self, name: &str) -> Result<Slot, ExecutionError>;
    fn fixed(&self, index: u32) -> Result<Slot, ExecutionError> {
        Ok(Slot::handle(u64::from(index)))
    }
    fn equals(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError>;
    fn compare(&self, left: Slot, right: Slot) -> Result<Option<Ordering>, ExecutionError>;
    fn concatenate(&self, left: Slot, right: Slot) -> Result<Slot, ExecutionError>;
    fn accumulate_text(&self, left: Slot, right: Slot) -> Result<Slot, ExecutionError>;
    fn enum_unary(&self, value: Slot) -> Result<Slot, ExecutionError>;
    fn enum_binary(&self, operator: u8, left: Slot, right: Slot) -> Result<Slot, ExecutionError>;
    fn is_type(&self, value: Slot, type_name: &str) -> Result<bool, ExecutionError>;
    fn callable(&self, value: Slot) -> Result<Callable<'_>, ExecutionError>;
    fn direct_callable(&self, _function: FunctionId) -> Result<&FunctionBinding, ExecutionError> {
        Err(error("执行宿主未提供映像程序区"))
    }
    fn call_host(&self, target: Slot, args: &[Slot]) -> Result<Slot, ExecutionError>;
    fn closure(&self, binding: FunctionBinding, template: bool) -> Result<Slot, ExecutionError>;
    fn array(&self, values: Vec<Slot>) -> Result<Slot, ExecutionError>;
    fn build(
        &self,
        _operation: &super::construction::BuildOp<Slot>,
        _ty: &crate::schema::CftValueType,
    ) -> Result<Slot, ExecutionError> {
        Err(error("执行宿主不支持局部构造"))
    }
    fn dictionary(&self, values: Vec<(Slot, Slot)>) -> Result<Slot, ExecutionError>;
    fn reserve_object(&self, type_name: &str) -> Result<Slot, ExecutionError>;
    fn initialize_object(
        &self,
        target: Slot,
        type_name: &str,
        fields: Vec<(&str, Slot)>,
    ) -> Result<Slot, ExecutionError>;
    fn object(&self, type_name: &str, fields: Vec<(&str, Slot)>) -> Result<Slot, ExecutionError>;
    fn template(&self, value: Slot) -> Result<Option<CallBinding<'_>>, ExecutionError>;
    fn format(&self, parts: &[FormatPart], values: &[Slot]) -> Result<Slot, ExecutionError>;
    fn length(&self, value: Slot) -> Result<usize, ExecutionError>;
    /// 单绑定迭代：按索引读一个值（数组/字典）。
    fn iterator(&self, value: Slot, index: usize) -> Result<Slot, ExecutionError>;
    /// 双绑定迭代融合：一次调用同时产出键与值，减少重复堆借用和适配。
    fn iter_next(&self, value: Slot, index: usize) -> Result<(Slot, Slot), ExecutionError>;
    fn builtin(
        &self,
        name: &str,
        receiver: Slot,
        args: &[Slot],
        result_type: &crate::schema::CftValueType,
    ) -> Result<Slot, ExecutionError>;
    /// 分配和调用前公布所有活跃根，Host 重入和回收可据此保留暂停的外层值。
    fn roots(&self, roots: &[Slot]) -> Result<(), ExecutionError>;
    fn publish_roots(&self, roots: &mut Vec<Slot>) -> Result<(), ExecutionError> { self.roots(roots) }
}
#[derive(Debug)]
struct Frame<'a> {
    binding: CallBinding<'a>,
    pc: usize,
    base: usize,
    destination: Option<usize>,
    builders: Option<Box<FrameBuilders>>,
}

/// 空帧缓冲只保存容量，借用程序及捕获在 clear 时归还。
/// Vec 的同布局迭代收集复用分配，不延长任何活动绑定的生命周期，也无需 unsafe。
fn recycle_frames<'a, 'b>(mut frames: Vec<Frame<'a>>) -> Vec<Frame<'b>> {
    frames.clear();
    frames.into_iter().map(|_| unreachable!("空闲帧不含活动绑定")).collect()
}

/// 普通标量调用无需构造器状态；仅首次 Start 分配，并将头部和根容量一起计费。
#[derive(Debug)]
struct FrameBuilders {
    values: Vec<Slot>,
    memory: TemporaryBytes,
}

pub(crate) fn execute<'a, H: ExecutionHost>(
    host: &'a H,
    binding: impl Into<CallBinding<'a>>,
    arguments: &[Slot],
    budget: Budget,
) -> Result<Slot, ExecutionError> {
    let binding = binding.into();
    let mut buffers = host.take_buffers();
    // 空闲容量按本次入口窗口裁剪；不让上次大调用挤占本次帧和根的配额。
    buffers.registers.shrink_to(binding.program.registers.len().max(4));
    buffers.roots.shrink_to((binding.program.registers.len() + binding.captures.len() + 1).max(4));
    buffers.operands.shrink_to(0);
    buffers.frames.shrink_to(4);
    // 缓存容量不改变小预算请求的可执行性；容量不足时按本次需求重新增长。
    if buffers.storage_bytes() > budget.remaining_heap_bytes().saturating_sub(host.heap_bytes()) { buffers = ExecutionBuffers::default(); }
    let buffers_memory = budget.reserve_temporary(buffers.storage_bytes(), host.heap_bytes())?;
    let mut execution = Execution {
        host,
        buffers_memory,
        budget,
        registers: buffers.registers,
        frames: recycle_frames(buffers.frames),
        roots_scratch: buffers.roots,
        operands_scratch: buffers.operands,
        iteration_debt: 0,
    };
    execution.push(binding, arguments, None)?;
    execution.publish_roots_force()?;
    let result = execution.run();
    result.map_err(|cause| {
        let stack = execution
            .frames
            .iter()
            .rev()
            .map(|frame| {
                let span = frame
                    .binding
                    .program
                    .spans
                    .get(frame.pc.saturating_sub(1))
                    .copied()
                    .unwrap_or_default();
                format!(
                    "{}:{}:{}..{}",
                    frame.binding.program.path.as_deref().unwrap_or("<memory>"),
                    frame.binding.program.name,
                    span.start,
                    span.end
                )
            })
            .collect::<Vec<_>>();
        if let Some(frame) = execution.frames.last() {
            let span = frame
                .binding
                .program
                .spans
                .get(frame.pc.saturating_sub(1))
                .copied()
                .unwrap_or_default();
            ExecutionError::Fault {
                cause: Box::new(cause),
                path: frame.binding.program.path.clone(),
                module: frame.binding.program.module.clone(),
                function: frame.binding.program.name.clone(),
                span,
                stack,
            }
        } else {
            cause
        }
    })
}
struct Execution<'a, H: ExecutionHost> {
    host: &'a H,
    budget: Budget,
    buffers_memory: TemporaryBytes,
    registers: Vec<Slot>,
    frames: Vec<Frame<'a>>,
    /// 根发布复用的草稿缓冲，避免每次发布重新分配。
    roots_scratch: Vec<Slot>,
    /// 内建操作数复用缓冲；函数调用直接使用已验证的连续参数窗口。
    operands_scratch: Vec<Slot>,
    /// 待扣减的循环迭代数；按语言级批次结算，不与字节码条数绑定。
    iteration_debt: u64,
}
impl<H: ExecutionHost> Drop for Execution<'_, H> {
    fn drop(&mut self) {
        for frame in &self.frames {
            for builder in frame.builders.iter().flat_map(|builders| &builders.values) {
                let _ = self.host.build(
                    &super::construction::BuildOp::Drop { builder: *builder },
                    &crate::schema::CftValueType::Unit,
                );
            }
            self.budget.leave(frame.binding.program.registers.len());
        }
        let _ = self.host.roots(&[]);
        self.registers.clear(); self.roots_scratch.clear(); self.operands_scratch.clear();
        self.host.return_buffers(ExecutionBuffers {
            registers: std::mem::take(&mut self.registers), roots: std::mem::take(&mut self.roots_scratch),
            operands: std::mem::take(&mut self.operands_scratch),
            frames: recycle_frames(std::mem::take(&mut self.frames)),
            published_roots: Vec::new(),
        });
    }
}
impl<'a, H: ExecutionHost> Execution<'a, H> {
    fn reserve_buffers(
        &mut self,
        registers: usize,
        frames: usize,
        roots: usize,
        operands: usize,
    ) -> Result<(), ExecutionError> {
        // 已分配容量仍由 buffers_memory 持续计费；复用时无需重新预留或读取堆占用。
        if registers <= self.registers.capacity()
            && frames <= self.frames.capacity()
            && roots <= self.roots_scratch.capacity()
            && operands <= self.operands_scratch.capacity()
        {
            return Ok(());
        }
        let capacity = |old: usize, required: usize| {
            if required <= old {
                old
            } else {
                old.saturating_mul(2).max(required).max(4)
            }
        };
        let registers = capacity(self.registers.capacity(), registers);
        let frames = capacity(self.frames.capacity(), frames);
        let roots = capacity(self.roots_scratch.capacity(), roots);
        let operands = capacity(self.operands_scratch.capacity(), operands);
        let bytes = registers
            .checked_add(roots)
            .and_then(|n| n.checked_add(operands))
            .and_then(|n| n.checked_mul(size_of::<Slot>()))
            .and_then(|n| {
                frames
                    .checked_mul(size_of::<Frame<'_>>())
                    .and_then(|frames| n.checked_add(frames))
            })
            .ok_or_else(|| error("执行缓冲容量溢出"))?;
        // 所有执行缓冲容量统一计入共享预算，暂停帧与重入执行器的容量同时有效。
        self.buffers_memory.resize(bytes, self.host.heap_bytes())?;
        self.registers
            .try_reserve_exact(registers - self.registers.len())
            .map_err(|_| error("寄存器分配失败"))?;
        self.frames
            .try_reserve_exact(frames - self.frames.len())
            .map_err(|_| error("调用帧分配失败"))?;
        self.roots_scratch
            .try_reserve_exact(roots - self.roots_scratch.len())
            .map_err(|_| error("根缓冲分配失败"))?;
        self.operands_scratch
            .try_reserve_exact(operands - self.operands_scratch.len())
            .map_err(|_| error("操作数缓冲分配失败"))?;
        Ok(())
    }
    fn reserve_frame(&mut self, registers: usize) -> Result<(), ExecutionError> {
        self.budget.enter(registers)?;
        // 预算先于实际分配；分配失败时尚未发布帧，必须在这里退回预算。
        let required = self.registers.len().saturating_add(registers);
        if let Err(cause) = self.reserve_buffers(
            required,
            self.frames.len().saturating_add(1),
            self.roots_scratch.len(),
            self.operands_scratch.len(),
        ) {
            self.budget.leave(registers);
            return Err(cause);
        }
        Ok(())
    }
    fn push(
        &mut self,
        binding: CallBinding<'a>,
        arguments: &[Slot],
        destination: Option<usize>,
    ) -> Result<(), ExecutionError> {
        if arguments.len() != binding.program.parameters.len()
            || arguments.len() > binding.program.registers.len()
            || binding.captures.len() != binding.program.captures.len()
        {
            return Err(error("调用签名或捕获布局不匹配"));
        }
        self.reserve_frame(binding.program.registers.len())?;
        let base = self.registers.len();
        self.registers
            .resize(base + binding.program.registers.len(), Slot::Empty);
        self.registers[base..base + arguments.len()].copy_from_slice(arguments);
        self.frames.push(Frame {
            binding,
            pc: 0,
            base,
            destination,
            builders: None,
        });
        Ok(())
    }
    fn get(&self, base: usize, register: Register) -> Result<Slot, ExecutionError> {
        match self.registers.get(base + usize::from(register)).copied() {
            Some(Slot::Empty) | None => Err(error("读取未初始化或越界寄存器")),
            Some(value) => Ok(value),
        }
    }
    fn push_window(
        &mut self,
        binding: CallBinding<'a>,
        start: usize,
        count: usize,
        destination: usize,
    ) -> Result<(), ExecutionError> {
        if count != binding.program.parameters.len()
            || binding.captures.len() != binding.program.captures.len()
        {
            return Err(error("调用签名或捕获布局不匹配"));
        }
        let frame = self.frames.last().ok_or_else(|| error("调用栈为空"))?;
        let tail = frame.builders.as_ref().is_none_or(|builders| builders.values.is_empty())
            && frame
                .binding
                .program
                .is_tail_call(frame.pc.saturating_sub(1));
        if tail {
            let base = frame.base;
            let old = frame.binding.program.registers.len();
            let new = binding.program.registers.len();
            let remaining = self.budget.replacement_capacity(old);
            if new > remaining {
                return Err(ExecutionError::LimitExceeded(super::LimitKind::Registers));
            }
            if new > old {
                self.reserve_buffers(
                    base.saturating_add(new),
                    self.frames.len(),
                    self.roots_scratch.len(),
                    self.operands_scratch.len(),
                )?;
            }
            // 参数窗口仍属于旧帧，先完成重叠复制再缩短缓冲；不保留扩容前的引用。
            self.registers.copy_within(start..start + count, base);
            self.registers.resize(base + new, Slot::Empty);
            self.registers[base + count..base + new].fill(Slot::Empty);
            self.budget.replace_registers(old, new);
            let frame = self.frames.last_mut().ok_or_else(|| error("调用栈为空"))?;
            frame.binding = binding;
            frame.pc = 0;
            return Ok(());
        }
        self.reserve_frame(binding.program.registers.len())?;
        let base = self.registers.len();
        self.registers
            .resize(base + binding.program.registers.len(), Slot::Empty);
        // 源窗口属于暂停的调用帧；扩容后使用索引重新定位，不持有可能失效的指针。
        self.registers.copy_within(start..start + count, base);
        self.frames.push(Frame {
            binding,
            pc: 0,
            base,
            destination: Some(destination),
            builders: None,
        });
        Ok(())
    }
    fn set(&mut self, index: usize, value: Slot) -> Result<(), ExecutionError> {
        let slot = self
            .registers
            .get_mut(index)
            .ok_or_else(|| error("写入越界寄存器"))?;
        *slot = value;
        Ok(())
    }
    fn values(
        &self,
        base: usize,
        registers: &[Register],
    ) -> Result<(Vec<Slot>, TemporaryBytes), ExecutionError> {
        let bytes = registers
            .len()
            .checked_mul(size_of::<Slot>())
            .ok_or_else(|| error("操作数缓冲超限"))?;
        let memory = self
            .budget
            .reserve_temporary(bytes, self.host.heap_bytes())?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(registers.len())
            .map_err(|_| error("操作数缓冲分配失败"))?;
        for register in registers {
            values.push(self.get(base, *register)?);
        }
        Ok((values, memory))
    }
    fn gather_operands(
        &mut self,
        base: usize,
        registers: &[Register],
    ) -> Result<(), ExecutionError> {
        self.operands_scratch.clear();
        self.reserve_buffers(
            self.registers.len(),
            self.frames.len(),
            self.roots_scratch.len(),
            registers.len(),
        )?;
        for register in registers {
            let value = self.get(base, *register)?;
            self.operands_scratch.push(value);
        }
        Ok(())
    }
    fn flush_iterations(&mut self) -> Result<(), ExecutionError> {
        if self.iteration_debt != 0 {
            let batch = std::mem::take(&mut self.iteration_debt);
            self.budget.iteration_batch(batch)?;
        }
        Ok(())
    }
    /// 固定调用直接借用映像程序；动态闭包持有程序 Arc 以跨帧变更保活。
    fn program(&self) -> Result<ActiveProgram<'a>, ExecutionError> {
        let frame = self.frames.last().ok_or_else(|| error("调用栈为空"))?;
        Ok(match &frame.binding {
            CallBinding::Fixed(binding) => ActiveProgram::Fixed(&binding.program),
            CallBinding::Closure(binding) => ActiveProgram::Closure(binding.program.clone()),
        })
    }
    /// 调用 Host 前发布活跃句柄，并丢弃上一安全点之后的临时分配根。
    fn publish_roots(&mut self) -> Result<(), ExecutionError> {
        if !self.host.needs_roots() {
            return Ok(());
        }
        self.publish_roots_force()
    }
    fn publish_roots_force(&mut self) -> Result<(), ExecutionError> {
        let required = self
            .frames
            .iter()
            .try_fold(self.registers.len(), |n, frame| {
                n.checked_add(frame.binding.captures.len())
                    .and_then(|n| n.checked_add(1))
            })
            .ok_or_else(|| error("根缓冲容量溢出"))?;
        self.reserve_buffers(
            self.registers.len(),
            self.frames.len(),
            required,
            self.operands_scratch.len(),
        )?;
        self.roots_scratch.clear();
        for frame in &self.frames {
            let live = frame
                .binding
                .program
                .live
                .get(frame.pc.saturating_sub(1))
                .ok_or_else(|| error("缺少寄存器存活信息"))?;
            for register in live {
                if let value @ Slot::Handle(_) = self.get(frame.base, *register)? {
                    self.roots_scratch.push(value);
                }
            }
            self.roots_scratch.push(frame.binding.owner);
            self.roots_scratch
                .extend(frame.binding.captures.iter().copied());
        }
        // 寄存器根相同不代表宿主根相同：分配器会追加临时根，必须在安全点替换。
        self.host.publish_roots(&mut self.roots_scratch)?;
        self.roots_scratch.clear();
        // 交换只转移容量归属；总占用不变，更新临时预算中的缓冲份额。
        let bytes = (self.registers.capacity() + self.roots_scratch.capacity() + self.operands_scratch.capacity()) * size_of::<Slot>()
            + self.frames.capacity() * size_of::<Frame<'_>>();
        self.buffers_memory.resize(bytes, self.host.heap_bytes())?;
        Ok(())
    }
    /// 帧切换留在同一执行循环；局部 PC 在可观察边界或失败时写回。
    fn run(&mut self) -> Result<Slot, ExecutionError> {
        let mut pc = 0;
        let mut dirty_pc = false;
        let result = (|| {
            'frames: loop {
                let active_program = self.program()?;
                let frame = self.frames.last().ok_or_else(|| error("调用栈为空"))?;
                let (base, owner) = (frame.base, frame.binding.owner);
                pc = frame.pc;
                loop {
                    let instruction = active_program
                        .instructions
                        .get(pc)
                        .copied()
                        .ok_or_else(|| error("执行越过函数出口"))?;
                    pc += 1;
                    dirty_pc = true;
                    let opcode = instruction.opcode().ok_or_else(|| error("未知操作码"))?;
                    // 只有确定不调用 Host、不分配的操作可延迟同步；故障出口统一补写当前位置。
                    let scalar = matches!(
                        opcode,
                        Opcode::Move
                            | Opcode::SelfValue
                            | Opcode::Capture
                            | Opcode::IntBinary
                            | Opcode::IntBinaryImmediate
                            | Opcode::FloatBinary
                            | Opcode::ConvertFloat
                            | Opcode::IsSome
                            | Opcode::Jump
                            | Opcode::JumpFalse
                            | Opcode::ForPrep
                            | Opcode::ForLoop
                            | Opcode::Iteration
                    ) || (opcode == Opcode::Constant && instruction.flags() != 0);
                    if !scalar {
                        self.frames
                            .last_mut()
                            .ok_or_else(|| error("调用栈为空"))?
                            .pc = pc;
                        dirty_pc = false;
                    }
                    #[cfg(test)]
                    self.host.observe_instruction(opcode);
                    let destination = base + usize::from(instruction.a());
                    let index = instruction.index() as usize;

                    let value = match opcode {
                        Opcode::Build => {
                            self.publish_roots()?;
                            let program = &active_program;
                            let operation = program.builders[index]
                                .map(|r| self.get(base, r).map_err(|e| e.to_string()))
                                .map_err(|message| error(&message))?;
                            use super::construction::BuildOp as B;
                            if matches!(operation, B::Start { .. }) {
                                let frame = self
                                    .frames
                                    .last_mut()
                                    .ok_or_else(|| error("构造操作缺少调用帧"))?;
                                if frame.builders.is_none() {
                                    let memory = self.budget.reserve_temporary(size_of::<FrameBuilders>(), self.host.heap_bytes())?;
                                    frame.builders = Some(Box::new(FrameBuilders { values: Vec::new(), memory }));
                                }
                                let builders = frame.builders.as_mut().expect("构造状态已建立");
                                if builders.values.len() == builders.values.capacity() {
                                    let capacity = builders.values.capacity().saturating_mul(2).max(4);
                                    builders.memory.resize(
                                        capacity.checked_mul(size_of::<Slot>()).and_then(|bytes| bytes.checked_add(size_of::<FrameBuilders>()))
                                            .ok_or_else(|| error("构造根容量溢出"))?,
                                        self.host.heap_bytes(),
                                    )?;
                                    builders.values.try_reserve_exact(capacity - builders.values.len())
                                        .map_err(|_| error("构造根分配失败"))?;
                                }
                            }
                            let result = self
                                .host
                                .build(&operation, &program.registers[instruction.a() as usize])?;
                            let frame = self
                                .frames
                                .last_mut()
                                .ok_or_else(|| error("构造操作缺少调用帧"))?;
                            match operation {
                                B::Start { .. } => frame.builders.as_mut().expect("构造状态已建立").values.push(result),
                                B::Freeze { builder } | B::Drop { builder } => {
                                    if let Some(builders) = &mut frame.builders { builders.values.retain(|value| *value != builder); }
                                }
                                _ => {}
                            }
                            result
                        }
                        Opcode::Iteration => {
                            // 循环回边按语言级批次计费，允许有限超额但保持终止性。
                            self.iteration_debt += 1;
                            if self.iteration_debt >= ITERATION_BUDGET_BATCH {
                                let batch = self.iteration_debt;
                                self.iteration_debt = 0;
                                self.budget.iteration_batch(batch)?;
                            }
                            continue;
                        }
                        Opcode::Constant => {
                            // 内联常量（int/float/bool/None/Unit）直接装寄存器；
                            // 只有查表常量（string/enum）才分配并发布根。
                            match instruction.flags() {
                                0 => {
                                    self.publish_roots()?;
                                    let program = &active_program;
                                    self.host.constant(
                                        program
                                            .constants
                                            .get(index)
                                            .ok_or_else(|| error("常量索引越界"))?,
                                    )?
                                }
                                1 => Slot::Int(instruction.index() as i32),
                                2 => Slot::Float(f32::from_bits(instruction.index())),
                                3 => match instruction.b() {
                                    0 => Slot::Unit,
                                    1 => Slot::None,
                                    2 => Slot::Bool(false),
                                    _ => Slot::Bool(true),
                                },
                                _ => return Err(error("无效的常量标志")),
                            }
                        }
                        Opcode::Move => self.get(base, instruction.b())?,
                        Opcode::SelfValue => owner,
                        Opcode::Capture => {
                            let captures = &self
                                .frames
                                .last()
                                .ok_or_else(|| error("调用栈为空"))?
                                .binding
                                .captures;
                            *captures.get(index).ok_or_else(|| error("捕获索引越界"))?
                        }
                        Opcode::Field => self
                            .host
                            .field(self.get(base, instruction.b())?, instruction.c())?,
                        Opcode::Index
                        | Opcode::IndexArray
                        | Opcode::IndexDict
                        | Opcode::IndexString => {
                            // 数组和字典索引只返回既有槽，不创建身份；字符串索引需要物化单字符值。
                            if matches!(opcode, Opcode::Index | Opcode::IndexString) {
                                self.publish_roots()?;
                            }
                            // flags=1：键与 receiver 都在附表；否则寄存器形式。
                            let (receiver, key) = if instruction.flags() == 1 {
                                let program = &active_program;
                                let site = program
                                    .index_consts
                                    .get(index)
                                    .ok_or_else(|| error("索引附表越界"))?;
                                let Constant::Int(key) = &site.key else {
                                    return Err(error("内联索引键不是 int"));
                                };
                                (self.get(base, site.receiver)?, Slot::Int(*key))
                            } else {
                                (
                                    self.get(base, instruction.b())?,
                                    self.get(base, instruction.c())?,
                                )
                            };
                            match opcode {
                                Opcode::IndexArray => self.host.index_array(receiver, key)?,
                                Opcode::IndexDict => self.host.index_dict(receiver, key)?,
                                Opcode::IndexString => self.host.index_string(receiver, key)?,
                                _ => self.host.index(receiver, key)?,
                            }
                        }
                        Opcode::Reference => {
                            let program = &active_program;
                            self.host.reference(
                                program
                                    .names
                                    .get(index)
                                    .ok_or_else(|| error("引用索引越界"))?,
                            )?
                        }
                        Opcode::LoadFixed => self.host.fixed(instruction.index())?,
                        Opcode::LoadHost => {
                            let program = &active_program;
                            self.host.reference(
                                program
                                    .names
                                    .get(index)
                                    .ok_or_else(|| error("Host 引用索引越界"))?,
                            )?
                        }
                        Opcode::Unary => {
                            let value = self.get(base, instruction.b())?;
                            let op = instruction.flags();
                            // 标量一元在寄存器内完成；flag 取反会分配新值，先发布根。
                            match (op, value) {
                                (0, Slot::Int(value)) => Slot::Int(
                                    value.checked_neg().ok_or_else(|| error("int 取负溢出"))?,
                                ),
                                (0, Slot::Float(value)) => Slot::Float(-value),
                                (1, Slot::Bool(value)) => Slot::Bool(!value),
                                (2, Slot::Int(value)) => Slot::Int(!value),
                                (2, value) => {
                                    self.publish_roots()?;
                                    self.host.enum_unary(value)?
                                }
                                _ => return Err(error("无效的一元操作")),
                            }
                        }
                        Opcode::Binary => {
                            let left = self.get(base, instruction.b())?;
                            let right = self.get(base, instruction.c())?;
                            let op = instruction.flags();
                            if op == 0x80 {
                                self.publish_roots_force()?;
                                self.host.accumulate_text(left, right)?
                            } else {
                                // 标量快速路径：int/float 运算与相等、比较完全在寄存器内完成。
                                match scalar_binary(op, left, right) {
                                    Some(result) => result?,
                                    None => {
                                        self.publish_roots_force()?;
                                        binary(self.host, op, left, right)?
                                    }
                                }
                            }
                        }
                        Opcode::IntBinary => int_binary(
                            instruction.flags(),
                            integer(self.get(base, instruction.b())?)?,
                            integer(self.get(base, instruction.c())?)?,
                        )?,
                        Opcode::IntBinaryImmediate => {
                            let current = integer(self.get(base, instruction.a())?)?;
                            let immediate = instruction.index() as i32;
                            let flags = instruction.flags();
                            if flags & 0x80 == 0 {
                                int_binary(flags, current, immediate)?
                            } else {
                                int_binary(flags & 0x7f, immediate, current)?
                            }
                        }
                        Opcode::FloatBinary => {
                            let Slot::Float(left) = self.get(base, instruction.b())? else {
                                return Err(error("需要 float"));
                            };
                            let Slot::Float(right) = self.get(base, instruction.c())? else {
                                return Err(error("需要 float"));
                            };
                            float_binary(instruction.flags(), left, right)?
                        }
                        Opcode::ConvertFloat => match self.get(base, instruction.b())? {
                            Slot::None => Slot::None,
                            Slot::Int(value) => Slot::Float(value as f32),
                            Slot::Float(value) => Slot::Float(value),
                            _ => return Err(error("无效的数值提升")),
                        },
                        Opcode::IsType => {
                            let program = &active_program;
                            Slot::Bool(
                                self.host.is_type(
                                    self.get(base, instruction.b())?,
                                    program
                                        .names
                                        .get(usize::from(instruction.c()))
                                        .ok_or_else(|| error("类型索引越界"))?,
                                )?,
                            )
                        }
                        Opcode::IsSome => {
                            Slot::Bool(self.get(base, instruction.b())? != Slot::None)
                        }
                        Opcode::Jump | Opcode::JumpFalse => {
                            let jump = if opcode == Opcode::Jump {
                                true
                            } else {
                                !boolean(self.get(base, instruction.a())?)?
                            };
                            if jump {
                                pc = index;
                            }
                            continue;
                        }
                        Opcode::Return => {
                            let value = self.get(base, instruction.a())?;
                            let frame =
                                self.frames.pop().ok_or_else(|| error("返回时调用栈为空"))?;
                            for builder in frame.builders.iter().flat_map(|builders| &builders.values) {
                                self.host.build(
                                    &super::construction::BuildOp::Drop { builder: *builder },
                                    &crate::schema::CftValueType::Unit,
                                )?;
                            }
                            self.budget.leave(frame.binding.program.registers.len());
                            self.registers.truncate(frame.base);
                            if let Some(destination) = frame.destination {
                                self.set(destination, value)?;
                                continue 'frames;
                            }
                            self.flush_iterations()?;
                            return Ok(value);
                        }
                        Opcode::Call | Opcode::CallDirect => {
                            self.budget.charge(1)?;
                            // 程序调用直接借用当前帧的附表，不为每次调用克隆 Program Arc。
                            let (target, arguments_start, arguments_len) = {
                                let frame =
                                    self.frames.last().ok_or_else(|| error("调用栈为空"))?;
                                let program = &frame.binding.program;
                                let (target, arguments_start, arguments_len) =
                                    if opcode == Opcode::CallDirect {
                                        let site = &program.direct_calls[index];
                                        (
                                            Callable::Program(
                                                self.host.direct_callable(site.function)?.into(),
                                            ),
                                            site.arguments_start,
                                            site.arguments_len,
                                        )
                                    } else {
                                        let site = &program.calls[index];
                                        let target = self.get(base, site.target)?;
                                        (
                                            self.host.callable(target)?,
                                            site.arguments_start,
                                            site.arguments_len,
                                        )
                                    };
                                let arguments = program
                                    .operands(arguments_start, arguments_len)
                                    .ok_or_else(|| error("调用操作数范围越界"))?;
                                (
                                    target,
                                    base + arguments.first().copied().map_or(0, usize::from),
                                    arguments.len(),
                                )
                            };
                            match target {
                                Callable::Program(binding) => {
                                    self.push_window(
                                        binding,
                                        arguments_start,
                                        arguments_len,
                                        destination,
                                    )?;
                                    continue 'frames;
                                }
                                Callable::Host(target) => {
                                    // 副作用边界前必须结算，不能让已超限循环继续进入 Host。
                                    self.flush_iterations()?;
                                    let program = &active_program;
                                    self.host.location(
                                        &program,
                                        *program
                                            .spans
                                            .get(
                                                self.frames
                                                    .last()
                                                    .map_or(0, |frame| frame.pc.saturating_sub(1)),
                                            )
                                            .ok_or_else(|| error("缺少调用位置"))?,
                                    );
                                    self.publish_roots_force()?;
                                    self.host.call_host(
                                        target,
                                        &self.registers
                                            [arguments_start..arguments_start + arguments_len],
                                    )?
                                }
                            }
                        }
                        Opcode::Closure => {
                            self.publish_roots()?;
                            let program = &active_program;
                            let site = program
                                .closures
                                .get(index)
                                .ok_or_else(|| error("闭包附表越界"))?;
                            let (captures, _memory) = self.values(base, &site.captures)?;
                            let closure_owner = if let Some(register) = site.owner {
                                self.get(base, register)?
                            } else {
                                owner
                            };
                            self.host.closure(
                                FunctionBinding {
                                    program: program
                                        .closure(index)
                                        .ok_or_else(|| error("闭包程序越界"))?,
                                    owner: closure_owner,
                                    captures: captures.into(),
                                },
                                site.template,
                            )?
                        }
                        Opcode::Format => {
                            self.publish_roots()?;
                            let program = &active_program;
                            let plan = program
                                .formats
                                .get(index)
                                .ok_or_else(|| error("格式计划越界"))?;
                            self.host.format(
                                plan,
                                &self.registers[base..base + program.registers.len()],
                            )?
                        }
                        Opcode::Array | Opcode::Dictionary => {
                            let program = &active_program;
                            let (values, _memory) = self.values(
                                base,
                                program
                                    .collections
                                    .get(index)
                                    .ok_or_else(|| error("集合附表越界"))?,
                            )?;
                            self.budget.charge(values.len() as u64)?;
                            self.publish_roots()?;
                            match opcode {
                                Opcode::Array => self.host.array(values)?,
                                Opcode::Dictionary => {
                                    if values.len() % 2 != 0 {
                                        return Err(error("字典键值数量不匹配"));
                                    }
                                    self.host.dictionary(
                                        values
                                            .chunks_exact(2)
                                            .map(|pair| (pair[0], pair[1]))
                                            .collect(),
                                    )?
                                }
                                _ => unreachable!(),
                            }
                        }
                        Opcode::Object => {
                            let program = &active_program;
                            let site = program
                                .objects
                                .get(index)
                                .ok_or_else(|| error("对象附表越界"))?;
                            if instruction.flags() == 1 {
                                self.publish_roots()?;
                                let value = self.host.reserve_object(&site.type_name)?;
                                self.set(destination, value)?;
                                continue;
                            }
                            let values = site
                                .fields
                                .iter()
                                .map(|(name, register)| {
                                    Ok((name.as_str(), self.get(base, *register)?))
                                })
                                .collect::<Result<Vec<_>, ExecutionError>>()?;
                            self.publish_roots()?;
                            self.host.initialize_object(
                                self.get(base, instruction.a())?,
                                &site.type_name,
                                values,
                            )?
                        }
                        Opcode::ReadTemplate => {
                            let value = self.get(base, instruction.b())?;
                            // 模板可能执行字节码并分配，先发布根。
                            self.publish_roots()?;
                            if let Some(binding) = self.host.template(value)? {
                                if let Some(text) = binding.program.static_text() {
                                    self.host.constant(text)?
                                } else {
                                    self.push(binding, &[], Some(destination))?;
                                    continue 'frames;
                                }
                            } else {
                                value
                            }
                        }
                        Opcode::Length => Slot::Int(
                            i32::try_from(self.host.length(self.get(base, instruction.b())?)?)
                                .map_err(|_| error("集合长度超出 int"))?,
                        ),
                        Opcode::IteratorValue => {
                            let index = integer(self.get(base, instruction.c())?)?;
                            self.host.iterator(
                                self.get(base, instruction.b())?,
                                usize::try_from(index).map_err(|_| error("负迭代索引"))?,
                            )?
                        }
                        Opcode::IterNext => {
                            // 附表条目是 Copy：借用作用域内取副本，避免克隆程序 Arc。
                            let site = *active_program
                                .iter_nexts
                                .get(index)
                                .ok_or_else(|| error("迭代附表越界"))?;
                            let counter = integer(self.get(base, site.counter)?)?;
                            let (key, value) = self.host.iter_next(
                                self.get(base, site.collection)?,
                                usize::try_from(counter).map_err(|_| error("负迭代索引"))?,
                            )?;
                            self.set(base + usize::from(site.key), key)?;
                            self.set(base + usize::from(site.value), value)?;
                            continue;
                        }
                        Opcode::ForPrep => {
                            let site = {
                                let program = &active_program;
                                *program
                                    .for_sites
                                    .get(index)
                                    .ok_or_else(|| error("区间循环附表越界"))?
                            };
                            let value = integer(self.get(base, instruction.a())?)?;
                            let limit = integer(self.get(base, site.limit)?)?;
                            // 入口守卫：空区间或倒序区间直接跳出。
                            let exhausted = if site.exclusive {
                                value >= limit
                            } else {
                                value > limit
                            };
                            if exhausted {
                                pc = site.target as usize;
                            }
                            continue;
                        }
                        Opcode::ForLoop => {
                            let site = {
                                let program = &active_program;
                                *program
                                    .for_sites
                                    .get(index)
                                    .ok_or_else(|| error("区间循环附表越界"))?
                            };
                            let value = integer(self.get(base, instruction.a())?)?;
                            let limit = integer(self.get(base, site.limit)?)?;
                            // ForLoop 即区间循环的回边：批量计费与 Iteration 相同。
                            self.iteration_debt += 1;
                            if self.iteration_debt >= ITERATION_BUDGET_BATCH {
                                let batch = self.iteration_debt;
                                self.iteration_debt = 0;
                                self.budget.iteration_batch(batch)?;
                            }
                            // 闭区间在最大整数处落出，不执行会溢出的自增。
                            let exhausted = if site.exclusive {
                                value >= limit
                            } else {
                                value == limit
                            };
                            if !exhausted {
                                let next =
                                    value.checked_add(1).ok_or_else(|| error("区间自增溢出"))?;
                                self.set(base + usize::from(instruction.a()), Slot::Int(next))?;
                                pc = site.target as usize;
                            }
                            continue;
                        }
                        Opcode::SelfField => self.host.field(owner, instruction.c())?,
                        Opcode::Builtin => {
                            let program = &active_program;
                            let site = program
                                .builtins
                                .get(index)
                                .ok_or_else(|| error("内建附表越界"))?;
                            let receiver = self.get(base, site.receiver)?;
                            let result_type = program
                                .registers
                                .get(usize::from(instruction.a()))
                                .ok_or_else(|| error("结果寄存器越界"))?;
                            let arguments = program
                                .operands(site.arguments_start, site.arguments_len)
                                .ok_or_else(|| error("内建操作数范围越界"))?;
                            self.gather_operands(base, arguments)?;
                            self.publish_roots_force()?;
                            let value = self.host.builtin(
                                &site.name,
                                receiver,
                                &self.operands_scratch,
                                result_type,
                            )?;
                            self.operands_scratch.clear();
                            value
                        }
                    };
                    self.set(destination, value)?;
                }
            }
        })();
        if dirty_pc {
            // 标量失败尚未经过安全点，错误栈必须看到实际失败指令。
            if let Some(frame) = self.frames.last_mut() {
                frame.pc = pc;
            }
        }
        result
    }
}
fn binary<H: ExecutionHost>(
    host: &H,
    op: u8,
    left: Slot,
    right: Slot,
) -> Result<Slot, ExecutionError> {
    if op == 7 || op == 8 {
        let equal = host.equals(left, right)?;
        return Ok(Slot::Bool(if op == 7 { equal } else { !equal }));
    }
    if (9..=12).contains(&op) {
        let ordering = host.compare(left, right)?;
        return Ok(Slot::Bool(match op {
            9 => ordering == Some(Ordering::Less),
            10 => matches!(ordering, Some(Ordering::Less | Ordering::Equal)),
            11 => ordering == Some(Ordering::Greater),
            _ => matches!(ordering, Some(Ordering::Greater | Ordering::Equal)),
        }));
    }
    if op == 0 {
        return host.concatenate(left, right);
    }
    if (15..=17).contains(&op) {
        return host.enum_binary(op, left, right);
    }
    Err(error("二元操作数不匹配"))
}

#[cfg(test)]
mod compact_slot_tests {
    use super::*;
    #[test]
    fn temporary_reservations_accumulate_across_nested_budgets_and_unwind() {
        let budget = Budget::new(ExecutionLimits {
            max_heap_bytes: 100,
            ..ExecutionLimits::default()
        });
        let outer = budget.reserve_temporary(40, 10).unwrap();
        let nested = budget.clone();
        assert!(nested.reserve_temporary(51, 10).is_err());
        let inner = nested.reserve_temporary(50, 10).unwrap();
        assert_eq!(budget.remaining_heap_bytes(), 10);
        drop(inner);
        assert_eq!(budget.remaining_heap_bytes(), 60);
        drop(outer);
        assert_eq!(budget.remaining_heap_bytes(), 100);
    }
    #[test]
    fn slots_are_eight_bytes_and_references_round_trip_without_losing_float_bits() {
        assert_eq!(size_of::<Slot>(), 8);
        for id in [0, 1, 65535, 65536, u64::from(u32::MAX), Slot::MAX_HANDLE] {
            let Slot::Handle(handle) = Slot::handle(id) else {
                unreachable!();
            };
            assert_eq!(handle.get(), id);
        }
        for bits in [0, 0x80000000, 0x7fc00001, 0xff800000] {
            let Slot::Float(value) = Slot::Float(f32::from_bits(bits)) else {
                unreachable!();
            };
            assert_eq!(value.to_bits(), bits);
        }
    }
}

#[cfg(test)]
mod buffer_tests {
    use super::*;
    #[test]
    fn empty_frames_recycle_their_allocation_without_retaining_bindings() {
        let frames: Vec<Frame<'_>> = Vec::with_capacity(8);
        let allocation = frames.as_ptr();
        let recycled = recycle_frames(frames);
        assert!(recycled.is_empty());
        assert_eq!(recycled.capacity(), 8);
        assert_eq!(recycled.as_ptr(), allocation);
    }
}
