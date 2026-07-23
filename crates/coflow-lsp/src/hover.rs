use coflow_cft::syntax::ast::{Annotation, Item};
use coflow_cft::{CftConstValue, CftType};
use serde_json::{json, Value};
use std::fmt::Write as _;

use crate::documentation::{annotation_documentation, static_documentation};
use crate::position::{byte_offset_from_position, byte_range, range_from_span, LspPosition};
use crate::{
    current_type_at, dotted_chain_at, enum_variant_by_chain, field_by_chain, is_trivia_position,
    word_at, LspBuild, LspDocument,
};

pub(crate) fn hover_at(
    build: &LspBuild,
    document: &LspDocument,
    position: &LspPosition,
) -> Option<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    if is_trivia_position(&document.source, offset) {
        return None;
    }
    if let Some(annotation) = annotation_at(document, offset) {
        if let Some((_, documentation)) = annotation_documentation(annotation) {
            return Some(hover_response(
                documentation,
                &range_from_span(&document.source, annotation.span),
            ));
        }
    }

    let word = word_at(&document.source, offset)?;
    if let Some(documentation) = static_documentation(&word.text) {
        return Some(hover_response(
            documentation,
            &byte_range(&document.source, word.start, word.end),
        ));
    }

    if let Some(chain) = dotted_chain_at(&document.source, &word) {
        if chain.len() == 2 {
            if let Some((enum_def, variant)) = enum_variant_by_chain(build, &chain) {
                return Some(hover_response(
                    &format!(
                        "CFT enum variant `{}`.`{}` = `{}`.",
                        enum_def.name, variant.name, variant.value
                    ),
                    &byte_range(&document.source, word.start, word.end),
                ));
            }
        }
        if let Some((type_name, field)) = field_by_chain(build, document, offset, &chain) {
            return Some(hover_response(
                &format!(
                    "CFT field `{}`.`{}`: `{}`.",
                    type_name,
                    field.name,
                    field.value_type.display_label()
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
    }

    if let Some(container) = build.schema() {
        if let Some(ty) = container.resolve_type(&word.text) {
            return Some(hover_response(
                &type_hover_text(ty),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(enum_def) = container.resolve_enum(&word.text) {
            return Some(hover_response(
                &format!(
                    "CFT enum `{}` with {} variant(s).",
                    enum_def.name,
                    enum_def.variants.len()
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(constant) = container.resolve_const(&word.text) {
            return Some(hover_response(
                &format!(
                    "CFT constant `{}` = `{}`.",
                    constant.name,
                    const_value_to_string(&constant.value)
                ),
                &byte_range(&document.source, word.start, word.end),
            ));
        }
        if let Some(current_type) = current_type_at(build, document, offset) {
            if let Some(field) = current_type
                .all_fields()
                .find(|field| field.name.as_str() == word.text)
            {
                return Some(hover_response(
                    &format!(
                        "CFT field `{}`.`{}`: `{}`.",
                        current_type.name,
                        field.name,
                        field.value_type.display_label()
                    ),
                    &byte_range(&document.source, word.start, word.end),
                ));
            }
        }
    }

    None
}

fn type_hover_text(ty: &CftType) -> String {
    let mut flags = Vec::new();
    if ty.is_abstract {
        flags.push("abstract");
    }
    if ty.is_sealed {
        flags.push("sealed");
    }
    let mut text = if flags.is_empty() {
        format!("CFT type `{}`", ty.name)
    } else {
        format!("CFT {} type `{}`", flags.join(" "), ty.name)
    };
    if let Some(parent) = &ty.parent {
        let _ = write!(text, " extends `{parent}`");
    }
    let _ = write!(text, " with {} field(s).", ty.all_fields().count());
    text
}

fn const_value_to_string(value: &CftConstValue) -> String {
    match value {
        CftConstValue::Int(value) => value.to_string(),
        CftConstValue::Float(value) => value.to_string(),
        CftConstValue::Bool(value) => value.to_string(),
        CftConstValue::String(value) => format!("{value:?}"),
    }
}

fn hover_response(contents: &str, range: &Value) -> Value {
    json!({
        "contents": {
            "kind": "markdown",
            "value": contents
        },
        "range": range
    })
}

fn annotation_at(document: &LspDocument, offset: usize) -> Option<&Annotation> {
    fn find_in(annotations: &[Annotation], offset: usize) -> Option<&Annotation> {
        annotations.iter().find(|annotation| {
            annotation.name_span.start <= offset && offset <= annotation.name_span.end
        })
    }

    let ast = document.ast.as_ref()?;
    if let Some(annotation) = find_in(&ast.dangling_annotations, offset) {
        return Some(annotation);
    }
    for item in &ast.items {
        match item {
            Item::Const(constant) => {
                if let Some(annotation) = find_in(&constant.annotations, offset) {
                    return Some(annotation);
                }
            }
            Item::Enum(enum_def) => {
                if let Some(annotation) = find_in(&enum_def.annotations, offset)
                    .or_else(|| find_in(&enum_def.dangling_annotations, offset))
                {
                    return Some(annotation);
                }
                for variant in &enum_def.variants {
                    if let Some(annotation) = find_in(&variant.annotations, offset) {
                        return Some(annotation);
                    }
                }
            }
            Item::Type(ty) => {
                if let Some(annotation) = find_in(&ty.annotations, offset)
                    .or_else(|| find_in(&ty.dangling_annotations, offset))
                {
                    return Some(annotation);
                }
                for field in &ty.fields {
                    if let Some(annotation) = find_in(&field.annotations, offset) {
                        return Some(annotation);
                    }
                }
            }
            Item::Check(check) => {
                if let Some(annotation) = find_in(&check.annotations, offset) {
                    return Some(annotation);
                }
            }
        }
    }
    None
}
