//! 不依赖 Runtime 固定快照的字节码改写。
use super::{bytecode::{Constant, Program}, executor::{self, Slot}};
use std::collections::HashMap;

pub(crate) fn fuse_int_immediates(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};

    // 其他前驱可能绕过常量写入；只在同一基本块中融合。
    program.build_local_liveness()?;
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

pub(crate) fn compact_instructions(program: &mut Program, remove: &[bool]) -> Result<(), String> {
    if remove.len() != program.instructions.len() {
        return Err("指令删除掩码长度不匹配".into());
    }
    if !remove.iter().any(|remove| *remove) {
        return Ok(());
    }
    program.rewrite_with_liveness(|_, pc, instruction, output| {
        if !remove[pc] {
            output.push(instruction);
        }
        Ok(())
    })?;
    Ok(())
}

/// 只折叠实际成功的标量计算，随后按真实跳转边删除不可达节点。
/// 调用者须先链接所有符号，未执行分支里的非法引用仍然阻止发布。
pub(crate) fn fold_scalar_control_flow(program: &mut Program) -> Result<(), String> {
    use crate::vm::bytecode::{Instruction, Opcode};
    let leaders = program.block_leaders()?;
    let mut known = HashMap::new();
    let mut remove = vec![false; program.instructions.len()];
    for pc in 0..program.instructions.len() {
        if leaders.contains(&pc) {
            known.clear();
        }
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
