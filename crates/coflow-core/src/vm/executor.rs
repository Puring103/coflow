//! 显式调用帧解释器。语言递归不会递归 Rust 调用栈，Host 边界只传稳定值。
use super::{bytecode::*, ExecutionError};
use std::{
    cmp::Ordering,
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Slot {
    Empty,
    Unit,
    None,
    Bool(bool),
    Int(i32),
    Float(f32),
    Handle(usize),
}
#[derive(Debug, Clone)]
pub struct Binding {
    pub program: Arc<Program>,
    pub owner: Slot,
    pub captures: Arc<[Slot]>,
}
#[derive(Debug, Clone)]
pub enum Callable {
    Program(Binding),
    Host(Slot),
}

/// 值存储和宿主适配由 Runtime 提供；解释器只负责指令、调用和错误栈。
pub trait ExecutionHost {
    fn location(&self, _program: &Program, _span: crate::source::Span) {}
    fn constant(&self, value: &Constant) -> Result<Slot, ExecutionError>;
    fn field(&self, receiver: Slot, slot: u16) -> Result<Slot, ExecutionError>;
    fn index(&self, receiver: Slot, key: Slot) -> Result<Slot, ExecutionError>;
    fn reference(&self, name: &str) -> Result<Slot, ExecutionError>;
    fn equals(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError>;
    fn compare(&self, left: Slot, right: Slot) -> Result<Option<Ordering>, ExecutionError>;
    fn concatenate(&self, left: Slot, right: Slot) -> Result<Slot, ExecutionError>;
    fn enum_unary(&self, value: Slot) -> Result<Slot, ExecutionError>;
    fn enum_binary(&self, operator: u8, left: Slot, right: Slot) -> Result<Slot, ExecutionError>;
    fn is_type(&self, value: Slot, type_name: &str) -> Result<bool, ExecutionError>;
    fn callable(&self, value: Slot) -> Result<Callable, ExecutionError>;
    fn call_host(&self, target: Slot, args: &[Slot]) -> Result<Slot, ExecutionError>;
    fn closure(&self, binding: Binding, template: bool) -> Result<Slot, ExecutionError>;
    fn append(&self, target: Slot, value: Slot, key: Option<Slot>) -> Result<(), ExecutionError>;
    fn array(&self, values: Vec<Slot>) -> Result<Slot, ExecutionError>;
    fn dictionary(&self, values: Vec<(Slot, Slot)>) -> Result<Slot, ExecutionError>;
    fn reserve_object(&self, type_name: &str) -> Result<Slot, ExecutionError>;
    fn initialize_object(
        &self,
        target: Slot,
        type_name: &str,
        fields: Vec<(&str, Slot)>,
    ) -> Result<Slot, ExecutionError>;
    fn object(&self, type_name: &str, fields: Vec<(&str, Slot)>) -> Result<Slot, ExecutionError>;
    fn template(&self, value: Slot) -> Result<Option<Binding>, ExecutionError>;
    fn format(&self, values: &[Slot]) -> Result<Slot, ExecutionError>;
    fn length(&self, value: Slot) -> Result<usize, ExecutionError>;
    fn iterator(&self, value: Slot, index: usize, key: bool) -> Result<Slot, ExecutionError>;
    fn builtin(
        &self,
        name: &str,
        receiver: Slot,
        args: &[Slot],
        result_type: &crate::schema::CftValueType,
    ) -> Result<Slot, ExecutionError>;
    /// 分配和调用前公布所有活跃根，Host 重入和回收可据此保留暂停的外层值。
    fn roots(&self, roots: &[Slot]) -> Result<(), ExecutionError>;
}
#[derive(Debug, Clone, Copy)]
pub struct ExecutionLimits {
    pub max_work: u64,
    pub max_iterations: u64,
    pub max_depth: usize,
    pub max_registers: usize,
    pub max_heap_bytes: usize,
}
impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_work: 10_000_000,
            max_iterations: 1_000_000,
            max_depth: 256,
            max_registers: 1_000_000,
            max_heap_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug)]
struct Usage {
    remaining: u64,
    iterations: u64,
    depth: usize,
    registers: usize,
}
#[derive(Debug, Clone)]
pub struct Budget {
    limits: ExecutionLimits,
    usage: Arc<Mutex<Usage>>,
}
impl Budget {
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            usage: Arc::new(Mutex::new(Usage {
                remaining: limits.max_work,
                iterations: limits.max_iterations,
                depth: 0,
                registers: 0,
            })),
        }
    }
    pub fn charge(&self, work: u64) -> Result<(), ExecutionError> {
        let mut usage = self.usage.lock().map_err(|_| error("执行预算状态失效"))?;
        if work > usage.remaining {
            usage.remaining = 0;
            return Err(error("执行工作量预算耗尽"));
        }
        usage.remaining -= work;
        Ok(())
    }
    /// 循环体入口扣除一次迭代；break、continue 和高阶集合遍历使用同一请求预算。
    fn iteration(&self) -> Result<(), ExecutionError> {
        let mut usage = self.usage.lock().map_err(|_| error("执行预算状态失效"))?;
        if usage.iterations == 0 {
            usage.remaining = 0;
            return Err(error("循环迭代预算耗尽"));
        }
        usage.iterations -= 1;
        Ok(())
    }
    pub fn max_heap_bytes(&self) -> usize {
        self.limits.max_heap_bytes
    }
    pub fn remaining(&self) -> u64 {
        self.usage.lock().map_or(0, |usage| usage.remaining)
    }
    fn enter(&self, registers: usize) -> Result<Window, ExecutionError> {
        let mut usage = self.usage.lock().map_err(|_| error("执行预算状态失效"))?;
        if usage.depth >= self.limits.max_depth
            || registers > self.limits.max_registers.saturating_sub(usage.registers)
        {
            return Err(error("调用深度或寄存器预算耗尽"));
        }
        usage.depth += 1;
        usage.registers += registers;
        Ok(Window {
            budget: self.clone(),
            registers,
        })
    }
}
#[derive(Debug)]
struct Window {
    budget: Budget,
    registers: usize,
}
impl Drop for Window {
    fn drop(&mut self) {
        if let Ok(mut usage) = self.budget.usage.lock() {
            usage.depth -= 1;
            usage.registers -= self.registers;
        }
    }
}
#[derive(Debug)]
struct Frame {
    binding: Binding,
    pc: usize,
    base: usize,
    destination: Option<usize>,
    _window: Window,
}

pub fn execute(
    host: &dyn ExecutionHost,
    binding: Binding,
    arguments: &[Slot],
    budget: Budget,
) -> Result<Slot, ExecutionError> {
    let mut execution = Execution {
        host,
        budget,
        registers: Vec::new(),
        frames: Vec::new(),
    };
    execution.push(binding, arguments, None)?;
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
                message: cause.to_string(),
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
struct Execution<'a> {
    host: &'a dyn ExecutionHost,
    budget: Budget,
    registers: Vec<Slot>,
    frames: Vec<Frame>,
}
impl Drop for Execution<'_> {
    fn drop(&mut self) {
        let _ = self.host.roots(&[]);
    }
}
impl Execution<'_> {
    fn push(
        &mut self,
        binding: Binding,
        arguments: &[Slot],
        destination: Option<usize>,
    ) -> Result<(), ExecutionError> {
        if arguments.len() != binding.program.parameters.len()
            || arguments.len() > binding.program.registers.len()
            || binding.captures.len() != binding.program.captures.len()
        {
            return Err(error("调用签名或捕获布局不匹配"));
        }
        let window = self.budget.enter(binding.program.registers.len())?;
        let base = self.registers.len();
        self.registers
            .resize(base + binding.program.registers.len(), Slot::Empty);
        self.registers[base..base + arguments.len()].copy_from_slice(arguments);
        self.frames.push(Frame {
            binding,
            pc: 0,
            base,
            destination,
            _window: window,
        });
        Ok(())
    }
    fn get(&self, base: usize, register: Register) -> Result<Slot, ExecutionError> {
        match self.registers.get(base + usize::from(register)).copied() {
            Some(Slot::Empty) | None => Err(error("读取未初始化或越界寄存器")),
            Some(value) => Ok(value),
        }
    }
    fn set(&mut self, index: usize, value: Slot) -> Result<(), ExecutionError> {
        let slot = self
            .registers
            .get_mut(index)
            .ok_or_else(|| error("写入越界寄存器"))?;
        *slot = value;
        Ok(())
    }
    fn values(&self, base: usize, registers: &[Register]) -> Result<Vec<Slot>, ExecutionError> {
        registers.iter().map(|r| self.get(base, *r)).collect()
    }
    fn run(&mut self) -> Result<Slot, ExecutionError> {
        loop {
            self.budget.charge(1)?;
            let frame = self.frames.last_mut().ok_or_else(|| error("调用栈为空"))?;
            let program = frame.binding.program.clone();
            let instruction = program
                .instructions
                .get(frame.pc)
                .copied()
                .ok_or_else(|| error("执行越过函数出口"))?;
            frame.pc += 1;
            let base = frame.base;
            let owner = frame.binding.owner;
            let captures = frame.binding.captures.clone();
            let opcode = instruction.opcode().ok_or_else(|| error("未知操作码"))?;
            let destination = base + usize::from(instruction.a());
            let index = instruction.index() as usize;
            // 固定句柄被复制进独立根列表；调用宿主时不保留寄存器元素引用。
            if matches!(
                opcode,
                Opcode::Constant
                    | Opcode::Field
                    | Opcode::Index
                    | Opcode::Reference
                    | Opcode::Unary
                    | Opcode::Binary
                    | Opcode::Call
                    | Opcode::Closure
                    | Opcode::Array
                    | Opcode::Dictionary
                    | Opcode::Object
                    | Opcode::ReadTemplate
                    | Opcode::Format
                    | Opcode::Builtin
                    | Opcode::Append
            ) {
                let mut roots = Vec::new();
                for frame in &self.frames {
                    let live = frame
                        .binding
                        .program
                        .live
                        .get(frame.pc.saturating_sub(1))
                        .ok_or_else(|| error("缺少寄存器存活信息"))?;
                    self.budget.charge(live.len() as u64)?;
                    for register in live {
                        if let value @ Slot::Handle(_) = self.get(frame.base, *register)? {
                            roots.push(value);
                        }
                    }
                    roots.push(frame.binding.owner);
                    roots.extend(frame.binding.captures.iter().copied());
                }
                self.host.roots(&roots)?;
            }
            let value = match opcode {
                Opcode::Iteration => {
                    self.budget.iteration()?;
                    continue;
                }
                Opcode::Constant => self.host.constant(
                    program
                        .constants
                        .get(index)
                        .ok_or_else(|| error("常量索引越界"))?,
                )?,
                Opcode::Move => self.get(base, instruction.b())?,
                Opcode::SelfValue => owner,
                Opcode::Capture => *captures.get(index).ok_or_else(|| error("捕获索引越界"))?,
                Opcode::Field => self
                    .host
                    .field(self.get(base, instruction.b())?, instruction.c())?,
                Opcode::Index => self.host.index(
                    self.get(base, instruction.b())?,
                    self.get(base, instruction.c())?,
                )?,
                Opcode::Reference => self.host.reference(
                    program
                        .names
                        .get(index)
                        .ok_or_else(|| error("引用索引越界"))?,
                )?,
                Opcode::Unary => unary(
                    self.host,
                    instruction.flags(),
                    self.get(base, instruction.b())?,
                )?,
                Opcode::Binary => binary(
                    self.host,
                    instruction.flags(),
                    self.get(base, instruction.b())?,
                    self.get(base, instruction.c())?,
                )?,
                Opcode::ConvertFloat => match self.get(base, instruction.b())? {
                    Slot::None => Slot::None,
                    Slot::Int(value) => Slot::Float(value as f32),
                    Slot::Float(value) => Slot::Float(value),
                    _ => return Err(error("无效的数值提升")),
                },
                Opcode::IsType => Slot::Bool(
                    self.host.is_type(
                        self.get(base, instruction.b())?,
                        program
                            .names
                            .get(usize::from(instruction.c()))
                            .ok_or_else(|| error("类型索引越界"))?,
                    )?,
                ),
                Opcode::IsSome => Slot::Bool(self.get(base, instruction.b())? != Slot::None),
                Opcode::Jump | Opcode::JumpFalse => {
                    let jump = if opcode == Opcode::Jump {
                        true
                    } else {
                        !boolean(self.get(base, instruction.a())?)?
                    };
                    if jump {
                        if index >= program.instructions.len() {
                            return Err(error("跳转越界"));
                        }
                        if let Some(frame) = self.frames.last_mut() {
                            frame.pc = index;
                        }
                    }
                    continue;
                }
                Opcode::Return => {
                    let value = self.get(base, instruction.a())?;
                    let frame = self.frames.pop().ok_or_else(|| error("返回时调用栈为空"))?;
                    self.registers.truncate(frame.base);
                    if let Some(destination) = frame.destination {
                        self.set(destination, value)?;
                        continue;
                    }
                    return Ok(value);
                }
                Opcode::Call => {
                    let site = program
                        .calls
                        .get(index)
                        .ok_or_else(|| error("调用附表越界"))?;
                    let target = self.get(base, site.target)?;
                    let arguments = self.values(base, &site.arguments)?;
                    match self.host.callable(target)? {
                        Callable::Program(binding) => {
                            self.push(binding, &arguments, Some(destination))?;
                            continue;
                        }
                        Callable::Host(target) => {
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
                            self.host.call_host(target, &arguments)?
                        }
                    }
                }
                Opcode::Closure => {
                    let site = program
                        .closures
                        .get(index)
                        .ok_or_else(|| error("闭包附表越界"))?;
                    let captures = self.values(base, &site.captures)?;
                    let owner = if let Some(register) = site.owner {
                        self.get(base, register)?
                    } else {
                        owner
                    };
                    self.host.closure(
                        Binding {
                            program: site.program.clone(),
                            owner,
                            captures: captures.into(),
                        },
                        site.template,
                    )?
                }
                Opcode::Array | Opcode::Dictionary | Opcode::Format => {
                    let values = self.values(
                        base,
                        program
                            .collections
                            .get(index)
                            .ok_or_else(|| error("集合附表越界"))?,
                    )?;
                    self.budget.charge(values.len() as u64)?;
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
                        _ => self.host.format(&values)?,
                    }
                }
                Opcode::Object => {
                    let site = program
                        .objects
                        .get(index)
                        .ok_or_else(|| error("对象附表越界"))?;
                    if instruction.flags() == 1 {
                        let value = self.host.reserve_object(&site.type_name)?;
                        self.set(destination, value)?;
                        continue;
                    }
                    let values = site
                        .fields
                        .iter()
                        .map(|(name, register)| Ok((name.as_str(), self.get(base, *register)?)))
                        .collect::<Result<Vec<_>, ExecutionError>>()?;
                    self.host.initialize_object(
                        self.get(base, instruction.a())?,
                        &site.type_name,
                        values,
                    )?
                }
                Opcode::ReadTemplate => {
                    let value = self.get(base, instruction.b())?;
                    if let Some(binding) = self.host.template(value)? {
                        self.push(binding, &[], Some(destination))?;
                        continue;
                    }
                    value
                }
                Opcode::Length => Slot::Int(
                    i32::try_from(self.host.length(self.get(base, instruction.b())?)?)
                        .map_err(|_| error("集合长度超出 int"))?,
                ),
                Opcode::IteratorKey | Opcode::IteratorValue => {
                    let index = integer(self.get(base, instruction.c())?)?;
                    self.host.iterator(
                        self.get(base, instruction.b())?,
                        usize::try_from(index).map_err(|_| error("负迭代索引"))?,
                        opcode == Opcode::IteratorKey,
                    )?
                }
                Opcode::Append => {
                    let target = self.get(base, instruction.a())?;
                    let value = self.get(base, instruction.b())?;
                    let key = if instruction.flags() == 1 {
                        Some(self.get(base, instruction.c())?)
                    } else {
                        None
                    };
                    self.host.append(target, value, key)?;
                    continue;
                }
                Opcode::Builtin => {
                    let site = program
                        .builtins
                        .get(index)
                        .ok_or_else(|| error("内建附表越界"))?;
                    let args = self.values(base, &site.arguments)?;
                    self.host.builtin(
                        &site.name,
                        self.get(base, site.receiver)?,
                        &args,
                        program
                            .registers
                            .get(usize::from(instruction.a()))
                            .ok_or_else(|| error("结果寄存器越界"))?,
                    )?
                }
            };
            self.set(destination, value)?;
        }
    }
}
fn error(message: &str) -> ExecutionError {
    ExecutionError::InvalidAccess(message.into())
}
fn integer(value: Slot) -> Result<i32, ExecutionError> {
    if let Slot::Int(value) = value {
        Ok(value)
    } else {
        Err(error("需要 int"))
    }
}
fn boolean(value: Slot) -> Result<bool, ExecutionError> {
    if let Slot::Bool(value) = value {
        Ok(value)
    } else {
        Err(error("需要 bool"))
    }
}
fn unary(host: &dyn ExecutionHost, op: u8, value: Slot) -> Result<Slot, ExecutionError> {
    Ok(match (op, value) {
        (0, Slot::Int(value)) => {
            Slot::Int(value.checked_neg().ok_or_else(|| error("int 取负溢出"))?)
        }
        (0, Slot::Float(value)) => Slot::Float(-value),
        (1, Slot::Bool(value)) => Slot::Bool(!value),
        (2, Slot::Int(value)) => Slot::Int(!value),
        (2, value) => host.enum_unary(value)?,
        _ => return Err(error("无效的一元操作")),
    })
}
fn binary(
    host: &dyn ExecutionHost,
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
    match (left, right) {
        (Slot::Int(a), Slot::Int(b)) => {
            let value = match op {
                0 => a.checked_add(b),
                1 => a.checked_sub(b),
                2 => a.checked_mul(b),
                4 => a.checked_div(b),
                5 => a.checked_rem(b),
                6 => u32::try_from(b).ok().and_then(|b| a.checked_pow(b)),
                13 => u32::try_from(b)
                    .ok()
                    .filter(|b| *b < 32)
                    .map(|b| a.wrapping_shl(b)),
                14 => u32::try_from(b).ok().filter(|b| *b < 32).map(|b| a >> b),
                15 => Some(a & b),
                16 => Some(a | b),
                17 => Some(a ^ b),
                _ => return Err(error("无效的 int 运算")),
            }
            .ok_or_else(|| error("整数溢出、除零、负指数或非法移位"))?;
            Ok(Slot::Int(value))
        }
        (Slot::Float(a), Slot::Float(b)) => Ok(Slot::Float(match op {
            0 => a + b,
            1 => a - b,
            2 => a * b,
            3 => a / b,
            6 => a.powf(b),
            _ => return Err(error("无效的 float 运算")),
        })),
        _ if op == 0 => host.concatenate(left, right),
        _ if (15..=17).contains(&op) => host.enum_binary(op, left, right),
        _ => Err(error("二元操作数不匹配")),
    }
}
