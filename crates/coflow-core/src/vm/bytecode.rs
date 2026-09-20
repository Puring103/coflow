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
    Builtin,
    Iteration,
    ForPrep,
    ForLoop,
    IterNext,
    SelfField,
    IntBinary,
    FloatBinary,
    /// Runtime 链接后的固定值槽；Contract 编译阶段不会直接生成。
    LoadFixed,
    /// 已解析的 Host 符号；不参与固定记录与常量查找。
    LoadHost,
    /// A = A op immediate；最高标志位表示 immediate 位于左侧。
    IntBinaryImmediate,
    /// 已链接程序编号调用，不读取动态函数值。
    CallDirect,
    Build,
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
            24 => Self::Builtin,
            25 => Self::Iteration,
            26 => Self::ForPrep,
            27 => Self::ForLoop,
            28 => Self::IterNext,
            29 => Self::SelfField,
            30 => Self::IntBinary,
            31 => Self::FloatBinary,
            32 => Self::LoadFixed,
            33 => Self::LoadHost,
            34 => Self::IntBinaryImmediate,
            35 => Self::CallDirect,
            36 => Self::Build,
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
    pub const fn with_flags(self, flags: u8) -> Self {
        Self((self.0 & 0x00ff_ffff_ffff_ffff) | ((flags as u64) << 56))
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
    pub arguments_start: u32,
    pub arguments_len: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FunctionId(pub u32);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCallSite {
    pub function: FunctionId,
    pub arguments_start: u32,
    pub arguments_len: u16,
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
    pub arguments_start: u32,
    pub arguments_len: u16,
}
/// 区间循环附表：value 寄存器在指令 A 位，limit、回边目标和区间开闭在附表。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ForSite {
    pub limit: Register,
    pub target: u32,
    pub exclusive: bool,
}
/// 双绑定迭代附表：一次调用同时产出键与值。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IterNextSite {
    pub collection: Register,
    pub counter: Register,
    pub key: Register,
    pub value: Register,
}
/// 内联 int 键的索引附表：键可以是完整 i32，receiver 在附表。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexSite {
    pub receiver: Register,
    pub key: Constant,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FormatPart { Text(String), Value(Register) }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    /// 仅 Release 映像允许尾调用消帧，Debug 保留逻辑调用栈。
    pub(crate) tail_calls: bool,
    pub owner_type: Option<CftValueType>,
    pub name: String,
    /// 闭包与主程序共享同一份源文本（Arc<str>），创建闭包不复制源码。
    pub source: std::sync::Arc<str>,
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
    pub direct_calls: Vec<DirectCallSite>,
    pub builders: Vec<super::construction::BuildOp<Register>>,
    pub closures: Vec<ClosureSite>,
    pub objects: Vec<ObjectSite>,
    pub collections: Vec<Vec<Register>>,
    pub formats: Vec<Vec<FormatPart>>,
    pub builtins: Vec<BuiltinSite>,
    /// Call/Builtin 共用的连续操作数池，站点仅保存范围。
    pub operands: Vec<Register>,
    pub captures: Vec<CftValueType>,
    pub live: Vec<Vec<Register>>,
    pub for_sites: Vec<ForSite>,
    pub iter_nexts: Vec<IterNextSite>,
    pub index_consts: Vec<IndexSite>,
}
impl Program {
    pub fn new(
        name: String,
        source: String,
        parameters: Vec<CftValueType>,
        result: CftValueType,
    ) -> Self {
        Self {
            tail_calls: false,
            owner_type: None,
            name,
            source: source.into(),
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
            direct_calls: Vec::new(),
            builders: Vec::new(),
            closures: Vec::new(),
            objects: Vec::new(),
            collections: Vec::new(),
            formats: Vec::new(),
            builtins: Vec::new(),
            operands: Vec::new(),
            captures: Vec::new(),
            live: Vec::new(),
            for_sites: Vec::new(),
            iter_nexts: Vec::new(),
            index_consts: Vec::new(),
        }
    }
    pub fn add_operands(&mut self, registers: &[Register]) -> Result<(u32, u16), String> {
        let start = u32::try_from(self.operands.len()).map_err(|_| "操作数池超限")?;
        let len = u16::try_from(registers.len()).map_err(|_| "调用参数数量超限")?;
        self.operands.extend_from_slice(registers);
        Ok((start, len))
    }
    pub fn operands(&self, start: u32, len: u16) -> Option<&[Register]> {
        let start = usize::try_from(start).ok()?;
        self.operands.get(start..start.checked_add(usize::from(len))?)
    }
    /// 发布时把调用参数布置成连续窗口，执行器可直接复制寄存器切片。
    /// 窗口在普通槽位之后按类型签名复用，平行搬运不会覆盖任何源操作数。
    pub(crate) fn prepare_call_windows(&mut self) -> Result<(), String> {
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).prepare_call_windows()?;
        }
        if self.instructions.len() != self.spans.len() { return Err("程序缺少源码映射".into()); }
        let old_instructions = std::mem::take(&mut self.instructions);
        let old_spans = std::mem::take(&mut self.spans);
        let old_calls = std::mem::take(&mut self.calls);
        let old_direct = std::mem::take(&mut self.direct_calls);
        let mut windows: Vec<(Vec<CftValueType>, Register)> = Vec::new();
        let mut relocated = vec![0u32; old_instructions.len()];
        for (pc, (mut instruction, span)) in old_instructions.into_iter().zip(old_spans).enumerate() {
            relocated[pc] = u32::try_from(self.instructions.len()).map_err(|_| "程序过大")?;
            let opcode = instruction.opcode().ok_or("未知操作码")?;
            if matches!(opcode, Opcode::Call | Opcode::CallDirect) {
                let index = instruction.index() as usize;
                let (start, len) = if opcode == Opcode::Call {
                    let site = old_calls.get(index).ok_or("调用附表越界")?;
                    (site.arguments_start, site.arguments_len)
                } else {
                    let site = old_direct.get(index).ok_or("直接调用附表越界")?;
                    (site.arguments_start, site.arguments_len)
                };
                let arguments = self.operands(start, len).ok_or("调用参数越界")?.to_vec();
                let contiguous = arguments.windows(2).all(|pair| pair[0].checked_add(1) == Some(pair[1]));
                let (start, len) = if contiguous { (start, len) } else {
                    let types = arguments.iter().map(|r| self.registers.get(*r as usize).cloned()
                        .ok_or_else(|| "调用参数槽越界".to_string())).collect::<Result<Vec<_>, _>>()?;
                    let window = if let Some((_, start)) = windows.iter().find(|(signature, _)| *signature == types) { *start }
                    else {
                        let start = Register::try_from(self.registers.len()).map_err(|_| "调用参数窗口超限")?;
                        if self.registers.len() + types.len() > usize::from(u16::MAX) + 1 { return Err("调用参数窗口超限".into()); }
                        self.registers.extend(types.iter().cloned());
                        windows.push((types, start));
                        start
                    };
                    let mut operands = Vec::with_capacity(arguments.len());
                    for (offset, source) in arguments.into_iter().enumerate() {
                        let destination = window + offset as u16;
                        self.instructions.push(Instruction::new(Opcode::Move, destination, source, 0, 0));
                        self.spans.push(span);
                        operands.push(destination);
                    }
                    self.add_operands(&operands)?
                };
                let index = if opcode == Opcode::Call {
                    let mut site = old_calls[index].clone();
                    site.arguments_start = start; site.arguments_len = len;
                    let index = self.calls.len(); self.calls.push(site); index
                } else {
                    let mut site = old_direct[index].clone();
                    site.arguments_start = start; site.arguments_len = len;
                    let index = self.direct_calls.len(); self.direct_calls.push(site); index
                };
                instruction = Instruction::indexed(opcode, instruction.a(), u32::try_from(index).map_err(|_| "调用附表过大")?);
            }
            self.instructions.push(instruction);
            self.spans.push(span);
        }
        for instruction in &mut self.instructions {
            if let Some(opcode @ (Opcode::Jump | Opcode::JumpFalse)) = instruction.opcode() {
                let target = *relocated.get(instruction.index() as usize).ok_or("跳转目标越界")?;
                *instruction = Instruction::indexed(opcode, instruction.a(), target);
            }
        }
        for site in &mut self.for_sites { site.target = *relocated.get(site.target as usize).ok_or("循环目标越界")?; }
        Ok(())
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
    /// 统一解释控制流目标：区间指令的 BC 是附表编号，不是 PC。
    pub(crate) fn branch_target(&self, instruction: Instruction) -> Result<Option<usize>, String> {
        let target = match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Jump | Opcode::JumpFalse => instruction.index() as usize,
            Opcode::ForPrep | Opcode::ForLoop => self.for_sites
                .get(instruction.index() as usize).ok_or("区间循环附表越界")?.target as usize,
            _ => return Ok(None),
        };
        if target >= self.instructions.len() {
            return Err("跳转越界".into());
        }
        Ok(Some(target))
    }

    /// 多目的指令必须完整报告写集合，供寄存器分配和常量失效共用。
    pub(crate) fn written_registers(&self, instruction: Instruction) -> Result<Vec<Register>, String> {
        Ok(match instruction.opcode().ok_or("未知操作码")? {
            Opcode::Jump | Opcode::JumpFalse | Opcode::Return | Opcode::Iteration
                | Opcode::ForPrep => Vec::new(),
            Opcode::IterNext => {
                let site = self.iter_nexts.get(instruction.index() as usize).ok_or("迭代附表越界")?;
                vec![site.key, site.value]
            }
            _ => vec![instruction.a()],
        })
    }

    pub(crate) fn block_leaders(&self) -> Result<std::collections::BTreeSet<usize>, String> {
        let mut leaders = std::collections::BTreeSet::from([0]);
        for (pc, instruction) in self.instructions.iter().copied().enumerate() {
            let target = self.branch_target(instruction)?;
            if let Some(target) = target {
                leaders.insert(target);
            }
            if (target.is_some() || instruction.opcode() == Some(Opcode::Return))
                && pc + 1 < self.instructions.len() {
                leaders.insert(pc + 1);
            }
        }
        Ok(leaders)
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
                if let Some(target) = self.branch_target(instruction)? {
                    next.extend(live[target].iter().copied());
                }
                if !matches!(
                    opcode,
                    Opcode::Jump
                        | Opcode::JumpFalse
                        | Opcode::Return
                        | Opcode::Iteration
                        | Opcode::ForPrep
                        | Opcode::ForLoop
                        | Opcode::IterNext
                ) {
                    next.remove(&instruction.a());
                }
                match opcode {
                    Opcode::Build => {
                        next.extend(self.builders.get(index).ok_or("构造操作越界")?.inputs());
                    }
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
                    Opcode::Index => {
                        if instruction.flags() == 0 {
                            next.extend([instruction.b(), instruction.c()]);
                        } else {
                            next.insert(self.index_consts.get(index).ok_or("索引附表越界")?.receiver);
                        }
                    }
                    Opcode::Binary
                    | Opcode::IntBinary
                    | Opcode::FloatBinary
                    | Opcode::IteratorValue => {
                        next.extend([instruction.b(), instruction.c()]);
                    }
                    Opcode::SelfField => {
                        // 读 owner（非寄存器），写字段值到目标寄存器；字段槽不是寄存器。
                    }
                    Opcode::LoadFixed | Opcode::LoadHost => {}
                    Opcode::IntBinaryImmediate => {
                        next.insert(instruction.a());
                    }
                    Opcode::ForPrep | Opcode::ForLoop => {
                        let site = self.for_sites.get(index).ok_or("区间循环附表越界")?;
                        next.insert(instruction.a());
                        next.insert(site.limit);
                    }
                    Opcode::IterNext => {
                        let site = self.iter_nexts.get(index).ok_or("迭代附表越界")?;
                        // 键与值由本指令写入，从这里向上不再活跃。
                        next.remove(&site.key);
                        next.remove(&site.value);
                        // 读发生在写之前，输入与输出重叠时仍必须保留输入根。
                        next.extend([site.collection, site.counter]);
                    }
                    Opcode::JumpFalse | Opcode::Return => {
                        next.insert(instruction.a());
                    }
                    Opcode::Call => {
                        let site = self.calls.get(index).ok_or("调用附表越界")?;
                        next.insert(site.target);
                        next.extend(
                            self.operands(site.arguments_start, site.arguments_len)
                                .ok_or("调用操作数范围越界")?,
                        );
                    }
                    Opcode::CallDirect => {
                        let site = self.direct_calls.get(index).ok_or("直接调用附表越界")?;
                        next.extend(self.operands(site.arguments_start, site.arguments_len)
                            .ok_or("直接调用操作数范围越界")?);
                    }
                    Opcode::Closure => {
                        let site = self.closures.get(index).ok_or("闭包附表越界")?;
                        next.extend(&site.captures);
                        next.extend(site.owner);
                    }
                    Opcode::Format => {
                        for part in self.formats.get(index).ok_or("格式计划越界")? { if let FormatPart::Value(register) = part { next.insert(*register); } }
                    }
                    Opcode::Array | Opcode::Dictionary => {
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
                        next.extend(
                            self.operands(site.arguments_start, site.arguments_len)
                                .ok_or("内建操作数范围越界")?,
                        );
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
            for register in self.written_registers(instruction)? {
                let range = intervals.get_mut(usize::from(register)).ok_or("寄存器越界")?;
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
            if !matches!(op, Opcode::Jump | Opcode::Iteration | Opcode::IterNext) {
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
                Opcode::Index if instruction.flags() == 0 => {
                    b = map(b);
                    c = map(c);
                }
                Opcode::Binary
                | Opcode::IntBinary
                | Opcode::FloatBinary
                | Opcode::IteratorValue => {
                    b = map(b);
                    c = map(c);
                }
                _ => {}
            }
            *instruction = Instruction::new(op, a, b, c, instruction.flags());
        }
        for site in &mut self.calls {
            site.target = map(site.target);
        }
        for operation in &mut self.builders { *operation = operation.map(|r| Ok(map(r)))?; }
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
        for plan in &mut self.formats { for part in plan { if let FormatPart::Value(register) = part { *register = map(*register); } } }
        for site in &mut self.builtins {
            site.receiver = map(site.receiver);
        }
        for register in &mut self.operands {
            *register = map(*register);
        }
        for site in &mut self.for_sites {
            site.limit = map(site.limit);
        }
        for site in &mut self.iter_nexts {
            site.collection = map(site.collection);
            site.counter = map(site.counter);
            site.key = map(site.key);
            site.value = map(site.value);
        }
        for site in &mut self.index_consts {
            site.receiver = map(site.receiver);
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
                Opcode::Binary | Opcode::IntBinary => instruction.flags() <= 17,
                Opcode::IntBinaryImmediate => instruction.flags() & 0x7f <= 17,
                Opcode::FloatBinary => matches!(instruction.flags(), 0..=3 | 6..=12),
                Opcode::Constant => instruction.flags() <= 3,
                Opcode::Index => instruction.flags() <= 1,
                Opcode::Object | Opcode::ForPrep | Opcode::ForLoop => {
                    instruction.flags() <= 1
                }
                _ => instruction.flags() == 0,
            };
            if !valid_flags {
                return Err("无效的指令操作标志".into());
            }
            if !matches!(opcode, Opcode::Jump | Opcode::Iteration | Opcode::IterNext) {
                register(instruction.a())?;
            }
            let valid = match opcode {
                Opcode::Constant => {
                    if instruction.flags() == 0 {
                        index < self.constants.len()
                    } else {
                        true
                    }
                }
                Opcode::Capture => index < self.captures.len(),
                Opcode::Jump | Opcode::JumpFalse => index < self.instructions.len(),
                Opcode::ForPrep | Opcode::ForLoop => {
                    let site = self.for_sites.get(index).ok_or("区间循环附表越界")?;
                    register(instruction.a())?;
                    register(site.limit)?;
                    (site.target as usize) < self.instructions.len()
                }
                Opcode::IterNext => {
                    let site = self.iter_nexts.get(index).ok_or("迭代附表越界")?;
                    register(site.collection)?;
                    register(site.counter)?;
                    register(site.key)?;
                    register(site.value)?;
                    true
                }
                Opcode::SelfField => {
                    // C 是字段槽（编译期已限制在 u16），不是寄存器。
                    true
                }
                Opcode::Call => index < self.calls.len(),
                Opcode::CallDirect => index < self.direct_calls.len(),
                Opcode::Build => {
                    let operation = self.builders.get(index).ok_or("构造操作越界")?;
                    for input in operation.inputs() { register(input)?; }
                    true
                }
                Opcode::Closure => index < self.closures.len(),
                Opcode::Object => index < self.objects.len(),
                Opcode::Format => {
                    for part in self.formats.get(index).ok_or("格式计划越界")? { if let FormatPart::Value(value) = part { register(*value)?; } }
                    true
                }
                Opcode::Array | Opcode::Dictionary => {
                    index < self.collections.len()
                }
                Opcode::Builtin => index < self.builtins.len(),
                Opcode::Reference => index < self.names.len(),
                // 固定槽属于 Runtime 映像，Program 自身只验证编码宽度。
                Opcode::LoadFixed => true,
                Opcode::LoadHost => index < self.names.len(),
                Opcode::IntBinaryImmediate => true,
                Opcode::IsType => {
                    register(instruction.b())?;
                    usize::from(instruction.c()) < self.names.len()
                }
                Opcode::Field => {
                    register(instruction.b())?;
                    true
                }
                Opcode::Index => {
                    if instruction.flags() == 0 {
                        register(instruction.b())?;
                        register(instruction.c())?;
                    } else {
                        let site = self.index_consts.get(index).ok_or("索引附表越界")?;
                        register(site.receiver)?;
                        if !matches!(site.key, Constant::Int(_)) {
                            return Err("内联索引键不是 int".into());
                        }
                    }
                    true
                }
                Opcode::Binary
                | Opcode::IntBinary
                | Opcode::FloatBinary
                | Opcode::IteratorValue => {
                    register(instruction.b())?;
                    register(instruction.c())?;
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
            for r in self
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("调用操作数范围越界")?
            {
                register(*r)?;
            }
        }
        for site in &self.direct_calls {
            for r in self.operands(site.arguments_start, site.arguments_len)
                .ok_or("直接调用操作数范围越界")? {
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
            for r in self
                .operands(site.arguments_start, site.arguments_len)
                .ok_or("内建操作数范围越界")?
            {
                register(*r)?;
            }
        }
        Ok(())
    }
}
