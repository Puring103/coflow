//! Text `.cfd` loader for Coflow data models.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]
#![allow(clippy::missing_const_for_fn, clippy::similar_names, clippy::use_self)]

use coflow_api::{
    DataLoader, Diagnostic, DiagnosticSet, Label, LoadContext, LoadedRecords, LoaderDescriptor,
    ProbeResult, ProjectSourceRef, RecordOrigin, ResolvedSource, SourceLocation,
    SourceLocationSpec, SourceResolveContext, TextSpan,
};

pub mod writer;
use coflow_cft::{record_key_ident_error, CftContainer, CftSchemaField, CftSchemaTypeRef};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostics, CfdInputDictKey, CfdInputRecord, CfdInputValue,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;
pub use writer::{CfdWriter, CFD_WRITER_DESCRIPTOR};

/// Parses `.cfd` text into source-neutral input records.
///
/// The returned records use the top-level CFD record name as
/// [`CfdInputRecord::key`]. No `id` field is emitted.
///
/// # Errors
///
/// Returns text diagnostics when parsing or schema-guided conversion fails.
pub fn parse_cfd_input_records(
    schema: &CftContainer,
    source: &str,
) -> Result<Vec<CfdInputRecord>, CfdTextLoadError> {
    parse_cfd_input_records_with_spans(schema, source).map(|records| {
        records
            .into_iter()
            .map(|record| record.record)
            .collect::<Vec<_>>()
    })
}

fn parse_cfd_input_records_with_spans(
    schema: &CftContainer,
    source: &str,
) -> Result<Vec<ParsedCfdInputRecord>, CfdTextLoadError> {
    Parser::new(schema, source)
        .parse_records_with_spans()
        .map_err(CfdTextLoadError::Text)
}

/// Parses `.cfd` text and builds a validated [`CfdDataModel`].
///
/// # Errors
///
/// Returns text diagnostics for CFD syntax/conversion errors or data-model
/// diagnostics for schema/data/reference errors.
pub fn load_cfd_model(
    schema: &CftContainer,
    source: &str,
) -> Result<CfdDataModel, CfdTextLoadError> {
    let records = parse_cfd_input_records(schema, source)?;
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_input_record(record);
    }
    builder.build().map_err(CfdTextLoadError::DataModel)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CfdLoader;

pub const CFD_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "cfd",
    display_name: "Coflow data text",
    extensions: &["cfd"],
    uri_schemes: &[],
    option_keys: &[],
};

impl DataLoader for CfdLoader {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CFD_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(CFD_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if matches!(
            source.location,
            SourceLocationSpec::Path(path)
                if path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
        ) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            if source.provider_id == CFD_LOADER_DESCRIPTOR.id {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CFD-SOURCE",
                    "CFD",
                    "cfd source requires `path`",
                )));
            }
            return Ok(Vec::new());
        };
        if path.is_dir() {
            return collect_cfd_sources(path, source);
        }
        if is_cfd_path(path) {
            if source.options.get("sheets").is_some() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CFD-SOURCE",
                    "CFD",
                    format!(
                        "CFD source `{}` cannot define `sheets`",
                        source.display_name
                    ),
                )));
            }
            return Ok(vec![source.clone()]);
        }
        Err(DiagnosticSet::one(Diagnostic::error(
            "CFD-SOURCE",
            "CFD",
            format!(
                "source file `{}` has unsupported extension",
                source.display_name
            ),
        )))
    }

    fn load(
        &self,
        ctx: LoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedRecords, DiagnosticSet> {
        let SourceLocationSpec::Path(file) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                "cfd source requires `path`",
            )));
        };
        let contents = fs::read_to_string(file).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-READ",
                "CFD",
                format!("failed to read CFD source `{}`: {err}", file.display()),
            ))
        })?;
        parse_cfd_input_records_with_spans(ctx.schema, &contents)
            .map(|records| {
                let records = records
                    .into_iter()
                    .map(|record| {
                        let span = text_span(&contents, record.span);
                        record.record.with_origin(RecordOrigin::File {
                            path: file.clone(),
                            span: Some(span),
                        })
                    })
                    .collect();
                LoadedRecords { records }
            })
            .map_err(|err| cfd_error_to_diagnostics(file, &contents, err))
    }
}

fn collect_cfd_sources(
    dir: &Path,
    source: &ResolvedSource,
) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CFD-SOURCE",
                "CFD",
                format!("failed to read data source dir `{}`: {err}", dir.display()),
            ))
        })?;
    entries.sort_by_key(fs::DirEntry::path);

    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            sources.extend(collect_cfd_sources(&path, source)?);
        } else if is_cfd_path(&path) {
            sources.push(ResolvedSource {
                provider_id: CFD_LOADER_DESCRIPTOR.id.to_string(),
                display_name: path.display().to_string(),
                location: SourceLocationSpec::Path(path),
                options: source.options.clone(),
            });
        }
    }
    Ok(sources)
}

fn is_cfd_path(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("cfd")
}

fn cfd_error_to_diagnostics(file: &Path, source: &str, err: CfdTextLoadError) -> DiagnosticSet {
    match err {
        CfdTextLoadError::Text(diagnostics) => DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| {
                    let start = byte_position(source, diagnostic.span.start);
                    let end =
                        byte_position(source, diagnostic.span.end.max(diagnostic.span.start + 1));
                    Diagnostic::error(
                        format!("CFD-TEXT-{:?}", diagnostic.code),
                        "CFD",
                        diagnostic.message,
                    )
                    .with_primary(Label {
                        location: SourceLocation::FileSpan {
                            path: file.to_path_buf(),
                            start_line: start.line,
                            start_character: start.character,
                            end_line: end.line,
                            end_character: end.character,
                        },
                        message: None,
                    })
                })
                .collect(),
        },
        CfdTextLoadError::DataModel(diagnostics) => DiagnosticSet {
            diagnostics: diagnostics
                .diagnostics
                .into_iter()
                .map(|diagnostic| {
                    Diagnostic::error(
                        diagnostic.code.as_str().to_string(),
                        diagnostic.stage.to_string(),
                        diagnostic.message,
                    )
                })
                .collect(),
        },
    }
}

#[derive(Debug, Clone, Copy)]
struct Position {
    line: usize,
    character: usize,
}

fn byte_position(source: &str, byte_offset: usize) -> Position {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    Position { line, character }
}

fn text_span(source: &str, span: CfdTextSpan) -> TextSpan {
    let start = byte_position(source, span.start);
    let end = byte_position(source, span.end.max(span.start + 1));
    TextSpan {
        start_line: start.line,
        start_character: start.character,
        end_line: end.line,
        end_character: end.character,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfdTextLoadError {
    Text(CfdTextDiagnostics),
    DataModel(CfdDiagnostics),
}

impl fmt::Display for CfdTextLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(diagnostics) => diagnostics.fmt(f),
            Self::DataModel(diagnostics) => {
                let first = diagnostics
                    .diagnostics
                    .first()
                    .map_or("data model error", |diagnostic| diagnostic.message.as_str());
                f.write_str(first)
            }
        }
    }
}

impl Error for CfdTextLoadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdTextDiagnostics {
    pub diagnostics: Vec<CfdTextDiagnostic>,
}

impl CfdTextDiagnostics {
    #[must_use]
    pub fn one(diagnostic: CfdTextDiagnostic) -> Self {
        Self {
            diagnostics: vec![diagnostic],
        }
    }
}

impl fmt::Display for CfdTextDiagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let first = self
            .diagnostics
            .first()
            .map_or("CFD text error", |diagnostic| diagnostic.message.as_str());
        f.write_str(first)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfdTextDiagnostic {
    pub code: CfdTextErrorCode,
    pub message: String,
    pub span: CfdTextSpan,
}

impl CfdTextDiagnostic {
    fn error(code: CfdTextErrorCode, message: impl Into<String>, span: CfdTextSpan) -> Self {
        Self {
            code,
            message: message.into(),
            span,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfdTextErrorCode {
    Syntax,
    UnknownType,
    AbstractObjectType,
    ObjectTypeMismatch,
    UnknownField,
    DuplicateField,
    ReservedIdField,
    TypeMismatch,
    InvalidEnumVariant,
    ReferenceNeedsMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CfdTextSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
struct FieldMeta {
    name: String,
    ty: CftSchemaTypeRef,
}

#[derive(Debug, Clone)]
struct ParsedObjectFields {
    spreads: Vec<CfdInputValue>,
    fields: BTreeMap<String, CfdInputValue>,
}

struct Parser<'a> {
    schema: &'a CftContainer,
    source: &'a str,
    pos: usize,
}

#[derive(Debug, Clone)]
struct ParsedCfdInputRecord {
    record: CfdInputRecord,
    span: CfdTextSpan,
}

impl<'a> Parser<'a> {
    fn new(schema: &'a CftContainer, source: &'a str) -> Self {
        Self {
            schema,
            source,
            pos: 0,
        }
    }

    fn parse_records_with_spans(
        &mut self,
    ) -> Result<Vec<ParsedCfdInputRecord>, CfdTextDiagnostics> {
        let mut records = Vec::new();
        self.skip_ws_and_comments();
        while !self.is_eof() {
            let record_start = self.pos;
            let first = self.parse_key("record key or group type")?;
            self.skip_ws_and_comments();
            if self.eat_char(':') {
                self.validate_record_key(&first)?;
                let actual_type = self.parse_name("record type")?;
                self.validate_record_type(&actual_type)?;
                let parsed = self.parse_record_fields(&actual_type)?;
                records.push(ParsedCfdInputRecord {
                    record: CfdInputRecord::with_spreads(
                        first,
                        actual_type,
                        parsed.spreads,
                        parsed.fields,
                    ),
                    span: CfdTextSpan {
                        start: record_start,
                        end: self.pos,
                    },
                });
            } else if self.peek_char() == Some('{') {
                self.validate_group_type(&first)?;
                records.extend(self.parse_group_records(&first)?);
            } else {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "expected record type separator `:` or group body `{`",
                ));
            }
            self.skip_ws_and_comments();
        }
        Ok(records)
    }

    fn validate_group_type(&self, type_name: &str) -> Result<(), CfdTextDiagnostics> {
        if self.schema.resolve_type(type_name).is_none() {
            return Err(self.error(
                CfdTextErrorCode::UnknownType,
                format!("unknown type `{type_name}`"),
            ));
        }
        Ok(())
    }

    fn validate_record_type(&self, actual_type: &str) -> Result<(), CfdTextDiagnostics> {
        let Some(schema_type) = self.schema.resolve_type(actual_type) else {
            return Err(self.error(
                CfdTextErrorCode::UnknownType,
                format!("unknown type `{actual_type}`"),
            ));
        };
        if schema_type.is_abstract {
            return Err(self.error(
                CfdTextErrorCode::AbstractObjectType,
                format!("abstract type `{actual_type}` cannot be instantiated"),
            ));
        }
        Ok(())
    }

    fn parse_group_records(
        &mut self,
        group_type: &str,
    ) -> Result<Vec<ParsedCfdInputRecord>, CfdTextDiagnostics> {
        self.expect_char('{', "group start `{`")?;
        let mut records = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            let record_start = self.pos;
            let key = self.parse_key("record key")?;
            self.validate_record_key(&key)?;
            self.skip_ws_and_comments();
            let actual_type = if self.eat_char(':') {
                let actual_type = self.parse_name("record type")?;
                self.validate_actual_type(group_type, &actual_type)?;
                actual_type
            } else {
                self.validate_record_type(group_type)?;
                group_type.to_string()
            };
            let parsed = self.parse_record_fields(&actual_type)?;
            records.push(ParsedCfdInputRecord {
                record: CfdInputRecord::with_spreads(
                    key,
                    actual_type,
                    parsed.spreads,
                    parsed.fields,
                ),
                span: CfdTextSpan {
                    start: record_start,
                    end: self.pos,
                },
            });
        }
        Ok(records)
    }

    fn parse_record_fields(
        &mut self,
        actual_type: &str,
    ) -> Result<ParsedObjectFields, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let value = self.parse_object_value(actual_type)?;
        match value {
            CfdInputValue::Object { fields, .. } => Ok(ParsedObjectFields {
                spreads: Vec::new(),
                fields,
            }),
            CfdInputValue::ObjectSpread {
                spreads, fields, ..
            } => Ok(ParsedObjectFields { spreads, fields }),
            _ => Err(self.error(
                CfdTextErrorCode::TypeMismatch,
                "top-level record value must be an object",
            )),
        }
    }

    fn parse_value(&mut self, ty: &CftSchemaTypeRef) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if let CftSchemaTypeRef::Nullable(inner) = ty {
            if self.eat_keyword("null") {
                return Ok(CfdInputValue::Null);
            }
            return self.parse_value(inner);
        }
        if self.peek_keyword("null") {
            return Err(self.error(CfdTextErrorCode::TypeMismatch, "unexpected null value"));
        }
        if self.peek_char() == Some('@') {
            return Err(self.error(
                CfdTextErrorCode::Syntax,
                "typed and path references are no longer supported; use `&key`",
            ));
        }

        match ty {
            CftSchemaTypeRef::Int => self.parse_int(),
            CftSchemaTypeRef::Float => self.parse_float(),
            CftSchemaTypeRef::Bool => self.parse_bool(),
            CftSchemaTypeRef::String => self.parse_string_value(),
            CftSchemaTypeRef::Named(name) if self.schema.has_enum(name) => self.parse_enum(name),
            CftSchemaTypeRef::Named(name) => self.parse_object_value(name),
            CftSchemaTypeRef::Ref(name) => self.parse_ref_value(name),
            CftSchemaTypeRef::Array(inner) => self.parse_array(inner),
            CftSchemaTypeRef::Dict(key, value) => self.parse_dict(key, value),
            CftSchemaTypeRef::Nullable(inner) => self.parse_value(inner),
        }
    }

    fn parse_int(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("int")?;
        let value = text.parse::<i64>().map_err(|_| {
            CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected int",
                span,
            ))
        })?;
        Ok(CfdInputValue::Int(value))
    }

    fn parse_float(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("float")?;
        let value = text.parse::<f64>().map_err(|_| {
            CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected float",
                span,
            ))
        })?;
        if !value.is_finite() {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "float value must be finite",
                span,
            )));
        }
        Ok(CfdInputValue::Float(value))
    }

    fn parse_bool(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("bool")?;
        match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(CfdInputValue::Bool(true)),
            "false" | "0" | "no" | "n" => Ok(CfdInputValue::Bool(false)),
            _ => Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected bool",
                span,
            ))),
        }
    }

    fn parse_string_value(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let value = if self.peek_char() == Some('"') {
            self.parse_quoted_string()?
        } else {
            let (text, _) = self.parse_scalar_token("string")?;
            text
        };
        Ok(CfdInputValue::String(value))
    }

    fn parse_enum(&mut self, enum_name: &str) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (raw, span) = self.parse_scalar_token("enum value")?;
        let variant = raw
            .strip_prefix(enum_name)
            .and_then(|rest| rest.strip_prefix('.'))
            .map_or(raw.as_str(), |variant| variant);
        let Some(schema_enum) = self.schema.resolve_enum(enum_name) else {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                format!("expected enum `{enum_name}`"),
                span,
            )));
        };
        if schema_enum
            .variants
            .iter()
            .any(|schema_variant| schema_variant.name == variant)
        {
            Ok(CfdInputValue::enum_variant(enum_name, variant))
        } else {
            Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::InvalidEnumVariant,
                format!("unknown enum variant `{enum_name}.{variant}`"),
                span,
            )))
        }
    }

    fn parse_array(
        &mut self,
        inner: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.expect_char('[', "array start `[`")?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char(']') {
                break;
            }
            out.push(self.parse_value(inner)?);
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char(']');
                if self.previous_char() == Some(']') {
                    break;
                }
                continue;
            }
            self.expect_char(']', "array separator or closing `]`")?;
            break;
        }
        Ok(CfdInputValue::Array(out))
    }

    fn parse_dict(
        &mut self,
        key: &CftSchemaTypeRef,
        value: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.expect_char('{', "dict start `{`")?;
        let mut spreads = Vec::new();
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.eat_spread() {
                spreads.push(self.parse_spread_value(&CftSchemaTypeRef::Dict(
                    Box::new(key.clone()),
                    Box::new(value.clone()),
                ))?);
            } else {
                let entry_key = self.parse_dict_key(key)?;
                self.skip_ws_and_comments();
                self.expect_char(':', "dict key separator `:`")?;
                let entry_value = self.parse_value(value)?;
                out.push((entry_key, entry_value));
            }
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char('}');
                if self.previous_char() == Some('}') {
                    break;
                }
                continue;
            }
            self.expect_char('}', "dict separator or closing `}`")?;
            break;
        }
        if spreads.is_empty() {
            Ok(CfdInputValue::dict(out))
        } else {
            Ok(CfdInputValue::dict_spread(spreads, out))
        }
    }

    fn parse_dict_key(
        &mut self,
        ty: &CftSchemaTypeRef,
    ) -> Result<CfdInputDictKey, CfdTextDiagnostics> {
        match ty {
            CftSchemaTypeRef::String => {
                if self.peek_char() == Some('"') {
                    self.parse_quoted_string().map(CfdInputDictKey::String)
                } else {
                    let (text, _) = self.parse_scalar_token("dict string key")?;
                    Ok(CfdInputDictKey::String(text))
                }
            }
            CftSchemaTypeRef::Int => {
                let (text, span) = self.parse_scalar_token("dict int key")?;
                let value = text.parse::<i64>().map_err(|_| {
                    CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                        CfdTextErrorCode::TypeMismatch,
                        "expected int dict key",
                        span,
                    ))
                })?;
                Ok(CfdInputDictKey::Int(value))
            }
            CftSchemaTypeRef::Named(enum_name) if self.schema.has_enum(enum_name) => {
                let CfdInputValue::EnumVariant { variant, .. } = self.parse_enum(enum_name)? else {
                    return Err(self.error(CfdTextErrorCode::TypeMismatch, "expected enum key"));
                };
                Ok(CfdInputDictKey::enum_variant(enum_name, variant))
            }
            CftSchemaTypeRef::Nullable(inner) => self.parse_dict_key(inner),
            _ => Err(self.error(CfdTextErrorCode::TypeMismatch, "invalid dict key type")),
        }
    }

    fn parse_object_value(
        &mut self,
        expected_type: &str,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if matches!(self.peek_char(), Some('@' | '&')) {
            return if self.peek_char() == Some('@') {
                Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "typed and path references are no longer supported; use `&key`",
                ))
            } else {
                Err(self.error(
                    CfdTextErrorCode::TypeMismatch,
                    "inline object fields do not accept record references",
                ))
            };
        }

        let actual_type = if self.peek_char() == Some('{') {
            None
        } else {
            let saved = self.pos;
            let marker = self.parse_name("object type or reference key")?;
            self.skip_ws_and_comments();
            if self.peek_char() == Some('{') {
                self.validate_actual_type(expected_type, &marker)?;
                Some(marker)
            } else {
                self.pos = saved;
                let key = self.parse_name("object reference")?;
                return Err(self.reference_needs_marker(&key));
            }
        };

        let value_type = actual_type.as_deref().unwrap_or(expected_type);
        let parsed = self.parse_object_fields(value_type)?;
        Ok(if let Some(actual_type) = actual_type {
            if parsed.spreads.is_empty() {
                CfdInputValue::object(actual_type, parsed.fields)
            } else {
                CfdInputValue::object_spread_with_actual_type(
                    actual_type,
                    parsed.spreads,
                    parsed.fields,
                )
            }
        } else if parsed.spreads.is_empty() {
            CfdInputValue::object_with_declared_type(parsed.fields)
        } else {
            CfdInputValue::object_spread(parsed.spreads, parsed.fields)
        })
    }

    fn validate_actual_type(
        &self,
        expected_type: &str,
        actual_type: &str,
    ) -> Result<(), CfdTextDiagnostics> {
        let Some(schema_type) = self.schema.resolve_type(actual_type) else {
            return Err(self.error(
                CfdTextErrorCode::UnknownType,
                format!("unknown type `{actual_type}`"),
            ));
        };
        if schema_type.is_abstract {
            return Err(self.error(
                CfdTextErrorCode::AbstractObjectType,
                format!("abstract type `{actual_type}` cannot be instantiated"),
            ));
        }
        if !self.schema.is_assignable(actual_type, expected_type) {
            return Err(self.error(
                CfdTextErrorCode::ObjectTypeMismatch,
                format!("type `{actual_type}` is not assignable to `{expected_type}`"),
            ));
        }
        Ok(())
    }

    fn parse_object_fields(
        &mut self,
        type_name: &str,
    ) -> Result<ParsedObjectFields, CfdTextDiagnostics> {
        let fields = full_fields(self.schema, type_name)?;
        let fields_by_name = fields
            .iter()
            .map(|field| (field.name.as_str(), field))
            .collect::<BTreeMap<_, _>>();
        self.expect_char('{', "object start `{`")?;
        let mut spreads = Vec::new();
        let mut out = BTreeMap::new();
        let mut seen = BTreeSet::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.eat_spread() {
                spreads.push(
                    self.parse_spread_value(&CftSchemaTypeRef::Named(type_name.to_string()))?,
                );
                self.skip_ws_and_comments();
                if self.eat_char(',') {
                    self.skip_ws_and_comments();
                    self.eat_char('}');
                    if self.previous_char() == Some('}') {
                        break;
                    }
                    continue;
                }
                self.expect_char('}', "field separator or closing `}`")?;
                break;
            }
            let field_name = self.parse_key("field name")?;
            self.skip_ws_and_comments();
            if field_name == "check" && self.peek_char() == Some('{') {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "check blocks are not valid CFD data syntax",
                ));
            }
            if field_name == "id" {
                return Err(self.error(
                    CfdTextErrorCode::ReservedIdField,
                    "`id` is reserved for the record key",
                ));
            }
            if !seen.insert(field_name.clone()) {
                return Err(self.error(
                    CfdTextErrorCode::DuplicateField,
                    format!("duplicate field `{field_name}`"),
                ));
            }
            let Some(field) = fields_by_name.get(field_name.as_str()) else {
                return Err(self.error(
                    CfdTextErrorCode::UnknownField,
                    format!("unknown field `{field_name}` on type `{type_name}`"),
                ));
            };
            if !self.eat_char(':') {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "field value separator must be `:`",
                ));
            }
            let value = self.parse_value(&field.ty)?;
            out.insert(field_name, value);
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char('}');
                if self.previous_char() == Some('}') {
                    break;
                }
                continue;
            }
            self.expect_char('}', "field separator or closing `}`")?;
            break;
        }
        Ok(ParsedObjectFields {
            spreads,
            fields: out,
        })
    }

    fn parse_ref_value(
        &mut self,
        _expected_type: &str,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.eat_char('&') {
            let key = self.parse_ref_name("reference key")?;
            if self.peek_char().is_some_and(|ch| matches!(ch, '.' | '[')) {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "record references do not support paths",
                ));
            }
            self.validate_record_key(&key)?;
            return Ok(CfdInputValue::record_ref(key));
        }

        Err(self.error(
            CfdTextErrorCode::Syntax,
            "typed and path references are no longer supported; use `&key`",
        ))
    }

    fn parse_spread_value(
        &mut self,
        ty: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('&') {
            return self.parse_ref_value("");
        }
        self.parse_value(ty)
    }

    fn parse_key(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('"') {
            return self.parse_quoted_string();
        }
        self.parse_name(label)
    }

    fn parse_name(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '=' | ';' | ',' | '{' | '}' | '[' | ']' | '(' | ')' | '@' | '&' | '"'
                )
            {
                break;
            }
            self.pos += ch.len_utf8();
        }
        if self.pos == start {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::Syntax,
                format!("{label} is missing"),
                CfdTextSpan {
                    start,
                    end: self.pos,
                },
            )));
        }
        Ok(self.source[start..self.pos].to_string())
    }

    fn parse_ref_name(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace()
                || matches!(
                    ch,
                    '.' | '[' | ']' | ',' | ';' | '}' | ')' | ':' | '@' | '&'
                )
            {
                break;
            }
            self.pos += ch.len_utf8();
        }
        if self.pos == start {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::Syntax,
                format!("{label} is missing"),
                CfdTextSpan {
                    start,
                    end: self.pos,
                },
            )));
        }
        Ok(self.source[start..self.pos].to_string())
    }

    fn parse_scalar_token(
        &mut self,
        label: &str,
    ) -> Result<(String, CfdTextSpan), CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() || matches!(ch, ':' | ',' | ';' | '}' | ']' | '|') {
                break;
            }
            self.pos += ch.len_utf8();
        }
        if self.pos == start {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::Syntax,
                format!("{label} is missing"),
                CfdTextSpan {
                    start,
                    end: self.pos,
                },
            )));
        }
        let span = CfdTextSpan {
            start,
            end: self.pos,
        };
        Ok((self.source[start..self.pos].to_string(), span))
    }

    fn parse_quoted_string(&mut self) -> Result<String, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let start = self.pos;
        self.expect_char('"', "string opening quote")?;
        let mut out = String::new();
        let mut escaped = false;
        while let Some(ch) = self.peek_char() {
            self.pos += ch.len_utf8();
            if escaped {
                match ch {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => {
                        return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                            CfdTextErrorCode::Syntax,
                            format!("unsupported string escape `\\{other}`"),
                            CfdTextSpan {
                                start,
                                end: self.pos,
                            },
                        )));
                    }
                }
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Ok(out);
            } else {
                out.push(ch);
            }
        }
        Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            CfdTextErrorCode::Syntax,
            "unterminated string",
            CfdTextSpan {
                start,
                end: self.pos,
            },
        )))
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while self.peek_char().is_some_and(char::is_whitespace) {
                self.pos += self.peek_char().map_or(0, char::len_utf8);
            }
            if self.source[self.pos..].starts_with("//") {
                self.pos += 2;
                while self.peek_char().is_some_and(|ch| ch != '\n') {
                    self.pos += self.peek_char().map_or(0, char::len_utf8);
                }
                continue;
            }
            if self.source[self.pos..].starts_with('#') {
                self.pos += 1;
                while self.peek_char().is_some_and(|ch| ch != '\n') {
                    self.pos += self.peek_char().map_or(0, char::len_utf8);
                }
                continue;
            }
            break;
        }
    }

    fn expect_char(&mut self, expected: char, label: &str) -> Result<(), CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.eat_char(expected) {
            Ok(())
        } else {
            Err(self.error(CfdTextErrorCode::Syntax, format!("expected {label}")))
        }
    }

    fn eat_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn eat_spread(&mut self) -> bool {
        self.skip_ws_and_comments();
        if self.source[self.pos..].starts_with("...") {
            self.pos += 3;
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, expected: &str) -> bool {
        self.skip_ws_and_comments();
        if !self.source[self.pos..].starts_with(expected) {
            return false;
        }
        let end = self.pos + expected.len();
        if self
            .source
            .get(end..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|ch| !is_value_boundary(ch))
        {
            return false;
        }
        self.pos = end;
        true
    }

    fn peek_keyword(&mut self, expected: &str) -> bool {
        let saved = self.pos;
        let result = self.eat_keyword(expected);
        self.pos = saved;
        result
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn previous_char(&self) -> Option<char> {
        self.source[..self.pos].chars().next_back()
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    fn error(&self, code: CfdTextErrorCode, message: impl Into<String>) -> CfdTextDiagnostics {
        CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            code,
            message,
            CfdTextSpan {
                start: self.pos,
                end: self.pos,
            },
        ))
    }

    fn reference_needs_marker(&self, key: &str) -> CfdTextDiagnostics {
        self.error(
            CfdTextErrorCode::ReferenceNeedsMarker,
            format!("object reference `{key}` must be written as `&{key}`"),
        )
    }

    fn validate_record_key(&self, key: &str) -> Result<(), CfdTextDiagnostics> {
        if let Some(reason) = record_key_ident_error(key) {
            return Err(self.error(
                CfdTextErrorCode::Syntax,
                format!("invalid record key `{key}`: {reason}"),
            ));
        }
        Ok(())
    }
}

fn full_fields(
    schema: &CftContainer,
    type_name: &str,
) -> Result<Vec<FieldMeta>, CfdTextDiagnostics> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            CfdTextErrorCode::UnknownType,
            format!("unknown type `{type_name}`"),
            CfdTextSpan::default(),
        )));
    };
    Ok(schema_type
        .all_fields
        .iter()
        .map(field_meta)
        .collect::<Vec<_>>())
}

fn field_meta(field: &CftSchemaField) -> FieldMeta {
    FieldMeta {
        name: field.name.clone(),
        ty: field.ty_ref.clone(),
    }
}

fn is_value_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | ';' | '}' | ']' | '|' | ':')
}
