//! 构造能力与未发布 self 绑定的控制流验证；解码后的 IR 也必须满足同一边界。
use super::{construction::BuildOp as B, ir::{Function, Operation as O, IrValueId}};
use crate::schema::CftValueType as Ty;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Default, PartialEq, Eq)]
struct State {
    active: BTreeSet<IrValueId>,
    possible: BTreeSet<IrValueId>,
    dependencies: BTreeMap<IrValueId, BTreeSet<IrValueId>>,
}
fn carries_binding(ty: &Ty) -> bool {
    match ty {
        Ty::Function(..) | Ty::FString => true,
        Ty::Array(inner) | Ty::Option(inner) => carries_binding(inner),
        Ty::Dict(_, inner) => carries_binding(inner),
        _ => false,
    }
}
impl Function {
    pub(super) fn validate_construction(&self, reads: &[Vec<IrValueId>], writes: &[Vec<IrValueId>], successors: &[Vec<usize>]) -> Result<(), String> {
        let explicit = self.body.iter().filter_map(|node| matches!(node.operation, O::Build(B::Start { .. })).then_some(node.destination)).collect::<BTreeSet<_>>();
        let implicit = self.body.iter().filter_map(|node| matches!(node.operation, O::ReserveObject { .. }).then_some(node.destination)).collect::<BTreeSet<_>>();
        if explicit.is_empty() && implicit.is_empty() { return Ok(()); }
        // 构造身份在循环中重复执行；丢弃前禁止把依赖旧身份的绑定带到作用域外。
        let mut live = vec![BTreeSet::<IrValueId>::new(); self.body.len()];
        loop {
            let mut changed = false;
            for pc in (0..self.body.len()).rev() {
                let mut values = successors[pc].iter().flat_map(|next| live[*next].iter().copied()).collect::<BTreeSet<_>>();
                for output in &writes[pc] { values.remove(output); }
                values.extend(&reads[pc]);
                if live[pc] != values { live[pc] = values; changed = true; }
            }
            if !changed { break; }
        }
        let mut incoming = vec![None::<State>; self.body.len()];
        incoming[0] = Some(State::default());
        let mut queue = VecDeque::from([0usize]);
        let mut steps = 0usize;
        while let Some(pc) = queue.pop_front() {
            steps += 1;
            if steps > 1_000_000 { return Err("构造能力控制流验证工作量超限".into()); }
            let mut state = incoming[pc].clone().unwrap();
            let node = &self.body[pc];
            let operation = &node.operation;
            for input in &reads[pc] {
                if !state.possible.contains(input) { continue; }
                let permitted = match operation {
                    O::Build(B::DefaultField { owner, .. }) => input == owner,
                    O::Build(B::Set { builder, .. } | B::Append { builder, .. } | B::Remove { builder, .. } | B::Freeze { builder } | B::Drop { builder }) => input == builder,
                    O::Index { receiver, .. } | O::IndexConstant { receiver, .. } => input == receiver,
                    O::Length(receiver) => input == receiver,
                    O::InitializeObject { .. } => *input == node.destination,
                    O::Closure { owner, .. } => owner.as_ref() == Some(input),
                    _ => false,
                };
                if !permitted { return Err("构造能力不能复制、捕获、存储或逃逸".into()); }
            }
            if let O::Build(B::Set { builder, .. } | B::Append { builder, .. } | B::Remove { builder, .. } | B::Freeze { builder } | B::Drop { builder }) = operation {
                if !explicit.contains(builder) || !state.active.contains(builder) { return Err("构造写权限未建立或已经消费".into()); }
            }
            let dependencies = reads[pc].iter().flat_map(|id| state.dependencies.get(id).into_iter().flatten().copied()).collect::<BTreeSet<_>>();
            if matches!(operation, O::Call { .. } | O::Return(_) | O::ReadTemplate(_) | O::Builtin { .. })
                && dependencies.iter().any(|id| state.possible.contains(id)) {
                return Err("未冻结或已丢弃对象的 self 绑定不能执行或向外传递".into());
            }
            let mut result_dependencies = dependencies.clone();
            match operation {
                O::Build(B::Start { .. }) | O::ReserveObject { .. } => {
                    state.active.insert(node.destination);
                    state.possible.insert(node.destination);
                    if matches!(self.values[node.destination.0 as usize], Ty::Object(_)) { result_dependencies.insert(node.destination); }
                }
                O::Build(B::Freeze { builder }) => {
                    state.active.remove(builder); state.possible.remove(builder);
                    for dependencies in state.dependencies.values_mut() { dependencies.remove(builder); }
                    result_dependencies.remove(builder);
                }
                O::Build(B::Drop { builder }) => {
                    let live_after = successors[pc].iter().flat_map(|next| live[*next].iter()).collect::<BTreeSet<_>>();
                    if state.dependencies.iter().any(|(value, dependencies)| value != builder && live_after.contains(value) && dependencies.contains(builder)) {
                        return Err("丢弃构造对象时不能保留依赖其 self 的绑定".into());
                    }
                    state.active.remove(builder); state.possible.remove(builder);
                    for dependencies in state.dependencies.values_mut() { dependencies.remove(builder); }
                    result_dependencies.clear();
                }
                O::Build(B::Set { builder, .. } | B::Append { builder, .. }) => {
                    state.dependencies.entry(*builder).or_default().extend(&dependencies);
                    result_dependencies.clear();
                }
                O::Build(B::DefaultField { owner, .. }) => {
                    result_dependencies.clear();
                    if carries_binding(&self.values[node.destination.0 as usize]) { result_dependencies.insert(*owner); }
                }
                O::InitializeObject { .. } if implicit.contains(&node.destination) => {
                    state.active.remove(&node.destination); state.possible.remove(&node.destination);
                    for dependencies in state.dependencies.values_mut() { dependencies.remove(&node.destination); }
                    result_dependencies.remove(&node.destination);
                }
                O::Constant(_) | O::Owner | O::Capture(_) | O::Reference(_) | O::OwnerField(_) => result_dependencies.clear(),
                _ => {}
            }
            for output in &writes[pc] {
                let ty = &self.values[output.0 as usize];
                if matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Enum(_) | Ty::Unit) {
                    state.dependencies.remove(output);
                } else if result_dependencies.is_empty() { state.dependencies.remove(output); }
                else { state.dependencies.insert(*output, result_dependencies.clone()); }
            }
            for &successor in &successors[pc] {
                let target = incoming.get_mut(successor).ok_or("构造控制流越界")?;
                let changed = if let Some(previous) = target {
                    let before = previous.clone();
                    previous.active.retain(|id| state.active.contains(id));
                    previous.possible.extend(&state.possible);
                    for (value, dependencies) in &state.dependencies { previous.dependencies.entry(*value).or_default().extend(dependencies); }
                    *previous != before
                } else { *target = Some(state.clone()); true };
                if changed { queue.push_back(successor); }
            }
        }
        Ok(())
    }
}
