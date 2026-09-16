//! 内存 CFD 加载，不发现文件，也不依赖项目配置。
mod constants;
mod diagnostics;
pub mod lower;
use crate::schema::CftSchema;
use crate::{CfdDataModel, LoadedRecordDraft, RecordOrigin};
pub use diagnostics::*;
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub struct SourceInput {
    pub path: PathBuf,
    pub text: Arc<str>,
}
impl SourceInput {
    pub fn new(path: impl Into<PathBuf>, text: impl Into<Arc<str>>) -> Self {
        Self {
            path: path.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceAnalysis {
    pub input: SourceInput,
    pub syntax: coflow_language::cfd::CfdAst,
    pub diagnostics: Vec<CfdTextDiagnostic>,
}

pub fn analyze(schema: &CftSchema, input: SourceInput) -> (SourceAnalysis, Vec<LoadedRecordDraft>) {
    let (syntax, errors) = coflow_language::cfd::parse_cfd(&input.text);
    let mut diagnostics = lower::syntax_diagnostics(errors).diagnostics;
    let (records, errors) = lower::lower_records_partial(schema, &syntax);
    diagnostics.extend(errors);
    let records = records
        .into_iter()
        .map(|mut record| {
            for value in record.record.fields.values_mut() {
                locate_callable_paths(value, &input.path.to_string_lossy());
            }
            record.record.with_origin(RecordOrigin::File {
                path: input.path.clone(),
                span: None,
            })
        })
        .collect();
    (
        SourceAnalysis {
            input,
            syntax,
            diagnostics,
        },
        records,
    )
}

pub fn load(
    schema: &CftSchema,
    inputs: impl IntoIterator<Item = SourceInput>,
) -> (Vec<SourceAnalysis>, Result<CfdDataModel, CfdTextLoadError>) {
    let mut analyses = Vec::new();
    let mut errors = Vec::new();
    let mut builder = CfdDataModel::builder(schema);
    let mut origins = Vec::new();
    let mut dimension_records = std::collections::BTreeSet::new();
    for input in inputs {
        let (analysis, records) = analyze(schema, input);
        errors.extend(analysis.diagnostics.iter().cloned());
        for record in records {
            origins.push(record.origin.clone());
            if let Some((dimension, field)) = dimension_source(schema, &record.actual_type) {
                if !dimension_records.insert((record.actual_type.clone(), record.key.clone())) {
                    errors.push(CfdTextDiagnostic::error(
                        CfdTextErrorCode::TypeMismatch,
                        format!(
                            "duplicate dimension record `{}::{}`",
                            record.actual_type, record.key
                        ),
                        CfdTextSpan::default(),
                    ));
                    continue;
                }
                for (variant, value) in record.fields {
                    let Some(variant) = dimension
                        .variants
                        .iter()
                        .find(|name| name.as_str() == variant)
                    else {
                        continue;
                    };
                    let key = match crate::schema::RecordKey::new(record.key.clone()) {
                        Ok(key) => key,
                        Err(error) => {
                            errors.push(CfdTextDiagnostic::error(
                                CfdTextErrorCode::TypeMismatch,
                                error.to_string(),
                                CfdTextSpan::default(),
                            ));
                            continue;
                        }
                    };
                    builder.add_dimension_value_draft(crate::DimensionValueDraft {
                        source_type: field.declaring_type.clone(),
                        source_key: key,
                        field: field.name.clone(),
                        dimension: dimension.name.clone(),
                        variant: variant.clone(),
                        value,
                        origin: record.origin.clone(),
                    });
                }
            } else {
                builder.add_loaded_record(record);
            }
        }
        analyses.push(analysis);
    }
    if !errors.is_empty() {
        return (
            analyses,
            Err(CfdTextLoadError::Text(CfdTextDiagnostics {
                diagnostics: errors,
            })),
        );
    }
    let result = builder
        .build()
        .map_err(|diagnostics| CfdTextLoadError::DataModel {
            diagnostics,
            origins,
        })
        .and_then(|model| {
            for (generated, key) in dimension_records {
                let Some((_, field)) = dimension_source(schema, &generated) else {
                    continue;
                };
                if !model
                    .records_assignable_to(schema, field.declaring_type.as_str())
                    .any(|(_, record)| record.key() == key)
                {
                    return Err(CfdTextLoadError::Text(CfdTextDiagnostics {
                        diagnostics: vec![CfdTextDiagnostic::error(
                            CfdTextErrorCode::TypeMismatch,
                            format!("dimension record `{generated}::{key}` has no business record"),
                            CfdTextSpan::default(),
                        )],
                    }));
                }
            }
            Ok(model)
        });
    (analyses, result)
}

pub fn dimension_source<'a>(
    schema: &'a CftSchema,
    name: &str,
) -> Option<(&'a crate::schema::CftDimension, &'a crate::schema::CftField)> {
    schema.all_dimensions().find_map(|dimension| {
        dimension
            .fields
            .iter()
            .find(|field| {
                crate::schema::dimension_record_type(
                    dimension.name.as_str(),
                    field.declaring_type.as_str(),
                    field.name.as_str(),
                ) == name
            })
            .map(|field| (dimension, field.as_ref()))
    })
}

fn locate_callable_paths(value: &mut crate::LoadedValueDraft, path: &str) {
    use crate::LoadedValueDraft as V;
    match value {
        V::Function(value) => {
            if let Some(location) = &mut value.location {
                location.path = Some(path.into());
            }
        }
        V::FormattedString(value) => {
            if let Some(location) = &mut value.location {
                location.path = Some(path.into());
            }
        }
        V::Array(values) => {
            for value in values {
                locate_callable_paths(value, path);
            }
        }
        V::Dict(values) => {
            for (_, value) in values {
                locate_callable_paths(value, path);
            }
        }
        V::Object { fields, .. } => {
            for value in fields.values_mut() {
                locate_callable_paths(value, path);
            }
        }
        V::OptionSome(value) => locate_callable_paths(value, path),
        _ => {}
    }
}
