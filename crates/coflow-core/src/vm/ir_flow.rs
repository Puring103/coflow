//! 语义 IR 的统一控制流与读写集合，验证器和 SSA 共用同一解释。
use super::ir::{Function, Operation, IrValueId};
use std::collections::BTreeSet;
pub(super) struct Flow {
    pub reads: Vec<Vec<IrValueId>>,
    pub writes: Vec<Vec<IrValueId>>,
    pub successors: Vec<Vec<usize>>,
    pub reachable: BTreeSet<usize>,
}
impl Function {
    /// 扩展节点后统一重定位所有控制流目标；附表在后续降低时重新生成。
    pub(super) fn rewrite_nodes(
        &mut self,
        mut expansions: std::collections::BTreeMap<usize, Vec<super::ir::Node>>,
        removed: &BTreeSet<usize>,
    ) {
        let mut positions = Vec::with_capacity(self.body.len() + 1);
        let mut position = 0usize;
        for pc in 0..self.body.len() {
            positions.push(position);
            position += expansions.get(&pc).map_or(1, Vec::len);
        }
        positions.push(position);
        let mut body = Vec::with_capacity(position);
        for (pc, mut node) in std::mem::take(&mut self.body).into_iter().enumerate() {
            if removed.contains(&pc) {
                node.operation = Operation::Jump(super::ir::NodeIndex((pc + 1) as u32));
            }
            for mut node in expansions.remove(&pc).unwrap_or_else(|| vec![node]) {
                match &mut node.operation {
                    Operation::Jump(target)
                    | Operation::JumpFalse { target, .. }
                    | Operation::ForPrep { target, .. }
                    | Operation::ForLoop { target, .. } => {
                        target.0 = positions[target.0 as usize] as u32
                    }
                    _ => {}
                }
                body.push(node);
            }
        }
        self.body = body;
    }
    pub(super) fn reachable(&self) -> Result<BTreeSet<usize>, String> {
        let mut reachable = BTreeSet::new();
        let mut pending = vec![0];
        while let Some(pc) = pending.pop() {
            let node = self.body.get(pc).ok_or("IR 控制流越界")?;
            if !reachable.insert(pc) {
                continue;
            }
            match node.operation {
                Operation::Return(_) => {}
                Operation::Jump(target) => pending.push(target.0 as usize),
                Operation::JumpFalse { target, .. }
                | Operation::ForPrep { target, .. }
                | Operation::ForLoop { target, .. } => {
                    pending.push(target.0 as usize);
                    pending.push(pc + 1);
                }
                _ => pending.push(pc + 1),
            }
        }
        Ok(reachable)
    }
    pub(super) fn flow(&self) -> Result<Flow, String> {
        let reachable = self.reachable()?;
        let mut reads = Vec::with_capacity(self.body.len());
        let mut writes = Vec::with_capacity(self.body.len());
        let mut successors = Vec::with_capacity(self.body.len());
        for (pc, node) in self.body.iter().enumerate() {
            use Operation as O;
            let mut input = Vec::new();
            let mut output = vec![node.destination];
            let mut next = vec![pc + 1];
            match &node.operation {
                O::Build(operation) => input.extend(operation.inputs()),
                O::Constant(_)
                | O::Owner
                | O::Capture(_)
                | O::OwnerField(_)
                | O::Reference(_)
                | O::ReserveObject { .. } => {}
                O::Copy(value)
                | O::ConvertFloat(value)
                | O::IsSome(value)
                | O::ReadTemplate(value)
                | O::Length(value)
                | O::Unary { value, .. }
                | O::IsType { value, .. } => input.push(*value),
                O::Field { receiver, .. } | O::IndexConstant { receiver, .. } => {
                    input.push(*receiver)
                }
                O::Index { receiver, key } => input.extend([*receiver, *key]),
                O::Binary { left, right, .. } | O::AccumulateText { left, right } => {
                    input.extend([*left, *right])
                }
                O::Jump(target) => {
                    output.clear();
                    next = vec![target.0 as usize];
                }
                O::JumpFalse { condition, target } => {
                    output.clear();
                    input.push(*condition);
                    next.push(target.0 as usize);
                }
                O::Return(value) => {
                    input.push(*value);
                    output.clear();
                    next.clear();
                }
                O::Call { target, arguments } => {
                    input.push(*target);
                    input.extend(arguments);
                }
                O::Closure {
                    captures, owner, ..
                } => {
                    input.extend(captures);
                    input.extend(owner);
                }
                O::Array(values)
                | O::Dictionary(values)
                | O::Format(values)
                | O::Concat(values) => input.extend(values),
                O::InitializeObject { fields, .. } => {
                    input.push(node.destination);
                    input.extend(fields.iter().map(|(_, value)| *value));
                }
                O::IteratorValue { collection, index } => input.extend([*collection, *index]),
                O::Builtin {
                    receiver,
                    arguments,
                    ..
                } => {
                    input.push(*receiver);
                    input.extend(arguments);
                }
                O::Iteration => output.clear(),
                O::ForPrep { limit, target, .. } | O::ForLoop { limit, target, .. } => {
                    input.extend([node.destination, *limit]);
                    next.push(target.0 as usize);
                    if matches!(node.operation, O::ForPrep { .. }) {
                        output.clear();
                    }
                }
                O::IterNext {
                    collection,
                    counter,
                    key,
                    value,
                } => {
                    input.extend([*collection, *counter]);
                    output = vec![*key, *value];
                }
            }
            if input
                .iter()
                .chain(&output)
                .any(|value| value.0 as usize >= self.values.len())
            {
                return Err("IR 定义或使用的值编号越界".into());
            }
            // 不可达的隐式出口允许存在；所有显式分支仍必须指向真实节点。
            let branch = match &node.operation {
                O::Jump(target)
                | O::JumpFalse { target, .. }
                | O::ForPrep { target, .. }
                | O::ForLoop { target, .. } => Some(target.0 as usize),
                _ => None,
            };
            if branch.is_some_and(|target| target >= self.body.len()) {
                return Err("IR 分支目标越界".into());
            }
            for target in &next {
                if *target >= self.body.len() && (reachable.contains(&pc) || *target != pc + 1) {
                    return Err("IR 控制流越界".into());
                }
            }
            reads.push(input);
            writes.push(output);
            successors.push(next);
        }
        Ok(Flow {
            reads,
            writes,
            successors,
            reachable,
        })
    }
}
