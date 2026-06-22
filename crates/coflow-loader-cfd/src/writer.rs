//! Writer that persists field edits back to `.cfd` source text using span
//! patches against the parsed AST.
//!
//! `CfdWriter` is the [`DataWriter`] implementation used by sources whose
//! origin is [`RecordOrigin::File`]. It maintains an in-memory cache of
//! `(source_text, CfdAst)` keyed by absolute file path so that repeated edits
//! avoid re-reading and re-parsing the file.
use coflow_api::{
    CfdDataModel, CfdValue, DataWriter, Diagnostic, DiagnosticSet, RecordOrigin, WriteCellRequest,
    WriteContext, WriteFieldPathSegment, WriteOutcome, WriterCapabilities, WriterDescriptor,
};
use coflow_cfd::ast::{CfdBlockEntry, CfdRecord as AstRecord, CfdValue as AstValue};
use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::Span;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

pub const CFD_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "cfd",
    display_name: "Coflow data text",
    capabilities: WriterCapabilities::local_full(),
};

/// Writer for `.cfd` text sources. Holds a cache of source text + AST per
/// file so repeated edits don't re-parse from disk.
#[derive(Debug, Default)]
pub struct CfdWriter {
    cache: RwLock<HashMap<PathBuf, (String, CfdAst)>>,
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
}

impl DataWriter for CfdWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &CFD_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        ctx: WriteContext<'_>,
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

        // Read or fetch from cache.
        let (source, ast) = self
            .cache
            .read()
            .map_or(None, |cache| cache.get(path).cloned())
            .map_or_else(
                || -> Result<(String, CfdAst), DiagnosticSet> {
                    let text = std::fs::read_to_string(path).map_err(|err| {
                        DiagnosticSet::one(diag(
                            "CFD-READ",
                            format!("failed to read `{}`: {err}", path.display()),
                        ))
                    })?;
                    let (ast, _) = parse_cfd(&text);
                    Ok((text, ast))
                },
                Ok,
            )?;

        let new_source = apply_patch(&source, &ast, request, ctx.model)?;

        // Write back to disk and refresh cache.
        std::fs::write(path, &new_source).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("failed to write `{}`: {err}", path.display()),
            ))
        })?;

        let (new_ast, _) = parse_cfd(&new_source);
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(path.clone(), (new_source, new_ast));
        }

        Ok(WriteOutcome {
            touched_record_origins: vec![request.origin.clone()],
            diagnostics: DiagnosticSet::empty(),
        })
    }
}

fn apply_patch(
    source: &str,
    ast: &CfdAst,
    request: &WriteCellRequest<'_>,
    model: Option<&CfdDataModel>,
) -> Result<String, DiagnosticSet> {
    validate_value(request.new_value)?;
    let record = ast
        .records
        .iter()
        .find(|r| r.key == request.record_key)
        .ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-WRITE",
                format!("record `{}` not found in AST", request.record_key),
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

    match locate_target(record, request.field_path)? {
        WriteTarget::Replace(span) => {
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
            let fragment = serialize_value(request.new_value, 1, model);
            Ok(format!(
                "{}{}{}",
                &source[..span.start],
                fragment,
                &source[span.end..]
            ))
        }
        WriteTarget::InsertTopLevel => {
            // The record's block doesn't have this field yet. Insert at the
            // end of the record body, before the closing brace.
            let block_end = record.span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            let fragment = format!(
                "  {top_field}: {},\n",
                serialize_value(request.new_value, 2, model)
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
        } => {
            // Insert a local override field inside a nested block (object
            // or dict) right before its closing `}`. This is the spread
            // override path: the field's value used to be inherited from
            // a `...@Source.path` spread, and editing it materialises a
            // local override that takes precedence.
            let block_end = block_span.end.min(source.len());
            let insert_pos = find_closing_brace(source, block_end)?;
            // Match the parser's whitespace tolerance: emit a leading
            // newline + indent to land on its own line.
            let indent = "  ".repeat(depth + 1);
            let outer = "  ".repeat(depth);
            let fragment = format!(
                "{indent}{field_name}: {},\n{outer}",
                serialize_value(request.new_value, depth + 2, model)
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

/// Where to apply the edit relative to the parsed source text.
enum WriteTarget {
    /// Replace bytes `[start, end)` with the serialised new value.
    Replace(Span),
    /// The top-level field doesn't exist yet; insert it at the end of the
    /// record's block body. The default branch from the original `patch.rs`
    /// behaviour, kept so adding brand-new fields still works.
    InsertTopLevel,
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
    },
}

fn validate_value(v: &CfdValue) -> Result<(), DiagnosticSet> {
    match v {
        CfdValue::Ref { key, .. } if key.is_empty() => Err(DiagnosticSet::one(diag(
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
        return Ok(WriteTarget::InsertTopLevel);
    };
    if path.len() == 1 {
        return Ok(WriteTarget::Replace(full_value_span(&field.value)));
    }
    locate_target_in_value(&field.value, &path[1..], 1)
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
#[allow(clippy::option_if_let_else)]
fn locate_target_in_value(
    value: &AstValue,
    path: &[WriteFieldPathSegment],
    depth: usize,
) -> Result<WriteTarget, DiagnosticSet> {
    if path.is_empty() {
        return Ok(WriteTarget::Replace(full_value_span(value)));
    }
    match (&path[0], value) {
        (WriteFieldPathSegment::Field(name), AstValue::Block(block)) => {
            let field = block.entries.iter().find_map(|e| match e {
                CfdBlockEntry::Field(f) if &f.name == name => Some(f),
                _ => None,
            });
            match field {
                Some(field) => {
                    if path.len() == 1 {
                        Ok(WriteTarget::Replace(full_value_span(&field.value)))
                    } else {
                        locate_target_in_value(&field.value, &path[1..], depth + 1)
                    }
                }
                None => {
                    // Field is missing from this block. If the path ends
                    // here, the caller wants to set a value that was
                    // inherited via a spread — insert a local override.
                    // Otherwise, drilling deeper is impossible.
                    if path.len() == 1 {
                        Ok(WriteTarget::InsertNested {
                            block_span: block.span,
                            depth,
                            field_name: name.clone(),
                        })
                    } else {
                        Err(DiagnosticSet::one(diag(
                            "CFD-WRITE",
                            format!(
                                "field `{name}` is inherited from a `...spread` and the editor \
                                 cannot drill further into it; edit the source record directly"
                            ),
                        )))
                    }
                }
            }
        }
        (WriteFieldPathSegment::Index(i), AstValue::Array(items, _)) => {
            let item = items.get(*i).ok_or_else(|| {
                DiagnosticSet::one(diag("CFD-WRITE", format!("index {i} out of bounds")))
            })?;
            if path.len() == 1 {
                Ok(WriteTarget::Replace(full_value_span(item)))
            } else {
                locate_target_in_value(item, &path[1..], depth + 1)
            }
        }
        _ => Err(DiagnosticSet::one(diag(
            "CFD-WRITE",
            format!("cannot navigate path segment {:?} in value", path[0]),
        ))),
    }
}

/// Serialize a `CfdValue` to CFD source text.
///
/// `depth` controls indentation for nested object bodies. When `model` is
/// provided, refs are emitted as fully-qualified `@Type.key` (safe for
/// polymorphic fields); otherwise the shortcut `&key` form is used.
#[must_use]
pub fn serialize_value(v: &CfdValue, depth: usize, model: Option<&CfdDataModel>) -> String {
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
        CfdValue::Ref { key, target } => model.and_then(|m| m.record(*target)).map_or_else(
            || format!("&{key}"),
            |record| format!("@{}.{key}", record.actual_type),
        ),
        CfdValue::Object(boxed) => {
            let body = boxed
                .fields
                .iter()
                .fold(String::new(), |mut acc, (name, value)| {
                    use std::fmt::Write;
                    let _ = writeln!(
                        acc,
                        "{indent}{name}: {},",
                        serialize_value(value, depth + 1, model)
                    );
                    acc
                });
            format!("{} {{\n{body}{outer}}}", boxed.actual_type)
        }
        CfdValue::Array(items) => {
            let elems: Vec<String> = items
                .iter()
                .map(|i| serialize_value(i, depth, model))
                .collect();
            format!("[{}]", elems.join(", "))
        }
        CfdValue::Dict(entries) => {
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
                    format!("{key}: {}", serialize_value(v, depth, model))
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

fn diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, "CFD", message)
}
