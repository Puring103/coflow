//! 与 Rust 内存布局解耦的寄存器字节码；序列化总是使用显式小端整数。
use crate::{schema::CftValueType, source::Span};
use serde::{Deserialize, Serialize};

pub type Register = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Opcode {
    Constant,
    Move,
    SelfValue,
    Capture,
    Field,
    Index,
    Reference,
    Unary,
    Binary,
    ConvertFloat,
    IsType,
    IsSome,
    Jump,
    JumpFalse,
    Return,
    Call,
    Closure,
    Array,
    Dictionary,
    Object,
    ReadTemplate,
    Format,
    Length,
    IteratorValue,
    IteratorKey,
    Builtin,
    Append,
    Iteration,
}
impl Opcode {
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Constant,
            1 => Self::Move,
            2 => Self::SelfValue,
            3 => Self::Capture,
            4 => Self::Field,
            5 => Self::Index,
            6 => Self::Reference,
            7 => Self::Unary,
            8 => Self::Binary,
            9 => Self::ConvertFloat,
            10 => Self::IsType,
            11 => Self::IsSome,
            12 => Self::Jump,
            13 => Self::JumpFalse,
            14 => Self::Return,
            15 => Self::Call,
            16 => Self::Closure,
            17 => Self::Array,
            18 => Self::Dictionary,
            19 => Self::Object,
            20 => Self::ReadTemplate,
            21 => Self::Format,
            22 => Self::Length,
            23 => Self::IteratorValue,
            24 => Self::IteratorKey,
            25 => Self::Builtin,
            26 => Self::Append,
            27 => Self::Iteration,
            _ => return None,
        })
    }
}

/// 64 位逻辑指令：opcode:8、A:16、B:16、C:16、flags:8。
/// BC 也可解释为 u32 附表索引或绝对指令序号；跳转不使用宿主字节地址。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction(u64);
impl Instruction {
    pub const fn new(opcode: Opcode, a: Register, b: Register, c: Register, flags: u8) -> Self {
        Self(
            opcode as u64
                | ((a as u64) << 8)
                | ((b as u64) << 24)
                | ((c as u64) << 40)
                | ((flags as u64) << 56),
        )
    }
    pub const fn indexed(opcode: Opcode, a: Register, index: u32) -> Self {
        Self::new(opcode, a, index as u16, (index >> 16) as u16, 0)
    }
    pub fn opcode(self) -> Option<Opcode> {
        Opcode::from_byte(self.0 as u8)
    }
    pub const fn a(self) -> Register {
        (self.0 >> 8) as Register
    }
    pub const fn b(self) -> Register {
        (self.0 >> 24) as Register
    }
    pub const fn c(self) -> Register {
        (self.0 >> 40) as Register
    }
    pub const fn flags(self) -> u8 {
        (self.0 >> 56) as u8
    }
    pub const fn index(self) -> u32 {
        (self.0 >> 24) as u32
    }
    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }
    pub const fn from_le_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_le_bytes(bytes))
    }
}

/// 紧凑候选编码保留相同逻辑指令；高操作数通过标记和完整字扩展。
/// 两种编码共用指令语义，便于按实际程序测量总体积与解码成本。
pub fn encode_compact(instructions: &[Instruction]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for instruction in instructions {
        if instruction.a() <= 255
            && instruction.b() <= 255
            && instruction.c() <= 255
            && instruction.flags() == 0
        {
            bytes.extend_from_slice(&[
                instruction.0 as u8,
                instruction.a() as u8,
                instruction.b() as u8,
                instruction.c() as u8,
            ]);
        } else {
            bytes.extend_from_slice(&[255, 0, 0, 0]);
            bytes.extend_from_slice(&instruction.to_le_bytes());
        }
    }
    bytes
}
pub fn decode_compact(bytes: &[u8]) -> Result<Vec<Instruction>, String> {
    let mut instructions = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        let word = bytes
            .get(position..position + 4)
            .ok_or("截断的字节码指令")?;
        position += 4;
        let instruction = if word[0] == 255 {
            if word[1..] != [0, 0, 0] {
                return Err("无效的扩展标记".into());
            }
            let extended = bytes.get(position..position + 8).ok_or("截断的扩展指令")?;
            position += 8;
            Instruction::from_le_bytes(extended.try_into().map_err(|_| "无效的扩展指令")?)
        } else {
            Instruction::new(
                Opcode::from_byte(word[0]).ok_or("未知操作码")?,
                u16::from(word[1]),
                u16::from(word[2]),
                u16::from(word[3]),
                0,
            )
        };
        instruction.opcode().ok_or("未知操作码")?;
        instructions.push(instruction);
    }
    Ok(instructions)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Unit,
    None,
    Bool(bool),
    Int(i32),
    Float(f32),
    String(String),
    Enum { name: String, value: u32 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSite {
    pub target: Register,
    pub arguments: Vec<Register>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureSite {
    pub program: std::sync::Arc<Program>,
    pub captures: Vec<Register>,
    pub owner: Option<Register>,
    pub template: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectSite {
    pub type_name: String,
    pub fields: Vec<(String, Register)>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltinSite {
    pub name: String,
    pub receiver: Register,
    pub arguments: Vec<Register>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub name: String,
    pub source: String,
    pub module: Option<crate::schema::ModuleId>,
    pub path: Option<String>,
    pub parameters: Vec<CftValueType>,
    pub result: CftValueType,
    pub registers: Vec<CftValueType>,
    pub instructions: Vec<Instruction>,
    pub spans: Vec<Span>,
    pub constants: Vec<Constant>,
    pub names: Vec<String>,
    pub calls: Vec<CallSite>,
    pub closures: Vec<ClosureSite>,
    pub objects: Vec<ObjectSite>,
    pub collections: Vec<Vec<Register>>,
    pub builtins: Vec<BuiltinSite>,
    pub captures: Vec<CftValueType>,
    pub live: Vec<Vec<Register>>,
}
impl Program {
    pub fn new(
        name: String,
        source: String,
        parameters: Vec<CftValueType>,
        result: CftValueType,
    ) -> Self {
        Self {
            name,
            source,
            module: None,
            path: None,
            registers: parameters.clone(),
            parameters,
            result,
            instructions: Vec::new(),
            spans: Vec::new(),
            constants: Vec::new(),
            names: Vec::new(),
            calls: Vec::new(),
            closures: Vec::new(),
            objects: Vec::new(),
            collections: Vec::new(),
            builtins: Vec::new(),
            captures: Vec::new(),
            live: Vec::new(),
        }
    }
    /// 编译器使用局部字节偏移；发布程序时一次性映射到原始文件，闭包共享相同来源。
    pub fn locate(
        &mut self,
        module: Option<crate::schema::ModuleId>,
        path: Option<String>,
        offset: usize,
    ) {
        self.module = module.clone();
        self.path = path.clone();
        for span in &mut self.spans {
            span.start += offset;
            span.end += offset;
        }
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).locate(
                module.clone(),
                path.clone(),
                offset,
            );
        }
    }
    /// 类型别名展开只改变函数头长度；函数体偏移按展开前后的头长度映射。
    pub fn map_expanded_header(&mut self, expanded: usize, original: usize) {
        for span in &mut self.spans {
            span.start = original + span.start.saturating_sub(expanded);
            span.end = original + span.end.saturating_sub(expanded);
        }
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).map_expanded_header(expanded, original);
        }
    }
    /// 在控制流图上反向求活跃值。暂停调用使用调用点的 live-in，保留参数及后续读取值。
    pub fn build_liveness(&mut self) -> Result<(), String> {
        use std::collections::BTreeSet;
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).build_liveness()?;
        }
        let count = self.instructions.len();
        let mut live = vec![BTreeSet::new(); count];
        let mut steps = 0u64;
        loop {
            let mut changed = false;
            for pc in (0..count).rev() {
                steps += 1;
                if steps > 10_000_000 {
                    return Err("寄存器存活分析工作量超限".into());
                }
                let instruction = self.instructions[pc];
                let opcode = instruction.opcode().ok_or("未知操作码")?;
                let index = instruction.index() as usize;
                let mut next = BTreeSet::new();
                if !matches!(opcode, Opcode::Jump | Opcode::Return) {
                    if let Some(successor) = live.get(pc + 1) {
                        next.extend(successor.iter().copied());
                    }
                }
                if matches!(opcode, Opcode::Jump | Opcode::JumpFalse) {
                    next.extend(live.get(index).ok_or("跳转越界")?.iter().copied());
                }
                if !matches!(
                    opcode,
                    Opcode::Jump
                        | Opcode::JumpFalse
                        | Opcode::Return
                        | Opcode::Append
                        | Opcode::Iteration
                ) {
                    next.remove(&instruction.a());
                }
                match opcode {
                    Opcode::Move
                    | Opcode::Field
                    | Opcode::Unary
                    | Opcode::ConvertFloat
                    | Opcode::IsType
                    | Opcode::IsSome
                    | Opcode::ReadTemplate
                    | Opcode::Length => {
                        next.insert(instruction.b());
                    }
                    Opcode::Index
                    | Opcode::Binary
                    | Opcode::IteratorKey
                    | Opcode::IteratorValue => {
                        next.extend([instruction.b(), instruction.c()]);
                    }
                    Opcode::JumpFalse | Opcode::Return => {
                        next.insert(instruction.a());
                    }
                    Opcode::Call => {
                        let site = self.calls.get(index).ok_or("调用附表越界")?;
                        next.insert(site.target);
                        next.extend(&site.arguments);
                    }
                    Opcode::Closure => {
                        let site = self.closures.get(index).ok_or("闭包附表越界")?;
                        next.extend(&site.captures);
                        next.extend(site.owner);
                    }
                    Opcode::Array | Opcode::Dictionary | Opcode::Format => {
                        next.extend(self.collections.get(index).ok_or("集合附表越界")?);
                    }
                    Opcode::Object => {
                        if instruction.flags() == 0 {
                            next.insert(instruction.a());
                            next.extend(
                                self.objects
                                    .get(index)
                                    .ok_or("对象附表越界")?
                                    .fields
                                    .iter()
                                    .map(|(_, r)| *r),
                            );
                        }
                    }
                    Opcode::Builtin => {
                        let site = self.builtins.get(index).ok_or("内建附表越界")?;
                        next.insert(site.receiver);
                        next.extend(&site.arguments);
                    }
                    Opcode::Append => {
                        next.extend([instruction.a(), instruction.b()]);
                        if instruction.flags() == 1 {
                            next.insert(instruction.c());
                        }
                    }
                    _ => {}
                }
                if live[pc] != next {
                    live[pc] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        self.live = live
            .into_iter()
            .map(|registers| registers.into_iter().collect())
            .collect();
        Ok(())
    }
    /// 用 CFG 存活信息形成保守区间，复用类型相同且不重叠的槽；参数保持起始 ABI。
    pub fn allocate_registers(&mut self) -> Result<(), String> {
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).allocate_registers()?;
        }
        self.build_liveness()?;
        let mut intervals = vec![(usize::MAX, 0); self.registers.len()];
        for (pc, live) in self.live.iter().enumerate() {
            for register in live {
                let range = &mut intervals[usize::from(*register)];
                range.0 = range.0.min(pc);
                range.1 = range.1.max(pc);
            }
            let instruction = self.instructions[pc];
            if !matches!(instruction.opcode(), Some(Opcode::Jump | Opcode::Iteration)) {
                let range = &mut intervals[usize::from(instruction.a())];
                range.0 = range.0.min(pc);
                range.1 = range.1.max(pc);
            }
        }
        let mut mapping = vec![0u16; self.registers.len()];
        let mut types = self.parameters.clone();
        let mut ends = Vec::new();
        for (index, range) in intervals.iter_mut().enumerate().take(self.parameters.len()) {
            range.0 = 0;
            mapping[index] = index as u16;
            ends.push(range.1);
        }
        let mut order = (self.parameters.len()..self.registers.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| intervals[*index].0);
        for index in order {
            let (start, end) = intervals[index];
            let slot = types.iter().enumerate().find_map(|(slot, ty)| {
                if *ty == self.registers[index] && ends[slot] < start {
                    Some(slot)
                } else {
                    None
                }
            });
            let slot = slot.unwrap_or_else(|| {
                types.push(self.registers[index].clone());
                ends.push(0);
                types.len() - 1
            });
            mapping[index] = slot as u16;
            ends[slot] = end;
        }
        let map = |register: Register| mapping[usize::from(register)];
        for instruction in &mut self.instructions {
            let op = instruction.opcode().ok_or("未知操作码")?;
            let (mut a, mut b, mut c) = (instruction.a(), instruction.b(), instruction.c());
            if !matches!(op, Opcode::Jump | Opcode::Iteration) {
                a = map(a);
            }
            match op {
                Opcode::Move
                | Opcode::Field
                | Opcode::Unary
                | Opcode::ConvertFloat
                | Opcode::IsType
                | Opcode::IsSome
                | Opcode::ReadTemplate
                | Opcode::Length => b = map(b),
                Opcode::Index | Opcode::Binary | Opcode::IteratorKey | Opcode::IteratorValue => {
                    b = map(b);
                    c = map(c);
                }
                Opcode::Append => {
                    b = map(b);
                    if instruction.flags() == 1 {
                        c = map(c);
                    }
                }
                _ => {}
            }
            *instruction = Instruction::new(op, a, b, c, instruction.flags());
        }
        for site in &mut self.calls {
            site.target = map(site.target);
            for r in &mut site.arguments {
                *r = map(*r);
            }
        }
        for site in &mut self.closures {
            for r in &mut site.captures {
                *r = map(*r);
            }
            site.owner = site.owner.map(map);
        }
        for site in &mut self.objects {
            for (_, r) in &mut site.fields {
                *r = map(*r);
            }
        }
        for site in &mut self.collections {
            for r in site {
                *r = map(*r);
            }
        }
        for site in &mut self.builtins {
            site.receiver = map(site.receiver);
            for r in &mut site.arguments {
                *r = map(*r);
            }
        }
        self.registers = types;
        self.build_liveness()
    }
    /// 编译结束时检查附表和控制流边界；可信契约加载无需再运行源码类型检查。
    pub fn validate(&self) -> Result<(), String> {
        if self.instructions.is_empty() || self.instructions.len() != self.spans.len() {
            return Err("程序缺少指令或源码映射".into());
        }
        if self.registers.len() > usize::from(u16::MAX) + 1
            || self.parameters.len() > self.registers.len()
        {
            return Err("无效的寄存器窗口".into());
        }
        if self.live.len() != self.instructions.len() {
            return Err("程序缺少完整存活信息".into());
        }
        for live in &self.live {
            if live.iter().any(|r| usize::from(*r) >= self.registers.len())
                || live.windows(2).any(|r| r[0] >= r[1])
            {
                return Err("无效的存活寄存器集合".into());
            }
        }
        if self.live.first().is_some_and(|live| {
            live.iter()
                .any(|r| usize::from(*r) >= self.parameters.len())
        }) {
            return Err("控制流存在未初始化寄存器读取".into());
        }
        if !self.registers.starts_with(&self.parameters) {
            return Err("参数槽类型与签名不一致".into());
        }
        let register = |r: Register| {
            if usize::from(r) < self.registers.len() {
                Ok(())
            } else {
                Err("寄存器越界".to_string())
            }
        };
        for instruction in &self.instructions {
            let opcode = instruction.opcode().ok_or("未知操作码")?;
            let index = instruction.index() as usize;
            let valid_flags = match opcode {
                Opcode::Unary => instruction.flags() <= 2,
                Opcode::Binary => instruction.flags() <= 17,
                Opcode::Object | Opcode::Append => instruction.flags() <= 1,
                _ => instruction.flags() == 0,
            };
            if !valid_flags {
                return Err("无效的指令操作标志".into());
            }
            if !matches!(opcode, Opcode::Jump | Opcode::Iteration) {
                register(instruction.a())?;
            }
            let valid = match opcode {
                Opcode::Constant => index < self.constants.len(),
                Opcode::Capture => index < self.captures.len(),
                Opcode::Jump | Opcode::JumpFalse => index < self.instructions.len(),
                Opcode::Call => index < self.calls.len(),
                Opcode::Closure => index < self.closures.len(),
                Opcode::Object => index < self.objects.len(),
                Opcode::Array | Opcode::Dictionary | Opcode::Format => {
                    index < self.collections.len()
                }
                Opcode::Builtin => index < self.builtins.len(),
                Opcode::Reference => index < self.names.len(),
                Opcode::IsType => {
                    register(instruction.b())?;
                    usize::from(instruction.c()) < self.names.len()
                }
                Opcode::Field => {
                    register(instruction.b())?;
                    true
                }
                Opcode::Index | Opcode::Binary | Opcode::IteratorValue | Opcode::IteratorKey => {
                    register(instruction.b())?;
                    register(instruction.c())?;
                    true
                }
                Opcode::Append => {
                    register(instruction.b())?;
                    if instruction.flags() == 1 {
                        register(instruction.c())?;
                    }
                    true
                }
                Opcode::Move
                | Opcode::Unary
                | Opcode::ConvertFloat
                | Opcode::IsSome
                | Opcode::ReadTemplate
                | Opcode::Length => {
                    register(instruction.b())?;
                    true
                }
                Opcode::SelfValue | Opcode::Return | Opcode::Iteration => true,
            };
            if !valid {
                return Err("程序附表或跳转目标越界".into());
            }
        }
        for site in &self.calls {
            register(site.target)?;
            for r in &site.arguments {
                register(*r)?;
            }
        }
        for site in &self.closures {
            if site.captures.len() != site.program.captures.len() {
                return Err("闭包捕获布局不匹配".into());
            }
            for r in &site.captures {
                register(*r)?;
            }
            if let Some(owner) = site.owner {
                register(owner)?;
            }
            site.program.validate()?;
        }
        for site in &self.objects {
            for (_, r) in &site.fields {
                register(*r)?;
            }
        }
        for site in &self.collections {
            for r in site {
                register(*r)?;
            }
        }
        for site in &self.builtins {
            register(site.receiver)?;
            for r in &site.arguments {
                register(*r)?;
            }
        }
        Ok(())
    }
}
