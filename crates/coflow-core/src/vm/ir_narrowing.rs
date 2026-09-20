//! 解码 IR 的类型收窄必须由实际控制流守卫证明，不能信任 Copy 的目标类型。
use super::ir::{Function, Operation as O, ValueId};
use crate::schema::{CftSchema, CftValueType as Ty};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fact { Some(ValueId), Type(ValueId, String), Invalid(ValueId) }
impl Fact { fn source(&self) -> ValueId { match self { Self::Some(value) | Self::Type(value, _) | Self::Invalid(value) => *value } } }
#[derive(Clone, PartialEq, Eq)]
struct Predicate { yes: BTreeSet<Fact>, no: BTreeSet<Fact> }
#[derive(Clone, Default, PartialEq, Eq)]
struct State { facts: BTreeSet<Fact>, predicates: BTreeMap<ValueId, Predicate>, required: BTreeMap<ValueId, BTreeSet<Fact>> }
impl Function {
    pub(super) fn validate_narrowing(&self, schema: &CftSchema, reads: &[Vec<ValueId>], writes: &[Vec<ValueId>], successors: &[Vec<usize>]) -> Result<(), String> {
        if !self.body.iter().any(|node| matches!(node.operation, O::Copy(source) if !schema.value_type_assignable(&self.values[source.0 as usize], &self.values[node.destination.0 as usize]))) { return Ok(()); }
        let mut incoming = vec![None::<State>; self.body.len()]; incoming[0] = Some(State::default());
        let mut pending = VecDeque::from([0]); let mut steps = 0;
        while let Some(pc) = pending.pop_front() {
            steps += 1; if steps > 1_000_000 { return Err("类型收窄验证工作量超限".into()); }
            let mut state = incoming[pc].clone().unwrap(); let node = &self.body[pc];
            let mut required = BTreeSet::new();
            for input in &reads[pc] {
                if let Some(requirements) = state.required.get(input) { required.extend(requirements.iter().cloned()); }
            }
            if !matches!(node.operation, O::Copy(_)) && !required.is_subset(&state.facts) {
                return Err(format!("IR 第 {pc} 个节点使用收窄值时缺少控制流守卫"));
            }
            if let O::Copy(source) = node.operation {
                let from = &self.values[source.0 as usize]; let to = &self.values[node.destination.0 as usize];
                if !schema.value_type_assignable(from, to) {
                    let fact = match (from, to) {
                        (Ty::Option(inner), _) if schema.value_type_assignable(inner, to) => Fact::Some(source),
                        (Ty::Object(_), Ty::Object(name)) | (Ty::RecordRef(_), Ty::RecordRef(name)) => Fact::Type(source, name.to_string()),
                        (Ty::Option(_), Ty::Object(name) | Ty::RecordRef(name)) => Fact::Type(source, name.to_string()),
                        _ => return Err("IR 类型收窄无有效守卫形式".into()),
                    };
                    required.insert(fact);
                }
            }
            // 已在当前路径证实的来源在复制时解除待证明状态。
            required.retain(|fact| !state.facts.contains(fact));
            let mut predicate = match &node.operation {
                O::IsSome(source) => Some(Predicate { yes: BTreeSet::from([Fact::Some(*source)]), no: BTreeSet::new() }),
                O::IsType { value, name } => Some(Predicate { yes: BTreeSet::from([Fact::Type(*value, name.clone()), Fact::Some(*value)]), no: BTreeSet::new() }),
                O::Copy(source) => state.predicates.get(source).cloned(),
                O::Unary { operator: 1, value } => state.predicates.get(value).cloned().map(|predicate| Predicate { yes: predicate.no, no: predicate.yes }),
                _ => None,
            };
            if self.values[node.destination.0 as usize] == Ty::Bool && !writes[pc].is_empty() {
                let predicate = predicate.get_or_insert(Predicate { yes: BTreeSet::new(), no: BTreeSet::new() });
                predicate.yes.extend(state.facts.iter().cloned()); predicate.no.extend(state.facts.iter().cloned());
            }
            for output in &writes[pc] {
                state.facts.retain(|fact| fact.source() != *output);
                state.required.remove(output);
                // 未证实的转换依赖旧来源；重写来源后不能用新值的守卫证明旧转换。
                for requirements in state.required.values_mut() {
                    if requirements.iter().any(|fact| fact.source() == *output) {
                        requirements.retain(|fact| fact.source() != *output);
                        requirements.insert(Fact::Invalid(*output));
                    }
                }
                state.predicates.remove(output);
                for predicate in state.predicates.values_mut() {
                    predicate.yes.retain(|fact| fact.source() != *output);
                    predicate.no.retain(|fact| fact.source() != *output);
                }
            }
            if let Some(predicate) = predicate { state.predicates.insert(node.destination, predicate); }
            if matches!(node.operation, O::Copy(_)) && !required.is_empty() { state.required.insert(node.destination, required); }
            for &next in &successors[pc] {
                let mut outgoing = state.clone();
                if let O::JumpFalse { condition, target } = &node.operation {
                    if let Some(predicate) = state.predicates.get(condition) {
                        let truth = next != target.0 as usize;
                        outgoing.facts.extend((if truth { &predicate.yes } else { &predicate.no }).iter().cloned());
                    }
                }
                let previous = incoming.get_mut(next).ok_or("类型收窄控制流越界")?;
                let changed = if let Some(previous) = previous {
                    let before = previous.clone();
                    previous.facts.retain(|fact| outgoing.facts.contains(fact));
                    previous.predicates.retain(|value, predicate| {
                        if let Some(next) = outgoing.predicates.get(value) {
                            predicate.yes.retain(|fact| next.yes.contains(fact)); predicate.no.retain(|fact| next.no.contains(fact)); true
                        } else { false }
                    });
                    for (value, requirements) in &outgoing.required { previous.required.entry(*value).or_default().extend(requirements.iter().cloned()); }
                    *previous != before
                } else { *previous = Some(outgoing); true };
                if changed { pending.push_back(next); }
            }
        }
        Ok(())
    }
}
