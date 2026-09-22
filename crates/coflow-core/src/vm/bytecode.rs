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
    IndexArray,
    IndexDict,
    IndexString,
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
            37 => Self::IndexArray,
            38 => Self::IndexDict,
            39 => Self::IndexString,
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
pub enum FormatPart {
    Text(String),
    Value(Register),
}
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
        source: impl Into<std::sync::Arc<str>>,
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
        self.operands
            .get(start..start.checked_add(usize::from(len))?)
    }
    /// 发布时把调用参数布置成连续窗口，执行器可直接复制寄存器切片。
    /// 窗口在普通槽位之后按类型签名复用，平行搬运不会覆盖任何源操作数。
    pub(crate) fn prepare_call_windows(&mut self) -> Result<(), String> {
        for closure in &mut self.closures {
            std::sync::Arc::make_mut(&mut closure.program).prepare_call_windows()?;
        }
        if self.instructions.len() != self.spans.len() {
            return Err("程序缺少源码映射".into());
        }
        let old_instructions = std::mem::take(&mut self.instructions);
        let old_spans = std::mem::take(&mut self.spans);
        let old_calls = std::mem::take(&mut self.calls);
        let old_direct = std::mem::take(&mut self.direct_calls);
        let mut windows: Vec<(Vec<CftValueType>, Register)> = Vec::new();
        let mut relocated = vec![0u32; old_instructions.len()];
        for (pc, (mut instruction, span)) in old_instructions.into_iter().zip(old_spans).enumerate()
        {
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
                let contiguous = arguments
                    .windows(2)
                    .all(|pair| pair[0].checked_add(1) == Some(pair[1]));
                let (start, len) = if contiguous {
                    (start, len)
                } else {
                    let types = arguments
                        .iter()
                        .map(|r| {
                            self.registers
                                .get(*r as usize)
                                .cloned()
                                .ok_or_else(|| "调用参数槽越界".to_string())
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let window = if let Some((_, start)) =
                        windows.iter().find(|(signature, _)| *signature == types)
                    {
                        *start
                    } else {
                        let start = Register::try_from(self.registers.len())
                            .map_err(|_| "调用参数窗口超限")?;
                        if self.registers.len() + types.len() > usize::from(u16::MAX) + 1 {
                            return Err("调用参数窗口超限".into());
                        }
                        self.registers.extend(types.iter().cloned());
                        windows.push((types, start));
                        start
                    };
                    let mut operands = Vec::with_capacity(arguments.len());
                    for (offset, source) in arguments.into_iter().enumerate() {
                        let destination = window + offset as u16;
                        self.instructions.push(Instruction::new(
                            Opcode::Move,
                            destination,
                            source,
                            0,
                            0,
                        ));
                        self.spans.push(span);
                        operands.push(destination);
                    }
                    self.add_operands(&operands)?
                };
                let index = if opcode == Opcode::Call {
                    let mut site = old_calls[index].clone();
                    site.arguments_start = start;
                    site.arguments_len = len;
                    let index = self.calls.len();
                    self.calls.push(site);
                    index
                } else {
                    let mut site = old_direct[index].clone();
                    site.arguments_start = start;
                    site.arguments_len = len;
                    let index = self.direct_calls.len();
                    self.direct_calls.push(site);
                    index
                };
                instruction = Instruction::indexed(
                    opcode,
                    instruction.a(),
                    u32::try_from(index).map_err(|_| "调用附表过大")?,
                );
            }
            self.instructions.push(instruction);
            self.spans.push(span);
        }
        for instruction in &mut self.instructions {
            if let Some(opcode @ (Opcode::Jump | Opcode::JumpFalse)) = instruction.opcode() {
                let target = *relocated
                    .get(instruction.index() as usize)
                    .ok_or("跳转目标越界")?;
                *instruction = Instruction::indexed(opcode, instruction.a(), target);
            }
        }
        for site in &mut self.for_sites {
            site.target = *relocated.get(site.target as usize).ok_or("循环目标越界")?;
        }
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
}
