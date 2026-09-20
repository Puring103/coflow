//! 局部构造操作显式携带独占能力，不使用用户可调用的内建名称编码写权限。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuildOp<R> {
    Start { source: Option<R> },
    DefaultField { owner: R, field: super::ir::FieldId },
    Set { builder: R, key: R, value: R },
    Append { builder: R, value: R },
    Remove { builder: R, key: R },
    Freeze { builder: R },
    Drop { builder: R },
}
impl<R: Copy> BuildOp<R> {
    pub fn inputs(&self) -> Vec<R> {
        match self {
            Self::Start { source } => source.iter().copied().collect(),
            Self::DefaultField { owner, .. } => vec![*owner],
            Self::Set { builder, key, value } => vec![*builder, *key, *value],
            Self::Append { builder, value } => vec![*builder, *value],
            Self::Remove { builder, key } => vec![*builder, *key],
            Self::Freeze { builder } | Self::Drop { builder } => vec![*builder],
        }
    }
    pub fn map<T>(&self, mut map: impl FnMut(R) -> Result<T, String>) -> Result<BuildOp<T>, String> {
        Ok(match self {
            Self::Start { source } => BuildOp::Start { source: source.map(&mut map).transpose()? },
            Self::DefaultField { owner, field } => BuildOp::DefaultField { owner: map(*owner)?, field: *field },
            Self::Set { builder, key, value } => BuildOp::Set { builder: map(*builder)?, key: map(*key)?, value: map(*value)? },
            Self::Append { builder, value } => BuildOp::Append { builder: map(*builder)?, value: map(*value)? },
            Self::Remove { builder, key } => BuildOp::Remove { builder: map(*builder)?, key: map(*key)? },
            Self::Freeze { builder } => BuildOp::Freeze { builder: map(*builder)? },
            Self::Drop { builder } => BuildOp::Drop { builder: map(*builder)? },
        })
    }
}
