//! 区间证明只用于移动保证成功的标量计算，不改变 checked 算术的 fault 位置。
use super::{bytecode::Constant, ir::{Function, Node, Operation as O, IrValueId}};
use crate::schema::CftValueType as Ty;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct Range { low: i64, high: i64 }
impl Range {
    const INT: Self = Self { low: i32::MIN as i64, high: i32::MAX as i64 };
    fn checked(low: i64, high: i64) -> Option<Self> {
        (low >= Self::INT.low && high <= Self::INT.high).then_some(Self { low, high })
    }
}
impl Function {
    pub(super) fn is_total_scalar_callback(&self) -> bool {
        if self.parameters.len() != 1 || !self.captures.is_empty() || self.body.len() > 32
            || self.values.iter().any(|ty| !matches!(ty, Ty::Int | Ty::Float | Ty::Bool)) { return false; }
        let mut ranges = BTreeMap::new();
        for (pc, node) in self.body.iter().enumerate() {
            if matches!(node.operation, O::Return(_)) { return pc + 1 == self.body.len(); }
            let Some(range) = self.total_range(node, &ranges) else { return false; };
            ranges.insert(node.destination, range);
        }
        false
    }
    fn total_range(&self, node: &Node, ranges: &BTreeMap<IrValueId, Range>) -> Option<Range> {
        let range = |value: &IrValueId| ranges.get(value).copied().unwrap_or(Range::INT);
        let ty = |value: &IrValueId| &self.values[value.0 as usize];
        match &node.operation {
            O::Constant(Constant::Int(value)) => Some(Range { low: i64::from(*value), high: i64::from(*value) }),
            O::Constant(Constant::Bool(_) | Constant::Float(_)) | O::ConvertFloat(_) | O::IsSome(_) => Some(Range::INT),
            O::Copy(value) if matches!(ty(value), Ty::Int | Ty::Float | Ty::Bool) => Some(range(value)),
            O::Unary { operator: 0, value } if *ty(value) == Ty::Int => { let range = range(value); Range::checked(-range.high, -range.low) },
            O::Unary { operator: 0, value } if *ty(value) == Ty::Float => Some(Range::INT),
            O::Unary { operator: 1, value } if *ty(value) == Ty::Bool => Some(Range::INT),
            O::Unary { operator: 2, value } if *ty(value) == Ty::Int => Some(Range::INT),
            O::Binary { operator, left, right } if *ty(left) == Ty::Int && *ty(right) == Ty::Int => {
                let (a, b) = (range(left), range(right));
                match operator {
                    0 => Range::checked(a.low + b.low, a.high + b.high),
                    1 => Range::checked(a.low - b.high, a.high - b.low),
                    2 => { let bounds = [a.low * b.low, a.low * b.high, a.high * b.low, a.high * b.high]; Range::checked(*bounds.iter().min()?, *bounds.iter().max()?) },
                    7..=12 | 16 | 17 => Some(Range::INT),
                    15 if b.low == b.high && b.low >= 0 => Some(Range { low: 0, high: b.high }),
                    15 => Some(Range::INT),
                    13 | 14 if b.low >= 0 && b.high < 32 => Some(Range::INT),
                    _ => None,
                }
            }
            O::Binary { operator: 0..=3 | 6..=12, left, right } if *ty(left) == Ty::Float && *ty(right) == Ty::Float => Some(Range::INT),
            O::Binary { operator: 7 | 8, left, right } if *ty(left) == Ty::Bool && *ty(right) == Ty::Bool => Some(Range::INT),
            _ => None,
        }
    }
    pub(super) fn hoist_total_loop_invariants(&mut self) -> Result<(), String> {
        for node in &mut self.body { if let O::Closure { function, .. } = &mut node.operation { function.hoist_total_loop_invariants()?; } }
        let flow = self.flow()?;
        let mut writes = vec![0usize; self.values.len()];
        for values in &flow.writes { for value in values { writes[value.0 as usize] += 1; } }
        let mut prefix = self.parameters.iter().enumerate().filter(|(index, _)| writes[*index] == 0)
            .map(|(index, _)| IrValueId(index as u32)).collect::<BTreeSet<_>>();
        let mut prefix_ranges = BTreeMap::new();
        let mut prefix_end = 0;
        for (pc, node) in self.body.iter().enumerate() {
            if flow.successors[pc].as_slice() != [pc + 1] || matches!(node.operation, O::Jump(_)) { break; }
            let range = self.total_range(node, &prefix_ranges);
            for value in &flow.writes[pc] {
                if writes[value.0 as usize] == 1 { prefix.insert(*value); if let Some(range) = range { prefix_ranges.insert(*value, range); } }
            }
            prefix_end = pc + 1;
        }
        let mut expansions = BTreeMap::new(); let mut removed = BTreeSet::new(); let mut work = 0usize;
        for (prep, node) in self.body.iter().enumerate() {
            let O::ForPrep { target, .. } = node.operation else { continue; };
            let exit = target.0 as usize;
            if prep == 0 || prep > prefix_end || exit <= prep || removed.contains(&(prep - 1))
                || flow.successors[prep - 1].as_slice() != [prep] || matches!(self.body[prep - 1].operation, O::Jump(_)) { continue; }
            // 只接受单入口自然区间循环。回边继续指向 guard，不能重复执行外提序列。
            if flow.successors.iter().enumerate().any(|(pc, successors)|
                !(prep..exit).contains(&pc) && successors.iter().any(|next|
                    (prep..exit).contains(next) && !(pc == prep - 1 && *next == prep))) { continue; }
            let mut ready = prefix.clone(); let mut ranges = prefix_ranges.clone(); let mut hoisted = Vec::new();
            for pc in prep + 1..exit {
                work += flow.reads[pc].len() + 1; if work > 1_000_000 { break; }
                let node = &self.body[pc];
                if removed.contains(&pc) || writes[node.destination.0 as usize] != 1
                    || !flow.reads[pc].iter().all(|value| ready.contains(value)) { continue; }
                let Some(range) = self.total_range(node, &ranges) else { continue; };
                ready.insert(node.destination); ranges.insert(node.destination, range);
                removed.insert(pc); hoisted.push(node.clone());
            }
            if !hoisted.is_empty() {
                let mut replacement = vec![self.body[prep - 1].clone()]; replacement.extend(hoisted); expansions.insert(prep - 1, replacement);
            }
            if work > 1_000_000 { break; }
        }
        if !expansions.is_empty() { self.rewrite_nodes(expansions, &removed); }
        Ok(())
    }
}

#[cfg(all(test, feature = "cft-compiler"))]
mod tests {
    use super::*;
    use crate::{schema::{build_schema, parse_modules, CftFile, ModuleId}, vm::compiler::{analyze, CompileContext}};
    #[test]
    fn only_total_invariants_move_before_an_empty_loop() {
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("ranges"), "table Rule {}")])).unwrap();
        let mut function = analyze(&schema, "fn(x: int, count: int) -> int { var sum: int = 0; for i in 0..count { sum += (x & 255) + 1; sum += x + 1; } sum }", "ranges", CompileContext::default()).unwrap();
        function.hoist_total_loop_invariants().unwrap();
        function.lower().unwrap();
        let prep = function.body.iter().position(|node| matches!(node.operation, O::ForPrep { .. })).unwrap();
        assert!(function.body[..prep].iter().any(|node| matches!(node.operation, O::Binary { operator: 0, .. })));
        assert!(function.body[prep..].iter().any(|node| matches!(node.operation, O::Binary { operator: 0, .. })));
    }
}
