use std::path::Path;

use coflow_language::source::Span;
use sha2::{Digest, Sha256};

use crate::{RecordCoordinate, RecordRestoreSnapshot, SourceRestorePatch};

use super::{diag, CfdWriter, DiagnosticSet};

impl CfdWriter {
    /// 与删除共用实际跨度，保留原始表达式、默认值省略及分组内的位置。
    pub(super) fn capture_removals(
        &self,
        path: &Path,
        source: &str,
        spans: &[Span],
    ) -> Result<String, DiagnosticSet> {
        let mut spans = spans.to_vec();
        spans.sort_by_key(|span| std::cmp::Reverse(span.start));
        let mut text = source.to_owned();
        let mut patches = Vec::with_capacity(spans.len());
        for span in spans {
            let content = text
                .get(span.start..span.end)
                .ok_or_else(conflict)?
                .to_owned();
            text.replace_range(span.start..span.end, "");
            let mut left = span.start.saturating_sub(128);
            while !text.is_char_boundary(left) {
                left += 1;
            }
            let mut right = (span.start + 128).min(text.len());
            while !text.is_char_boundary(right) {
                right -= 1;
            }
            patches.push(SourceRestorePatch {
                file: path.to_string_lossy().into_owned(),
                offset: span.start,
                expected_digest: digest(&text),
                before_context: text[left..span.start].to_owned(),
                after_context: text[span.start..right].to_owned(),
                at_start: left == 0,
                at_end: right == text.len(),
                content,
            });
        }
        self.workspace
            .lock()
            .map_err(|_| conflict())?
            .deletions
            .extend(patches);
        Ok(text)
    }

    pub(crate) fn take_record_restore(
        &self,
        record: &RecordCoordinate,
        project_root: &Path,
    ) -> Result<RecordRestoreSnapshot, DiagnosticSet> {
        let mut patches =
            std::mem::take(&mut self.workspace.lock().map_err(|_| conflict())?.deletions);
        // 每个逆补丁对应删除时的一步，反序恢复才能保持同文件多个维度覆盖的定位上下文。
        patches.reverse();
        for patch in &mut patches {
            patch.file = crate::project_path(project_root, Path::new(&patch.file));
        }
        Ok(RecordRestoreSnapshot {
            record: record.clone(),
            patches,
        })
    }

    pub(crate) fn restore_source_patch(
        &self,
        path: &Path,
        patch: &SourceRestorePatch,
    ) -> Result<(), DiagnosticSet> {
        let mut source = self.read_source(path)?;
        let offset = if digest(&source) == patch.expected_digest {
            patch.offset
        } else {
            // 无关编辑可保留；上下文缺失或不唯一时整批拒绝，不能猜测位置或覆盖整文件。
            let context = format!("{}{}", patch.before_context, patch.after_context);
            let index = if patch.at_start {
                if !source.starts_with(&context) || (patch.at_end && source.len() != context.len())
                {
                    return Err(conflict());
                }
                0
            } else if patch.at_end {
                if !source.ends_with(&context) {
                    return Err(conflict());
                }
                source.len() - context.len()
            } else {
                let first = source.find(&context).ok_or_else(conflict)?;
                // 从下一个字符继续搜索，重叠的上下文也属于歧义，不能跳过整个匹配。
                let next = first + source[first..].chars().next().map_or(0, char::len_utf8);
                if next > first && source[next..].contains(&context) {
                    return Err(conflict());
                }
                first
            };
            index + patch.before_context.len()
        };
        if !source.is_char_boundary(offset) {
            return Err(conflict());
        }
        source.insert_str(offset, &patch.content);
        self.write_source(path, &source)
    }
}

fn digest(source: &str) -> String {
    format!("{:x}", Sha256::digest(source.as_bytes()))
}

fn conflict() -> DiagnosticSet {
    DiagnosticSet::one(diag(
        "WRITE-CONFLICT",
        "deleted record source context changed; reload the project before restoring",
    ))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn restore_preserves_unrelated_edits_outside_the_record_context() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data.cfd");
        let prefix = format!("# original\n# {}\n", "padding".repeat(50));
        let record = "b: Item { title: \"ビー\" }";
        let original = format!("{prefix}{record}\n");
        std::fs::write(&path, &original).unwrap();
        let writer = CfdWriter::new();
        let remaining = writer
            .capture_removals(
                &path,
                &original,
                &[Span::new(prefix.len(), prefix.len() + record.len())],
            )
            .unwrap();
        let snapshot = writer
            .take_record_restore(
                &RecordCoordinate::try_new("Item", "b").unwrap(),
                root.path(),
            )
            .unwrap();
        let external = remaining.replace("original", "external text of a different length");
        std::fs::write(&path, &external).unwrap();
        let restore = CfdWriter::new();
        restore
            .restore_source_patch(&path, &snapshot.patches[0])
            .unwrap();
        assert_eq!(
            restore.read_source(&path).unwrap(),
            original.replace("original", "external text of a different length")
        );
    }

    #[test]
    fn empty_source_restores_exactly_and_rejects_new_content() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data.cfd");
        let record = "b: Item {}";
        std::fs::write(&path, record).unwrap();
        let writer = CfdWriter::new();
        assert_eq!(
            writer
                .capture_removals(&path, record, &[Span::new(0, record.len())])
                .unwrap(),
            ""
        );
        let snapshot = writer
            .take_record_restore(
                &RecordCoordinate::try_new("Item", "b").unwrap(),
                root.path(),
            )
            .unwrap();
        std::fs::write(&path, "").unwrap();
        let restore = CfdWriter::new();
        restore
            .restore_source_patch(&path, &snapshot.patches[0])
            .unwrap();
        assert_eq!(restore.read_source(&path).unwrap(), record);
        std::fs::write(&path, "other: Item {}").unwrap();
        assert!(CfdWriter::new()
            .restore_source_patch(&path, &snapshot.patches[0])
            .is_err());
    }

    #[test]
    fn overlapping_restore_context_is_ambiguous_after_external_edits() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data.cfd");
        let padding = " ".repeat(150);
        let record = "b: Item {}";
        let original = format!("{padding}{record}{padding}");
        std::fs::write(&path, &original).unwrap();
        let writer = CfdWriter::new();
        let remaining = writer
            .capture_removals(
                &path,
                &original,
                &[Span::new(padding.len(), padding.len() + record.len())],
            )
            .unwrap();
        let snapshot = writer
            .take_record_restore(
                &RecordCoordinate::try_new("Item", "b").unwrap(),
                root.path(),
            )
            .unwrap();
        let external = format!("{remaining} ");
        std::fs::write(&path, &external).unwrap();
        let restore = CfdWriter::new();
        assert!(restore
            .restore_source_patch(&path, &snapshot.patches[0])
            .is_err());
        assert_eq!(restore.read_source(&path).unwrap(), external);
    }
}
