use coflow_api::{
    byte_range, DecodedSourceOptions, Diagnostic, DiagnosticSet, DimensionSourceLoadRequest,
    DimensionSourceLoadResult, DimensionSourceManager, DimensionSourceManagerDescriptor,
    DimensionSourceOptionsRequest, DimensionSourceRequest, DimensionSourceResult,
    RewriteDimensionRecordRequest, SourceLocationSpec, TableContext, WriteDimensionValueRequest,
};
use coflow_cfd::ast::CfdBlockEntry;
use coflow_cfd::parse_cfd;
use coflow_cft::{CftValueType, RecordKey};
use coflow_data_model::{CfdInputDimensionValue, RecordOrigin, TextSpan};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use super::render::serialize_value;
use super::{diag, raw_span, CfdWriter};

pub(super) static CFD_DIMENSION_SOURCE_MANAGER_DESCRIPTOR: DimensionSourceManagerDescriptor =
    DimensionSourceManagerDescriptor {
        id: "cfd",
        display_name: "Coflow data text dimension source",
    };

impl DimensionSourceManager for CfdWriter {
    fn descriptor(&self) -> &'static DimensionSourceManagerDescriptor {
        &CFD_DIMENSION_SOURCE_MANAGER_DESCRIPTOR
    }

    fn load_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceLoadRequest<'_>,
    ) -> Result<DimensionSourceLoadResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let text = std::fs::read_to_string(path).map_err(|err| {
            DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                format!(
                    "failed to read dimension source `{}`: {err}",
                    path.display()
                ),
            ))
        })?;
        let (ast, syntax) = parse_cfd(&text);
        if !syntax.is_empty() {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                syntax
                    .into_iter()
                    .map(|item| item.message)
                    .collect::<Vec<_>>()
                    .join("; "),
            )));
        }
        let nullable_type = CftValueType::Nullable(Box::new(
            request.schema.source_field.value_type.non_nullable().clone(),
        ));
        let mut values = Vec::new();
        let mut diagnostics = DiagnosticSet::empty();
        for record in ast.records {
            if request.schema.source_type.is_singleton
                && record.key != request.schema.source_field.name.as_str()
            {
                continue;
            }
            let source_key = match RecordKey::new(record.key.clone()) {
                Ok(key) => key,
                Err(err) => {
                    diagnostics.push(Diagnostic::error("CFD-DIMENSION", "CFD", err.to_string()));
                    continue;
                }
            };
            for entry in record.entries {
                let CfdBlockEntry::Field(field) = entry else {
                    continue;
                };
                let Some(variant) = request.schema.dimension.variant(&field.name) else {
                    continue;
                };
                let value = match crate::lower::lower_value(
                    request.schema.schema,
                    &field.value,
                    &nullable_type,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        diagnostics.push(Diagnostic::error(
                            "CFD-DIMENSION-VALUE",
                            "CFD",
                            err.diagnostics
                                .into_iter()
                                .map(|item| item.message)
                                .collect::<Vec<_>>()
                                .join("; "),
                        ));
                        continue;
                    }
                };
                let span = field.value.span();
                let range = byte_range(&text, span.start, span.end);
                values.push(CfdInputDimensionValue {
                    source_type: request.schema.source_type.name.clone(),
                    source_key: source_key.clone(),
                    field: request.schema.source_field.name.clone(),
                    dimension: request.schema.dimension.name.clone(),
                    variant: variant.clone(),
                    value,
                    origin: RecordOrigin::File {
                        path: path.clone(),
                        span: Some(TextSpan {
                            start_line: range.start.line,
                            start_character: range.start.character,
                            end_line: range.end.line,
                            end_character: range.end.character,
                        }),
                    },
                });
            }
        }
        if diagnostics.is_empty() {
            Ok(DimensionSourceLoadResult { values })
        } else {
            Err(diagnostics)
        }
    }

    fn source_options(
        &self,
        _request: &DimensionSourceOptionsRequest<'_>,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        crate::options::decode_cfd_source_options(&serde_json::Value::Null)
    }

    fn write_dimension_value(
        &self,
        _ctx: TableContext<'_>,
        request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let variants = request
            .schema
            .dimension
            .variants
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut rows = read_existing_dimension_cfd(path, &variants, None)?;
        let physical_key = if request.schema.source_type.is_singleton {
            request.schema.source_field.name.as_str()
        } else {
            request.source_key.as_str()
        };
        let row = rows.get_mut(physical_key).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-DIMENSION-WRITE",
                format!("dimension source has no record `{physical_key}`"),
            ))
        })?;
        match request.new_value {
            Some(value) => {
                row.variants
                    .insert(request.variant.to_string(), serialize_value(value, 2));
            }
            None => {
                row.variants.remove(request.variant.as_str());
            }
        }
        let out = render_dimension_cfd(&rows, request.schema.source_type.name.as_str(), &variants);
        write_if_changed(path, &out, "CFD-DIMENSION-WRITE")
    }

    fn rewrite_dimension_record(
        &self,
        _ctx: TableContext<'_>,
        request: &RewriteDimensionRecordRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        if request.schema.source_type.is_singleton {
            return Ok(DimensionSourceResult::default());
        }
        let SourceLocationSpec::Path(path) = &request.source.location;
        let variants = request
            .schema
            .dimension
            .variants
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut rows = read_existing_dimension_cfd(path, &variants, None)?;
        let row = rows.remove(request.old_key.as_str()).ok_or_else(|| {
            DiagnosticSet::one(diag(
                "CFD-DIMENSION-WRITE",
                format!("dimension source has no record `{}`", request.old_key),
            ))
        })?;
        if let Some(new_key) = request.new_key {
            if rows.insert(new_key.to_string(), row).is_some() {
                return Err(DiagnosticSet::one(diag(
                    "CFD-DIMENSION-WRITE",
                    format!("dimension source already has record `{new_key}`"),
                )));
            }
        }
        let out = render_dimension_cfd(&rows, request.schema.source_type.name.as_str(), &variants);
        write_if_changed(path, &out, "CFD-DIMENSION-WRITE")
    }

    fn sync_dimension_source(
        &self,
        _ctx: TableContext<'_>,
        request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &request.source.location;
        let expected_keys = request
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<BTreeSet<_>>();
        let existing = read_existing_dimension_cfd(path, request.variants, Some(&expected_keys))?;
        let mut out = String::new();
        for entry in request.entries {
            let row = existing.get(&entry.key);
            let actual_type = entry.actual_type.as_str();
            let _ = writeln!(out, "{}: {actual_type} {{", entry.key);
            let _ = writeln!(out, "    default: {},", serialize_value(&entry.default, 2));
            for variant in request.variants {
                if let Some(value) = row.and_then(|row| row.variants.get(variant)) {
                    let _ = writeln!(out, "    {variant}: {},", render_cfd_cell(value));
                } else if row.is_none() {
                    let _ = writeln!(out, "    {variant}: null,");
                }
            }
            out.push_str("}\n\n");
        }
        write_if_changed(path, &out, "CFD-DIMENSION")
    }
}

#[derive(Debug, Clone, Default)]
struct DimensionCfdRow {
    default: String,
    variants: BTreeMap<String, String>,
}

fn render_dimension_cfd(
    rows: &BTreeMap<String, DimensionCfdRow>,
    actual_type: &str,
    variants: &[String],
) -> String {
    let mut out = String::new();
    for (key, row) in rows {
        let _ = writeln!(out, "{key}: {actual_type} {{");
        let _ = writeln!(out, "    default: {},", render_cfd_cell(&row.default));
        for variant in variants {
            if let Some(value) = row.variants.get(variant) {
                let _ = writeln!(out, "    {variant}: {},", render_cfd_cell(value));
            }
        }
        out.push_str("}\n\n");
    }
    out
}

fn read_existing_dimension_cfd(
    path: &Path,
    variants: &[String],
    expected_keys: Option<&BTreeSet<&str>>,
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
        if expected_keys.is_some_and(|keys| !keys.contains(record.key.as_str())) {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                format!(
                    "dimension source `{}` contains unmanaged id `{}`; variant tables can only edit existing records",
                    path.display(),
                    record.key
                ),
            )));
        }
        if out.contains_key(&record.key) {
            return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                format!(
                    "dimension source `{}` contains duplicate id `{}`; variant tables can only edit existing records",
                    path.display(),
                    record.key
                ),
            )));
        }
        let mut row = DimensionCfdRow::default();
        for entry in record.entries {
            let CfdBlockEntry::Field(field) = entry else {
                continue;
            };
            if field.name == "default" {
                row.default = raw_span(&text, field.value.span());
            } else if variants.iter().any(|variant| variant == &field.name) {
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
    CfdWriter::write_source(path, body)?;
    Ok(DimensionSourceResult { changed: true })
}
