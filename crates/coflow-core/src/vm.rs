//! 虚拟机与函数编译的预留边界，不包含解释器或替代执行路径。
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionError {
    Unavailable,
    RuntimeBusy,
    Released,
    ForeignRuntime,
    InvalidHandle,
    MissingHostBinding(String),
    InvalidAccess(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("当前版本未实现函数编译与执行"),
            Self::RuntimeBusy => f.write_str("Runtime 忙"),
            Self::Released => f.write_str("Runtime 已释放"),
            Self::ForeignRuntime => f.write_str("值不属于当前 Runtime"),
            Self::InvalidHandle => f.write_str("句柄已失效"),
            Self::MissingHostBinding(name) => write!(f, "Host 服务未绑定：{name}"),
            Self::InvalidAccess(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for ExecutionError {}

#[derive(Debug, Clone, Default)]
pub struct VirtualMachine;
impl VirtualMachine {
    pub fn call(&self) -> Result<(), ExecutionError> {
        Err(ExecutionError::Unavailable)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FunctionCompiler;
impl FunctionCompiler {
    pub fn compile(&self, _source: &str) -> Result<(), ExecutionError> {
        Err(ExecutionError::Unavailable)
    }
}
