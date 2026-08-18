//! Project-wide record search over one immutable runtime generation.

use coflow_data_model::{format_cfd_dict_key, CfdValue, RecordCoordinate};

use crate::{value_summary, ProjectQueries};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordSearchMode {
    Key,
    FullText,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSearchOptions {
    pub pattern: String,
    pub mode: RecordSearchMode,
    pub file: Option<String>,
    pub actual_type: Option<String>,
    pub limit: Option<usize>,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSearchHit {
    pub file_path: String,
    pub coordinate: RecordCoordinate,
    pub field_path: Option<String>,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSearchResults {
    pub hits: Vec<RecordSearchHit>,
    pub truncated: bool,
}

impl ProjectQueries<'_> {
    /// Search source-backed records in the current immutable generation.
    #[must_use]
    pub fn search_records(self, options: &RecordSearchOptions) -> RecordSearchResults {
        let normalized = options.pattern.trim().to_lowercase();
        if normalized.is_empty() {
            return RecordSearchResults {
                hits: Vec::new(),
                truncated: false,
            };
        }

        let mut hits = Vec::new();
        let mut skipped = 0_usize;
        for file in self.source_files() {
            if options.file.as_ref().is_some_and(|filter| filter != file) {
                continue;
            }
            for view in self.record_views_in_file(file) {
                if options
                    .actual_type
                    .as_ref()
                    .is_some_and(|actual_type| view.coordinate.actual_type.as_str() != actual_type)
                {
                    continue;
                }
                let key_matches = contains_normalized(&view.coordinate.key, &normalized);
                let field_match = if options.mode == RecordSearchMode::FullText && !key_matches {
                    find_record_field_match(view.record.fields(), &normalized)
                } else {
                    None
                };
                if !key_matches && field_match.is_none() {
                    continue;
                }
                if skipped < options.offset {
                    skipped += 1;
                    continue;
                }
                if options.limit.is_some_and(|limit| hits.len() == limit) {
                    return RecordSearchResults {
                        hits,
                        truncated: true,
                    };
                }
                let (field_path, preview) = field_match.map_or((None, None), |matched| {
                    (Some(matched.path), Some(matched.preview))
                });
                hits.push(RecordSearchHit {
                    file_path: view.display_path.to_string(),
                    coordinate: view.coordinate,
                    field_path,
                    preview,
                });
            }
        }
        RecordSearchResults {
            hits,
            truncated: false,
        }
    }
}

struct FieldMatch {
    path: String,
    preview: String,
}

fn find_record_field_match(
    fields: &std::collections::BTreeMap<coflow_cft::FieldName, CfdValue>,
    query: &str,
) -> Option<FieldMatch> {
    fields.iter().find_map(|(name, value)| {
        let path = name.to_string();
        if contains_normalized(name.as_str(), query) {
            return Some(FieldMatch {
                preview: format!("{path}: {}", value_summary(value)),
                path,
            });
        }
        find_value_match(value, query, &path)
    })
}

fn find_value_match(value: &CfdValue, query: &str, path: &str) -> Option<FieldMatch> {
    if scalar_text(value).is_some_and(|text| contains_normalized(&text, query)) {
        return Some(FieldMatch {
            path: path.to_string(),
            preview: format!("{path}: {}", value_summary(value)),
        });
    }
    match value {
        CfdValue::Object(object) => {
            if contains_normalized(object.actual_type(), query) {
                return Some(FieldMatch {
                    path: path.to_string(),
                    preview: format!("{path}: {}", object.actual_type()),
                });
            }
            object.fields().iter().find_map(|(name, child)| {
                let child_path = format!("{path}.{}", name.as_str());
                if contains_normalized(name.as_str(), query) {
                    Some(FieldMatch {
                        preview: format!("{child_path}: {}", value_summary(child)),
                        path: child_path,
                    })
                } else {
                    find_value_match(child, query, &child_path)
                }
            })
        }
        CfdValue::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, child)| find_value_match(child, query, &format!("{path}[{index}]"))),
        CfdValue::Dict(entries) => entries.iter().find_map(|(key, child)| {
            let key_text = format_cfd_dict_key(key);
            let child_path = format!("{path}[{key_text}]");
            if contains_normalized(&key_text, query) {
                Some(FieldMatch {
                    preview: format!("{child_path}: {}", value_summary(child)),
                    path: child_path,
                })
            } else {
                find_value_match(child, query, &child_path)
            }
        }),
        _ => None,
    }
}

fn scalar_text(value: &CfdValue) -> Option<String> {
    match value {
        CfdValue::Null => Some("null".to_string()),
        CfdValue::Bool(value) => Some(value.to_string()),
        CfdValue::Int(value) => Some(value.to_string()),
        CfdValue::Float(value) => Some(value.to_string()),
        CfdValue::String(value) => Some(value.clone()),
        CfdValue::Enum(value) => Some(format!(
            "{} {} {}",
            value.enum_name,
            value.variant.as_ref().map_or("", AsRef::as_ref),
            value.value
        )),
        CfdValue::Ref(value) => Some(value.to_string()),
        CfdValue::Object(_) | CfdValue::Array(_) | CfdValue::Dict(_) => None,
    }
}

fn contains_normalized(value: &str, normalized_query: &str) -> bool {
    value.to_lowercase().contains(normalized_query)
}
