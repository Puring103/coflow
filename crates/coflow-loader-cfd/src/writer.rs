//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` is the [`DataWriter`] implementation used by sources whose
//! origin is [`RecordOrigin::File`]. It maintains an in-memory cache of
//! `(source_text, CfdAst)` keyed by absolute file path so that repeated edits
//! avoid re-reading and re-parsing the file.
use coflow_api::{
    CfdObject, CfdValue, CftContainer, CftSchemaField, CftSchemaTypeRef, CreateTableRequest,
    DataWriter,
    DeleteRecordRequest, Diagnostic, DiagnosticSet, DimensionSourceManager,
    DimensionSourceManagerDescriptor, DimensionSourceRequest, DimensionSourceResult,
    InsertRecordRequest, RecordOrigin, RenameRecordRequest, RewriteRecordReferencesRequest,
    SourceLocationSpec, SyncHeaderRequest, TableContext, TableManager, TableManagerDescriptor,
    TableOperationResult, TextSpan, WriteCellRequest, WriteContext, WriteFieldPathSegment,
    WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_cfd::ast::{CfdBlock, CfdBlockEntry, CfdRecord as AstRecord, CfdValue as AstValue};
use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::{CftSchemaDefaultValue, Span};
use coflow_data_model::CfdEnumValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

pub static CFD_DIMENSION_SOURCE_MANAGER_DESCRIPTOR: DimensionSourceManagerDescriptor =
    DimensionSourceManagerDescriptor {
        id: "cfd",
        display_name: "Coflow data text dimension source",
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

impl DimensionSourceManager for CfdWriter {
    fn descriptor(&self) -> &'static DimensionSourceManagerDescriptor {
        &CFD_DIMENSION_SOURCE_MANAGER_DESCRIPTOR
    }

    fn sync_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location else {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                "cfd dimension source requires a path source",
            )));
        };
        let existing = read_existing_dimension_cfd(path, request.variants)?;
        let mut out = String::new();
        for entry in request.entries {
            let row = existing.get(&entry.key);
            let actual_type = row
                .and_then(|row| (!row.actual_type.is_empty()).then_some(row.actual_type.as_str()))
                .unwrap_or(entry.actual_type.as_str());
            out.push_str(&format!("{}: {actual_type} {{\n", entry.key));
            out.push_str(&format!(
                "    default: {},\n",
                serialize_value(&entry.default, 2)
            ));
            for variant in request.variants {
                let value = row
                    .and_then(|row| row.variants.get(variant))
                    .cloned()
                    .unwrap_or_default();
                out.push_str(&format!("    {variant}: {},\n", render_cfd_cell(&value)));
            }
            out.push_str("}\n\n");
        }
        write_if_changed(path, &out, "CFD-DIMENSION", self)
    }
}

#[derive(Debug, Clone, Default)]
struct DimensionCfdRow {
    actual_type: String,
    variants: BTreeMap<String, String>,
}

fn read_existing_dimension_cfd(
    path: &Path,
    variants: &[String],
) -> Result<BTreeMap<String, DimensionCfdRow>, DiagnosticSet> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(err) => {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    };
    let (ast, diagnostics) = parse_cfd(&text);
    if let Some(diagnostic) = diagnostics.first() {
        return Err(DiagnosticSet::one(diag(
            "CFD-DIMENSION",
            format!(
                "failed to parse dimension source `{}`: {}",
                path.display(),
                diagnostic.message
            ),
        )));
    }
    let mut out = BTreeMap::new();
    for record in ast.records {
        let mut row = DimensionCfdRow {
            actual_type: record.type_name,
            ..DimensionCfdRow::default()
        };
        for entry in record.entries {
            let CfdBlockEntry::Field(field) = entry else {
                continue;
            };
            if variants.iter().any(|variant| variant == &field.name) {
                row.variants
                    .insert(field.name, raw_span(&text, field.value.span()));
            }
        }
        out.insert(record.key, row);
    }
    Ok(out)
}

fn render_cfd_cell(value: &str) -> String {
    if value.is_empty() {
        "null".to_string()
    } else {
        value.to_string()
    }
}

fn write_if_changed(
    path: &Path,
    body: &str,
    code: &'static str,
    writer: &CfdWriter,
) -> Result<DimensionSourceResult, DiagnosticSet> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == body => {
            return Ok(DimensionSourceResult { changed: false });
        }
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(DiagnosticSet::one(diag(
                code,
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            )));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            DiagnosticSet::one(diag(
                code,
                format!("failed to create `{}`: {err}", parent.display()),
            ))
        })?;
    }
    writer.write_source(path, body.to_string())?;
    Ok(DimensionSourceResult { changed: true })
}

fn cfd_top_level_fields(records: &[AstRecord], actual_type: &str) -> Vec<String> {
    let mut fields = BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        for field in &record.fields {
            fields.insert(field.name.clone());
        }
    }
    let mut out = vec!["id".to_string()];
    out.extend(fields);
    out
}

fn rewrite_cfd_records(
    source: &str,
    records: &[AstRecord],
    actual_type: &str,
    schema: &CftContainer,
) -> Result<String, DiagnosticSet> {
    let schema_type = schema.resolve_type(actual_type).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-TABLE",
            format!("unknown CFT type `{actual_type}`"),
        ))
    })?;
    let fields = schema_type
        .all_fields
        .iter()
        .map(|field| (field.name.clone(), field))
        .collect::<BTreeMap<_, _>>();
    let mut replacements = Vec::new();
    for record in records
        .iter()
        .filter(|record| record.type_name == actual_type)
    {
        replacements.push((
            record.span,
            render_cfd_record(source, record, schema, &fields),
        ));
    }
    replace_spans(source, &replacements)
}

fn render_cfd_record(
    source: &str,
    record: &AstRecord,
    schema: &CftContainer,
    fields: &BTreeMap<String, &CftSchemaField>,
) -> String {
    let existing = record
        .fields
        .iter()
        .map(|field| (field.name.clone(), raw_span(source, field.value.span())))
        .collect::<BTreeMap<_, _>>();
    let mut out = format!(
        "{}: {} {{\n",
        format_record_key(&record.key),
        record.type_name
    );
    for entry in &record.entries {
        let CfdBlockEntry::Spread(_, span) = entry else {
            continue;
        };
        out.push_str("  ");
        out.push_str(raw_span(source, *span).trim());
        out.push_str(",\n");
    }
    for (field_name, field) in fields {
        let value = existing
            .get(field_name)
            .cloned()
            .unwrap_or_else(|| default_cfd_value(schema, field));
        out.push_str("  ");
        out.push_str(field_name);
        out.push_str(": ");
        out.push_str(&value);
        out.push_str(",\n");
    }
    out.push_str("}\n");
    out
}

fn raw_span(source: &str, span: Span) -> String {
    source
        .get(span.start..span.end)
        .map(str::trim)
        .unwrap_or_default()
        .to_string()
}

fn format_record_key(key: &str) -> String {
    if coflow_cft::is_cft_identifier(key) {
        key.to_string()
    } else {
        format!("{key:?}")
    }
}

fn default_cfd_value(schema: &CftContainer, field: &CftSchemaField) -> String {
    let value = field.default.as_ref().map_or_else(
        || value_from_type_default(schema, &field.ty_ref),
        |default| value_from_schema_default(schema, &field.ty_ref, default),
    );
    serialize_value_for_type(&value, Some(schema), Some(&field.ty_ref), 2)
}

fn value_from_schema_default(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    default: &CftSchemaDefaultValue,
) -> CfdValue {
    match default {
        CftSchemaDefaultValue::Null => CfdValue::Null,
        CftSchemaDefaultValue::Int(value) => CfdValue::Int(*value),
        CftSchemaDefaultValue::Float(value) => CfdValue::Float(*value),
        CftSchemaDefaultValue::Bool(value) => CfdValue::Bool(*value),
        CftSchemaDefaultValue::String(value) => CfdValue::String(value.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => CfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.clone(),
            variant: Some(variant.clone()),
            value: *value,
        }),
        CftSchemaDefaultValue::EmptyArray => CfdValue::Array(Vec::new()),
        CftSchemaDefaultValue::EmptyObject => value_from_type_default(schema, ty),
    }
}

fn value_from_type_default(schema: &CftContainer, ty: &CftSchemaTypeRef) -> CfdValue {
    match ty {
        CftSchemaTypeRef::Int => CfdValue::Int(0),
        CftSchemaTypeRef::Float => CfdValue::Float(0.0),
        CftSchemaTypeRef::Bool => CfdValue::Bool(false),
        CftSchemaTypeRef::String => CfdValue::String(String::new()),
        CftSchemaTypeRef::Ref(_) | CftSchemaTypeRef::Nullable(_) => CfdValue::Null,
        CftSchemaTypeRef::Array(_) => CfdValue::Array(Vec::new()),
        CftSchemaTypeRef::Dict(_, _) => CfdValue::Dict(Vec::new()),
        CftSchemaTypeRef::Named(name) if schema.has_enum(name) => schema
            .resolve_enum(name)
            .and_then(|enm| enm.variants.first())
            .map_or_else(
                || {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: None,
                        value: 0,
                    })
                },
                |variant| {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: Some(variant.name.clone()),
                        value: variant.value,
                    })
                },
            ),
        CftSchemaTypeRef::Named(name) => {
            let fields = schema
                .resolve_type(name)
                .map(|ty| {
                    ty.all_fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                value_from_type_default(schema, &field.ty_ref),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            CfdValue::Object(Box::new(CfdObject::new(name.clone(), fields)))
        }
    }
}

fn added_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let old = old_header.iter().collect::<BTreeSet<_>>();
    new_header
        .iter()
        .filter(|header| !old.contains(header))
        .cloned()
        .collect()
}

fn removed_columns(new_header: &[String], old_header: &[String]) -> Vec<String> {
    let new = new_header.iter().collect::<BTreeSet<_>>();
    old_header
        .iter()
        .filter(|header| !new.contains(header))
        .cloned()
        .collect()
}

fn apply_patch(
    source: &str,
    ast: &CfdAst,
    request: &WriteCellRequest<'_>,
) -> Result<String, DiagnosticSet> {
    validate_value(request.new_value)?;
    let record = find_record(ast, request.actual_type, request.record_key).ok_or_else(|| {
        DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "record `{}.{}` not found in AST",
                request.actual_type, request.record_key
            ),
        ))
    })?;
    if request.field_path.is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "field_path must not be empty",
        )));
    }
    let WriteFieldPathSegment::Field(top_field) = &request.field_path[0] else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "top-level path must start with a field name",
        )));
    };

    match locate_target(
        request.schema,
        request.actual_type,
        record,
        request.field_path,
    )? {
        WriteTarget::Replace { span, ty } => {
            if span.start > source.len() || span.end > source.len() || span.start > span.end {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!(
                        "span [{}, {}) is out of bounds for source of length {}",
                        span.start,
                        span.end,
                        source.len()
                    ),
                )));
            }
            let fragment =
                serialize_value_for_type(request.new_value, Some(request.schema), Some(&ty), 1);
            Ok(format!(
                "{}{}{}",
                &source[..span.start],
                fragment,
                &source[span.end..]
            ))
        }
        WriteTarget::InsertTopLevel { ty } => {
            // The record's block doesn't have this field yet. Insert at the
            // end of the record body, before the closing brace.
            let block_end = record.span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let fragment = format!(
                "  {top_field}: {},\n",
                serialize_value_for_type(request.new_value, Some(request.schema), Some(&ty), 2)
            );
            Ok(format!(
                "{}{}{}",
                &source[..insert_pos],
                fragment,
                &source[insert_pos..]
            ))
        }
        WriteTarget::InsertNested {
            block_span,
            depth,
            field_name,
            ty,
        } => {
            // Insert a local override field inside a nested block (object
            // or dict) right before its closing `}`. This is the spread
            // override path: the field's value used to be inherited from
            // a `...&source` spread, and editing it materialises a
            // local override that takes precedence.
            let block_end = block_span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            // Match the parser's whitespace tolerance: emit a leading
            // newline + indent to land on its own line.
            let indent = "  ".repeat(depth + 1);
            let outer = "  ".repeat(depth);
            let fragment = format!(
                "{indent}{field_name}: {},\n{outer}",
                serialize_value_for_type(
                    request.new_value,
                    Some(request.schema),
                    Some(&ty),
                    depth + 2
                )
            );
            Ok(format!(
                "{}{}{}",
                &source[..insert_pos],
                fragment,
                &source[insert_pos..]
            ))
        }
    }
}

fn find_record<'a>(ast: &'a CfdAst, actual_type: &str, key: &str) -> Option<&'a AstRecord> {
    ast.records
        .iter()
        .find(|record| record.type_name == actual_type && record.key == key)
}

/// Where to apply the edit relative to the parsed source text.
enum WriteTarget {
    /// Replace bytes `[start, end)` with the serialised new value.
    Replace { span: Span, ty: CftSchemaTypeRef },
    /// The top-level field doesn't exist yet; insert it at the end of the
    /// record's block body. The default branch from the original `patch.rs`
    /// behaviour, kept so adding brand-new fields still works.
    InsertTopLevel { ty: CftSchemaTypeRef },
    /// The field path drilled into a nested block (object or dict) but the
    /// final field isn't materialised — typically because it lives in a
    /// `...spread` that the loader expanded but the source text doesn't
    /// declare. The writer materialises a local override there.
    InsertNested {
        /// Span of the entire `{ ... }` block we're inserting into.
        block_span: Span,
        /// Indent depth (0 = top of the record body, 1 = once-nested, ...).
        depth: usize,
        /// Name of the field to insert.
        field_name: String,
        /// Schema type expected at the inserted field.
        ty: CftSchemaTypeRef,
    },
}

fn validate_value(v: &CfdValue) -> Result<(), DiagnosticSet> {
    match v {
        CfdValue::Ref(target_key) if target_key.is_empty() => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "cannot write empty reference; pick a target key first",
        ))),
        CfdValue::Object(record) => {
            for v in record.fields.values() {
                validate_value(v)?;
            }
            Ok(())
        }
        CfdValue::Array(items) => {
            for v in items {
                validate_value(v)?;
            }
            Ok(())
        }
        CfdValue::Dict(entries) => {
            for (_, v) in entries {
                validate_value(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn validate_values<'a>(
    values: impl IntoIterator<Item = &'a CfdValue>,
) -> Result<(), DiagnosticSet> {
    for value in values {
        validate_value(value)?;
    }
    Ok(())
}

fn validate_record_key(key: &str) -> Result<(), DiagnosticSet> {
    if key.trim().is_empty() {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "record key must not be empty",
        )));
    }
    if let Some(reason) = coflow_cft::record_key_ident_error(key) {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("record key `{key}` is invalid: {reason}"),
        )));
    }
    Ok(())
}

fn serialize_record(
    schema: &CftContainer,
    key: &str,
    actual_type: &str,
    fields: &BTreeMap<String, CfdValue>,
) -> String {
    let mut out = format!("{key}: {actual_type} {{\n");
    for (name, value) in fields {
        out.push_str("  ");
        out.push_str(name);
        out.push_str(": ");
        let ty = type_after_field_segment(schema, actual_type, name);
        out.push_str(&serialize_value_for_type(
            value,
            Some(schema),
            ty.as_ref(),
            2,
        ));
        out.push_str(",\n");
    }
    out.push_str("}\n");
    out
}

fn append_record_source(source: &str, fragment: &str) -> String {
    if source.trim().is_empty() {
        return fragment.to_string();
    }
    let mut out = source.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(fragment);
    out
}

fn delete_record_span(source: &str, span: Span) -> Span {
    let mut start = span.start.min(source.len());
    let end = span.end.min(source.len());
    while start > 0 {
        let Some(prev) = source[..start].chars().next_back() else {
            break;
        };
        if prev == '\n' || prev == '\r' {
            start -= prev.len_utf8();
            continue;
        }
        break;
    }
    Span::new(start, end)
}

fn find_closing_brace(source: &str, near: usize) -> Result<usize, DiagnosticSet> {
    let end = near.min(source.len());
    let bytes = source.as_bytes();
    for i in (0..end).rev() {
        if bytes[i] == b'}' {
            return Ok(i);
        }
    }
    Err(DiagnosticSet::one(diag(
        "CFD-WRITE",
        "closing brace not found",
    )))
}

fn full_value_span(value: &AstValue) -> Span {
    if let AstValue::Block(b) = value {
        if let Some((_, tm_span)) = &b.type_marker {
            return Span::new(tm_span.start, b.span.end);
        }
    }
    value.span()
}

/// Walk the field path inside the AST and decide whether the writer
/// should replace an existing span, append a brand-new top-level field, or
/// insert a local override into a nested block (the spread case).
fn locate_target(
    schema: &CftContainer,
    actual_type: &str,
    record: &AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<WriteTarget, DiagnosticSet> {
    let WriteFieldPathSegment::Field(name) = &path[0] else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "top-level path must start with a field name",
        )));
    };
    let Some(field) = find_field_in_record(record, name) else {
        // Top-level field doesn't exist yet. If the caller is pointing
        // deeper than the top, that's a contradiction — they're asking us
        // to drill through a field we don't have. Surface a clean error.
        if path.len() > 1 {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("top-level field `{name}` not found in record"),
            )));
        }
        let Some(ty) = type_after_field_segment(schema, actual_type, name) else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("field `{name}` not found on type `{actual_type}`"),
            )));
        };
        return Ok(WriteTarget::InsertTopLevel { ty });
    };
    let Some(next_type) = type_after_field_segment(schema, actual_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` not found on type `{actual_type}`"),
        )));
    };
    if path.len() == 1 {
        return Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        });
    }
    locate_target_in_value(schema, &next_type, &field.value, &path[1..], 1)
}

fn find_field_in_record<'a>(record: &'a AstRecord, name: &str) -> Option<&'a coflow_cfd::CfdField> {
    record.fields.iter().find(|f| f.name == name).or_else(|| {
        record.entries.iter().find_map(|e| match e {
            CfdBlockEntry::Field(f) if f.name == name => Some(f),
            _ => None,
        })
    })
}

/// Recursive walker: navigate `path` inside `value` and decide whether to
/// replace an existing span or insert into the surrounding block. `depth`
/// is the current nesting depth from the top-level record body, used to
/// pick indentation for inserted overrides.
fn locate_target_in_value(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    value: &AstValue,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    if path.is_empty() {
        return Ok(WriteTarget::Replace {
            span: full_value_span(value),
            ty: current_type.clone(),
        });
    }
    match (&path[0], value) {
        (WriteFieldPathSegment::Field(name), AstValue::Block(block)) => {
            locate_field_target(schema, current_type, block, name, path, depth)
        }
        (WriteFieldPathSegment::Index(index), AstValue::Array(items, _)) => {
            locate_array_target(schema, current_type, items, *index, path, depth)
        }
        (WriteFieldPathSegment::DictKey(key), AstValue::Block(block)) => {
            locate_dict_target(schema, current_type, block, key, path, depth)
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

#[allow(clippy::option_if_let_else)]
fn locate_field_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    block: &CfdBlock,
    name: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_field_segment_for_ref(schema, current_type, name) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("field `{name}` cannot be selected from this value"),
        )));
    };
    let field = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field) if field.name == name => Some(field),
        _ => None,
    });
    match field {
        Some(field) if path.len() == 1 => Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        }),
        Some(field) => {
            locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
        }
        None if path.len() == 1 => Ok(WriteTarget::InsertNested {
            block_span: block.span,
            depth,
            field_name: name.to_string(),
            ty: next_type,
        }),
        None => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!(
                "field `{name}` is inherited from a `...spread` and the editor \
                 cannot drill further into it; edit the source record directly"
            ),
        ))),
    }
}

fn locate_array_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    items: &[AstValue],
    index: usize,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some(next_type) = type_after_index_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from this value"),
        )));
    };
    let item = items.get(index).ok_or_else(|| {
        DiagnosticSet::one(diag("CFD-WRITE", format!("index {index} out of bounds")))
    })?;
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(item),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, item, &path[1..], depth + 1)
    }
}

fn locate_dict_target(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    block: &CfdBlock,
    key: &str,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` cannot be selected from this value"),
        )));
    };
    let Some(field) = block.entries.iter().find_map(|entry| match entry {
        CfdBlockEntry::Field(field)
            if dict_key_path_matches(schema, &key_type, &field.name, key) =>
        {
            Some(field)
        }
        _ => None,
    }) else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("dict key `{key}` not found in source block"),
        )));
    };
    if path.len() == 1 {
        Ok(WriteTarget::Replace {
            span: full_value_span(&field.value),
            ty: next_type,
        })
    } else {
        locate_target_in_value(schema, &next_type, &field.value, &path[1..], depth + 1)
    }
}

fn type_after_field_segment(
    schema: &CftContainer,
    actual_type: &str,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    schema
        .resolve_type(actual_type)?
        .all_fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.ty_ref.clone())
}

fn type_after_field_segment_for_ref(
    schema: &CftContainer,
    current_type: &CftSchemaTypeRef,
    field_name: &str,
) -> Option<CftSchemaTypeRef> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Named(type_name) if schema.has_type(type_name) => {
            type_after_field_segment(schema, type_name, field_name)
        }
        _ => None,
    }
}

fn object_type_name<'a>(
    expected: Option<&'a CftSchemaTypeRef>,
    actual_type: &'a str,
) -> Option<&'a str> {
    match expected.map(non_nullable) {
        Some(CftSchemaTypeRef::Named(type_name)) => Some(type_name.as_str()),
        Some(CftSchemaTypeRef::Ref(_)) => None,
        Some(_) | None => Some(actual_type),
    }
}

fn type_after_index_segment(current_type: &CftSchemaTypeRef) -> Option<CftSchemaTypeRef> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Array(inner) => Some((**inner).clone()),
        _ => None,
    }
}

fn type_after_dict_key_segment(
    current_type: &CftSchemaTypeRef,
) -> Option<(CftSchemaTypeRef, CftSchemaTypeRef)> {
    match non_nullable(current_type) {
        CftSchemaTypeRef::Dict(key, item) => Some(((**key).clone(), (**item).clone())),
        _ => None,
    }
}

fn dict_key_path_matches(
    schema: &CftContainer,
    key_type: &CftSchemaTypeRef,
    source_key: &str,
    path_key: &str,
) -> bool {
    if source_key == path_key {
        return true;
    }
    match non_nullable(key_type) {
        CftSchemaTypeRef::String if path_key.starts_with('"') => {
            serde_json::from_str::<String>(path_key).is_ok_and(|decoded| decoded == source_key)
        }
        CftSchemaTypeRef::Named(enum_name) if schema.has_enum(enum_name) => path_key
            .strip_prefix(enum_name)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|variant| variant == source_key),
        CftSchemaTypeRef::Nullable(inner) => {
            dict_key_path_matches(schema, inner, source_key, path_key)
        }
        _ => false,
    }
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

/// Serialize a `CfdValue` to CFD source text.
///
/// `depth` controls indentation for nested object bodies. Refs are always
/// emitted as `&key`; the target type is supplied by the surrounding schema
/// context rather than by the value syntax.
#[must_use]
pub fn serialize_value(v: &CfdValue, depth: usize) -> String {
    serialize_value_for_type(v, None, None, depth)
}

fn serialize_value_for_type(
    v: &CfdValue,
    schema: Option<&CftContainer>,
    expected: Option<&CftSchemaTypeRef>,
    depth: usize,
) -> String {
    let indent = "  ".repeat(depth);
    let outer = "  ".repeat(depth.saturating_sub(1));
    match v {
        CfdValue::Null => "null".to_string(),
        CfdValue::Bool(v) => v.to_string(),
        CfdValue::Int(v) => v.to_string(),
        CfdValue::Float(v) => {
            let s = v.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') {
                s
            } else {
                format!("{s}.0")
            }
        }
        CfdValue::String(v) => format!("{v:?}"),
        CfdValue::Enum(e) => e
            .variant
            .clone()
            .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
        CfdValue::Ref(target_key)
            if matches!(expected.map(non_nullable), Some(CftSchemaTypeRef::Ref(_))) =>
        {
            format!("&{target_key}")
        }
        CfdValue::Ref(target_key) => format!("&{target_key}"),
        CfdValue::Object(boxed) => {
            let body = boxed
                .fields
                .iter()
                .fold(String::new(), |mut acc, (name, value)| {
                    use std::fmt::Write;
                    let field_type = schema
                        .zip(object_type_name(expected, &boxed.actual_type))
                        .and_then(|(schema, type_name)| {
                            type_after_field_segment(schema, type_name, name)
                        });
                    let _ = writeln!(
                        acc,
                        "{indent}{name}: {},",
                        serialize_value_for_type(value, schema, field_type.as_ref(), depth + 1)
                    );
                    acc
                });
            format!("{} {{\n{body}{outer}}}", boxed.actual_type)
        }
        CfdValue::Array(items) => {
            let item_type = expected.and_then(|ty| match non_nullable(ty) {
                CftSchemaTypeRef::Array(inner) => Some(inner.as_ref()),
                _ => None,
            });
            let elems: Vec<String> = items
                .iter()
                .map(|i| serialize_value_for_type(i, schema, item_type, depth))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        CfdValue::Dict(entries) => {
            let item_type = expected.and_then(|ty| match non_nullable(ty) {
                CftSchemaTypeRef::Dict(_, item) => Some(item.as_ref()),
                _ => None,
            });
            let pairs: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        coflow_api::CfdDictKey::String(s) => format!("{s:?}"),
                        coflow_api::CfdDictKey::Int(n) => n.to_string(),
                        coflow_api::CfdDictKey::Enum(e) => e
                            .variant
                            .clone()
                            .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
                    };
                    format!(
                        "{key}: {}",
                        serialize_value_for_type(v, schema, item_type, depth)
                    )
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CFD", message)
}

fn spread_entries_at_path<'a>(
    schema: &CftContainer,
    actual_type: &str,
    record: &'a AstRecord,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        return Ok(record.entries.as_slice());
    }
    let root_type = CftSchemaTypeRef::Named(actual_type.to_string());
    let Some((value, value_type)) =
        value_at_spread_path_segment(schema, record.entries.as_slice(), &root_type, &path[0])?
    else {
        return Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            "spread rewrite site was not found",
        )));
    };
    block_entries_at_path(schema, value, &value_type, &path[1..])
}

fn block_entries_at_path<'a>(
    schema: &CftContainer,
    value: &'a AstValue,
    ty: &CftSchemaTypeRef,
    path: &[WriteFieldPathSegment],
) -> Result<&'a [CfdBlockEntry], DiagnosticSet> {
    if path.is_empty() {
        let AstValue::Block(block) = value else {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                "spread rewrite site is not an object block",
            )));
        };
        return Ok(block.entries.as_slice());
    }
    match value {
        AstValue::Block(block) => {
            let Some((next, next_type)) =
                value_at_spread_path_segment(schema, block.entries.as_slice(), ty, &path[0])?
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "spread rewrite site was not found",
                )));
            };
            block_entries_at_path(schema, next, &next_type, &path[1..])
        }
        AstValue::Array(items, _) => {
            let WriteFieldPathSegment::Index(index) = path[0] else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("cannot navigate path segment {:?} in array value", path[0]),
                )));
            };
            let Some(item_type) = type_after_index_segment(ty) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    "array index cannot be selected from this value",
                )));
            };
            let Some(item) = items.get(index) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("index {index} out of bounds while locating spread rewrite site"),
                )));
            };
            block_entries_at_path(schema, item, &item_type, &path[1..])
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

fn value_at_spread_path_segment<'a>(
    schema: &CftContainer,
    entries: &'a [CfdBlockEntry],
    current_type: &CftSchemaTypeRef,
    segment: &WriteFieldPathSegment,
) -> Result<Option<(&'a AstValue, CftSchemaTypeRef)>, DiagnosticSet> {
    match segment {
        WriteFieldPathSegment::Field(field_name) => {
            let Some(next_type) =
                type_after_field_segment_for_ref(schema, current_type, field_name)
            else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("field `{field_name}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field) if field.name == *field_name => Some(&field.value),
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::DictKey(key) => {
            let Some((key_type, next_type)) = type_after_dict_key_segment(current_type) else {
                return Err(DiagnosticSet::one(diag(
                    "CFD-WRITE",
                    format!("dict key `{key}` cannot be selected from this value"),
                )));
            };
            Ok(entries
                .iter()
                .find_map(|entry| match entry {
                    CfdBlockEntry::Field(field)
                        if dict_key_path_matches(schema, &key_type, &field.name, key) =>
                    {
                        Some(&field.value)
                    }
                    _ => None,
                })
                .map(|value| (value, next_type)))
        }
        WriteFieldPathSegment::Index(index) => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("array index `{index}` cannot be selected from an object block"),
        ))),
    }
}

fn collect_spread_ref_key_spans(entries: &[CfdBlockEntry], old_key: &str, out: &mut Vec<Span>) {
    for entry in entries {
        if let CfdBlockEntry::Spread(value, _) = entry {
            collect_ref_key_spans_in_value(value, old_key, out);
        }
    }
}

fn collect_ref_key_spans_in_value(value: &AstValue, old_key: &str, out: &mut Vec<Span>) {
    match value {
        AstValue::Ref(reference) => {
            if reference.key.0 == old_key {
                out.push(reference.key.1);
            }
        }
        AstValue::Array(items, _) => {
            for item in items {
                collect_ref_key_spans_in_value(item, old_key, out);
            }
        }
        AstValue::Spread(inner, _) => {
            collect_ref_key_spans_in_value(inner, old_key, out);
        }
        AstValue::Block(_)
        | AstValue::Scalar(_, _)
        | AstValue::QuotedString(_, _)
        | AstValue::Null(_) => {}
    }
}

fn replace_spans(source: &str, replacements: &[(Span, String)]) -> Result<String, DiagnosticSet> {
    let mut out = source.to_string();
    let mut sorted = replacements.to_vec();
    sorted.sort_by_key(|(span, _)| span.start);
    for (span, _) in &sorted {
        if span.start > source.len() || span.end > source.len() || span.start > span.end {
            return Err(DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!(
                    "span [{}, {}) is out of bounds for source of length {}",
                    span.start,
                    span.end,
                    source.len()
                ),
            )));
        }
    }
    sorted.dedup_by_key(|(span, _)| (span.start, span.end));
    for (span, replacement) in sorted.into_iter().rev() {
        out.replace_range(span.start..span.end, &replacement);
    }
    Ok(out)
}
