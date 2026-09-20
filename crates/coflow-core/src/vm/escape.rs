//! 有界局部逃逸证明：只有用途全部可见的标量闭包可以消除创建和间接调用。
use super::ir::{Function, Node, Operation as O, ValueId};
use crate::schema::CftValueType as Ty;
use std::collections::{BTreeMap, BTreeSet};

impl Function {
    pub(super) fn eliminate_scalar_closures(&mut self) -> Result<(), String> {
        for node in &mut self.body {
            if let O::Closure { function, .. } = &mut node.operation { function.eliminate_scalar_closures()?; }
        }
        let flow = self.flow()?;
        let mut writes = vec![0usize; self.values.len()];
        for values in &flow.writes { for value in values { writes[value.0 as usize] += 1; } }
        let mut predecessors = vec![0usize; self.body.len()];
        for &pc in &flow.reachable { for &next in &flow.successors[pc] { predecessors[next] += 1; } }
        let mut expansions = BTreeMap::<usize, Vec<Node>>::new();
        let mut removed = BTreeSet::new();
        let mut work = 0usize;
        for &pc in &flow.reachable {
            let O::Closure { function, captures, template: false, .. } = &self.body[pc].operation else { continue; };
            if function.body.len() > 16 || !matches!(function.body.last().map(|node| &node.operation), Some(O::Return(_)))
                || function.values.iter().any(|ty| !matches!(ty, Ty::Int | Ty::Float | Ty::Bool | Ty::Unit))
                || function.body[..function.body.len() - 1].iter().any(|node| !matches!(node.operation,
                    O::Constant(_) | O::Copy(_) | O::Capture(_) | O::Unary { .. } | O::Binary { .. } | O::ConvertFloat(_) | O::IsSome(_))) {
                continue;
            }
            let root = self.body[pc].destination;
            if writes[root.0 as usize] != 1 { continue; }
            let mut aliases = BTreeSet::from([root]);
            loop {
                let before = aliases.len();
                for node in &self.body {
                    work += 1;
                    if work > 1_000_000 { break; }
                    if let O::Copy(source) = node.operation {
                        if aliases.contains(&source) && writes[node.destination.0 as usize] == 1 { aliases.insert(node.destination); }
                    }
                }
                if aliases.len() == before || work > 1_000_000 { break; }
            }
            if work > 1_000_000 { break; }
            let mut calls = Vec::new();
            let mut copies = Vec::new();
            let mut safe = true;
            for &consumer in &flow.reachable {
                work += flow.reads[consumer].len() + 1;
                if work > 1_000_000 { safe = false; break; }
                if !flow.reads[consumer].iter().any(|value| aliases.contains(value)) { continue; }
                match &self.body[consumer].operation {
                    O::Copy(source) if aliases.contains(source) && aliases.contains(&self.body[consumer].destination) => copies.push(consumer),
                    O::Call { target, arguments } if aliases.contains(target) && !arguments.iter().any(|value| aliases.contains(value)) => calls.push(consumer),
                    _ => { safe = false; break; }
                }
            }
            // 可变捕获只在创建到每次调用的唯一顺序路径上没有重写时读取原槽。
            // 这允许循环内立即使用的闭包消除，同时保留创建后修改捕获的快照语义。
            let mutable_capture = captures.iter().any(|value| writes[value.0 as usize] > 1
                || (value.0 as usize) < self.parameters.len() && writes[value.0 as usize] != 0);
            if mutable_capture {
                for &call in &calls {
                    if call <= pc { safe = false; break; }
                    for current in pc + 1..=call {
                        work += 1;
                        if work > 1_000_000 || predecessors[current] != 1
                            || flow.successors[current - 1].as_slice() != [current]
                            || flow.writes[current - 1].iter().any(|value| captures.contains(value)) {
                            safe = false; break;
                        }
                    }
                    if !safe { break; }
                }
            }
            let extra = calls.len().saturating_mul(function.values.len());
            let instructions = calls.len().saturating_mul(function.body.len() + function.parameters.len());
            if !safe || calls.is_empty() || self.values.len().saturating_add(extra) > 65_536
                || instructions > 4096 || expansions.values().map(Vec::len).sum::<usize>() + instructions > 4096 { continue; }
            // 所有调用体都是有限直线标量计算，不读取 owner/Host，不创建身份。
            for call in calls {
                let O::Call { arguments, .. } = &self.body[call].operation else { unreachable!() };
                let offset = self.values.len() as u32;
                let map = |value: ValueId| ValueId(offset + value.0);
                self.values.extend(function.values.iter().cloned());
                let span = self.body[call].span;
                let mut body = arguments.iter().enumerate().map(|(index, value)| Node {
                    destination: map(ValueId(index as u32)), operation: O::Copy(*value), span,
                }).collect::<Vec<_>>();
                for node in &function.body {
                    let (destination, operation) = match &node.operation {
                        O::Constant(value) => (map(node.destination), O::Constant(value.clone())),
                        O::Capture(index) => (map(node.destination), O::Copy(captures[*index as usize])),
                        O::Copy(value) => (map(node.destination), O::Copy(map(*value))),
                        O::ConvertFloat(value) => (map(node.destination), O::ConvertFloat(map(*value))),
                        O::IsSome(value) => (map(node.destination), O::IsSome(map(*value))),
                        O::Unary { operator, value } => (map(node.destination), O::Unary { operator: *operator, value: map(*value) }),
                        O::Binary { operator, left, right } => (map(node.destination), O::Binary { operator: *operator, left: map(*left), right: map(*right) }),
                        O::Return(value) => (self.body[call].destination, O::Copy(map(*value))),
                        _ => unreachable!("标量闭包已经过用途和操作检查"),
                    };
                    body.push(Node { destination, operation, span });
                }
                expansions.insert(call, body);
            }
            removed.insert(pc);
            removed.extend(copies);
        }
        if expansions.is_empty() { return Ok(()); }
        self.rewrite_nodes(expansions, &removed);
        Ok(())
    }
}

#[cfg(all(test, feature = "cft-compiler"))]
mod tests {
    use super::*;
    use crate::{schema::{build_schema, parse_modules, CftFile, ModuleId}, vm::compiler::{analyze, CompileContext}};
    fn optimized(source: &str) -> Function {
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("escape"), "table Rule {}")])).unwrap();
        let mut function = analyze(&schema, source, "escape", CompileContext::default()).unwrap();
        function.eliminate_scalar_closures().unwrap();
        function.lower().unwrap();
        function
    }
    #[test]
    fn scalar_closure_elimination_preserves_escaping_identity_and_capture_snapshots() {
        let local = optimized("fn(x: int) -> int { var f: fn(int) -> int = fn(y: int) -> int { x + y }; f(3) + f(4) }");
        assert!(!local.body.iter().any(|node| matches!(node.operation, O::Closure { .. } | O::Call { .. })));
        let escaped = optimized("fn(x: int) -> fn() -> int { fn() -> int { x } }");
        assert!(escaped.body.iter().any(|node| matches!(node.operation, O::Closure { .. })));
        let changed = optimized("fn(x: int) -> int { var value: int = x; var f: fn() -> int = fn() -> int { value }; value = 9; f() }");
        assert!(changed.body.iter().any(|node| matches!(node.operation, O::Closure { .. })));
        let identity = optimized("fn() -> bool { var f: fn() -> int = fn() -> int { 1 }; f == f }");
        assert!(identity.body.iter().any(|node| matches!(node.operation, O::Closure { .. })));
        let looped = optimized("fn() -> int { var total: int = 0; for i in 0..100 { var f: fn() -> int = fn() -> int { i }; total += f(); } total }");
        assert!(!looped.body.iter().any(|node| matches!(node.operation, O::Closure { .. } | O::Call { .. })));
    }
}
