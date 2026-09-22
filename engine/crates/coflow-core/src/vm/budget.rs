//! 同线程执行预算：帧、循环、临时内存和 Host 重入共用计数。
use super::ExecutionError;
use std::{cell::Cell, rc::Rc};
#[derive(Debug, Clone, Copy)]
pub struct ExecutionLimits {
    pub max_work: u64,
    pub max_iterations: u64,
    pub max_depth: usize,
    pub max_registers: usize,
    pub max_heap_bytes: usize,
}
impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_work: 10_000_000,
            max_iterations: 1_000_000,
            max_depth: 256,
            max_registers: 1_000_000,
            max_heap_bytes: 64 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Default)]
struct Usage {
    remaining: Cell<u64>,
    iterations: Cell<u64>,
    depth: Cell<usize>,
    registers: Cell<usize>,
    temporary_bytes: Cell<usize>,
}
#[derive(Debug, Clone)]
pub(crate) struct Budget {
    limits: ExecutionLimits,
    usage: Rc<Usage>,
}
/// Host 重入同样消耗共享调用深度；退出或展开时必须归还，不能依赖 VM 帧存在。
pub(crate) struct HostBudgetGuard(Budget);
impl Drop for HostBudgetGuard {
    fn drop(&mut self) {
        self.0.leave(0);
    }
}
/// 同步重入与递归导入共享临时缓冲占用；归还只撤销本次预留，不重置外层预算。
#[derive(Debug)]
pub(crate) struct TemporaryBytes {
    budget: Budget,
    bytes: usize,
}
impl TemporaryBytes {
    pub(crate) fn resize(&mut self, bytes: usize, heap_bytes: usize) -> Result<(), ExecutionError> {
        if bytes > self.bytes {
            let extra = bytes - self.bytes;
            if extra > self.budget.remaining_heap_bytes().saturating_sub(heap_bytes) {
                return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
            }
            self.budget
                .usage
                .temporary_bytes
                .set(self.budget.usage.temporary_bytes.get() + extra);
        } else {
            self.budget
                .usage
                .temporary_bytes
                .set(self.budget.usage.temporary_bytes.get() - (self.bytes - bytes));
        }
        self.bytes = bytes;
        Ok(())
    }
}
impl Drop for TemporaryBytes {
    fn drop(&mut self) {
        let usage = &self.budget.usage;
        usage
            .temporary_bytes
            .set(usage.temporary_bytes.get() - self.bytes);
    }
}
impl Budget {
    pub(crate) fn reserve_temporary(
        &self,
        bytes: usize,
        heap_bytes: usize,
    ) -> Result<TemporaryBytes, ExecutionError> {
        if bytes > self.remaining_heap_bytes().saturating_sub(heap_bytes) {
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Memory));
        }
        self.usage
            .temporary_bytes
            .set(self.usage.temporary_bytes.get() + bytes);
        Ok(TemporaryBytes {
            budget: self.clone(),
            bytes,
        })
    }
    pub(crate) fn enter_host(&self) -> Result<HostBudgetGuard, ExecutionError> {
        self.charge(1)?;
        self.enter(0)?;
        Ok(HostBudgetGuard(self.clone()))
    }
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            usage: Rc::new(Usage {
                remaining: Cell::new(limits.max_work),
                iterations: Cell::new(limits.max_iterations),
                depth: Cell::new(0),
                registers: Cell::new(0),
                temporary_bytes: Cell::new(0),
            }),
        }
    }
    /// 扣减一次加权工作量。max_work 的单位是"昂贵操作加权成本"（循环回边、
    /// 调用、分配、文本长度），不是字节码指令数；直线代码必然终止，无需计费。
    pub fn charge(&self, work: u64) -> Result<(), ExecutionError> {
        let current = self.usage.remaining.get();
        if work > current {
            self.usage.remaining.set(0);
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Work));
        }
        self.usage.remaining.set(current - work);
        Ok(())
    }
    /// 批量扣减迭代预算；批次允许有限超额，避免热循环反复触碰共享计数。
    pub(super) fn iteration_batch(&self, batch: u64) -> Result<(), ExecutionError> {
        self.charge(batch)?;
        let current = self.usage.iterations.get();
        if current < batch {
            self.usage.iterations.set(0);
            return Err(ExecutionError::LimitExceeded(crate::vm::LimitKind::Iterations));
        }
        self.usage.iterations.set(current - batch);
        Ok(())
    }
    pub fn remaining_heap_bytes(&self) -> usize {
        self.limits
            .max_heap_bytes
            .saturating_sub(self.usage.temporary_bytes.get())
    }
    pub fn remaining(&self) -> u64 {
        self.usage.remaining.get()
    }
    pub(super) fn enter(&self, registers: usize) -> Result<(), ExecutionError> {
        let usage = &self.usage;
        if usage.depth.get() >= self.limits.max_depth {
            return Err(ExecutionError::LimitExceeded(super::LimitKind::CallDepth));
        }
        if registers > self.limits.max_registers.saturating_sub(usage.registers.get()) {
            return Err(ExecutionError::LimitExceeded(super::LimitKind::Registers));
        }
        usage.depth.set(usage.depth.get() + 1);
        usage.registers.set(usage.registers.get() + registers);
        Ok(())
    }
    pub(super) fn replacement_capacity(&self, previous: usize) -> usize {
        self.limits.max_registers.saturating_sub(self.usage.registers.get() - previous)
    }
    pub(super) fn replace_registers(&self, previous: usize, next: usize) {
        self.usage.registers.set(self.usage.registers.get() - previous + next);
    }
    pub(super) fn leave(&self, registers: usize) {
        let usage = &self.usage;
        usage.depth.set(usage.depth.get() - 1);
        usage.registers.set(usage.registers.get() - registers);
    }
}
