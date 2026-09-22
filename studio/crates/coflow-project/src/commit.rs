//! 已提交项目变更的影响范围，宿主据此更新派生状态。
use std::collections::BTreeSet;
#[derive(Debug, Clone, Default)]
pub struct ProjectCommit {
    pub generation_changed: bool,
    pub schema_changed: bool,
    pub written_files: Vec<String>,
    /// None 表示项目范围整体更新；Some 表示受影响数据文件的精确集合。
    pub affected_files: Option<BTreeSet<String>>,
}
impl crate::MutationReport {
    pub fn commit(&self) -> ProjectCommit {
        ProjectCommit {
            generation_changed: self.generation_changed,
            schema_changed: false,
            written_files: self.written_files.clone(),
            affected_files: Some(self.changed_records.keys().cloned().collect()),
        }
    }
}
