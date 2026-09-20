//! 共享静态函数编译与寄存器字节码虚拟机。
use std::fmt;
pub mod bytecode;
pub mod compiler;
pub mod construction;
pub mod contract_programs;
pub mod executor;
pub mod ir;
mod ir_validation;
mod ir_flow;
mod ssa;
mod escape;
mod ranges;
mod collections;
mod ir_construction;
mod ir_narrowing;
pub(crate) mod image;
pub(crate) mod optimization;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    RuntimeBusy,
    Released,
    ForeignRuntime,
    InvalidHandle,
    MissingHostBinding(String),
    InvalidAccess(String),
    Fault {
        message: String,
        path: Option<String>,
        module: Option<crate::schema::ModuleId>,
        function: String,
        span: crate::source::Span,
        stack: Vec<String>,
    },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeBusy => f.write_str("Runtime 忙"),
            Self::Released => f.write_str("Runtime 已释放"),
            Self::ForeignRuntime => f.write_str("值不属于当前 Runtime"),
            Self::InvalidHandle => f.write_str("句柄已失效"),
            Self::MissingHostBinding(name) => write!(f, "Host 服务未绑定：{name}"),
            Self::InvalidAccess(message) => f.write_str(message),
            Self::Fault {
                message,
                function,
                span,
                ..
            } => write!(f, "{function}:{}..{}: {message}", span.start, span.end),
        }
    }
}
impl std::error::Error for ExecutionError {}

#[cfg(test)]
mod dispatch_micro;
