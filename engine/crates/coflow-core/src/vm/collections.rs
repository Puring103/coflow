//! 有界集合流水线优化；融合仅重排已证明纯、有限且无计算 fault 的标量回调。
use super::{bytecode::Constant, construction::BuildOp as B, ir::{Function, Operation as O, IrValueId}};
use crate::schema::CftValueType as Ty;
use std::collections::BTreeSet;

fn identity(function: &Function) -> bool {
    if function.parameters.len() != 1 || !function.captures.is_empty() || function.parameters[0] != function.result
        || !matches!(function.result, Ty::Int | Ty::Float | Ty::Bool | Ty::String) { return false; }
    let mut aliases = BTreeSet::from([IrValueId(0)]);
    for (pc, node) in function.body.iter().enumerate() {
        match node.operation {
            O::Copy(source) if aliases.contains(&source) => { aliases.insert(node.destination); }
            O::Return(value) => return pc + 1 == function.body.len() && aliases.contains(&value),
            _ => return false,
        }
    }
    false
}
impl Function {
    pub(super) fn eliminate_identity_maps(&mut self) -> Result<(), String> {
        for node in &mut self.body { if let O::Closure { function, .. } = &mut node.operation { function.eliminate_identity_maps()?; } }
        let flow = self.flow()?;
        let mut writes = vec![Vec::new(); self.values.len()];
        let mut reads = vec![Vec::new(); self.values.len()];
        for &pc in &flow.reachable {
            for value in &flow.writes[pc] { writes[value.0 as usize].push(pc); }
            for value in &flow.reads[pc] { reads[value.0 as usize].push(pc); }
        }
        let mut work = 0usize;
        for start in 0..self.body.len().saturating_sub(13) {
            let nodes = &self.body[start..start + 14];
            let O::Build(B::Start { source: None }) = nodes[0].operation else { continue; };
            let builder = nodes[0].destination;
            if !matches!(self.values[builder.0 as usize], Ty::Array(_)) { continue; }
            let O::Constant(Constant::Int(0)) = nodes[1].operation else { continue; };
            let counter = nodes[1].destination;
            let O::Length(receiver) = nodes[2].operation else { continue; };
            let length = nodes[2].destination;
            if !matches!(nodes[3].operation, O::Binary { operator: 9, left, right } if left == counter && right == length)
                || !matches!(nodes[4].operation, O::JumpFalse { condition, target } if condition == nodes[3].destination && target.0 as usize == start + 13)
                || !matches!(nodes[5].operation, O::Iteration)
                || !matches!(nodes[6].operation, O::IteratorValue { collection, index } if collection == receiver && index == counter) { continue; }
            let O::Call { target, arguments } = &nodes[7].operation else { continue; };
            if arguments.as_slice() != [nodes[6].destination] { continue; }
            let [creation] = writes[target.0 as usize].as_slice() else { continue; };
            if reads[target.0 as usize].as_slice() != [start + 7] { continue; }
            let O::Closure { function, template: false, .. } = &self.body[*creation].operation else { continue; };
            if !identity(function) || self.values[nodes[6].destination.0 as usize] != function.parameters[0] { continue; }
            if !matches!(nodes[8].operation, O::Build(B::Append { builder: target, value }) if target == builder && value == nodes[7].destination)
                || !matches!(nodes[9].operation, O::Constant(Constant::Int(1)))
                || !matches!(nodes[10].operation, O::Binary { operator: 0, left, right } if left == counter && right == nodes[9].destination)
                || !matches!(nodes[11].operation, O::Copy(value) if value == nodes[10].destination && nodes[11].destination == counter)
                || !matches!(nodes[12].operation, O::Jump(target) if target.0 as usize == start + 3)
                || !matches!(nodes[13].operation, O::Build(B::Freeze { builder: target }) if target == builder) { continue; }
            // 中间循环没有其他入口，临时变量没有外部消费者；不能删除用户可观察的状态。
            work = work.saturating_add(self.body.len());
            if work > 1_000_000 { break; }
            if flow.successors.iter().enumerate().any(|(pc, successors)| !(start..start + 14).contains(&pc)
                && successors.iter().any(|next| (start + 1..start + 14).contains(next))) { continue; }
            if (start..start + 13).flat_map(|pc| &flow.writes[pc]).any(|value|
                reads[value.0 as usize].iter().any(|pc| !(start..start + 14).contains(pc))) { continue; }
            let creation = *creation;
            for pc in start..start + 13 { self.body[pc].operation = O::Jump(super::ir::NodeIndex((pc + 1) as u32)); }
            self.body[start + 13].operation = O::Copy(receiver);
            self.body[creation].operation = O::Jump(super::ir::NodeIndex((creation + 1) as u32));
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "cft-compiler"))]
mod tests {
    use super::*;
    use crate::{schema::{build_schema, parse_modules, CftFile, ModuleId}, vm::compiler::{analyze, CompileContext}};
    fn optimize(source: &str) -> Function {
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("maps"), "table Rule {}")])).unwrap();
        let mut function = analyze(&schema, source, "maps", CompileContext::default()).unwrap();
        function.eliminate_identity_maps().unwrap(); function.lower().unwrap(); function
    }
    #[test]
    fn total_map_filter_pairs_remove_one_buffer_and_faulting_callbacks_do_not_fuse() {
        let schema = build_schema(&parse_modules([CftFile::from_source(ModuleId::from("maps"), "table Rule {}")])).unwrap();
        for expression in [
            "values.map(fn(x: int) -> int { x & 255 }).map(fn(x: int) -> int { x | 2 })",
            "values.map(fn(x: int) -> int { x & 255 }).filter(fn(x: int) -> bool { x > 2 })",
            "values.filter(fn(x: int) -> bool { x > 2 }).map(fn(x: int) -> int { x & 255 })",
            "values.filter(fn(x: int) -> bool { x > 2 }).filter(fn(x: int) -> bool { x < 10 })",
        ] {
            let mut function = analyze(&schema, &format!("fn(values: [int]) -> [int] {{ {expression} }}"), "maps", CompileContext::default()).unwrap();
            function.fuse_total_collection_stages().unwrap();
            assert_eq!(function.body.iter().filter(|node| matches!(node.operation, O::Build(B::Start { .. }))).count(), 1, "{expression}: {:?}", function.body);
            function.lower().unwrap();
        }
        let mut function = analyze(&schema, "fn(values: [int]) -> [int] { values.map(fn(x: int) -> int { x + 1 }).map(fn(x: int) -> int { x | 2 }) }", "maps", CompileContext::default()).unwrap();
        function.fuse_total_collection_stages().unwrap();
        assert_eq!(function.body.iter().filter(|node| matches!(node.operation, O::Build(B::Start { .. }))).count(), 2);
    }
    #[test]
    fn identity_pipeline_removes_copy_buffers_but_keeps_filter_and_faulting_maps() {
        let same = optimize("fn(values: [int]) -> [int] { values.map(fn(x: int) -> int { x }).map(fn(x: int) -> int { x }) }");
        assert!(!same.body.iter().any(|node| matches!(node.operation, O::Build(_) | O::Call { .. } | O::Closure { .. })));
        for source in ["fn(values: [int]) -> [int] { values.map(fn(x: int) -> int { x + 1 }) }",
            "fn(values: [bool]) -> [bool] { values.filter(fn(x: bool) -> bool { x }) }"] {
            let function = optimize(source);
            assert!(function.body.iter().any(|node| matches!(node.operation, O::Build(_))));
        }
    }
}

#[derive(Clone, Copy)]
struct Stage { start: usize, append: usize, end: usize, builder: IrValueId, receiver: IrValueId, callback: IrValueId, item: IrValueId }
impl Function {
    fn scalar_stage(&self, start: usize) -> Option<Stage> {
        let nodes = self.body.get(start..)?;
        if nodes.len() < 14 { return None; }
        let O::Build(B::Start { source: None }) = nodes[0].operation else { return None; };
        let builder = nodes[0].destination;
        let Ty::Array(inner) = &self.values[builder.0 as usize] else { return None; };
        if !matches!(inner.as_ref(), Ty::Int | Ty::Float | Ty::Bool) { return None; }
        let O::Constant(Constant::Int(0)) = nodes[1].operation else { return None; };
        let counter = nodes[1].destination;
        let O::Length(receiver) = nodes[2].operation else { return None; };
        if !matches!(nodes[3].operation, O::Binary { operator: 9, left, right } if left == counter && right == nodes[2].destination)
            || !matches!(nodes[5].operation, O::Iteration)
            || !matches!(nodes[6].operation, O::IteratorValue { collection, index } if collection == receiver && index == counter) { return None; }
        let O::Call { target: callback, arguments } = &nodes[7].operation else { return None; };
        if arguments.as_slice() != [nodes[6].destination] { return None; }
        let append = if matches!(nodes[8].operation, O::JumpFalse { condition, target } if condition == nodes[7].destination && target.0 as usize == start + 10) { 9 } else { 8 };
        let end = append + 5;
        if nodes.len() <= end { return None; }
        let item = if append == 9 { nodes[6].destination } else { nodes[7].destination };
        if !matches!(nodes[4].operation, O::JumpFalse { condition, target } if condition == nodes[3].destination && target.0 as usize == start + end)
            || !matches!(nodes[append].operation, O::Build(B::Append { builder: target, value }) if target == builder && value == item)
            || !matches!(nodes[append + 1].operation, O::Constant(Constant::Int(1)))
            || !matches!(nodes[append + 2].operation, O::Binary { operator: 0, left, right } if left == counter && right == nodes[append + 1].destination)
            || !matches!(nodes[append + 3].operation, O::Copy(value) if value == nodes[append + 2].destination && nodes[append + 3].destination == counter)
            || !matches!(nodes[append + 4].operation, O::Jump(target) if target.0 as usize == start + 3)
            || !matches!(nodes[end].operation, O::Build(B::Freeze { builder: target }) if target == builder) { return None; }
        Some(Stage { start, append: start + append, end: start + end, builder, receiver, callback: *callback, item })
    }
    pub(super) fn fuse_total_collection_stages(&mut self) -> Result<(), String> {
        for node in &mut self.body { if let O::Closure { function, .. } = &mut node.operation { function.fuse_total_collection_stages()?; } }
        let flow = self.flow()?;
        let mut writes = vec![Vec::new(); self.values.len()];
        let mut reads = vec![Vec::new(); self.values.len()];
        for &pc in &flow.reachable {
            for value in &flow.writes[pc] { writes[value.0 as usize].push(pc); }
            for value in &flow.reads[pc] { reads[value.0 as usize].push(pc); }
        }
        let mut expansions = std::collections::BTreeMap::new();
        let mut removed = BTreeSet::new();
        let mut work = 0usize;
        for start in 0..self.body.len() {
            if removed.contains(&start) { continue; }
            let Some(first) = self.scalar_stage(start) else { continue; };
            let Some(second) = self.scalar_stage(first.end + 2) else { continue; };
            if second.receiver != self.body[first.end].destination { continue; }
            let creation = first.end + 1;
            let mut valid = true;
            for stage in [first, second] {
                let [defined] = writes[stage.callback.0 as usize].as_slice() else { valid = false; break; };
                if *defined >= stage.start || reads[stage.callback.0 as usize].as_slice() != [stage.start + 7] { valid = false; break; }
                let O::Closure { function, captures, template: false, owner: None } = &self.body[*defined].operation else { valid = false; break; };
                if !captures.is_empty() || !function.is_total_scalar_callback() { valid = false; break; }
                if stage.start == second.start && *defined != creation { valid = false; break; }
                work = work.saturating_add(self.body.len());
                if work > 1_000_000 { valid = false; break; }
                if flow.successors.iter().enumerate().any(|(pc, successors)| !(stage.start..=stage.end).contains(&pc)
                    && successors.iter().any(|next| (stage.start + 1..=stage.end).contains(next))) { valid = false; break; }
            }
            if !valid { continue; }
            // 两段循环的中间结果和临时槽不能被其他代码观察，最终输出保持原编号。
            if (first.start..second.end).flat_map(|pc| &flow.writes[pc]).any(|value|
                reads[value.0 as usize].iter().any(|pc| !(first.start..=second.end).contains(pc))) { continue; }
            let final_type = self.values[second.builder.0 as usize].clone();
            self.values[first.builder.0 as usize] = final_type.clone();
            self.values[self.body[first.end].destination.0 as usize] = final_type;
            expansions.insert(first.start, vec![self.body[creation].clone(), self.body[first.start].clone()]);
            let mut call = self.body[second.start + 7].clone();
            call.operation = O::Call { target: second.callback, arguments: vec![first.item] };
            let mut nodes = vec![call];
            let value = if second.append == second.start + 9 {
                let mut branch = self.body[second.start + 8].clone();
                branch.operation = O::JumpFalse { condition: self.body[second.start + 7].destination, target: super::ir::NodeIndex((first.append + 1) as u32) };
                nodes.push(branch); first.item
            } else { self.body[second.start + 7].destination };
            let mut append = self.body[first.append].clone();
            append.operation = O::Build(B::Append { builder: first.builder, value }); nodes.push(append);
            expansions.insert(first.append, nodes);
            removed.extend(creation..second.end);
            self.body[second.end].operation = O::Copy(self.body[first.end].destination);
        }
        if !expansions.is_empty() { self.rewrite_nodes(expansions, &removed); }
        Ok(())
    }
}
