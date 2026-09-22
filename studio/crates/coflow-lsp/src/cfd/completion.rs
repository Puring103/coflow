//! CFD completion 能力。
use super::{
    byte_range, completion_context, fmt_value_type, function_at, function_scoped_completions,
    incomplete_group_key_at, record_context_at, recovered_field_names, span_contains, CfdAst,
    CfdBitExpr, CfdBitExprKind, CftSchema, CftValueType, CompletionContext, LanguageCompletion,
    LanguageTextEdit, LspBuild, Span,
};

#[cfg(test)]
pub fn completion(
    source: &str,
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    offset: usize,
) -> Vec<LanguageCompletion> {
    completion_with_build(source, ast, schema, None, offset)
}

pub(crate) fn completion_with_build(
    source: &str,
    ast: &CfdAst,
    schema: Option<&CftSchema>,
    build: Option<&LspBuild>,
    offset: usize,
) -> Vec<LanguageCompletion> {
    if let Some(function) = function_at(ast, offset) {
        let body_start = function.body_span.start.saturating_sub(function.span.start);
        let signature_end = body_start.saturating_sub(1);
        let signature = function
            .source
            .get(..signature_end)
            .unwrap_or(&function.source);
        return function_scoped_completions(
            signature,
            function
                .source
                .get(..offset.saturating_sub(function.span.start))
                .unwrap_or(&function.source),
            schema,
        );
    }
    let Some(schema) = schema else {
        return Vec::new();
    };
    if let Some(items) = formatted_string_completion(source, ast, schema, build, offset) {
        return items;
    }
    if let Some((key, _, group_type)) = incomplete_group_key_at(source, schema, offset) {
        let fields = required_field_snippets(schema, &group_type, 1);
        let body = if fields.is_empty() {
            String::new()
        } else {
            format!("\n  {}\n", fields.join("\n  "))
        };
        return vec![LanguageCompletion {
            label: key.to_owned(),
            kind: Some(7),
            detail: Some(format!("new {group_type} record")),
            insert_text: Some(format!("{key} {{{body}}}")),
            insert_text_format: Some(2),
            ..Default::default()
        }];
    }

    if let Some(context) = completion_context(source, ast, schema, offset) {
        return match context {
            CompletionContext::Value(value_type) => {
                let mut items = selected_flag_values(source, offset, schema, value_type)
                    .map_or_else(
                        || value_completion_items(schema, value_type, build),
                        |selected| flag_completion_items(schema, value_type, &selected),
                    );
                attach_value_text_edits(source, offset, &mut items);
                items
            }
            CompletionContext::Flags {
                value_type,
                selected,
            } => {
                let mut items = flag_completion_items(schema, value_type, &selected);
                attach_value_text_edits(source, offset, &mut items);
                items
            }
            CompletionContext::Fields {
                type_name,
                existing,
            } => field_completion_items(source, schema, type_name, &existing, offset),
            CompletionContext::DimensionFields {
                value_type,
                has_default,
            } => {
                if has_default {
                    Vec::new()
                } else {
                    vec![LanguageCompletion {
                        label: ("default").to_string(),
                        kind: Some(5),
                        detail: Some(
                            (format!("default: {}", fmt_value_type(value_type))).to_string(),
                        ),
                        insert_text: Some(("default: $1").to_string()),
                        insert_text_format: Some(2),
                        ..Default::default()
                    }]
                }
            }
        };
    }

    for record in &ast.records {
        if !span_contains(record.span, offset) {
            continue;
        }
        let Some(schema_type) = schema.resolve_type(&record.type_name) else {
            continue;
        };
        let existing: std::collections::BTreeSet<&str> =
            record.fields().map(|f| f.name.as_str()).collect();
        let items = field_completion_items(source, schema, &schema_type.name, &existing, offset);
        return items;
    }

    if let Some((type_name, body_start)) = record_context_at(source, schema, offset) {
        let Some(schema_type) = schema.resolve_type(&type_name) else {
            return Vec::new();
        };
        let existing = recovered_field_names(source, body_start, offset);
        let items = field_completion_items(source, schema, &schema_type.name, &existing, offset);
        return items;
    }

    // Top-level: suggest known non-abstract type names.
    let types: Vec<LanguageCompletion> = schema
        .all_types()
        .filter(|t| !t.is_abstract)
        .map(|t| {
            let required = required_field_snippets(schema, &t.name, 2);
            let body = if required.is_empty() {
                String::new()
            } else {
                format!("\n  {}\n", required.join("\n  "))
            };
            LanguageCompletion {
                label: (t.name.as_str()).to_string(),
                kind: Some(7),
                detail: Some((format!("new {} record", t.name)).to_string()),
                insert_text: Some((format!("${{1:key}}: {} {{{body}}}", t.name)).to_string()),
                insert_text_format: Some(2),
                ..Default::default()
            }
        })
        .collect();
    types
}

// 格式化字符串补全同时处理记录引用、字段链与局部绑定，需共享同一深度限制和去重集合。
#[allow(clippy::too_many_lines)]
pub(super) fn formatted_string_completion(
    source: &str,
    ast: &CfdAst,
    schema: &CftSchema,
    build: Option<&LspBuild>,
    offset: usize,
) -> Option<Vec<LanguageCompletion>> {
    const FORMATTED_PATH_DEPTH: usize = 8;
    let type_name = ast
        .records
        .iter()
        .find(|record| span_contains(record.span, offset))
        .map(|record| record.type_name.clone())
        .or_else(|| record_context_at(source, schema, offset).map(|(name, _)| name))?;
    let schema_type = schema.resolve_type(&type_name)?;
    let (_, prefix) = formatted_reference_prefix_at(source, offset)?;
    let parts = prefix.split('.').collect::<Vec<_>>();
    let mut paths = Vec::new();

    if let Some(reference) = prefix.strip_prefix('&') {
        let (record, field_path) = reference
            .split_once('.')
            .map_or((reference, None), |(record, path)| (record, Some(path)));
        let (explicit_type, key_prefix) = record
            .rsplit_once("::")
            .map_or((None, record), |(type_name, key)| (Some(type_name), key));
        let owner_type = explicit_type.unwrap_or(schema_type.name.as_str());
        if let Some(field_path) = field_path {
            let field_parts = field_path.split('.').collect::<Vec<_>>();
            let completed = &field_parts[..field_parts.len().saturating_sub(1)];
            let mut nested_owner = owner_type;
            for field_name in completed {
                let field = schema.resolve_type(nested_owner)?.field(field_name)?;
                nested_owner = formatted_nested_type(&field.value_type)?;
            }
            let base = if completed.is_empty() {
                format!("&{record}")
            } else {
                format!("&{record}.{}", completed.join("."))
            };
            collect_formatted_field_paths(
                schema,
                nested_owner,
                &base,
                FORMATTED_PATH_DEPTH,
                &mut paths,
            );
        } else {
            paths.extend(
                formatted_record_keys(ast, schema, build, owner_type)
                    .into_iter()
                    .filter(|key| key.starts_with(key_prefix))
                    .map(|key| {
                        let label = explicit_type.map_or_else(
                            || format!("&{key}"),
                            |type_name| format!("&{type_name}::{key}"),
                        );
                        (label, format!("{owner_type} record"))
                    }),
            );
        }
    } else if parts.len() >= 2 && schema.resolve_type(parts[0]).is_some() {
        let type_name = parts[0];
        if parts.len() == 2 {
            if let Some(build) = build {
                paths.extend(
                    build
                        .cfd_definitions
                        .keys(schema, type_name)
                        .into_iter()
                        .map(|key| (format!("{type_name}.{key}"), format!("{type_name} record"))),
                );
            }
        } else {
            let path_prefix = parts[..parts.len() - 1].join(".");
            collect_formatted_field_paths(
                schema,
                type_name,
                &path_prefix,
                FORMATTED_PATH_DEPTH,
                &mut paths,
            );
        }
    } else if let Some((owner, path_prefix)) =
        formatted_path_owner(schema, schema_type.name.as_str(), &parts)
    {
        collect_formatted_field_paths(schema, owner, &path_prefix, 2, &mut paths);
    } else {
        collect_formatted_field_paths(
            schema,
            schema_type.name.as_str(),
            "",
            FORMATTED_PATH_DEPTH,
            &mut paths,
        );
        paths.extend(
            schema
                .all_types()
                .map(|ty| (ty.name.to_string(), "record type".to_string())),
        );
    }

    let range = formatted_reference_range(source, offset);
    Some(
        paths
            .into_iter()
            .map(|(label, detail)| LanguageCompletion {
                label: (label).to_string(),
                kind: Some(5),
                detail: Some((detail).to_string()),
                text_edit: Some(LanguageTextEdit {
                    range: (byte_range(source, range.start, range.end)).clone(),
                    new_text: (label).to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            })
            .collect(),
    )
}

pub(super) fn formatted_reference_prefix_at(source: &str, offset: usize) -> Option<(usize, &str)> {
    let end = offset.min(source.len());
    let mut in_string = false;
    let mut is_template = false;
    let mut escaped = false;
    let mut reference_start = None;
    for (index, character) in source[..end].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            if !in_string {
                is_template = index > 0 && source.as_bytes()[index - 1] == b'f';
            }
            in_string = !in_string;
            reference_start = None;
            continue;
        }
        if !in_string || !is_template {
            continue;
        }
        match character {
            '{' => reference_start = Some(index),
            '}' => reference_start = None,
            _ => {}
        }
    }
    let start = reference_start?;
    Some((start, source.get(start + 1..end)?.trim()))
}

pub(super) fn collect_formatted_field_paths(
    schema: &CftSchema,
    type_name: &str,
    prefix: &str,
    depth: usize,
    paths: &mut Vec<(String, String)>,
) {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return;
    };
    for field in schema_type.all_fields() {
        let label = if prefix.is_empty() {
            field.name.to_string()
        } else {
            format!("{prefix}.{}", field.name)
        };
        paths.push((
            label.clone(),
            format!("formatted field: {}", fmt_value_type(&field.value_type)),
        ));
        if depth > 1 {
            if let Some(nested) = formatted_nested_type(&field.value_type) {
                collect_formatted_field_paths(schema, nested, &label, depth - 1, paths);
            }
        }
    }
}

pub(super) fn formatted_path_owner<'a>(
    schema: &'a CftSchema,
    root_type: &'a str,
    parts: &[&str],
) -> Option<(&'a str, String)> {
    if parts.len() < 2 {
        return None;
    }
    let mut owner = root_type;
    for part in &parts[..parts.len() - 1] {
        let field = schema.resolve_type(owner)?.field(part)?;
        owner = formatted_nested_type(&field.value_type)?;
    }
    Some((owner, parts[..parts.len() - 1].join(".")))
}

pub(super) fn formatted_nested_type(value_type: &CftValueType) -> Option<&str> {
    match value_type {
        CftValueType::Object(name) | CftValueType::RecordRef(name) => Some(name),
        _ => None,
    }
}

pub(super) fn formatted_record_keys(
    ast: &CfdAst,
    schema: &CftSchema,
    build: Option<&LspBuild>,
    expected_type: &str,
) -> Vec<String> {
    let mut keys = build
        .map(|build| build.cfd_definitions.keys(schema, expected_type))
        .unwrap_or_default();
    let assignable = schema
        .concrete_assignable_types(expected_type)
        .unwrap_or_default();
    keys.extend(
        ast.records
            .iter()
            .filter(|record| {
                assignable
                    .iter()
                    .any(|actual| actual.as_str() == record.type_name)
            })
            .map(|record| record.key.clone()),
    );
    keys.sort();
    keys.dedup();
    keys
}

pub(super) fn formatted_reference_range(source: &str, offset: usize) -> Span {
    let end = offset.min(source.len());
    let start = source[..end]
        .char_indices()
        .rev()
        .take_while(|(_, character)| {
            *character == '_'
                || *character == '.'
                || *character == '&'
                || *character == ':'
                || character.is_alphanumeric()
        })
        .map(|(index, _)| index)
        .last()
        .unwrap_or(end);
    Span::new(start, end)
}

pub(super) fn value_completion_items(
    schema: &CftSchema,
    value_type: &CftValueType,
    build: Option<&LspBuild>,
) -> Vec<LanguageCompletion> {
    match value_type {
        CftValueType::Bool => ["true", "false"]
            .into_iter()
            .map(|label| LanguageCompletion {
                label: (label).to_string(),
                kind: Some(14),
                detail: Some(("bool").to_string()),
                ..Default::default()
            })
            .collect(),
        CftValueType::Enum(name) => {
            schema
                .resolve_enum(name.as_str())
                .map_or_else(Vec::new, |item| {
                    item.variants
                        .iter()
                        .map(|variant| LanguageCompletion {
                            label: (variant.name.as_str()).to_string(),
                            kind: Some(20),
                            detail: Some((format!("{} enum variant", item.name)).to_string()),
                            insert_text: Some((variant.name.as_str()).to_string()),
                            ..Default::default()
                        })
                        .collect()
                })
        }
        CftValueType::Option(inner) => {
            let mut items = vec![LanguageCompletion {
                label: ("None").to_string(),
                kind: Some(14),
                detail: Some((value_type.display_label()).to_string()),
                ..Default::default()
            }];
            items.extend(value_completion_items(schema, inner, build));
            items
        }
        CftValueType::Function(parameters, result) => {
            let parameters = parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    format!(
                        "{}: {}",
                        parameter
                            .name
                            .as_deref()
                            .map_or_else(|| format!("arg{index}"), str::to_string),
                        parameter.value_type.display_label(),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            vec![LanguageCompletion {
                label: ("fn").to_string(),
                kind: Some(3),
                detail: Some((value_type.display_label()).to_string()),
                insert_text: Some(
                    (format!(
                        "fn({parameters}) -> {} {{\n    ${{1}}\n}}",
                        result.display_label()
                    ))
                    .to_string(),
                ),
                insert_text_format: Some(2),
                ..Default::default()
            }]
        }
        CftValueType::Array(_) => {
            vec![LanguageCompletion {
                label: ("[]").to_string(),
                kind: Some(21),
                detail: Some((value_type.display_label()).to_string()),
                ..Default::default()
            }]
        }
        CftValueType::Dict(_, _) => {
            vec![LanguageCompletion {
                label: ("{}").to_string(),
                kind: Some(21),
                detail: Some((value_type.display_label()).to_string()),
                ..Default::default()
            }]
        }
        CftValueType::Object(name) => {
            let mut items = object_value_completion_items(schema, name);
            items.extend(record_reference_completion_items(
                schema, name, build, false,
            ));
            items
        }
        CftValueType::Unit => {
            vec![LanguageCompletion {
                label: ("()").to_string(),
                kind: Some(21),
                detail: Some(("unit").to_string()),
                ..Default::default()
            }]
        }
        CftValueType::Int | CftValueType::Float | CftValueType::String | CftValueType::FString => {
            Vec::new()
        }
        CftValueType::RecordRef(name) => {
            record_reference_completion_items(schema, name, build, true)
        }
    }
}

pub(super) fn record_reference_completion_items(
    schema: &CftSchema,
    expected_name: &str,
    build: Option<&LspBuild>,
    include_fallback: bool,
) -> Vec<LanguageCompletion> {
    let keys = build
        .map(|build| build.cfd_definitions.keys(schema, expected_name))
        .unwrap_or_default();
    if keys.is_empty() && include_fallback {
        return vec![LanguageCompletion {
            label: ("record reference").to_string(),
            kind: Some(18),
            detail: Some((format!("reference to {expected_name}")).to_string()),
            insert_text: Some(("&${1:key}").to_string()),
            insert_text_format: Some(2),
            ..Default::default()
        }];
    }
    keys.into_iter()
        .map(|key| LanguageCompletion {
            label: (key).to_string(),
            kind: Some(18),
            detail: Some((format!("{expected_name} record")).to_string()),
            insert_text: Some((format!("&{key}")).to_string()),
            ..Default::default()
        })
        .collect()
}

pub(super) fn flag_completion_items(
    schema: &CftSchema,
    value_type: &CftValueType,
    selected: &std::collections::BTreeSet<String>,
) -> Vec<LanguageCompletion> {
    let CftValueType::Enum(name) = value_type else {
        return Vec::new();
    };
    schema
        .resolve_enum(name.as_str())
        .map_or_else(Vec::new, |item| {
            item.variants
                .iter()
                .filter(|variant| !selected.contains(variant.name.as_str()))
                .map(|variant| LanguageCompletion {
                    label: (variant.name.as_str()).to_string(),
                    kind: Some(20),
                    detail: Some((format!("{} flag", item.name)).to_string()),
                    ..Default::default()
                })
                .collect()
        })
}

pub(super) fn collect_bit_expr_values(
    expression: &CfdBitExpr,
    values: &mut std::collections::BTreeSet<String>,
) {
    match &expression.kind {
        CfdBitExprKind::Value(value) => {
            values.insert(value.clone());
        }
        CfdBitExprKind::Binary { lhs, rhs, .. } => {
            collect_bit_expr_values(lhs, values);
            collect_bit_expr_values(rhs, values);
        }
    }
}

pub(super) fn selected_flag_values(
    source: &str,
    offset: usize,
    schema: &CftSchema,
    value_type: &CftValueType,
) -> Option<std::collections::BTreeSet<String>> {
    let CftValueType::Enum(name) = value_type else {
        return None;
    };
    let enum_def = schema.resolve_enum(name.as_str())?;
    if !enum_def.is_flag {
        return None;
    }
    let line = source
        .get(..offset.min(source.len()))?
        .rsplit('\n')
        .next()?;
    let value = line.rsplit_once(':').map_or(line, |(_, value)| value);
    if !value.contains(['|', '^', '&']) {
        return None;
    }
    Some(
        value
            .split(|character: char| !(character == '_' || character.is_alphanumeric()))
            .filter(|token| {
                enum_def
                    .variants
                    .iter()
                    .any(|variant| variant.name.as_str() == *token)
            })
            .map(str::to_string)
            .collect(),
    )
}

pub(super) fn object_value_completion_items(
    schema: &CftSchema,
    expected_name: &str,
) -> Vec<LanguageCompletion> {
    schema
        .concrete_assignable_types(expected_name)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|actual_name| {
            let actual = schema.resolve_type(&actual_name)?;
            let fields = required_field_snippets(schema, &actual.name, 1);
            let body = if fields.is_empty() {
                String::new()
            } else {
                format!("\n  {}\n", fields.join("\n  "))
            };
            let marker = if actual.name.as_str() != expected_name
                || schema.range_is_polymorphic(expected_name)
            {
                format!("{} ", actual.name)
            } else {
                String::new()
            };
            Some(LanguageCompletion {
                label: (actual.name.as_str()).to_string(),
                kind: Some(7),
                detail: Some((format!("{} object", actual.name)).to_string()),
                insert_text: Some((format!("{marker}{{{body}}}")).to_string()),
                insert_text_format: Some(2),
                ..Default::default()
            })
        })
        .collect()
}

pub(super) fn field_completion_items(
    source: &str,
    schema: &CftSchema,
    type_name: &str,
    existing: &std::collections::BTreeSet<&str>,
    offset: usize,
) -> Vec<LanguageCompletion> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Vec::new();
    };
    let range = identifier_range_before_cursor(source, offset);
    schema_type
        .all_fields()
        .filter(|field| !existing.contains(field.name.as_str()))
        .map(|field| {
            let required = field.default.is_none();
            let new_text = format!(
                "{}: ${{1:{}}}",
                field.name,
                value_placeholder(&field.value_type)
            );
            LanguageCompletion {
                label: (field.name.as_str()).to_string(),
                kind: Some(5),
                detail: Some((fmt_value_type(&field.value_type)).to_string()),
                documentation: Some(
                    (if required {
                        "Required field"
                    } else {
                        "Field with a schema default"
                    })
                    .to_string(),
                ),
                sort_text: Some(
                    (format!("{}{}", if required { "0" } else { "1" }, field.name)).to_string(),
                ),
                insert_text: Some((new_text).to_string()),
                insert_text_format: Some(2),
                text_edit: Some(LanguageTextEdit {
                    range: (byte_range(source, range.start, range.end)).clone(),
                    new_text: (new_text).to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            }
        })
        .collect()
}

pub(super) fn required_field_snippets(
    schema: &CftSchema,
    type_name: &str,
    first_tabstop: usize,
) -> Vec<String> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Vec::new();
    };
    schema_type
        .all_fields()
        .filter(|field| field.default.is_none())
        .enumerate()
        .map(|(index, field)| {
            format!(
                "{}: ${{{}:{}}},",
                field.name,
                first_tabstop + index,
                value_placeholder(&field.value_type)
            )
        })
        .collect()
}

pub(super) fn value_placeholder(value_type: &CftValueType) -> String {
    match value_type {
        CftValueType::Int => "0".to_string(),
        CftValueType::Float => "0.0".to_string(),
        CftValueType::Bool => "true".to_string(),
        CftValueType::String => "\"value\"".to_string(),
        CftValueType::FString => "f\"value\"".to_string(),
        CftValueType::Enum(name) => name.to_string(),
        CftValueType::RecordRef(_) => "&key".to_string(),
        CftValueType::Array(_) => "[]".to_string(),
        CftValueType::Dict(_, _) | CftValueType::Object(_) => "{}".to_string(),
        CftValueType::Option(_) => "None".to_string(),
        CftValueType::Function(_, _) => "fn() {}".to_string(),
        CftValueType::Unit => "()".to_string(),
    }
}

pub(super) fn identifier_range_before_cursor(source: &str, offset: usize) -> Span {
    let end = offset.min(source.len());
    let start = source[..end]
        .char_indices()
        .rev()
        .take_while(|(_, character)| *character == '_' || character.is_alphanumeric())
        .map(|(index, _)| index)
        .last()
        .unwrap_or(end);
    Span::new(start, end)
}

pub(super) fn attach_value_text_edits(
    source: &str,
    offset: usize,
    items: &mut [LanguageCompletion],
) {
    let mut range = identifier_range_before_cursor(source, offset);
    if range.start > 0 && source.as_bytes().get(range.start - 1) == Some(&b'&') {
        range.start -= 1;
    }
    for item in items {
        item.text_edit = Some(LanguageTextEdit {
            range: byte_range(source, range.start, range.end),
            new_text: item.insert_text.as_ref().unwrap_or(&item.label).clone(),
        });
    }
}
