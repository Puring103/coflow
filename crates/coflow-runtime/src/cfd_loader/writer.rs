//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` persists sources whose
//! origin is [`RecordOrigin::File`]. Writes are accumulated in a workspace,
//! verified against disk state, and published atomically enough that external
//! edits are observed by the next operation.
mod batch;
mod dimensions;
mod patch;
mod render;
mod schema_nav;
mod target;

#[cfg(test)]
mod batch_tests;
#[cfg(test)]
mod tests;

const CFD_INDENT: &str = "  ";

use crate::api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    ReorderRecordsOperation, ReorderRecordsRequest, WriteCellRequest, WriteOutcome,
    WriterCapabilities,
};
use crate::data_model::RecordOrigin;
use coflow_language::cfd::{parse_cfd, CfdAst, CfdSyntaxDiagnostic};
use patch::{
    append_record_source, apply_unset_field_patch, delete_record_span, find_record,
    reorder_record_spans, replace_spans, serialize_record, validate_record_id,
};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::{DataSourceTextOverride, ProjectFileUpdate};

pub(crate) const CFD_WRITER_CAPABILITIES: WriterCapabilities = WriterCapabilities {
    can_edit_field: true,
    can_edit_id: true,
    can_insert_record: true,
    can_delete_record: true,
    can_reorder_records: true,
    requires_full_refresh_after_write: true,
};

/// Writer for `.cfd` text sources.
#[derive(Debug, Default)]
pub(crate) struct CfdWriter {
    workspace: Arc<Mutex<WriteWorkspace>>,
    #[cfg(test)]
    field_batch_parses: std::sync::atomic::AtomicUsize,
}

#[derive(Debug, Default)]
struct WriteWorkspace {
    files: std::collections::BTreeMap<std::path::PathBuf, WorkspaceFile>,
}

#[derive(Debug)]
struct WorkspaceFile {
    original: Option<Vec<u8>>,
    current: String,
    deleted: bool,
    preloaded: Option<crate::api::LoadedCfdSource>,
}

impl CfdWriter {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            workspace: Arc::new(Mutex::new(WriteWorkspace::default())),
            #[cfg(test)]
            field_batch_parses: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn read_source(&self, path: &Path) -> Result<String, DiagnosticSet> {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-READ", "mutation write workspace is poisoned"))
        })?;
        if let Some(file) = workspace.files.get(path) {
            if file.deleted {
                drop(workspace);
                return Err(DiagnosticSet::one(diag(
                    "CFD-READ",
                    format!("source `{}` is staged for deletion", path.display()),
                )));
            }
            let current = file.current.clone();
            drop(workspace);
            return Ok(current);
        }
        let original = std::fs::read(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-READ",
                format!("failed to read `{}`: {err}", path.display()),
            ))
        })?;
        let text = String::from_utf8(original.clone()).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-READ",
                format!("source `{}` is not UTF-8: {err}", path.display()),
            ))
        })?;
        workspace.files.insert(
            path.to_path_buf(),
            WorkspaceFile {
                original: Some(original),
                current: text.clone(),
                deleted: false,
                preloaded: None,
            },
        );
        drop(workspace);
        Ok(text)
    }

    fn read_or_parse(&self, path: &Path) -> Result<(String, CfdAst), DiagnosticSet> {
        let text = self.read_source(path)?;
        let (ast, diagnostics) = parse_cfd(&text);
        ensure_parse_ok(path, &text, &diagnostics)?;
        Ok((text, ast))
    }

    fn write_source(&self, path: &Path, new_source: &str) -> Result<(), DiagnosticSet> {
        let (_, diagnostics) = parse_cfd(new_source);
        ensure_parse_ok(path, new_source, &diagnostics)?;
        self.stage_source(path, new_source.to_string())
    }

    /// 已经校验的候选文本直接转移所有权，避免批次收尾再次解析和复制整份源码。
    fn stage_source(&self, path: &Path, new_source: String) -> Result<(), DiagnosticSet> {
        self.stage_preloaded_source(path, new_source, None)
    }

    fn stage_preloaded_source(
        &self,
        path: &Path,
        new_source: String,
        preloaded: Option<crate::api::LoadedCfdSource>,
    ) -> Result<(), DiagnosticSet> {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        if let Some(file) = workspace.files.get_mut(path) {
            file.deleted = false;
            file.current = new_source;
            // 同一文件的结构/维度操作会替换文本，先前字段批次的降低结果随之失效。
            file.preloaded = preloaded;
        } else {
            let original = match std::fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        format!("failed to read `{}`: {error}", path.display()),
                    )));
                }
            };
            workspace.files.insert(
                path.to_path_buf(),
                WorkspaceFile {
                    original,
                    current: new_source,
                    deleted: false,
                    preloaded,
                },
            );
        }
        drop(workspace);
        Ok(())
    }

    /// 降低结果只在当前事务内传递，重建逐文件接收所有权后便可释放中间态。
    pub(crate) fn take_preloaded_sources(
        &self,
    ) -> Result<
        std::collections::BTreeMap<std::path::PathBuf, crate::api::LoadedCfdSource>,
        DiagnosticSet,
    > {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        let sources = workspace
            .files
            .iter_mut()
            .filter_map(|(path, file)| {
                file.preloaded
                    .take()
                    .map(|loaded| (crate::normalize_path(path), loaded))
            })
            .collect();
        drop(workspace);
        Ok(sources)
    }

    pub(crate) fn source_overrides(&self) -> Result<Vec<DataSourceTextOverride>, DiagnosticSet> {
        let workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        let overrides = workspace
            .files
            .iter()
            .map(|(path, file)| DataSourceTextOverride {
                normalized_path: crate::normalize_path(path),
                source: file.current.clone(),
                deleted: file.deleted,
            })
            .collect();
        drop(workspace);
        Ok(overrides)
    }

    pub(crate) fn publish(&self) -> Result<Vec<crate::PublishedFile>, DiagnosticSet> {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        let result = publish_workspace(&mut workspace);
        drop(workspace);
        result
    }

    pub(crate) fn add_project_file_updates(
        &self,
        updates: Vec<ProjectFileUpdate>,
    ) -> Result<(), DiagnosticSet> {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        for update in updates {
            if workspace.files.contains_key(&update.path) {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("project file `{}` is already staged", update.path.display()),
                )));
            }
            let current = String::from_utf8(update.contents).map_err(|error| {
                DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "project file `{}` is not UTF-8: {error}",
                        update.path.display()
                    ),
                ))
            })?;
            workspace.files.insert(
                update.path,
                WorkspaceFile {
                    original: update.expected,
                    current,
                    deleted: false,
                    preloaded: None,
                },
            );
        }
        drop(workspace);
        Ok(())
    }
}

fn ensure_parse_ok(
    path: &Path,
    source: &str,
    diagnostics: &[CfdSyntaxDiagnostic],
) -> Result<(), DiagnosticSet> {
    if let Some(diagnostic) = diagnostics.first() {
        let (line, column) = line_column(source, diagnostic.span.start);
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "failed to parse `{}` for write at {line}:{column}: {}",
                path.display(),
                diagnostic.message
            ),
        )));
    }
    Ok(())
}

fn publish_workspace(
    workspace: &mut WriteWorkspace,
) -> Result<Vec<crate::PublishedFile>, DiagnosticSet> {
    // 所有宿主共用发布编排；恢复失败保留完整文件回执。
    let changes = workspace
        .files
        .iter()
        .map(|(path, file)| coflow_staging::transaction::FileChange {
            path: path.clone(),
            before: file.original.clone(),
            after: (!file.deleted).then(|| file.current.as_bytes().to_vec()),
        })
        .collect::<Vec<_>>();
    coflow_staging::transaction::publish(&changes).map_err(|failure| {
        let mut diagnostics = DiagnosticSet::one(diag(
            if failure.conflict {
                "WRITE-CONFLICT"
            } else {
                "CFD-WRITE"
            },
            failure.message.clone(),
        ));
        diagnostics.publication = Some(crate::PublicationReport::from_failure(&failure));
        diagnostics
    })?;
    let published = workspace
        .files
        .iter()
        .map(|(path, file)| {
            crate::PublishedFile::from_content(
                path.clone(),
                (!file.deleted).then_some(file.current.as_bytes()),
            )
        })
        .collect();
    workspace.files.clear();
    Ok(published)
}

fn line_column(source: &str, byte_offset: usize) -> (usize, usize) {
    let prefix = source.get(..byte_offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    (line, column)
}

impl CfdWriter {
    pub(crate) fn write_field(
        &self,
        request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.write_field_batch(std::slice::from_ref(request))
            .map(|_| WriteOutcome::default())
            .map_err(|failure| failure.diagnostics)
    }

    pub(crate) fn unset_field(
        &self,
        origin: &RecordOrigin,
        record_id: &str,
        actual_type: &str,
        field_path: &[crate::api::WriteFieldPathSegment],
        schema: &coflow_language::cft::CftSchema,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::File { path, .. } = origin else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "cfd writer requires a File origin",
            )));
        };
        let (source, ast) = self.read_or_parse(path)?;
        let new_source =
            apply_unset_field_patch(schema, &source, &ast, actual_type, record_id, field_path)?;
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn insert_record(
        &self,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = request.source.location.path();
        validate_record_id(request.record_id)?;

        let (source, ast) = self.read_or_parse(path)?;
        if ast
            .records
            .iter()
            .any(|record| record.id == request.record_id && record.type_name == request.actual_type)
        {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "record `{}.{}` already exists",
                    request.actual_type, request.record_id
                ),
            )));
        }
        let fragment = serialize_record(
            request.schema,
            request.record_id,
            request.actual_type,
            request.fields,
        );
        let new_source = if let Some(before) = request.before {
            ensure_cfd_origin_path(before.origin, path)?;
            let anchor = ast
                .records
                .iter()
                .find(|record| {
                    record.type_name == before.actual_type && record.id == before.record_id
                })
                .ok_or_else(|| {
                    DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        format!(
                            "insert anchor `{}.{}` not found in AST",
                            before.actual_type, before.record_id
                        ),
                    ))
                })?;
            insert_record_before(&source, &fragment, anchor.span.start)?
        } else {
            append_record_source(&source, &fragment)
        };
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    /// 迁移沿用记录源码，只补齐分组省略的类型标记；不把默认值物化为新赋值。
    pub(crate) fn transfer_record(
        &self,
        record: &crate::WriteRecordRef<'_>,
        destination: &crate::CfdSource,
        before: Option<&crate::WriteRecordRef<'_>>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::File { path, .. } = record.origin else {
            return Err(DiagnosticSet::one(diag(
                "WRITE-TRANSFER-ORIGIN",
                "record has no writable source origin",
            )));
        };
        let target = destination.location.path();
        if crate::normalized_path_identity(path) == crate::normalized_path_identity(target) {
            return Err(DiagnosticSet::one(diag(
                "WRITE-TRANSFER-FILE",
                "record transfer requires different source and destination files",
            )));
        }
        let (source, ast) = self.read_or_parse(path)?;
        let declaration =
            find_record(&ast, record.actual_type, record.record_id).ok_or_else(|| {
                DiagnosticSet::one(diag("CFD-WRITE", "transfer source record does not exist"))
            })?;
        let mut fragment = source[declaration.span.start..declaration.span.end].to_string();
        if declaration.group_type.is_some() && declaration.type_span.start < declaration.span.start
        {
            fragment.insert_str(
                declaration.id_span.end - declaration.span.start,
                &format!(": {}", record.actual_type),
            );
        }
        let (target_source, target_ast) = self.read_or_parse(target)?;
        if find_record(&target_ast, record.actual_type, record.record_id).is_some() {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "transfer destination already contains the record",
            )));
        }
        let target_source = if let Some(before) = before {
            ensure_cfd_origin_path(before.origin, target)?;
            let anchor = find_record(&target_ast, before.actual_type, before.record_id)
                .ok_or_else(|| {
                    DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "transfer destination anchor does not exist",
                    ))
                })?;
            insert_record_before(&target_source, &fragment, anchor.span.start)?
        } else {
            append_record_source(&target_source, &fragment)
        };
        let removed = delete_record_span(&source, declaration.span);
        let remaining = format!("{}{}", &source[..removed.start], &source[removed.end..]);
        self.write_source(target, &target_source)?;
        self.write_source(path, &remaining)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn delete_record(
        &self,
        request: &DeleteRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::File { path, .. } = request.origin else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "cfd writer requires a File origin",
            )));
        };
        let (source, ast) = self.read_or_parse(path)?;
        let record =
            find_record(&ast, request.actual_type, request.record_id).ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "record `{}.{}` not found in AST",
                        request.actual_type, request.record_id
                    ),
                ))
            })?;
        let span = delete_record_span(&source, record.span);
        let new_source = format!("{}{}", &source[..span.start], &source[span.end..]);
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn rename_record(
        &self,
        request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let RecordOrigin::File { path, .. } = request.origin else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "cfd writer requires a File origin",
            )));
        };
        validate_record_id(request.new_id)?;
        let (source, ast) = self.read_or_parse(path)?;
        let record = find_record(&ast, request.actual_type, request.old_id).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "record `{}.{}` not found in AST",
                    request.actual_type, request.old_id
                ),
            ))
        })?;
        let new_source = replace_spans(&source, &[(record.id_span, request.new_id.to_string())])?;
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn reorder_records(
        &self,
        request: &ReorderRecordsRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = request.source.location.path();
        let (source, ast) = self.read_or_parse(path)?;
        let mut order = (0..ast.records.len()).collect::<Vec<_>>();
        match request.operation {
            ReorderRecordsOperation::Swap { first, second } => {
                if first.actual_type != second.actual_type {
                    return Err(DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "records must have the same type to exchange positions",
                    )));
                }
                ensure_cfd_origin_path(first.origin, path)?;
                ensure_cfd_origin_path(second.origin, path)?;
                let first = record_index(&ast, first.actual_type, first.record_id)?;
                let second = record_index(&ast, second.actual_type, second.record_id)?;
                order.swap(first, second);
            }
            ReorderRecordsOperation::MoveBefore { record, before } => {
                ensure_cfd_origin_path(record.origin, path)?;
                let record = record_index(&ast, record.actual_type, record.record_id)?;
                let before = before
                    .map(|before| {
                        ensure_cfd_origin_path(before.origin, path)?;
                        record_index(&ast, before.actual_type, before.record_id)
                    })
                    .transpose()?;
                let moved = order.remove(record);
                let destination = before.map_or(order.len(), |before| {
                    if record < before {
                        before - 1
                    } else {
                        before
                    }
                });
                if destination > order.len() {
                    return Err(DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "record reorder destination is outside the document",
                    )));
                }
                order.insert(destination, moved);
            }
        }
        let new_source = reorder_record_spans(&source, &ast.records, &order)?;
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }
}

fn insert_record_before(
    source: &str,
    fragment: &str,
    position: usize,
) -> Result<String, DiagnosticSet> {
    let Some((prefix, suffix)) = source.get(..position).zip(source.get(position..)) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "insert anchor span is outside the source document",
        )));
    };
    Ok(format!("{prefix}{fragment}\n{suffix}"))
}

fn record_index(ast: &CfdAst, actual_type: &str, key: &str) -> Result<usize, DiagnosticSet> {
    ast.records
        .iter()
        .position(|record| record.type_name == actual_type && record.id == key)
        .ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("record `{actual_type}.{key}` not found in AST"),
            ))
        })
}

fn ensure_cfd_origin_path(origin: &RecordOrigin, expected: &Path) -> Result<(), DiagnosticSet> {
    match origin {
        RecordOrigin::File { path, .. } if path == expected => Ok(()),
        RecordOrigin::File { path, .. } => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "record origin `{}` does not match source `{}`",
                path.display(),
                expected.display()
            ),
        ))),
        RecordOrigin::None => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "cfd reorder requires File origins",
        ))),
    }
}

pub(super) fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CFD", message)
}
