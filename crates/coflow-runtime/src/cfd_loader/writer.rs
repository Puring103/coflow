//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` persists sources whose
//! origin is [`RecordOrigin::File`]. Writes are accumulated in a workspace,
//! verified against disk state, and published atomically enough that external
//! edits are observed by the next operation.
mod dimensions;
mod patch;
mod render;
mod schema_nav;
mod target;

#[cfg(test)]
mod tests;

const CFD_INDENT: &str = "  ";

use crate::api::{
    DeleteRecordRequest, Diagnostic, DiagnosticSet, InsertRecordRequest, RenameRecordRequest,
    ReorderRecordsOperation, ReorderRecordsRequest, WriteBatchFailure, WriteCellRequest,
    WriteOutcome, WriterCapabilities,
};
use crate::data_model::RecordOrigin;
use coflow_language::cfd::{parse_cfd, CfdAst, CfdSyntaxDiagnostic};
use coflow_language::source::Span;
use coflow_staging::{StagedChange, StagedFile, StagedRemoval};
use patch::{
    append_record_source, apply_patch, apply_unset_field_patch, delete_record_span, find_record,
    reorder_record_spans, replace_spans, serialize_record, validate_record_key, validate_values,
};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::{DataSourceTextOverride, ProjectFileUpdate};

pub(crate) const CFD_WRITER_CAPABILITIES: WriterCapabilities = WriterCapabilities {
    can_edit_field: true,
    can_edit_key: true,
    can_insert_record: true,
    can_delete_record: true,
    can_reorder_records: true,
    requires_full_refresh_after_write: true,
};

/// Writer for `.cfd` text sources.
#[derive(Debug, Default)]
pub(crate) struct CfdWriter {
    workspace: Arc<Mutex<WriteWorkspace>>,
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
}

impl CfdWriter {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            workspace: Arc::new(Mutex::new(WriteWorkspace::default())),
        }
    }

    fn read_source(&self, path: &Path) -> Result<String, DiagnosticSet> {
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag(
                "CFD-READ",
                "mutation write workspace is poisoned",
            ))
        })?;
        if let Some(file) = workspace.files.get(path) {
            if file.deleted {
                return Err(DiagnosticSet::one(diag(
                    "CFD-READ",
                    format!("source `{}` is staged for deletion", path.display()),
                )));
            }
            return Ok(file.current.clone());
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
            },
        );
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

        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                "mutation write workspace is poisoned",
            ))
        })?;
        if let Some(file) = workspace.files.get_mut(path) {
            file.deleted = false;
            file.current = new_source.to_string();
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
                    current: new_source.to_string(),
                    deleted: false,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn source_overrides(&self) -> Result<Vec<DataSourceTextOverride>, DiagnosticSet> {
        let workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        Ok(workspace
            .files
            .iter()
            .map(|(path, file)| DataSourceTextOverride {
                normalized_path: crate::normalize_path(path),
                source: file.current.clone(),
                deleted: file.deleted,
            })
            .collect())
    }

    pub(crate) fn publish(&self) -> Result<(), DiagnosticSet> {
        let workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        let mut workspace = workspace;
        publish_workspace(&mut workspace)
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
                    format!("project file `{}` is not UTF-8: {error}", update.path.display()),
                ))
            })?;
            workspace.files.insert(
                update.path,
                WorkspaceFile {
                    original: update.expected,
                    current,
                    deleted: false,
                },
            );
        }
        Ok(())
    }

    pub(crate) fn delete_source(&self, path: &Path) -> Result<bool, DiagnosticSet> {
        let original = self.original_bytes(path)?;
        let mut workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        if let Some(file) = workspace.files.get_mut(path) {
            let changed = !file.deleted;
            file.deleted = true;
            return Ok(changed);
        }
        workspace.files.insert(
            path.to_path_buf(),
            WorkspaceFile {
                original,
                current: String::new(),
                deleted: true,
            },
        );
        Ok(true)
    }

    pub(crate) fn move_source(&self, from: &Path, to: &Path) -> Result<bool, DiagnosticSet> {
        let source = self.read_source(from)?;
        self.write_source(to, &source)?;
        self.delete_source(from)
    }

    fn original_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, DiagnosticSet> {
        let workspace = self.workspace.lock().map_err(|_| {
            DiagnosticSet::one(diag("CFD-WRITE", "mutation write workspace is poisoned"))
        })?;
        Ok(workspace.files.get(path).and_then(|file| file.original.clone()))
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

fn publish_workspace(workspace: &mut WriteWorkspace) -> Result<(), DiagnosticSet> {
    let mut staged = Vec::with_capacity(workspace.files.len());
    for (path, file) in &workspace.files {
        staged.push(if file.deleted {
            WorkspaceStagedChange::Delete(StagedRemoval::create(path))
        } else {
            WorkspaceStagedChange::Write(
                StagedFile::create(path, file.original.clone(), file.current.as_bytes())
                    .map_err(writer_staging_error)?,
            )
        });
    }
    for change in &staged {
        StagedChange::verify(change).map_err(writer_staging_error)?;
    }
    for index in 0..staged.len() {
        let result = match &mut staged[index] {
            WorkspaceStagedChange::Write(change) => change.publish(),
            WorkspaceStagedChange::Delete(change) => change.publish(),
        };
        if let Err(error) = result {
            for committed in staged[..=index].iter_mut().rev() {
                StagedChange::restore(committed);
            }
            return Err(writer_staging_error(error));
        }
    }
    for source in &mut staged {
        source.finish();
    }
    workspace.files.clear();
    Ok(())
}

enum WorkspaceStagedChange {
    Write(StagedFile),
    Delete(StagedRemoval),
}

impl StagedChange for WorkspaceStagedChange {
    fn verify(&self) -> Result<(), coflow_staging::StagingError> {
        match self {
            Self::Write(change) => change.verify(),
            Self::Delete(change) => change.verify(),
        }
    }

    fn publish(&mut self) -> Result<(), coflow_staging::StagingError> {
        match self {
            Self::Write(change) => change.publish(),
            Self::Delete(change) => change.publish(),
        }
    }

    fn restore(&mut self) {
        match self {
            Self::Write(change) => change.restore(),
            Self::Delete(change) => change.restore(),
        }
    }

    fn finish(&mut self) {
        match self {
            Self::Write(change) => change.finish(),
            Self::Delete(change) => change.finish(),
        }
    }
}

fn writer_staging_error(error: coflow_staging::StagingError) -> DiagnosticSet {
    if error.is_conflict() {
        DiagnosticSet::one(diag(
            "WRITE-CONFLICT",
            format!(
                "source `{}` changed while the mutation was prepared",
                error.path().display()
            ),
        ))
    } else {
        DiagnosticSet::one(diag("CFD-WRITE", error.to_string()))
    }
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
        let RecordOrigin::File { path, .. } = request.origin else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "cfd writer requires a File origin",
            )));
        };
        if request.field_path.is_empty() {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "field_path must not be empty",
            )));
        }

        let (source, ast) = self.read_or_parse(path)?;

        let new_source = apply_patch(&source, &ast, request)?;

        self.write_source(path, &new_source)?;

        Ok(WriteOutcome::default())
    }

    pub(crate) fn write_field_batch(
        &self,
        requests: &[WriteCellRequest<'_>],
    ) -> Result<Vec<WriteOutcome>, WriteBatchFailure> {
        let Some(first) = requests.first() else {
            return Ok(Vec::new());
        };
        let RecordOrigin::File { path, .. } = first.origin else {
            return Err(WriteBatchFailure {
                index: 0,
                diagnostics: DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "cfd writer requires a File origin",
                )),
            });
        };
        let (mut source, mut ast) =
            self.read_or_parse(path).map_err(|diagnostics| WriteBatchFailure {
                index: 0,
                diagnostics,
            })?;
        for (index, request) in requests.iter().enumerate() {
            let RecordOrigin::File {
                path: request_path, ..
            } = request.origin
            else {
                return Err(WriteBatchFailure {
                    index,
                    diagnostics: DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "cfd writer requires a File origin",
                    )),
                });
            };
            if request_path != path {
                return Err(WriteBatchFailure {
                    index,
                    diagnostics: DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        "cfd field batch must target one source file",
                    )),
                });
            }
            source = apply_patch(&source, &ast, request)
                .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })?;
            let (next_ast, diagnostics) = parse_cfd(&source);
            ensure_parse_ok(path, &source, &diagnostics)
                .map_err(|diagnostics| WriteBatchFailure { index, diagnostics })?;
            ast = next_ast;
        }
        self.write_source(path, &source).map_err(|diagnostics| WriteBatchFailure {
            index: requests.len() - 1,
            diagnostics,
        })?;
        Ok(vec![WriteOutcome::default(); requests.len()])
    }

    pub(crate) fn unset_field(
        &self,
        origin: &RecordOrigin,
        record_key: &str,
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
            apply_unset_field_patch(schema, &source, &ast, actual_type, record_key, field_path)?;
        self.write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn insert_record(
        &self,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = request.source.location.path();
        validate_record_key(request.record_key)?;
        validate_values(request.fields.values())?;

        let (source, ast) = self.read_or_parse(path)?;
        if ast.records.iter().any(|record| {
            record.key == request.record_key && record.type_name == request.actual_type
        }) {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "record `{}.{}` already exists",
                    request.actual_type, request.record_key
                ),
            )));
        }
        let fragment = serialize_record(
            request.schema,
            request.record_key,
            request.actual_type,
            request.fields,
        );
        let new_source = if let Some(before) = request.before {
            ensure_cfd_origin_path(before.origin, path)?;
            let anchor = ast
                .records
                .iter()
                .find(|record| {
                    record.type_name == before.actual_type && record.key == before.record_key
                })
                .ok_or_else(|| {
                    DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        format!(
                            "insert anchor `{}.{}` not found in AST",
                            before.actual_type, before.record_key
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
            find_record(&ast, request.actual_type, request.record_key).ok_or_else(|| {
                DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "record `{}.{}` not found in AST",
                        request.actual_type, request.record_key
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
        validate_record_key(request.new_key)?;
        let (source, ast) = self.read_or_parse(path)?;
        let record = find_record(&ast, request.actual_type, request.old_key).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "record `{}.{}` not found in AST",
                    request.actual_type, request.old_key
                ),
            ))
        })?;
        let new_source = replace_spans(&source, &[(record.key_span, request.new_key.to_string())])?;
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
                let first = record_index(&ast, first.actual_type, first.record_key)?;
                let second = record_index(&ast, second.actual_type, second.record_key)?;
                order.swap(first, second);
            }
            ReorderRecordsOperation::MoveBefore { record, before } => {
                ensure_cfd_origin_path(record.origin, path)?;
                let record = record_index(&ast, record.actual_type, record.record_key)?;
                let before = before
                    .map(|before| {
                        ensure_cfd_origin_path(before.origin, path)?;
                        record_index(&ast, before.actual_type, before.record_key)
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
        .position(|record| record.type_name == actual_type && record.key == key)
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
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "cfd reorder requires File origins",
        ))),
    }
}

pub(super) fn raw_span(source: &str, span: Span) -> String {
    source
        .get(span.start..span.end)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

pub(super) fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CFD", message)
}
