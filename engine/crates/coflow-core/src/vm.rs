//! 共享静态函数编译与寄存器字节码虚拟机。
use std::fmt;
// 编译器、IR 和字节码供语言工具与分析使用；执行入口统一走 Runtime。
pub mod bytecode;
pub mod compiler;
pub(crate) mod construction;
pub(crate) mod contract_programs;
pub(crate) mod executor;
pub(crate) mod budget;
pub(crate) mod slot;
pub(crate) mod scalar;
pub use budget::ExecutionLimits;
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

/// 资源限制使用类型化原因，诊断层不依赖消息文本或剩余工作量猜测。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind { Memory, Work, Iterations, CallDepth, Registers, ValueDepth, Values }
impl fmt::Display for LimitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Memory => "动态内存预算耗尽", Self::Work => "执行工作量预算耗尽",
            Self::Iterations => "循环迭代预算耗尽", Self::CallDepth => "调用深度预算耗尽",
            Self::Registers => "寄存器预算耗尽", Self::ValueDepth => "导入值嵌套深度超限",
            Self::Values => "动态值数量超限",
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    RuntimeBusy,
    Released,
    ForeignRuntime,
    InvalidHandle,
    LimitExceeded(LimitKind),
    MissingHostBinding(String),
    InvalidAccess(String),
    Fault {
        cause: Box<ExecutionError>,
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
            Self::LimitExceeded(kind) => kind.fmt(f),
            Self::InvalidHandle => f.write_str("句柄已失效"),
            Self::MissingHostBinding(name) => write!(f, "Host 服务未绑定：{name}"),
            Self::InvalidAccess(message) => f.write_str(message),
            Self::Fault {
                cause,
                function,
                span,
                ..
            } => write!(f, "{function}:{}..{}: {cause}", span.start, span.end),
        }
    }
}
impl ExecutionError {
    pub fn limit(&self) -> Option<LimitKind> {
        let mut error = self;
        loop { match error { Self::LimitExceeded(kind) => return Some(*kind), Self::Fault { cause, .. } => error = cause, _ => return None } }
    }
}
impl std::error::Error for ExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self { Self::Fault { cause, .. } => Some(cause.as_ref()), _ => None }
    }
}

#[cfg(test)]
mod dispatch_micro;

pub(crate) fn error(message: &str) -> ExecutionError { ExecutionError::InvalidAccess(message.into()) }

mod builtins;

mod bytecode_analysis;
mod bytecode_rewrite;
