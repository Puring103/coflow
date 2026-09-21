//! 映像内调用图的 effect 固定点。未知调用和递归保持保守，删除依据不依赖源码猜测。
use super::{bytecode::{Constant, Opcode, Program}, executor::Binding};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Effects(u16);
impl Effects {
    const HOST_READ: Self = Self(1);
    const HOST_EFFECT: Self = Self(2);
    const CONTEXT: Self = Self(4);
    const MAY_FAULT: Self = Self(8);
    const MAY_DIVERGE: Self = Self(16);
    const ALLOCATES: Self = Self(32);
    const IDENTITY: Self = Self(64);
    const LOCAL_WRITE: Self = Self(128);
    const UNKNOWN: Self = Self(255);
    fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
    pub(crate) fn discardable(self) -> bool { self.0 == 0 }
}

fn local(program: &Program) -> Result<(Effects, Vec<usize>), String> {
    let mut effects = Effects::default();
    let mut calls = Vec::new();
    for (pc, instruction) in program.instructions.iter().copied().enumerate() {
        let opcode = instruction.opcode().ok_or("effect 分析遇到未知指令")?;
        if program.branch_target(instruction)?.is_some_and(|target| target <= pc) {
            effects = effects.union(Effects::MAY_DIVERGE);
        }
        let effect = match opcode {
            Opcode::Constant => {
                if instruction.flags() == 0 && matches!(program.constants.get(instruction.index() as usize), Some(Constant::String(_) | Constant::Enum { .. })) {
                    Effects::ALLOCATES.union(Effects::MAY_FAULT)
                } else { Effects::default() }
            }
            Opcode::Move | Opcode::SelfValue | Opcode::Capture | Opcode::Return
                | Opcode::Jump | Opcode::JumpFalse | Opcode::ConvertFloat | Opcode::IsSome => Effects::default(),
            Opcode::CallDirect => {
                calls.push(program.direct_calls.get(instruction.index() as usize).ok_or("调用图附表越界")?.function.0 as usize);
                Effects::default()
            }
            // 内容比较可能读取模板；未知动态函数也可能进入 Host 或递归。
            Opcode::Call | Opcode::ReadTemplate | Opcode::Binary | Opcode::Builtin => Effects::UNKNOWN,
            Opcode::LoadHost | Opcode::Reference => Effects::HOST_EFFECT.union(Effects::CONTEXT).union(Effects::MAY_FAULT).union(Effects::ALLOCATES),
            Opcode::LoadFixed
            | Opcode::Field
            | Opcode::SelfField
            | Opcode::Index
            | Opcode::IndexArray
            | Opcode::IndexDict
            | Opcode::IndexString
                | Opcode::Length | Opcode::IteratorValue | Opcode::IterNext | Opcode::IsType => Effects::HOST_READ.union(Effects::MAY_FAULT),
            Opcode::Closure => Effects::ALLOCATES.union(Effects::IDENTITY).union(Effects::MAY_FAULT),
            Opcode::Array | Opcode::Dictionary | Opcode::Object | Opcode::Format => Effects::ALLOCATES.union(Effects::MAY_FAULT),
            Opcode::Build => Effects::LOCAL_WRITE.union(Effects::ALLOCATES).union(Effects::MAY_FAULT),
            Opcode::Iteration | Opcode::ForPrep | Opcode::ForLoop => Effects::MAY_DIVERGE.union(Effects::MAY_FAULT),
            Opcode::Unary | Opcode::IntBinary | Opcode::FloatBinary | Opcode::IntBinaryImmediate => Effects::MAY_FAULT,
        };
        effects = effects.union(effect);
    }
    calls.sort_unstable(); calls.dedup();
    Ok((effects, calls))
}

/// 先从叶节点证明有限调用链；剩余循环及其调用者保守标记可能不终止。
/// effect 沿反向调用边做单调固定点传播，每一位只会从 0 变为 1。
pub(crate) fn call_effects(bindings: &[Binding]) -> Result<Vec<Effects>, String> {
    let count = bindings.len();
    let mut effects = Vec::with_capacity(count);
    let mut calls = Vec::with_capacity(count);
    let mut callers = vec![Vec::new(); count];
    for (id, binding) in bindings.iter().enumerate() {
        let (effect, targets) = local(&binding.program)?;
        for target in &targets { callers.get_mut(*target).ok_or("调用图目标越界")?.push(id); }
        effects.push(effect); calls.push(targets);
    }
    let mut remaining = calls.iter().map(Vec::len).collect::<Vec<_>>();
    let mut pending = (0..count).filter(|id| remaining[*id] == 0).collect::<VecDeque<_>>();
    while let Some(id) = pending.pop_front() {
        for caller in &callers[id] {
            remaining[*caller] -= 1;
            if remaining[*caller] == 0 { pending.push_back(*caller); }
        }
    }
    for id in 0..count {
        if remaining[id] != 0 { effects[id] = effects[id].union(Effects::MAY_DIVERGE); }
    }
    let mut pending = (0..count).collect::<VecDeque<_>>();
    let mut queued = vec![true; count];
    while let Some(id) = pending.pop_front() {
        queued[id] = false;
        for caller in &callers[id] {
            let next = effects[*caller].union(effects[id]);
            if next != effects[*caller] {
                effects[*caller] = next;
                if !queued[*caller] { pending.push_back(*caller); queued[*caller] = true; }
            }
        }
    }
    Ok(effects)
}

/// 有界的直线标量内联：不复制闭包身份、Host 操作或构造能力，不改变可能 fault 的算术顺序。
pub(crate) fn inline_scalar_calls(program: &mut Program, callees: &[std::sync::Arc<super::image::ValidatedProgram>], remaining: &mut usize) -> Result<bool, String> {
    use super::bytecode::Instruction as I;
    let eligible = |callee: &Program| {
        callee.instructions.len() <= 16 && callee.captures.is_empty()
            && callee.instructions.last().is_some_and(|i| i.opcode() == Some(Opcode::Return))
            && callee.instructions.iter().enumerate().all(|(pc, i)| match i.opcode() {
                Some(Opcode::Constant) => i.flags() != 0 || matches!(callee.constants.get(i.index() as usize), Some(Constant::Unit | Constant::None | Constant::Bool(_) | Constant::Int(_) | Constant::Float(_))),
                Some(Opcode::Move | Opcode::Unary | Opcode::IntBinary | Opcode::FloatBinary | Opcode::IntBinaryImmediate | Opcode::ConvertFloat | Opcode::IsSome) => true,
                Some(Opcode::Return) => pc + 1 == callee.instructions.len(),
                _ => false,
            })
    };
    let original = program.instructions.clone();
    let mut instructions = Vec::new(); let mut spans = Vec::new(); let mut relocation = Vec::with_capacity(original.len());
    let mut changed = false;
    for (pc, instruction) in original.iter().copied().enumerate() {
        relocation.push(u32::try_from(instructions.len()).map_err(|_| "内联程序过大")?);
        let callee = if instruction.opcode() == Some(Opcode::CallDirect) {
            let site = program.direct_calls.get(instruction.index() as usize).ok_or("内联调用附表越界")?;
            let target = callees.get(site.function.0 as usize).ok_or("内联目标越界")?;
            let cost = target.instructions.len() + target.parameters.len();
            (cost <= *remaining && program.registers.len() + target.registers.len() <= 65_536 && eligible(target))
                .then_some((target, site, cost))
        } else { None };
        if let Some((callee, site, cost)) = callee {
            let arguments = program.operands(site.arguments_start, site.arguments_len).ok_or("内联参数越界")?.to_vec();
            if arguments.len() != callee.parameters.len() { return Err("内联参数数量不匹配".into()); }
            let base = program.registers.len(); program.registers.extend(callee.registers.iter().cloned());
            let register = |value: u16| -> u16 { (base + usize::from(value)) as u16 };
            for (index, argument) in arguments.into_iter().enumerate() { instructions.push(I::new(Opcode::Move, register(index as u16), argument, 0, 0)); }
            for inner in &callee.instructions {
                let opcode = inner.opcode().ok_or("内联指令无效")?;
                let rewritten = match opcode {
                    Opcode::Return => I::new(Opcode::Move, instruction.a(), register(inner.a()), 0, 0),
                    Opcode::Constant if inner.flags() == 0 => match &callee.constants[inner.index() as usize] {
                        Constant::Int(value) => I::indexed(Opcode::Constant, register(inner.a()), *value as u32).with_flags(1),
                        Constant::Float(value) => I::indexed(Opcode::Constant, register(inner.a()), value.to_bits()).with_flags(2),
                        value => I::new(Opcode::Constant, register(inner.a()), match value { Constant::Unit => 0, Constant::None => 1, Constant::Bool(false) => 2, Constant::Bool(true) => 3, _ => unreachable!() }, 0, 3),
                    },
                    Opcode::Constant | Opcode::IntBinaryImmediate => I::indexed(opcode, register(inner.a()), inner.index()).with_flags(inner.flags()),
                    Opcode::IntBinary | Opcode::FloatBinary => I::new(opcode, register(inner.a()), register(inner.b()), register(inner.c()), inner.flags()),
                    _ => I::new(opcode, register(inner.a()), register(inner.b()), 0, inner.flags()),
                };
                instructions.push(rewritten);
            }
            spans.resize(instructions.len(), program.spans[pc]); *remaining -= cost; changed = true;
        } else { instructions.push(instruction); spans.push(program.spans[pc]); }
    }
    if !changed { return Ok(false); }
    // 原分支仅指向原节点；内联片段内部不含跳转，因此所有旧目标统一重定位。
    for (pc, original) in original.iter().enumerate() {
        if matches!(original.opcode(), Some(Opcode::Jump | Opcode::JumpFalse)) {
            let instruction = &mut instructions[relocation[pc] as usize];
            *instruction = I::indexed(original.opcode().unwrap(), original.a(), *relocation.get(original.index() as usize).ok_or("内联跳转越界")?).with_flags(original.flags());
        }
    }
    for site in &mut program.for_sites { site.target = *relocation.get(site.target as usize).ok_or("内联循环边越界")?; }
    program.instructions = instructions; program.spans = spans;
    program.allocate_registers()?;
    Ok(true)
}
