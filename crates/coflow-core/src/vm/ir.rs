//! Contract 的类型化三地址 IR。值编号和控制流位置不携带物理寄存器或快照地址。
use super::bytecode::{self, Constant, Instruction, Opcode, Program, Register};
use crate::{
    schema::{CftValueType, ModuleId},
    source::Span,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValueId(pub u32);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocationId(pub u32);

/// 声明类型字段域中的语义编号，包含继承字段和记录虚拟 id；不是字节偏移或寄存器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldId(pub u16);
impl From<u16> for FieldId {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Build(super::construction::BuildOp<ValueId>),
    Constant(Constant),
    Copy(ValueId),
    Owner,
    Capture(u32),
    Field {
        receiver: ValueId,
        field: FieldId,
    },
    OwnerField(FieldId),
    Index {
        receiver: ValueId,
        key: ValueId,
    },
    IndexConstant {
        receiver: ValueId,
        key: Constant,
    },
    Reference(String),
    Unary {
        operator: u8,
        value: ValueId,
    },
    Binary {
        operator: u8,
        left: ValueId,
        right: ValueId,
    },
    ConvertFloat(ValueId),
    IsType {
        value: ValueId,
        name: String,
    },
    IsSome(ValueId),
    Jump(LocationId),
    JumpFalse {
        condition: ValueId,
        target: LocationId,
    },
    Return(ValueId),
    Call {
        target: ValueId,
        arguments: Vec<ValueId>,
    },
    Closure {
        function: Box<Function>,
        captures: Vec<ValueId>,
        owner: Option<ValueId>,
        template: bool,
    },
    Array(Vec<ValueId>),
    Dictionary(Vec<ValueId>),
    ReserveObject {
        type_name: String,
    },
    InitializeObject {
        type_name: String,
        fields: Vec<(String, ValueId)>,
    },
    ReadTemplate(ValueId),
    Format(Vec<ValueId>),
    Concat(Vec<ValueId>),
    /// Release 优化专用：SSA 已证明 left 是未被观察的循环携带前缀。
    AccumulateText {
        left: ValueId,
        right: ValueId,
    },
    Length(ValueId),
    IteratorValue {
        collection: ValueId,
        index: ValueId,
    },
    Builtin {
        name: String,
        receiver: ValueId,
        arguments: Vec<ValueId>,
    },
    Iteration,
    ForPrep {
        limit: ValueId,
        target: LocationId,
        exclusive: bool,
    },
    ForLoop {
        limit: ValueId,
        target: LocationId,
        exclusive: bool,
    },
    IterNext {
        collection: ValueId,
        counter: ValueId,
        key: ValueId,
        value: ValueId,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub destination: ValueId,
    pub operation: Operation,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub owner: Option<CftValueType>,
    pub name: String,
    pub source: Arc<str>,
    pub module: Option<ModuleId>,
    pub path: Option<String>,
    pub parameters: Vec<CftValueType>,
    pub result: CftValueType,
    pub values: Vec<CftValueType>,
    pub captures: Vec<CftValueType>,
    pub body: Vec<Node>,
}

impl Function {
    pub fn locate(&mut self, module: Option<ModuleId>, path: Option<String>, offset: usize) {
        self.module = module.clone();
        self.path = path.clone();
        for node in &mut self.body {
            node.span.start += offset;
            node.span.end += offset;
            if let Operation::Closure { function, .. } = &mut node.operation {
                function.locate(module.clone(), path.clone(), offset);
            }
        }
    }

    pub fn map_expanded_header(&mut self, expanded: usize, original: usize) {
        for node in &mut self.body {
            node.span.start = original + node.span.start.saturating_sub(expanded);
            node.span.end = original + node.span.end.saturating_sub(expanded);
            if let Operation::Closure { function, .. } = &mut node.operation {
                function.map_expanded_header(expanded, original);
            }
        }
    }

    /// 每次降低均创建独立程序；链接、寄存器分配和后续优化不修改共享 Contract。
    pub(crate) fn lower_optimized(&self, optimize: bool) -> Result<Program, String> {
        if !optimize {
            return self.lower();
        }
        let mut function = self.clone();
        function.fuse_total_collection_stages()?;
        function.eliminate_identity_maps()?;
        function.propagate_ssa()?;
        function.hoist_total_loop_invariants()?;
        function.eliminate_scalar_closures()?;
        function.lower()
    }

    pub(crate) fn lower(&self) -> Result<Program, String> {
        let mut program = self.lower_unallocated(0)?;
        program.allocate_registers()?;
        program.validate()?;
        Ok(program)
    }

    fn lower_unallocated(&self, depth: usize) -> Result<Program, String> {
        if depth > 256 || self.body.len() > 1_000_000 || self.values.len() > 65_536 {
            return Err("Contract IR 结构超限".into());
        }
        let register = |value: ValueId| -> Result<Register, String> {
            if value.0 as usize >= self.values.len() {
                return Err("IR 值编号越界".into());
            }
            Register::try_from(value.0).map_err(|_| "IR 值数量超限".into())
        };
        let registers = |values: &[ValueId]| {
            values
                .iter()
                .map(|v| register(*v))
                .collect::<Result<Vec<_>, _>>()
        };
        let mut p = Program::new(
            self.name.clone(),
            self.source.to_string(),
            self.parameters.clone(),
            self.result.clone(),
        );
        p.owner_type = self.owner.clone();
        p.module = self.module.clone();
        p.path = self.path.clone();
        p.registers = self.values.clone();
        p.captures = self.captures.clone();
        for node in &self.body {
            use Operation as O;
            // 无目的值的节点不使用 destination，其他节点必须引用有效的类型化值。
            let a = if matches!(
                node.operation,
                O::Jump(_) | O::JumpFalse { .. } | O::Return(_) | O::Iteration | O::IterNext { .. }
            ) {
                0
            } else {
                register(node.destination)?
            };
            let indexed = |opcode, index: usize| -> Result<Instruction, String> {
                Ok(Instruction::indexed(
                    opcode,
                    a,
                    u32::try_from(index).map_err(|_| "IR 附表超限")?,
                ))
            };
            let instruction = match &node.operation {
                O::Build(operation) => {
                    p.builders.push(operation.map(register)?);
                    indexed(Opcode::Build, p.builders.len() - 1)?
                }
                O::Constant(constant) => match constant {
                    Constant::Int(v) => {
                        Instruction::indexed(Opcode::Constant, a, *v as u32).with_flags(1)
                    }
                    Constant::Float(v) => {
                        Instruction::indexed(Opcode::Constant, a, v.to_bits()).with_flags(2)
                    }
                    Constant::Unit => Instruction::new(Opcode::Constant, a, 0, 0, 3),
                    Constant::None => Instruction::new(Opcode::Constant, a, 1, 0, 3),
                    Constant::Bool(v) => {
                        Instruction::new(Opcode::Constant, a, 2 + u16::from(*v), 0, 3)
                    }
                    _ => {
                        p.constants.push(constant.clone());
                        indexed(Opcode::Constant, p.constants.len() - 1)?
                    }
                },
                O::Copy(v) => Instruction::new(Opcode::Move, a, register(*v)?, 0, 0),
                O::Owner => Instruction::new(Opcode::SelfValue, a, 0, 0, 0),
                O::Capture(v) => Instruction::indexed(Opcode::Capture, a, *v),
                O::Field { receiver, field } => {
                    Instruction::new(Opcode::Field, a, register(*receiver)?, field.0, 0)
                }
                O::OwnerField(field) => Instruction::new(Opcode::SelfField, a, 0, field.0, 0),
                O::Index { receiver, key } => {
                    let receiver_register = register(*receiver)?;
                    let opcode = match &self.values[receiver_register as usize] {
                        CftValueType::Array(_) => Opcode::IndexArray,
                        CftValueType::Dict(..) => Opcode::IndexDict,
                        CftValueType::String => Opcode::IndexString,
                        _ => Opcode::Index,
                    };
                    Instruction::new(opcode, a, receiver_register, register(*key)?, 0)
                }
                O::IndexConstant { receiver, key } => {
                    p.index_consts.push(bytecode::IndexSite {
                        receiver: register(*receiver)?,
                        key: key.clone(),
                    });
                    let opcode = match &self.values[register(*receiver)? as usize] {
                        CftValueType::Array(_) => Opcode::IndexArray,
                        CftValueType::Dict(..) => Opcode::IndexDict,
                        CftValueType::String => Opcode::IndexString,
                        _ => Opcode::Index,
                    };
                    indexed(opcode, p.index_consts.len() - 1)?.with_flags(1)
                }
                O::Reference(name) => {
                    p.names.push(name.clone());
                    indexed(Opcode::Reference, p.names.len() - 1)?
                }
                O::Unary { operator, value } => {
                    Instruction::new(Opcode::Unary, a, register(*value)?, 0, *operator)
                }
                O::Binary {
                    operator,
                    left,
                    right,
                } => {
                    let (b, c) = (register(*left)?, register(*right)?);
                    let opcode = match (&self.values[b as usize], &self.values[c as usize]) {
                        (CftValueType::Int, CftValueType::Int) if *operator != 3 => {
                            Opcode::IntBinary
                        }
                        (CftValueType::Float, CftValueType::Float) => Opcode::FloatBinary,
                        _ => Opcode::Binary,
                    };
                    Instruction::new(opcode, a, b, c, *operator)
                }
                O::AccumulateText { left, right } => {
                    Instruction::new(Opcode::Binary, a, register(*left)?, register(*right)?, 0x80)
                }
                O::ConvertFloat(v) => {
                    Instruction::new(Opcode::ConvertFloat, a, register(*v)?, 0, 0)
                }
                O::IsType { value, name } => {
                    let index = u16::try_from(p.names.len()).map_err(|_| "类型符号数量超限")?;
                    p.names.push(name.clone());
                    Instruction::new(Opcode::IsType, a, register(*value)?, index, 0)
                }
                O::IsSome(v) => Instruction::new(Opcode::IsSome, a, register(*v)?, 0, 0),
                O::Jump(target) => Instruction::indexed(Opcode::Jump, 0, target.0),
                O::JumpFalse { condition, target } => {
                    Instruction::indexed(Opcode::JumpFalse, register(*condition)?, target.0)
                }
                O::Return(v) => Instruction::new(Opcode::Return, register(*v)?, 0, 0, 0),
                O::Call { target, arguments } => {
                    let (arguments_start, arguments_len) =
                        p.add_operands(&registers(arguments)?)?;
                    p.calls.push(bytecode::CallSite {
                        target: register(*target)?,
                        arguments_start,
                        arguments_len,
                    });
                    indexed(Opcode::Call, p.calls.len() - 1)?
                }
                O::Closure {
                    function,
                    captures,
                    owner,
                    template,
                } => {
                    p.closures.push(bytecode::ClosureSite {
                        program: Arc::new(function.lower_unallocated(depth + 1)?),
                        captures: registers(captures)?,
                        owner: owner.map(register).transpose()?,
                        template: *template,
                    });
                    indexed(Opcode::Closure, p.closures.len() - 1)?
                }
                O::Format(values) | O::Concat(values) => {
                    p.formats.push(
                        registers(values)?
                            .into_iter()
                            .map(bytecode::FormatPart::Value)
                            .collect(),
                    );
                    indexed(Opcode::Format, p.formats.len() - 1)?
                }
                O::Array(v) | O::Dictionary(v) => {
                    let opcode = match &node.operation {
                        O::Array(_) => Opcode::Array,
                        O::Dictionary(_) => Opcode::Dictionary,
                        _ => unreachable!(),
                    };
                    p.collections.push(registers(v)?);
                    indexed(opcode, p.collections.len() - 1)?
                }
                O::ReserveObject { type_name } => {
                    p.objects.push(bytecode::ObjectSite {
                        type_name: type_name.clone(),
                        fields: Vec::new(),
                    });
                    indexed(Opcode::Object, p.objects.len() - 1)?.with_flags(1)
                }
                O::InitializeObject { type_name, fields } => {
                    p.objects.push(bytecode::ObjectSite {
                        type_name: type_name.clone(),
                        fields: fields
                            .iter()
                            .map(|(name, v)| Ok((name.clone(), register(*v)?)))
                            .collect::<Result<_, String>>()?,
                    });
                    indexed(Opcode::Object, p.objects.len() - 1)?
                }
                O::ReadTemplate(v) => {
                    Instruction::new(Opcode::ReadTemplate, a, register(*v)?, 0, 0)
                }
                O::Length(v) => Instruction::new(Opcode::Length, a, register(*v)?, 0, 0),
                O::IteratorValue { collection, index } => Instruction::new(
                    Opcode::IteratorValue,
                    a,
                    register(*collection)?,
                    register(*index)?,
                    0,
                ),
                O::Builtin {
                    name,
                    receiver,
                    arguments,
                } => {
                    let (arguments_start, arguments_len) =
                        p.add_operands(&registers(arguments)?)?;
                    p.builtins.push(bytecode::BuiltinSite {
                        name: name.clone(),
                        receiver: register(*receiver)?,
                        arguments_start,
                        arguments_len,
                    });
                    indexed(Opcode::Builtin, p.builtins.len() - 1)?
                }
                O::Iteration => Instruction::new(Opcode::Iteration, 0, 0, 0, 0),
                O::ForPrep {
                    limit,
                    target,
                    exclusive,
                }
                | O::ForLoop {
                    limit,
                    target,
                    exclusive,
                } => {
                    p.for_sites.push(bytecode::ForSite {
                        limit: register(*limit)?,
                        target: target.0,
                        exclusive: *exclusive,
                    });
                    indexed(
                        if matches!(node.operation, O::ForPrep { .. }) {
                            Opcode::ForPrep
                        } else {
                            Opcode::ForLoop
                        },
                        p.for_sites.len() - 1,
                    )?
                }
                O::IterNext {
                    collection,
                    counter,
                    key,
                    value,
                } => {
                    p.iter_nexts.push(bytecode::IterNextSite {
                        collection: register(*collection)?,
                        counter: register(*counter)?,
                        key: register(*key)?,
                        value: register(*value)?,
                    });
                    indexed(Opcode::IterNext, p.iter_nexts.len() - 1)?
                }
            };
            p.instructions.push(instruction);
            p.spans.push(node.span);
        }
        // 先验证未分配的虚拟值图，防止畸形 Contract 进入分配器的直接索引路径。
        p.build_liveness()?;
        p.validate()?;
        Ok(p)
    }
}
