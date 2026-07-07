//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` is the [`DataWriter`] implementation used by sources whose
//! origin is [`RecordOrigin::File`]. It maintains an in-memory cache of
//! `(source_text, CfdAst)` keyed by absolute file path so that repeated edits
//! avoid re-reading and re-parsing the file.
mod dimensions;
mod patch;
mod render;
mod schema_nav;

use coflow_api::{
    CreateTableRequest, DataWriter, DeleteRecordRequest, Diagnostic, DiagnosticSet,
    InsertRecordRequest, RecordOrigin, RenameRecordRequest, RewriteRecordReferencesRequest,
    SourceLocationSpec, SyncHeaderRequest, TableContext, TableManager, TableManagerDescriptor,
    TableOperationResult, TextSpan, WriteCellRequest, WriteContext, WriteOutcome,
    WriterCapabilities, WriterDescriptor,
};
use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::Span;
use patch::{
    append_record_source, apply_patch, collect_spread_ref_key_spans, delete_record_span,
    find_record, replace_spans, serialize_record, spread_entries_at_path, validate_record_key,
    validate_values,
};
use render::{added_columns, cfd_top_level_fields, removed_columns, rewrite_cfd_records};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub static CFD_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "cfd",
    display_name: "Coflow data text",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: true,
        can_insert_record: true,
        can_delete_record: true,
        can_create_table: false,
        requires_full_refresh_after_write: true,
        is_remote: false,
    },
};

pub static CFD_TABLE_MANAGER_DESCRIPTOR: TableManagerDescriptor = TableManagerDescriptor {
    id: "cfd",
    display_name: "Coflow data text",
};

/// Writer for `.cfd` text sources. Holds a cache of source text + AST per
/// file so repeated edits don't re-parse from disk.
/// Cache entry tagged with the file's modification time at the moment the
/// `(source, ast)` pair was captured. We compare mtime on every read so an
/// external editor that edited the same `.cfd` between writes invalidates
/// us automatically — without that, patches built off a stale AST would
/// compute spans against the wrong text and silently corrupt the file.
#[derive(Debug, Clone)]
struct CacheEntry {
    mtime: Option<std::time::SystemTime>,
    source: String,
    ast: CfdAst,
}

#[derive(Debug, Default)]
pub struct CfdWriter {
    cache: RwLock<HashMap<PathBuf, CacheEntry>>,
}

impl CfdWriter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the cache entry for a file. Call this from the session when an
    /// external change (file watcher, user-driven reload) makes the cached
    /// AST stale.
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut cache) = self.cache.write() {
            cache.remove(path);
        }
    }

    fn read_or_parse(&self, path: &Path) -> Result<(String, CfdAst), DiagnosticSet> {
        let disk_mtime = file_mtime(path);
        let cached = self
            .cache
            .read()
            .ok()
            .and_then(|cache| cache.get(path).cloned());
        if let Some(entry) = cached {
            if entry.mtime == disk_mtime {
                return Ok((entry.source, entry.ast));
            }
            // mtime drifted — fall through to re-read from disk.
        }

        let text = std::fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-READ",
                format!("failed to read `{}`: {err}", path.display()),
            ))
        })?;
        let (ast, _) = parse_cfd(&text);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                path.to_path_buf(),
                CacheEntry {
                    mtime: disk_mtime,
                    source: text.clone(),
                    ast: ast.clone(),
                },
            );
        }
        Ok((text, ast))
    }

    fn write_source(&self, path: &Path, new_source: String) -> Result<(), DiagnosticSet> {
        std::fs::write(path, &new_source).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("failed to write `{}`: {err}", path.display()),
            ))
        })?;

        let (new_ast, _) = parse_cfd(&new_source);
        let mtime = file_mtime(path);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(
                path.to_path_buf(),
                CacheEntry {
                    mtime,
                    source: new_source,
                    ast: new_ast,
                },
            );
        }
        Ok(())
    }
}

impl CfdWriter {
    fn write_source_public(&self, path: &Path, new_source: String) -> Result<(), DiagnosticSet> {
        self.write_source(path, new_source)
    }
}

fn file_mtime(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

impl DataWriter for CfdWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &CFD_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        _ctx: WriteContext<'_>,
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

        self.write_source(path, new_source)?;

        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            inserted_record_origin: None,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn insert_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &InsertRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "cfd writer requires a path source",
            )));
        };
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
        let new_source = append_record_source(&source, &fragment);
        self.write_source(path, new_source)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: Some(RecordOrigin::File {
                path: path.clone(),
                span: Some(TextSpan {
                    start_line: 0,
                    start_character: 0,
                    end_line: 0,
                    end_character: 0,
                }),
            }),
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn delete_record(
        &self,
        _ctx: WriteContext<'_>,
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
        self.write_source(path, new_source)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: None,
            deleted_record_origin: Some(request.origin.clone()),
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn rename_record(
        &self,
        _ctx: WriteContext<'_>,
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
        self.write_source(path, new_source)?;
        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            inserted_record_origin: None,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Ok(WriteOutcome::default());
        };
        let (source, ast) = self.read_or_parse(path)?;
        let mut spans = Vec::new();
        for target in request.targets {
            let RecordOrigin::File {
                path: origin_path, ..
            } = &target.origin
            else {
                continue;
            };
            if origin_path != path {
                continue;
            }
            let record = ast
                .records
                .iter()
                .find(|record| {
                    record.key == target.record_key && record.type_name == target.actual_type
                })
                .ok_or_else(|| {
                    DiagnosticSet::one(diag(
                        "CFD-WRITE",
                        format!(
                            "record `{}.{}` not found in AST",
                            target.actual_type, target.record_key
                        ),
                    ))
                })?;
            let entries = spread_entries_at_path(
                request.schema,
                &target.actual_type,
                record,
                &target.object_path,
            )?;
            collect_spread_ref_key_spans(entries, request.old_key, &mut spans);
        }
        if spans.is_empty() {
            return Ok(WriteOutcome::default());
        }
        let replacements = spans
            .into_iter()
            .map(|span| (span, request.new_key.to_string()))
            .collect::<Vec<_>>();
        let new_source = replace_spans(&source, &replacements)?;
        self.write_source(path, new_source)?;
        Ok(WriteOutcome {
            touched_record_origins: Vec::new(),
            inserted_record_origin: None,
            deleted_record_origin: None,
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

impl TableManager for CfdWriter {
    fn descriptor(&self) -> &'static TableManagerDescriptor {
        &CFD_TABLE_MANAGER_DESCRIPTOR
    }

    fn create_table(
        &self,
        _ctx: TableContext<'_>,
        request: &CreateTableRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CFD-TABLE",
                "cfd table manager requires a path source",
            )));
        };
        if path.exists() {
            return Err(DiagnosticSet::one(diag(
                "CFD-TABLE",
                format!("file `{}` already exists", path.display()),
            )));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                DiagnosticSet::one(diag(
                    "CFD-TABLE",
                    format!("failed to create `{}`: {err}", parent.display()),
                ))
            })?;
        }
        self.write_source(path, String::new())?;
        Ok(TableOperationResult {
            headers: Vec::new(),
            added: Vec::new(),
            removed: Vec::new(),
            diagnostics: DiagnosticSet::empty(),
        })
    }

    fn sync_header(
        &self,
        _ctx: TableContext<'_>,
        request: &SyncHeaderRequest<'_>,
    ) -> Result<TableOperationResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CFD-TABLE",
                "cfd table manager requires a path source",
            )));
        };
        let (source, ast) = self.read_or_parse(path)?;
        let old_fields = cfd_top_level_fields(&ast.records, request.actual_type);
        let added = added_columns(request.headers, &old_fields);
        let removed = removed_columns(request.headers, &old_fields);
        let new_source =
            rewrite_cfd_records(&source, &ast.records, request.actual_type, request.schema)?;
        self.write_source(path, new_source)?;
        Ok(TableOperationResult {
            headers: request.headers.to_vec(),
            added,
            removed,
            diagnostics: DiagnosticSet::empty(),
        })
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
