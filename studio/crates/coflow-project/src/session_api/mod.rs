//! 项目会话能力：构建入口、只读查询、写事务与离锁源码准备。
mod factory;
mod mutations;
mod read;
mod source;
mod write;
pub use factory::ProjectSessionFactory;
pub use read::{BuildProjectSession, ReadOnlyProjectSession};
pub use source::{PreparedSourceUpdate, SourceUpdateContext, SourceValidationContext};
pub use write::WriteProjectSession;

/// 不可变代际的拥有型句柄；文件系统命令在宿主锁外运行。
#[derive(Debug, Clone)]
pub struct ProjectSnapshot {
    session: std::sync::Arc<crate::session::ProjectSession>,
    revision: u64,
}
impl ProjectSnapshot {
    pub fn queries(&self) -> crate::ProjectQueries<'_> {
        crate::ProjectQueries::new(&self.session, self.revision)
    }
    pub fn diff_against_head(&self) -> Result<crate::ProjectDiff, crate::DiagnosticSet> {
        crate::diff::diff_against_head(&self.session, self.revision)
    }
}
