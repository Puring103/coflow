use coflow_api::{
    DecodedSourceOptions, DiagnosticSet, DimensionSourceManager,
    DimensionSourceManagerDescriptor, DimensionSourceOptionsRequest, DimensionSourceRequest,
    DimensionSourceResult, SourceLocationSpec, TableContext,
};
use coflow_cfd::ast::CfdBlockEntry;
use coflow_cfd::parse_cfd;
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

    fn source_options(
        &self,
        _request: &DimensionSourceOptionsRequest<'_>,
    ) -> Result<DecodedSourceOptions, DiagnosticSet> {
        crate::options::decode_cfd_source_options(&serde_json::Value::Null)
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
        let expected_keys = request
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<BTreeSet<_>>();
        let existing = read_existing_dimension_cfd(path, request.variants, &expected_keys)?;
        let mut out = String::new();
        for entry in request.entries {
            let row = existing.get(&entry.key);
            let actual_type = row
                .and_then(|row| (!row.actual_type.is_empty()).then_some(row.actual_type.as_str()))
                .unwrap_or(entry.actual_type.as_str());
            let _ = writeln!(out, "{}: {actual_type} {{", entry.key);
            let _ = writeln!(out, "    default: {},", serialize_value(&entry.default, 2));
            for variant in request.variants {
                let value = row
                    .and_then(|row| row.variants.get(variant))
                    .cloned()
                    .unwrap_or_default();
                let _ = writeln!(out, "    {variant}: {},", render_cfd_cell(&value));
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
    expected_keys: &BTreeSet<&str>,
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
        if !expected_keys.contains(record.key.as_str()) {
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
    writer.write_source_public(path, body.to_string())?;
    Ok(DimensionSourceResult { changed: true })
}
