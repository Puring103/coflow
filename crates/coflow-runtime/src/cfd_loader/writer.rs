//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` persists sources whose
//! origin is [`RecordOrigin::File`]. Each write reads and parses the backing
//! file from disk so transaction rollback and external edits are always
//! observed by the next operation.
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
use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_language::cfd::{parse_cfd, CfdAst, CfdSyntaxDiagnostic};
use coflow_language::Span;
use patch::{
    append_record_source, apply_patch, delete_record_span, find_record, reorder_record_spans,
    replace_spans, serialize_record, validate_record_key, validate_values,
};
use std::io::Write;
use std::path::Path;

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
pub(crate) struct CfdWriter;

impl CfdWriter {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self
    }

    fn read_or_parse(path: &Path) -> Result<(String, CfdAst), DiagnosticSet> {
        let text = std::fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-READ",
                format!("failed to read `{}`: {err}", path.display()),
            ))
        })?;
        let (ast, diagnostics) = parse_cfd(&text);
        ensure_parse_ok(path, &text, &diagnostics)?;
        Ok((text, ast))
    }

    fn write_source(path: &Path, new_source: &str) -> Result<(), DiagnosticSet> {
        let (_, diagnostics) = parse_cfd(new_source);
        ensure_parse_ok(path, new_source, &diagnostics)?;

        AtomicFile::new(path, AllowOverwrite)
            .write(|file| file.write_all(new_source.as_bytes()))
            .map_err(|err| {
                DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("failed to write `{}`: {err}", path.display()),
                ))
            })?;
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

        let (source, ast) = Self::read_or_parse(path)?;

        let new_source = apply_patch(&source, &ast, request)?;

        Self::write_source(path, &new_source)?;

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
            Self::read_or_parse(path).map_err(|diagnostics| WriteBatchFailure {
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
        Self::write_source(path, &source).map_err(|diagnostics| WriteBatchFailure {
            index: requests.len() - 1,
            diagnostics,
        })?;
        Ok(vec![WriteOutcome::default(); requests.len()])
    }

    pub(crate) fn insert_record(
        &self,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = request.source.location.path();
        validate_record_key(request.record_key)?;
        validate_values(request.fields.values())?;

        let (source, ast) = Self::read_or_parse(path)?;
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
        Self::write_source(path, &new_source)?;
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
        let (source, ast) = Self::read_or_parse(path)?;
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
        Self::write_source(path, &new_source)?;
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
        let (source, ast) = Self::read_or_parse(path)?;
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
        Self::write_source(path, &new_source)?;
        Ok(WriteOutcome::default())
    }

    pub(crate) fn reorder_records(
        &self,
        request: &ReorderRecordsRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let path = request.source.location.path();
        let (source, ast) = Self::read_or_parse(path)?;
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
        Self::write_source(path, &new_source)?;
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
