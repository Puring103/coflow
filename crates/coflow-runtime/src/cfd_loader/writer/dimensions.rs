use crate::api::{
    byte_range, Diagnostic, DiagnosticSet, DimensionSourceLoadRequest, DimensionSourceLoadResult,
    DimensionSourceRequest, DimensionSourceResult, Label, RewriteDimensionRecordRequest,
    SourceLocation, WriteDimensionValueRequest,
};
use crate::data_model::{DimensionValueDraft, RecordOrigin, TextSpan};
use coflow_core::schema::RecordKey;
use coflow_language::cfd::parse_cfd;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::path::Path;

use super::render::serialize_value;
use super::CFD_INDENT;
use super::{diag, raw_span, CfdWriter};

impl CfdWriter {
    // 维度源加载需要在一次遍历中同时校验形状、键和值，保持完整流程更便于核对诊断位置。
    #[allow(clippy::too_many_lines)]
    pub(crate) fn load_dimension_source(
        &self,
        request: &DimensionSourceLoadRequest<'_>,
    ) -> Result<DimensionSourceLoadResult, DiagnosticSet> {
        let path = request.source.location.path();
        let text = self.read_source(path)?;
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
        let expected_type = coflow_core::schema::dimension_record_type(
            request.schema.dimension.name.as_str(),
            request.schema.source_field.declaring_type.as_str(),
            request.schema.source_field.name.as_str(),
        );
        // 先核对维度记录的类型位置，避免字段错误掩盖来源类型错误。
        let mut diagnostics = DiagnosticSet::empty();
        for record in &ast.records {
            let name = ast
                .imports
                .iter()
                .find(|name| name.rsplit("::").next() == Some(record.type_name.as_str()))
                .map_or(record.type_name.as_str(), String::as_str);
            if request
                .schema
                .schema
                .resolve_record_type_name(name)
                .ok()
                .as_deref()
                != Some(expected_type.as_str())
            {
                diagnostics.push(
                    Diagnostic::error(
                        "CFD-DIMENSION-TYPE",
                        "CFD",
                        format!("dimension record type must be `{expected_type}`"),
                    )
                    .with_primary(file_span_label(
                        path,
                        &text,
                        record.type_span.start,
                        record.type_span.end,
                        "incompatible dimension record type",
                    )),
                );
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let records =
            super::super::lower::lower_records(request.schema.schema, &ast).map_err(|errors| {
                DiagnosticSet {
                    diagnostics: errors
                        .diagnostics
                        .into_iter()
                        .map(|error| {
                            Diagnostic::error("CFD-DIMENSION", "CFD", error.message).with_primary(
                                file_span_label(
                                    path,
                                    &text,
                                    error.span.start,
                                    error.span.end,
                                    "invalid dimension value",
                                ),
                            )
                        })
                        .collect(),
                }
            })?;
        let mut seen = BTreeSet::new();
        let mut values = Vec::new();
        for (parsed, syntax) in records.into_iter().zip(&ast.records) {
            let record = parsed.record;
            if record.actual_type != expected_type {
                diagnostics.push(
                    Diagnostic::error(
                        "CFD-DIMENSION-TYPE",
                        "CFD",
                        format!(
                            "dimension record type `{}` must be `{}`",
                            record.actual_type, expected_type
                        ),
                    )
                    .with_primary(file_span_label(
                        path,
                        &text,
                        syntax.type_span.start,
                        syntax.type_span.end,
                        "incompatible dimension record type",
                    )),
                );
                continue;
            }
            if !seen.insert(record.key.clone()) {
                diagnostics.push(diag(
                    "CFD-DIMENSION",
                    format!("duplicate dimension record `{}`", record.key),
                ));
                continue;
            }
            let spans = syntax
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.value.span()))
                .collect::<BTreeMap<_, _>>();
            let source_key = match RecordKey::new(record.key.clone()) {
                Ok(key) => key,
                Err(err) => {
                    diagnostics.push(Diagnostic::error("CFD-DIMENSION", "CFD", err.to_string()));
                    continue;
                }
            };
            for (field, value) in record.fields {
                let Some(variant) = request.schema.dimension.variant(&field) else {
                    continue;
                };
                let span = spans.get(field.as_str()).copied().unwrap_or(syntax.span);
                let range = byte_range(&text, span.start, span.end);
                values.push(DimensionValueDraft {
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
            Ok(DimensionSourceLoadResult {
                values,
                source: text.into(),
            })
        } else {
            Err(diagnostics)
        }
    }

    pub(crate) fn write_dimension_value(
        &self,
        request: &WriteDimensionValueRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let path = request.source.location.path();
        let variants = request
            .schema
            .dimension
            .variants
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let (mut rows, imports) =
            self.read_existing_dimension_cfd(path, &variants, None, request.schema.schema)?;
        let physical_key = request.source_key.as_str();
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
        let out = render_dimension_cfd(&rows, &variants, &imports);
        self.write_if_changed(path, &out, "CFD-DIMENSION-WRITE")
    }

    pub(crate) fn rewrite_dimension_record(
        &self,
        request: &RewriteDimensionRecordRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let path = request.source.location.path();
        let variants = request
            .schema
            .dimension
            .variants
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let (mut rows, imports) =
            self.read_existing_dimension_cfd(path, &variants, None, request.schema.schema)?;
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
        let out = render_dimension_cfd(&rows, &variants, &imports);
        self.write_if_changed(path, &out, "CFD-DIMENSION-WRITE")
    }

    pub(crate) fn sync_dimension_source(
        &self,
        request: &DimensionSourceRequest<'_>,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        let path = request.source.location.path();
        let expected_keys = request
            .entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<BTreeSet<_>>();
        let (existing, imports) = self.read_existing_dimension_cfd(
            path,
            request.variants,
            Some(&expected_keys),
            request.schema,
        )?;
        let mut out = render_imports(&imports);
        for entry in request.entries {
            let row = existing.get(&entry.key);
            // 构建只更新合法辅助记录，避免把错误字段或旧格式静默改写成有效数据。
            if let Some(row) = row {
                if row.actual_type != entry.actual_type {
                    return Err(DiagnosticSet::one(diag(
                        "CFD-DIMENSION-TYPE",
                        format!(
                            "dimension record `{}` in `{}` has type `{}`, expected `{}`",
                            entry.key,
                            path.display(),
                            row.actual_type,
                            entry.actual_type
                        ),
                    )));
                }
            }
            let actual_type = entry.actual_type.as_str();
            let _ = writeln!(out, "{}: {actual_type} {{", entry.key);
            for variant in request.variants {
                if let Some(value) = row.and_then(|row| row.variants.get(variant)) {
                    let _ = writeln!(out, "{CFD_INDENT}{variant}: {},", render_cfd_cell(value));
                } else if row.is_none() {
                    let _ = writeln!(out, "{CFD_INDENT}{variant}: None,");
                }
            }
            out.push_str("}\n\n");
        }
        self.write_if_changed(path, &out, "CFD-DIMENSION")
    }
}

fn render_cfd_cell(value: &str) -> String {
    if value.is_empty() {
        "None".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_core::schema::{
        build_schema, dimension_record_type, parse_modules, CftDimensionInputs, CftFile, ModuleId,
    };

    #[test]
    fn dimension_rewrite_preserves_imported_types_and_values() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "namespace game; enum Tone { Warm } table Item { @localized tone: Tone; }",
        )]);
        let inputs = CftDimensionInputs::try_new([("language", vec!["zh".to_string()])])
            .expect("dimensions");
        let schema = build_schema(&modules, &inputs).expect("schema");
        let dimension_type = dimension_record_type("language", "game::Item", "tone");
        let short_name = dimension_type.rsplit("::").next().expect("type name");
        let source = format!(
            "use game::Tone;\nuse {dimension_type};\nsword: {short_name} {{ zh: Tone::Warm }}\n"
        );
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("language.cfd");
        std::fs::write(&path, source).expect("source");
        let writer = CfdWriter::new();
        let variants = vec!["zh".to_string()];
        let (rows, imports) = writer
            .read_existing_dimension_cfd(&path, &variants, None, &schema)
            .expect("read");
        assert_eq!(rows["sword"].actual_type, dimension_type);
        let rewritten = render_dimension_cfd(&rows, &variants, &imports);
        let (ast, errors) = parse_cfd(&rewritten);
        assert!(errors.is_empty());
        assert_eq!(ast.imports, imports);
        assert!(
            super::super::super::lower::lower_records(&schema, &ast).is_ok(),
            "{rewritten}"
        );
    }
}

fn file_span_label(path: &Path, text: &str, start: usize, end: usize, message: &str) -> Label {
    let range = byte_range(text, start, end);
    Label {
        location: SourceLocation::FileSpan {
            path: path.to_path_buf(),
            start_line: range.start.line,
            start_character: range.start.character,
            end_line: range.end.line,
            end_character: range.end.character,
        },
        message: Some(message.to_string()),
    }
}

#[derive(Debug, Clone, Default)]
struct DimensionCfdRow {
    actual_type: String,
    variants: BTreeMap<String, String>,
}

fn render_imports(imports: &[String]) -> String {
    // 原始单元格源码可能使用导入名，重写记录时必须保留其名称解析环境。
    let mut out = String::new();
    for import in imports {
        let _ = writeln!(out, "use {import};");
    }
    if !imports.is_empty() {
        out.push('\n');
    }
    out
}

fn render_dimension_cfd(
    rows: &BTreeMap<String, DimensionCfdRow>,
    variants: &[String],
    imports: &[String],
) -> String {
    let mut out = render_imports(imports);
    for (key, row) in rows {
        // 每条覆盖记录保留所属生成类型。
        let actual_type = &row.actual_type;
        let _ = writeln!(out, "{key}: {actual_type} {{");
        for variant in variants {
            if let Some(value) = row.variants.get(variant) {
                let _ = writeln!(out, "{CFD_INDENT}{variant}: {},", render_cfd_cell(value));
            }
        }
        out.push_str("}\n\n");
    }
    out
}

impl CfdWriter {
    fn read_existing_dimension_cfd(
        &self,
        path: &Path,
        variants: &[String],
        expected_keys: Option<&BTreeSet<&str>>,
        schema: &coflow_core::schema::CftSchema,
    ) -> Result<(BTreeMap<String, DimensionCfdRow>, Vec<String>), DiagnosticSet> {
        let text = match self.read_source(path) {
            Ok(text) => text,
            Err(_) if !path.exists() => return Ok((BTreeMap::new(), Vec::new())),
            Err(diagnostics) => return Err(diagnostics),
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
                // 同步按当前主体集合重建，已删除主体的维度记录无需带入输出。
                continue;
            }
            if out.contains_key(&record.key) {
                return Err(DiagnosticSet::one(diag(
                "CFD-DIMENSION",
                format!(
                    "dimension source `{}` contains duplicate id `{}`; variant records can only edit existing records",
                    path.display(),
                    record.key
                ),
            )));
            }
            let type_name = ast
                .imports
                .iter()
                .find(|name| name.rsplit("::").next() == Some(record.type_name.as_str()))
                .map_or(record.type_name.as_str(), String::as_str);
            let mut row = DimensionCfdRow {
                actual_type: schema
                    .resolve_record_type_name(type_name)
                    .map_err(|error| DiagnosticSet::one(diag("CFD-DIMENSION-TYPE", error)))?
                    .to_string(),
                ..DimensionCfdRow::default()
            };
            for field in record.fields {
                if variants.iter().any(|variant| variant == &field.name) {
                    row.variants
                        .insert(field.name, raw_span(&text, field.value.span()));
                }
            }
            out.insert(record.key, row);
        }
        Ok((out, ast.imports))
    }

    fn write_if_changed(
        &self,
        path: &Path,
        body: &str,
        code: &'static str,
    ) -> Result<DimensionSourceResult, DiagnosticSet> {
        match self.read_source(path) {
            Ok(existing) if existing == body => {
                return Ok(DimensionSourceResult { changed: false });
            }
            Ok(_) => {}
            Err(_) if !path.exists() => {}
            Err(diagnostics) => return Err(diagnostics),
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                DiagnosticSet::one(diag(
                    code,
                    format!("failed to create `{}`: {err}", parent.display()),
                ))
            })?;
        }
        self.write_source(path, body)?;
        Ok(DimensionSourceResult { changed: true })
    }
}
