//! 项目提交后的派生状态由单一入口更新，写入命令只声明影响范围。

use std::collections::BTreeSet;

use super::{build, Diagnostics, EditorSession};
use crate::editor::types::EditorError;

impl EditorSession {
    pub(super) fn publish_commit(
        &mut self,
        written_files: &[String],
        affected_files: Option<&BTreeSet<String>>,
        schema_changed: bool,
    ) -> Result<(), EditorError> {
        self.commit_internal_write(written_files);
        self.diagnostics = Diagnostics::from_queries(self.queries(), &self.project_root);
        self.ref_target_cache.clear();
        if schema_changed {
            self.schema_revision = self.schema_revision.saturating_add(1);
            self.shape_cache = crate::editor::convert::ShapeCache::default();
        }
        if let Some(files) = affected_files.filter(|_| !schema_changed) {
            // schema 未变时只重算受影响文件的计数；类型目录由整个会话共享。
            for file in files {
                let counts = build::file_type_counts(self.queries(), file);
                self.file_type_counts.insert(file.clone(), counts);
            }
        } else {
            let (names, counts) = build::type_navigation(self.queries());
            self.schema_type_names = names;
            self.file_type_counts = counts;
        }
        let paths = written_files
            .iter()
            .map(|file| self.project_root.join(file))
            .collect::<Vec<_>>();
        self.language_server
            .invalidate_files(&paths)
            .map_err(EditorError::other)
    }
}
