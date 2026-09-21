//! 在语义值编号上建立 CFG/SSA 版本和循环 phi；物理寄存器分配发生在本阶段之后。
use super::{
    bytecode::Constant,
    executor::{scalar_binary, Slot},
    ir::{Function, Operation as O, ValueId},
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version(usize);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ExpressionInput {
    Version(Version),
    Constant(u8, u32),
}
#[derive(Clone)]
enum Definition {
    Input,
    Write(usize),
    Phi(Vec<Option<Version>>),
}
#[derive(Clone, Debug)]
enum Fact {
    Pending,
    Constant(Constant),
    Unknown,
}
impl Fact {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pending, Self::Pending) | (Self::Unknown, Self::Unknown) => true,
            (Self::Constant(Constant::Float(a)), Self::Constant(Constant::Float(b))) => {
                a.to_bits() == b.to_bits()
            }
            (Self::Constant(a), Self::Constant(b)) => a == b,
            _ => false,
        }
    }
    fn join(&self, other: &Self) -> Self {
        if matches!(self, Self::Pending) {
            return other.clone();
        }
        if matches!(other, Self::Pending) || self.same(other) {
            return self.clone();
        }
        Self::Unknown
    }
}
fn scalar(value: &Constant) -> Option<Slot> {
    Some(match value {
        Constant::Unit => Slot::Unit,
        Constant::None => Slot::None,
        Constant::Bool(v) => Slot::Bool(*v),
        Constant::Int(v) => Slot::Int(*v),
        Constant::Float(v) => Slot::Float(*v),
        _ => return None,
    })
}
fn constant(value: Slot) -> Option<Constant> {
    Some(match value {
        Slot::Unit => Constant::Unit,
        Slot::None => Constant::None,
        Slot::Bool(v) => Constant::Bool(v),
        Slot::Int(v) => Constant::Int(v),
        Slot::Float(v) => Constant::Float(v),
        _ => return None,
    })
}
fn evaluate(operation: &O, input: impl Fn(ValueId) -> Fact) -> Fact {
    match operation {
        O::Constant(value) if scalar(value).is_some() => Fact::Constant(value.clone()),
        O::Copy(value) => input(*value),
        O::Binary {
            operator,
            left,
            right,
        } => {
            let (left, right) = (input(*left), input(*right));
            match (left, right) {
                (Fact::Unknown, _) | (_, Fact::Unknown) => Fact::Unknown,
                (Fact::Pending, _) | (_, Fact::Pending) => Fact::Pending,
                (Fact::Constant(a), Fact::Constant(b)) => scalar(&a)
                    .zip(scalar(&b))
                    .and_then(|(a, b)| scalar_binary(*operator, a, b))
                    .and_then(Result::ok)
                    .and_then(constant)
                    .map_or(Fact::Unknown, Fact::Constant),
            }
        }
        O::ConvertFloat(value) => match input(*value) {
            Fact::Constant(Constant::Int(value)) => Fact::Constant(Constant::Float(value as f32)),
            other => other,
        },
        O::IsSome(value) => match input(*value) {
            Fact::Constant(value) => Fact::Constant(Constant::Bool(value != Constant::None)),
            other => other,
        },
        O::Unary { operator, value } => match input(*value) {
            Fact::Constant(value) => {
                let value = match (operator, value) {
                    (0, Constant::Int(value)) => value.checked_neg().map(Constant::Int),
                    (0, Constant::Float(value)) => Some(Constant::Float(-value)),
                    (1, Constant::Bool(value)) => Some(Constant::Bool(!value)),
                    (2, Constant::Int(value)) => Some(Constant::Int(!value)),
                    _ => None,
                };
                value.map_or(Fact::Unknown, Fact::Constant)
            }
            other => other,
        },
        _ => Fact::Unknown,
    }
}

impl Function {
    /// 工作量上限只使优化回退，不拒绝原本合法的函数。
    pub(super) fn propagate_ssa(&mut self) -> Result<(), String> {
        for node in &mut self.body {
            if let O::Closure { function, .. } = &mut node.operation {
                function.propagate_ssa()?;
            }
        }
        let flow = self.flow()?;
        let count = self.body.len();
        let mut predecessors = vec![Vec::new(); count];
        for &pc in &flow.reachable {
            for &next in &flow.successors[pc] {
                predecessors[next].push(pc);
            }
        }
        let mut definitions = vec![Definition::Input; self.parameters.len()];
        let parameters = (0..self.parameters.len())
            .map(|i| (ValueId(i as u32), Version(i)))
            .collect::<BTreeMap<_, _>>();
        let mut writes = vec![BTreeMap::new(); count];
        for &pc in &flow.reachable {
            for value in &flow.writes[pc] {
                let version = Version(definitions.len());
                definitions.push(Definition::Write(pc));
                writes[pc].insert(*value, version);
            }
        }
        let mut incoming = vec![BTreeMap::new(); count];
        let mut outgoing = vec![None::<BTreeMap<ValueId, Version>>; count];
        let mut phis = BTreeMap::<(usize, ValueId), Version>::new();
        let mut pending = VecDeque::from([0]);
        let mut queued = vec![false; count];
        queued[0] = true;
        let mut work = 0usize;
        while let Some(pc) = pending.pop_front() {
            queued[pc] = false;
            let mut sources = predecessors[pc]
                .iter()
                .filter_map(|p| outgoing[*p].as_ref())
                .collect::<Vec<_>>();
            if pc == 0 {
                sources.push(&parameters);
            }
            let variables = sources
                .iter()
                .flat_map(|source| source.keys().copied())
                .collect::<BTreeSet<_>>();
            work = work.saturating_add(variables.len().saturating_mul(sources.len().max(1)));
            if work > 1_000_000 {
                return Ok(());
            }
            let mut merged = BTreeMap::new();
            for value in variables {
                let arguments = sources
                    .iter()
                    .map(|source| source.get(&value).copied())
                    .collect::<Vec<_>>();
                let first = arguments.first().copied().flatten();
                let key = (pc, value);
                if phis.contains_key(&key) || arguments.iter().any(|argument| *argument != first) {
                    let version = *phis.entry(key).or_insert_with(|| {
                        let version = Version(definitions.len());
                        definitions.push(Definition::Phi(Vec::new()));
                        version
                    });
                    definitions[version.0] = Definition::Phi(arguments);
                    merged.insert(value, version);
                } else if let Some(version) = first {
                    merged.insert(value, version);
                }
            }
            incoming[pc] = merged.clone();
            merged.extend(writes[pc].iter().map(|(value, version)| (*value, *version)));
            if outgoing[pc].as_ref() != Some(&merged) {
                outgoing[pc] = Some(merged);
                for &next in &flow.successors[pc] {
                    if !queued[next] {
                        pending.push_back(next);
                        queued[next] = true;
                    }
                }
            }
        }
        let mut facts = vec![Fact::Pending; definitions.len()];
        loop {
            let mut changed = false;
            for (id, definition) in definitions.iter().enumerate() {
                work += 1;
                if work > 2_000_000 {
                    return Ok(());
                }
                let fact = match definition {
                    Definition::Input => Fact::Unknown,
                    Definition::Write(pc) => evaluate(&self.body[*pc].operation, |value| {
                        incoming[*pc]
                            .get(&value)
                            .map_or(Fact::Unknown, |version| facts[version.0].clone())
                    }),
                    Definition::Phi(arguments) => {
                        arguments.iter().fold(Fact::Pending, |fact, argument| {
                            fact.join(
                                &argument.map_or(Fact::Unknown, |version| facts[version.0].clone()),
                            )
                        })
                    }
                };
                let next = facts[id].join(&fact);
                if !facts[id].same(&next) {
                    facts[id] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for &pc in &flow.reachable {
            if !matches!(
                self.body[pc].operation,
                O::Copy(_) | O::Binary { .. } | O::Unary { .. } | O::ConvertFloat(_) | O::IsSome(_)
            ) {
                continue;
            }
            if let Some(version) = writes[pc].get(&self.body[pc].destination) {
                if let Fact::Constant(value) = &facts[version.0] {
                    self.body[pc].operation = O::Constant(value.clone());
                }
            }
        }
        // 只在同一基本块复用已成功执行的标量表达式。即使算术可能 fault，第二次使用
        // 相同输入也不可能新增 fault；不外提第一次执行，不触碰 Host、模板或身份创建。
        let expression_input = |mut version: Version| {
            for _ in 0..64 {
                match &facts[version.0] {
                    Fact::Constant(Constant::Int(value)) => {
                        return ExpressionInput::Constant(0, *value as u32)
                    }
                    Fact::Constant(Constant::Float(value)) => {
                        return ExpressionInput::Constant(1, value.to_bits())
                    }
                    Fact::Constant(Constant::Bool(value)) => {
                        return ExpressionInput::Constant(2, *value as u32)
                    }
                    Fact::Constant(Constant::None) => return ExpressionInput::Constant(3, 0),
                    _ => {}
                }
                let Definition::Write(pc) = definitions[version.0] else {
                    break;
                };
                let O::Copy(value) = self.body[pc].operation else {
                    break;
                };
                let Some(&source) = incoming[pc].get(&value) else {
                    break;
                };
                version = source;
            }
            ExpressionInput::Version(version)
        };
        let canonical_inputs = (0..definitions.len())
            .map(|id| expression_input(Version(id)))
            .collect::<Vec<_>>();
        let incomplete_aliases = canonical_inputs.iter().any(|input| {
            let ExpressionInput::Version(version) = input else {
                return false;
            };
            let Definition::Write(pc) = definitions[version.0] else {
                return false;
            };
            matches!(self.body[pc].operation, O::Copy(_))
        });
        let mut expressions = BTreeMap::<(u8, u8, Vec<ExpressionInput>), (ValueId, Version)>::new();
        let mut previous: Option<usize> = None;
        for &pc in &flow.reachable {
            if previous.is_none_or(|previous| {
                predecessors[pc].as_slice() != [previous]
                    || flow.successors[previous].as_slice() != [pc]
            }) {
                expressions.clear();
            }
            previous = Some(pc);
            let scalar_type = |value: ValueId| {
                matches!(
                    self.values[value.0 as usize],
                    crate::schema::CftValueType::Int
                        | crate::schema::CftValueType::Float
                        | crate::schema::CftValueType::Bool
                )
            };
            let candidate = match &self.body[pc].operation {
                O::Binary {
                    operator,
                    left,
                    right,
                } if scalar_type(*left) && scalar_type(*right) => {
                    Some((0, *operator, vec![*left, *right]))
                }
                O::Unary { operator, value } if scalar_type(*value) => {
                    Some((1, *operator, vec![*value]))
                }
                O::ConvertFloat(value) => Some((2, 0, vec![*value])),
                O::IsSome(value) => Some((3, 0, vec![*value])),
                _ => None,
            };
            let Some((kind, operator, inputs)) = candidate else {
                continue;
            };
            let Some(versions) = inputs
                .iter()
                .map(|value| {
                    incoming[pc]
                        .get(value)
                        .map(|version| canonical_inputs[version.0].clone())
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let key = (kind, operator, versions);
            if let Some(&(value, version)) = expressions.get(&key) {
                if incoming[pc].get(&value) == Some(&version) {
                    self.body[pc].operation = O::Copy(value);
                    continue;
                }
            }
            let destination = self.body[pc].destination;
            if let Some(&version) = writes[pc].get(&destination) {
                expressions.insert(key, (destination, version));
            }
        }
        // 连接只拼接已经求值的 string；利用 SSA 证明叶值没有被覆盖，且中间结果仅使用一次。
        let mut uses = vec![0usize; definitions.len()];
        for &pc in &flow.reachable {
            for value in &flow.reads[pc] {
                if let Some(version) = incoming[pc].get(value) {
                    uses[version.0] += 1;
                }
            }
        }
        // 仅实际被读取的 phi 才传播使用关系；循环携带的死临时值不是逃逸根。
        let mut pending = uses
            .iter()
            .enumerate()
            .filter_map(|(id, uses)| (*uses != 0).then_some(id))
            .collect::<VecDeque<_>>();
        let mut visited = vec![false; definitions.len()];
        while let Some(id) = pending.pop_front() {
            if visited[id] {
                continue;
            }
            visited[id] = true;
            if let Definition::Phi(arguments) = &definitions[id] {
                for version in arguments.iter().flatten() {
                    uses[version.0] += 1;
                    if !visited[version.0] {
                        pending.push_back(version.0);
                    }
                }
            }
        }
        // 循环携带的字符串只有在旧前缀完全不可观察时才可改为可增长缓冲。
        // 回边结果必须只进入同一个 phi，phi 本身也只能由这一处追加读取。
        let mut direct_uses = vec![0usize; definitions.len()];
        for &consumer in &flow.reachable {
            for value in &flow.reads[consumer] {
                if let Some(version) = incoming[consumer].get(value) {
                    direct_uses[version.0] += 1;
                }
            }
        }
        for &pc in &flow.reachable {
            let O::Binary {
                operator: 0,
                left,
                right,
            } = self.body[pc].operation
            else {
                continue;
            };
            if self.values[left.0 as usize] != crate::schema::CftValueType::String
                || self.values[right.0 as usize] != crate::schema::CftValueType::String
            {
                continue;
            }
            let Some(&prefix) = incoming[pc].get(&left) else {
                continue;
            };
            let Some(&result) = writes[pc].get(&self.body[pc].destination) else {
                continue;
            };
            let Some((&(header, variable), _)) =
                phis.iter().find(|(_, version)| **version == prefix)
            else {
                continue;
            };
            let Definition::Phi(arguments) = &definitions[prefix.0] else {
                continue;
            };
            let mut backedge = result;
            let mut allowed_result_reads = 0usize;
            if !arguments
                .iter()
                .flatten()
                .any(|version| *version == backedge)
            {
                let copies = flow
                    .reachable
                    .iter()
                    .filter_map(|consumer| {
                        let O::Copy(source) = self.body[*consumer].operation else {
                            return None;
                        };
                        (incoming[*consumer].get(&source) == Some(&result)
                            && self.body[*consumer].destination == variable)
                            .then_some(*consumer)
                    })
                    .collect::<Vec<_>>();
                if copies.len() != 1 {
                    continue;
                }
                let Some(&copy_result) = writes[copies[0]].get(&variable) else {
                    continue;
                };
                backedge = copy_result;
                allowed_result_reads = 1;
            }
            let mut reaches_header = vec![false; count];
            let mut reverse = VecDeque::from([header]);
            reaches_header[header] = true;
            while let Some(node) = reverse.pop_front() {
                for &predecessor in &predecessors[node] {
                    if !reaches_header[predecessor] {
                        reaches_header[predecessor] = true;
                        reverse.push_back(predecessor);
                    }
                }
            }
            let loop_prefix_uses = flow
                .reachable
                .iter()
                .filter(|consumer| reaches_header[**consumer])
                .map(|consumer| {
                    flow.reads[*consumer]
                        .iter()
                        .filter(|value| incoming[*consumer].get(value) == Some(&prefix))
                        .count()
                })
                .sum::<usize>();
            if header > pc
                || loop_prefix_uses != 1
                || (variable != self.body[pc].destination && allowed_result_reads == 0)
                || !arguments
                    .iter()
                    .flatten()
                    .any(|version| *version == backedge)
            {
                continue;
            }
            if direct_uses[result.0] != allowed_result_reads {
                continue;
            }
            let mut header_phi_uses = 0usize;
            let mut other_backedge_use = direct_uses[backedge.0] != 0;
            for (id, definition) in definitions.iter().enumerate() {
                if let Definition::Phi(arguments) = definition {
                    let count = arguments
                        .iter()
                        .flatten()
                        .filter(|version| **version == backedge)
                        .count();
                    if count != 0 {
                        if id == prefix.0 {
                            header_phi_uses += count;
                        } else if phis
                            .iter()
                            .find_map(|((block, _), version)| (version.0 == id).then_some(*block))
                            .is_some_and(|block| reaches_header[block])
                        {
                            other_backedge_use = true;
                        }
                    }
                }
            }
            if other_backedge_use || header_phi_uses != 1 {
                continue;
            }
            self.body[pc].operation = O::AccumulateText { left, right };
        }
        for &pc in &flow.reachable {
            let O::Binary {
                operator: 0,
                left,
                right,
            } = self.body[pc].operation
            else {
                continue;
            };
            if self.values[left.0 as usize] != crate::schema::CftValueType::String
                || self.values[right.0 as usize] != crate::schema::CftValueType::String
            {
                continue;
            }
            let mut parts = Vec::new();
            let mut consumed = Vec::new();
            for value in [left, right] {
                let child = incoming[pc].get(&value).and_then(|version| {
                    if uses[version.0] != 1 {
                        return None;
                    }
                    if let Definition::Write(child) = definitions[version.0] {
                        Some(child)
                    } else {
                        None
                    }
                });
                if let Some(child) = child {
                    if let O::Concat(children) = &self.body[child].operation {
                        if !children.is_empty()
                            && children
                                .iter()
                                .all(|value| incoming[pc].get(value) == incoming[child].get(value))
                            && parts.len() + children.len() < 4096
                        {
                            parts.extend(children.iter().copied());
                            consumed.push((child, children[0]));
                            continue;
                        }
                    }
                }
                parts.push(value);
            }
            self.body[pc].operation = O::Concat(parts);
            for (child, value) in consumed {
                self.body[child].operation = O::Copy(value);
            }
        }
        // 非逃逸数组只用于已证明有效的固定索引/长度时，直接保留元素值。
        // 所有元素表达式仍在原位置执行；模板普通读取、越界、phi 和未知用途一律保留数组。
        // 复制链追踪预算耗尽时不能把尚未解析的别名误判为无使用。
        if incomplete_aliases {
            return Ok(());
        }
        let mut replacement_work = 0usize;
        for &pc in &flow.reachable {
            let O::Array(elements) = &self.body[pc].operation else {
                continue;
            };
            let elements = elements.clone();
            let Some(&version) = writes[pc].get(&self.body[pc].destination) else {
                continue;
            };
            let identity = ExpressionInput::Version(version);
            let resolves = |version: Version| canonical_inputs[version.0] == identity;
            if definitions.iter().enumerate().any(|(id, definition)| uses[id] != 0 && matches!(definition,
                Definition::Phi(arguments) if arguments.iter().flatten().any(|version| resolves(*version)))) { continue; }
            let mut replacements = Vec::new();
            let mut safe = true;
            for &consumer in &flow.reachable {
                replacement_work += flow.reads[consumer].len() + 1;
                if replacement_work > 1_000_000 {
                    return Ok(());
                }
                let is_array = |value: ValueId| {
                    incoming[consumer]
                        .get(&value)
                        .is_some_and(|version| resolves(*version))
                };
                if !flow.reads[consumer].iter().any(|value| is_array(*value)) {
                    continue;
                }
                let index = match &self.body[consumer].operation {
                    O::Copy(value) if is_array(*value) => {
                        replacements.push((
                            consumer,
                            O::Jump(super::ir::LocationId((consumer + 1) as u32)),
                        ));
                        continue;
                    }
                    O::Length(value) if is_array(*value) => {
                        let Ok(length) = i32::try_from(elements.len()) else {
                            safe = false;
                            break;
                        };
                        replacements.push((consumer, O::Constant(Constant::Int(length))));
                        continue;
                    }
                    O::IndexConstant {
                        receiver,
                        key: Constant::Int(index),
                    } if is_array(*receiver) => Some(*index),
                    O::Index { receiver, key } if is_array(*receiver) => incoming[consumer]
                        .get(key)
                        .and_then(|version| match facts[version.0] {
                            Fact::Constant(Constant::Int(index)) => Some(index),
                            _ => None,
                        }),
                    _ => None,
                };
                let element = index
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| elements.get(index))
                    .copied();
                let Some(element) = element else {
                    safe = false;
                    break;
                };
                if incoming[pc].get(&element) != incoming[consumer].get(&element)
                    || self.values[element.0 as usize] == crate::schema::CftValueType::FString
                {
                    safe = false;
                    break;
                }
                replacements.push((consumer, O::Copy(element)));
            }
            if safe {
                self.body[pc].operation = O::Jump(super::ir::LocationId((pc + 1) as u32));
                for (consumer, replacement) in replacements {
                    self.body[consumer].operation = replacement;
                }
            }
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "cft-compiler"))]
mod tests {
    use super::*;
    use crate::{
        schema::{build_schema, parse_modules, CftFile, ModuleId},
        vm::compiler::{analyze, CompileContext},
    };
    fn function(source: &str) -> Function {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("ssa"), "table Item {}")]);
        let schema = build_schema(&modules).unwrap();
        analyze(&schema, source, "ssa", CompileContext::default()).unwrap()
    }
    #[test]
    fn scalar_replacement_requires_non_escaping_in_bounds_array_reads() {
        let mut local =
            function("fn(x: int) -> int { var values: [int] = [x, x + 1]; values[0] + values[1] }");
        local.propagate_ssa().unwrap();
        assert!(!local.body.iter().any(|node| matches!(
            node.operation,
            O::Array(_) | O::Index { .. } | O::IndexConstant { .. }
        )));
        let mut escaped = function("fn(x: int) -> [int] { [x, x + 1] }");
        escaped.propagate_ssa().unwrap();
        assert!(escaped
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Array(_))));
        let mut fault = function("fn(x: int) -> int { var values: [int] = [x]; values[2] }");
        fault.propagate_ssa().unwrap();
        assert!(fault
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Array(_))));
        let mut dynamic = function("fn(x: int) -> int { var values: [int] = [1, 2]; values[x] }");
        dynamic.propagate_ssa().unwrap();
        assert!(dynamic
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Array(_))));
        let mut looped = function("fn() -> int { var total: int = 0; for i in 0..10 { var values: [int] = [1, 2, 3]; total += values[2]; } total }");
        looped.propagate_ssa().unwrap();
        looped.lower().unwrap();
        assert!(!looped
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Array(_))));
    }
    #[test]
    fn scalar_cse_respects_versions_and_control_flow() {
        let count = |function: &Function| {
            function
                .body
                .iter()
                .filter(|node| matches!(node.operation, O::Binary { operator: 0, .. }))
                .count()
        };
        let mut repeated =
            function("fn(x: int) -> int { var a: int = x + 1; var b: int = x + 1; a + b }");
        repeated.propagate_ssa().unwrap();
        assert_eq!(count(&repeated), 2);
        let mut changed = function(
            "fn(x: int) -> int { var y: int = x; var a: int = y + 1; y = 4; a + (y + 1) }",
        );
        changed.propagate_ssa().unwrap();
        assert!(changed
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Constant(Constant::Int(5)))));
        let mut branches =
            function("fn(x: int, flag: bool) -> int { if flag { x + 1 } else { x + 1 } }");
        branches.propagate_ssa().unwrap();
        assert_eq!(count(&branches), 2);
        let mut overwritten =
            function("fn(x: int) -> int { var a: int = x + 1; a = 0; a + (x + 1) }");
        overwritten.propagate_ssa().unwrap();
        // 结果槽被覆盖后必须重新计算，不能读取已经赋成 0 的 a。
        assert!(count(&overwritten) >= 2);
    }
    #[test]
    fn phi_merges_equal_constants_and_keeps_loop_updates() {
        let mut equal = function(
            "fn(flag: bool) -> int { var value: int = if flag { 2 } else { 2 }; value + 3 }",
        );
        equal.propagate_ssa().unwrap();
        assert!(equal
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Constant(Constant::Int(5)))));
        assert!(!equal
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Binary { .. })));
        let mut looped = function("fn(count: int) -> int { var value: int = 0; for i in 0..count { value = value + i; } value }");
        looped.propagate_ssa().unwrap();
        assert!(looped
            .body
            .iter()
            .any(|node| matches!(node.operation, O::Binary { .. })));
    }
    #[test]
    fn concat_plan_eliminates_only_single_use_intermediates() {
        let mut chain = function("fn(a: string, b: string, c: string) -> string { a + b + c }");
        chain.propagate_ssa().unwrap();
        assert_eq!(
            chain
                .body
                .iter()
                .filter(|node| matches!(node.operation, O::Concat(_)))
                .count(),
            1
        );
        assert!(chain
            .body
            .iter()
            .any(|node| matches!(&node.operation, O::Concat(parts) if parts.len() == 3)));
        let mut reused = function(
            "fn(a: string, b: string) -> string { var joined: string = a + b; joined + joined }",
        );
        reused.propagate_ssa().unwrap();
        assert_eq!(
            reused
                .body
                .iter()
                .filter(|node| matches!(node.operation, O::Concat(_)))
                .count(),
            2
        );
    }
    #[test]
    fn loop_text_accumulation_requires_an_unobserved_prefix() {
        let mut local = function("fn(count: int) -> string { var text: string = \"\"; for i in 0..count { text = text + i.string(); } text }");
        local.propagate_ssa().unwrap();
        assert_eq!(
            local
                .body
                .iter()
                .filter(|node| matches!(node.operation, O::AccumulateText { .. }))
                .count(),
            1
        );

        let mut observed = function("fn(count: int) -> [string] { var text: string = \"\"; var old: string = \"\"; for i in 0..count { old = text; text = text + i.string(); } [old, text] }");
        observed.propagate_ssa().unwrap();
        assert!(!observed
            .body
            .iter()
            .any(|node| matches!(node.operation, O::AccumulateText { .. })));

        let mut captured = function("fn(count: int) -> string { var text: string = \"\"; for i in 0..count { var read: fn() -> string = fn() -> string { text }; text = text + read(); } text }");
        captured.propagate_ssa().unwrap();
        assert!(!captured
            .body
            .iter()
            .any(|node| matches!(node.operation, O::AccumulateText { .. })));
    }
    #[test]
    fn overflow_remains_dynamic_and_float_sign_bits_survive() {
        let mut overflow = function("fn() -> int { (2147483647 + 1) - 1 }");
        overflow.propagate_ssa().unwrap();
        assert_eq!(
            overflow
                .body
                .iter()
                .filter(|node| matches!(node.operation, O::Binary { .. }))
                .count(),
            2
        );
        let mut negative_zero = function("fn() -> float { -0.0 }");
        negative_zero.propagate_ssa().unwrap();
        assert!(negative_zero.body.iter().any(|node| matches!(node.operation, O::Constant(Constant::Float(value)) if value.to_bits() == (-0.0f32).to_bits())));
        assert!(!Fact::Constant(Constant::Float(0.0)).same(&Fact::Constant(Constant::Float(-0.0))));
        assert!(Fact::Constant(Constant::Float(f32::NAN))
            .same(&Fact::Constant(Constant::Float(f32::NAN))));
    }
}
