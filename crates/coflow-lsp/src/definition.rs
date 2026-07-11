use coflow_cfd::parse_cfd;
use coflow_cft::ast::Item;
use coflow_cft::Span;
use serde_json::{json, Value};

use crate::{
    byte_offset_from_position, byte_range, cfd, current_type_at, dotted_chain_at, field_by_type,
    is_builtin_name, is_trivia_position, range_from_span, word_at, CfdProjectSource, LspBuild,
    LspDocument, LspPosition,
};

/// Find the LSP location (uri + range) of a CFT type definition by name.
pub(crate) fn cft_type_definition_location(build: &LspBuild, type_name: &str) -> Option<Value> {
    use coflow_cft::parser::parse_module;
    use coflow_cft::ModuleId;

    for (module_id, document) in &build.documents {
        let Some(ast) = document
            .ast
            .clone()
            .or_else(|| parse_module(&ModuleId::new(module_id.clone()), &document.source).ok())
        else {
            continue;
        };

        for item in &ast.items {
            use coflow_cft::ast::Item;
            let (name, name_span) = match item {
                Item::Type(t) => (t.name.as_str(), t.name_span),
                Item::Enum(e) => (e.name.as_str(), e.name_span),
                Item::Const(_) => continue,
            };
            if name == type_name {
                let range = byte_range(&document.source, name_span.start, name_span.end);
                return Some(json!({
                    "uri": document.uri,
                    "range": range,
                }));
            }
        }
    }
    None
}

/// Find the LSP location of a CFT field definition by owning type and field name.
pub(crate) fn cft_schema_field_definition_location(
    build: &LspBuild,
    type_name: &str,
    field_name: &str,
) -> Option<Value> {
    field_location(build, type_name, field_name)
}

pub(crate) fn definitions_at(
    build: &LspBuild,
    document: &LspDocument,
    position: &LspPosition,
) -> Vec<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    if is_trivia_position(&document.source, offset) {
        return Vec::new();
    }
    let Some(word) = word_at(&document.source, offset) else {
        return Vec::new();
    };
    if is_builtin_name(&word.text) {
        return Vec::new();
    }

    if let Some(chain) = dotted_chain_at(&document.source, &word) {
        if chain.len() == 2 {
            if let Some(location) = enum_variant_location_by_chain(build, &chain) {
                return vec![location];
            }
            if let Some(location) = ast_enum_variant_location_by_chain(build, &chain) {
                return vec![location];
            }
        }
        if let Some(location) = field_location_by_chain(build, document, offset, &chain) {
            return vec![location];
        }
    }

    if let Some(location) = global_location(build, &word.text) {
        return vec![location];
    }

    if let Some(location) = ast_global_location(build, &word.text) {
        return vec![location];
    }

    if let Some(current_type) = current_type_at(build, document, offset) {
        if let Some(location) = field_location(build, &current_type.name, &word.text) {
            return vec![location];
        }
    }

    Vec::new()
}

pub(crate) fn field_location_by_chain(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Option<Value> {
    let (field_name, receiver) = chain.split_last()?;
    let receiver_type = crate::type_of_chain(build, document, offset, receiver)?;
    let type_name = crate::type_name_of_schema_ref(&receiver_type)?;
    field_location(build, type_name, field_name)
}

pub(crate) fn field_location(build: &LspBuild, type_name: &str, field_name: &str) -> Option<Value> {
    let (owner, field) = field_by_type(build, type_name, field_name)?;
    let document = build.document_by_module(&owner.module)?;
    let span = ast_field_name_span(document, &owner.name, field_name).unwrap_or(field.span);
    Some(location(document, span))
}

fn ast_field_name_span(document: &LspDocument, type_name: &str, field_name: &str) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Type(ty) = item {
            if ty.name == type_name {
                return ty
                    .fields
                    .iter()
                    .find(|field| field.name == field_name)
                    .map(|field| field.name_span);
            }
        }
    }
    None
}

fn enum_variant_location_by_chain(build: &LspBuild, chain: &[String]) -> Option<Value> {
    let (enum_def, variant) = crate::enum_variant_by_chain(build, chain)?;
    let document = build.document_by_module(&enum_def.module)?;
    let span =
        ast_enum_variant_name_span(document, &enum_def.name, &variant.name).unwrap_or(variant.span);
    Some(location(document, span))
}

fn ast_enum_variant_location_by_chain(build: &LspBuild, chain: &[String]) -> Option<Value> {
    if chain.len() != 2 {
        return None;
    }
    ast_enum_variant_location(build, &chain[0], &chain[1])
}

pub(crate) fn ast_enum_variant_location(
    build: &LspBuild,
    enum_name: &str,
    variant_name: &str,
) -> Option<Value> {
    for document in build.documents.values() {
        let Some(span) = ast_enum_variant_name_span(document, enum_name, variant_name) else {
            continue;
        };
        return Some(location(document, span));
    }
    None
}

fn ast_enum_variant_name_span(
    document: &LspDocument,
    enum_name: &str,
    variant_name: &str,
) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    for item in &ast.items {
        if let Item::Enum(enum_def) = item {
            if enum_def.name == enum_name {
                return enum_def
                    .variants
                    .iter()
                    .find(|variant| variant.name == variant_name)
                    .map(|variant| variant.name_span);
            }
        }
    }
    None
}

fn global_location(build: &LspBuild, name: &str) -> Option<Value> {
    let container = build.container()?;
    if let Some(ty) = container.resolve_type(name) {
        let document = build.document_by_module(&ty.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(ty.span),
        ));
    }
    if let Some(enum_def) = container.resolve_enum(name) {
        let document = build.document_by_module(&enum_def.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(enum_def.span),
        ));
    }
    if let Some(constant) = container.resolve_const(name) {
        let document = build.document_by_module(&constant.module)?;
        return Some(location(
            document,
            ast_top_level_name_span(document, name).unwrap_or(constant.span),
        ));
    }
    None
}

fn ast_global_location(build: &LspBuild, name: &str) -> Option<Value> {
    for document in build.documents.values() {
        let Some(ast) = &document.ast else {
            continue;
        };
        for item in &ast.items {
            match item {
                Item::Const(constant) if constant.name == name => {
                    return Some(location(document, constant.name_span));
                }
                Item::Enum(enum_def) if enum_def.name == name => {
                    return Some(location(document, enum_def.name_span));
                }
                Item::Type(ty) if ty.name == name => {
                    return Some(location(document, ty.name_span));
                }
                Item::Const(_) | Item::Enum(_) | Item::Type(_) => {}
            }
        }
    }
    None
}

fn ast_top_level_name_span(document: &LspDocument, name: &str) -> Option<Span> {
    let ast = document.ast.as_ref()?;
    ast.items.iter().find_map(|item| match item {
        Item::Const(constant) if constant.name == name => Some(constant.name_span),
        Item::Enum(enum_def) if enum_def.name == name => Some(enum_def.name_span),
        Item::Type(ty) if ty.name == name => Some(ty.name_span),
        _ => None,
    })
}

fn location(document: &LspDocument, span: Span) -> Value {
    json!({
        "uri": document.uri,
        "range": range_from_span(&document.source, span)
    })
}

/// Find the LSP location (uri + range) of a CFD record definition by key.
///
/// Searches configured CFD source files and open CFD documents for a top-level
/// record whose key matches. Open documents override disk content for the same
/// path so dirty editor buffers can still be targeted.
pub(crate) fn cfd_record_definition_location(
    sources: &[CfdProjectSource],
    key: &str,
) -> Option<Value> {
    for source in sources {
        if let Some(location) =
            cfd_record_definition_location_in_source(&source.uri, &source.text, key)
        {
            return Some(location);
        }
    }
    None
}

fn cfd_record_definition_location_in_source(uri: &str, text: &str, key: &str) -> Option<Value> {
    let (ast, _) = parse_cfd(text);
    for record in &ast.records {
        if record.key == key {
            let range = cfd::byte_range(text, record.key_span.start, record.key_span.end);
            return Some(json!({
                "uri": uri,
                "range": range,
            }));
        }
    }
    None
}
