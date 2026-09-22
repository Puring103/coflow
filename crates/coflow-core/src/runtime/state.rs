//! 实例独占的可变状态，所有访问均位于创建线程。
use super::heap::Heap;
use crate::vm::{executor, budget::Budget};
use std::{cell::RefCell, collections::HashMap};
/// 每个实例独立拥有堆、执行预算和动态缓存。
#[derive(Debug, Default)]
pub(super) struct VmState {
    pub(super) heap: RefCell<Heap>,
    pub(super) budget: RefCell<Option<Budget>>,
    pub(super) regexes: RefCell<HashMap<String, CachedRegex>>,
    pub(super) buffers: RefCell<executor::ExecutionBuffers>,
}
#[derive(Debug)]
pub(super) struct CachedRegex {
    pub(super) program: regex_automata::nfa::thompson::pikevm::PikeVM,
    pub(super) cache: regex_automata::nfa::thompson::pikevm::Cache,
    pub(super) _memory: executor::TemporaryBytes,
}
