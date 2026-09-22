//! 保持语言内容语义的借用比较；模板求值可以同步重入。
use super::*;
impl ExecutionContext<'_> {
    pub(super) fn equal(&self, left: Slot, right: Slot) -> Result<bool, ExecutionError> {
        // 用户可以逐次构造很深的不可变数据链；结构比较使用显式工作栈。
        self.root(left)?;
        self.root(right)?;
        let (mut pending, mut pending_memory) = self.reserve_values(1)?;
        pending.push((left, right));
        while let Some((left, right)) = pending.pop() {
            self.budget.charge(1)?;
            // 所有种类均借用载荷，动态值的短期所有权跨模板重入保持有效。
            let left_read = self.read_value(left)?;
            let right_read = self.read_value(right)?;
            let equal = match (left_read.view(), right_read.view()) {
                (Stored::Scalar(a), Stored::Scalar(b)) => match (a, b) {
                    (Slot::Int(a), Slot::Float(b)) => a as f32 == b,
                    (Slot::Float(a), Slot::Int(b)) => a == b as f32,
                    _ => a == b,
                },
                (Stored::String(a), Stored::String(b)) => {
                    self.budget.charge(a.len().min(b.len()) as u64)?;
                    a == b
                }
                (Stored::Enum(a, av), Stored::Enum(b, bv)) => a == b && av == bv,
                (Stored::Function, Stored::Function) => left == right,
                (Stored::Array(a), Stored::Array(b)) => {
                    if a.len() != b.len() { return Ok(false); }
                    self.budget.charge(a.len() as u64)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, a.len())?;
                    for (a, b) in a.iter().zip(b.iter()).rev() { pending.push((self.slot(a)?, self.slot(b)?)); }
                    true
                }
                (Stored::Dict(a), Stored::Dict(b)) => {
                    if a.len() != b.len() { return Ok(false); }
                    self.budget.charge(a.len() as u64)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, a.len())?;
                    for (key, (_, value)) in a.iter() {
                        let Some((_, other)) = b.get(key) else { return Ok(false); };
                        pending.push((self.slot(*value)?, self.slot(*other)?));
                    }
                    true
                }
                (Stored::Object(a), Stored::Object(b)) => {
                    if a.key.is_some() || b.key.is_some() { left == right }
                    else {
                        if a.type_name != b.type_name || a.len() != b.len() { return Ok(false); }
                        self.reserve_temporary_vec(&mut pending, &mut pending_memory, a.len())?;
                        for index in (0..a.len()).rev() {
                            let (an, av) = a.named_at(index, &self.runtime.values).expect("字段已验证");
                            let (bn, bv) = b.named_at(index, &self.runtime.values).expect("字段已验证");
                            if an != bn { return Ok(false); }
                            pending.push((self.slot(av)?, self.slot(bv)?));
                        }
                        true
                    }
                }
                (Stored::Template, _) | (_, Stored::Template) => {
                    let read = |slot| -> Result<Slot, ExecutionError> {
                        if let Some(binding) = self.template(slot)? {
                            let host = self.runtime.execution_host(ExecutionLimits::default())?;
                            executor::execute(&host, binding, &[], self.budget.clone())
                        } else { Ok(slot) }
                    };
                    let left = read(left)?;
                    self.root(left)?;
                    let right = read(right)?;
                    self.root(right)?;
                    self.reserve_temporary_vec(&mut pending, &mut pending_memory, 1)?;
                    pending.push((left, right));
                    true
                }
                _ => false,
            };
            if !equal {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
