use coflow_cft::syntax::ast::{Item, TypeRef, TypeRefKind};
use coflow_cft::CftConstValue;
use serde_json::{json, Map, Value};

use crate::documentation::{
    builtin_functions, AnnotationCompletion, ANNOTATIONS, KEYWORDS, LITERALS, PRIMITIVE_TYPES,
};
use crate::position::{byte_offset_from_position, LspPosition};
use crate::{
    current_field_at, current_type_at, is_ident_continue, is_trivia_position, last_ident,
    line_prefix_at, parse_dotted_ident_chain, previous_char, quantifier_bindings_at,
    type_name_of_schema_ref, type_of_chain, LspBuild, LspDocument,
};

const COMPLETION_KIND_FUNCTION: u8 = 3;
const COMPLETION_KIND_FIELD: u8 = 5;
const COMPLETION_KIND_VARIABLE: u8 = 6;
const COMPLETION_KIND_CLASS: u8 = 7;
const COMPLETION_KIND_PROPERTY: u8 = 10;
const COMPLETION_KIND_ENUM: u8 = 13;
const COMPLETION_KIND_KEYWORD: u8 = 14;
const COMPLETION_KIND_ENUM_MEMBER: u8 = 20;
const COMPLETION_KIND_CONSTANT: u8 = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompletionScope {
    TopLevel,
    TypeBody,
    CheckBlock,
    EnumBody,
}

pub(crate) fn completion_items(
    build: &LspBuild,
    document: &LspDocument,
    position: &LspPosition,
) -> Vec<Value> {
    let offset = byte_offset_from_position(&document.source, *position);
    let line_prefix = line_prefix_at(&document.source, offset);
    let scope = completion_scope(document, offset);

    if is_trivia_position(&document.source, offset) {
        return Vec::new();
    }

    if is_type_predicate_context(line_prefix) {
        let mut items = named_type_completion_items(build);
        items.push(completion_item(
            "null",
            COMPLETION_KIND_KEYWORD,
            "Null predicate",
            Some("Nullable value."),
        ));
        return items;
    }

    if is_annotation_completion_context(line_prefix) {
        return annotation_completion_items(scope);
    }

    if let Some(chain) = receiver_chain_before_dot(line_prefix) {
        let mut items = dot_completion_items(build, document, offset, &chain);
        if scope == CompletionScope::CheckBlock {
            items.extend(function_completion_items());
        }
        return items;
    }

    if top_level_needs_type_keyword(line_prefix) {
        return top_level_completion_items(line_prefix);
    }

    if is_type_header_parent_context(line_prefix) {
        return named_type_completion_items(build);
    }

    match scope {
        CompletionScope::TopLevel => {
            if is_const_value_context(line_prefix) {
                return const_value_completion_items();
            }
            top_level_completion_items(line_prefix)
        }
        CompletionScope::TypeBody => {
            if is_field_default_context(line_prefix) {
                return field_default_completion_items(build, current_field_at(document, offset));
            }
            if is_value_typeerence_context(line_prefix) {
                return type_completion_items(build);
            }
            type_member_completion_items()
        }
        CompletionScope::CheckBlock => check_expression_completion_items(build, document, offset),
        CompletionScope::EnumBody => Vec::new(),
    }
}

pub(crate) fn top_level_completion_items(line_prefix: &str) -> Vec<Value> {
    let labels: &[&str] = if top_level_needs_type_keyword(line_prefix) {
        &["type"]
    } else {
        &["const", "enum", "type", "abstract", "sealed", "check"]
    };
    keyword_completion_items(labels)
}

fn type_member_completion_items() -> Vec<Value> {
    keyword_completion_items(&["check"])
}

pub(crate) fn check_expression_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Vec<Value> {
    if is_method_completion_context(&document.source, offset) {
        return function_completion_items();
    }

    let mut items = Vec::new();
    items.extend(keyword_completion_items(&["when", "all", "any", "none"]));
    items.extend(literal_completion_items(true));
    items.extend(const_completion_items(build));

    if let Some(current_type) = current_type_at(build, document, offset) {
        items.push(completion_item(
            "id",
            COMPLETION_KIND_FIELD,
            &format!("{} record key", current_type.name),
            None,
        ));
        for field in current_type.all_fields() {
            items.push(completion_item(
                &field.name,
                COMPLETION_KIND_FIELD,
                &format!("{} field", current_type.name),
                None,
            ));
        }
    }

    for binding in quantifier_bindings_at(document, offset) {
        items.push(completion_item(
            &binding,
            COMPLETION_KIND_VARIABLE,
            "CFT quantifier binding",
            None,
        ));
    }

    items
}

fn keyword_completion_items(labels: &[&str]) -> Vec<Value> {
    labels
        .iter()
        .filter_map(|requested| {
            KEYWORDS
                .iter()
                .find(|(label, _)| label == requested)
                .map(|(label, documentation)| {
                    completion_item(
                        label,
                        COMPLETION_KIND_KEYWORD,
                        "CFT keyword",
                        Some(documentation),
                    )
                })
        })
        .collect()
}

fn literal_completion_items(include_null: bool) -> Vec<Value> {
    LITERALS
        .iter()
        .filter(|(label, _)| include_null || *label != "null")
        .map(|(label, documentation)| {
            completion_item(
                label,
                COMPLETION_KIND_KEYWORD,
                "CFT literal",
                Some(documentation),
            )
        })
        .collect()
}

fn function_completion_items() -> Vec<Value> {
    builtin_functions()
        .map(|(label, documentation)| {
            let mut item = completion_item(
                label,
                COMPLETION_KIND_FUNCTION,
                "CFT built-in function",
                Some(documentation),
            );
            insert_object_field(&mut item, "insertText", json!(format!("{label}($1)")));
            insert_object_field(&mut item, "insertTextFormat", json!(2));
            item
        })
        .collect()
}

fn is_method_completion_context(source: &str, offset: usize) -> bool {
    let prefix = &source[..offset.min(source.len())];
    prefix
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_some_and(|ch| ch == '.')
}

fn const_value_completion_items() -> Vec<Value> {
    literal_completion_items(false)
}

fn field_default_completion_items(
    build: &LspBuild,
    field: Option<&coflow_cft::syntax::ast::FieldDef>,
) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(field) = field else {
        items.extend(literal_completion_items(true));
        items.extend(const_completion_items(build));
        return items;
    };

    collect_default_items_for_type(build, &field.ty, &mut items);
    items.extend(const_completion_items_for_type(build, &field.ty));
    items
}

fn collect_default_items_for_type(build: &LspBuild, ty: &TypeRef, items: &mut Vec<Value>) {
    match &ty.kind {
        TypeRefKind::Bool => items.extend(literal_completion_items(false)),
        TypeRefKind::Int | TypeRefKind::Float | TypeRefKind::String => {}
        TypeRefKind::Named(name) => {
            if let Some(enum_def) = build
                .schema()
                .and_then(|container| container.resolve_enum(name))
            {
                items.extend(enum_def.variants.iter().map(|variant| {
                    let label = format!("{}.{}", enum_def.name, variant.name);
                    completion_item(
                        &label,
                        COMPLETION_KIND_ENUM_MEMBER,
                        "CFT enum variant",
                        None,
                    )
                }));
            }
        }
        TypeRefKind::Array(_) => {
            items.push(completion_item(
                "[]",
                COMPLETION_KIND_CONSTANT,
                "Empty array default",
                None,
            ));
        }
        TypeRefKind::Dict(_, _) => {
            items.push(completion_item(
                "{}",
                COMPLETION_KIND_CONSTANT,
                "Empty object default",
                None,
            ));
        }
        TypeRefKind::Nullable(inner) => {
            items.push(completion_item(
                "null",
                COMPLETION_KIND_KEYWORD,
                "CFT literal",
                Some("Nullable value."),
            ));
            collect_default_items_for_type(build, inner, items);
        }
        TypeRefKind::Ref(inner) => collect_default_items_for_type(build, inner, items),
    }
}

pub(crate) fn dot_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
    chain: &[String],
) -> Vec<Value> {
    if chain.len() == 1 {
        if let Some(enum_def) = build.schema().and_then(|container| {
            container
                .resolve_enum(&chain[0])
                .or_else(|| container.resolve_enum(chain[0].as_str()))
        }) {
            return enum_def
                .variants
                .iter()
                .map(|variant| {
                    completion_item(
                        &variant.name,
                        COMPLETION_KIND_ENUM_MEMBER,
                        &format!("{} variant", enum_def.name),
                        None,
                    )
                })
                .collect();
        }
    }

    let Some(receiver_type) = type_of_chain(build, document, offset, chain) else {
        return Vec::new();
    };
    let Some(type_name) = type_name_of_schema_ref(&receiver_type) else {
        return Vec::new();
    };
    let Some(ty) = build
        .schema()
        .and_then(|container| container.resolve_type(type_name))
    else {
        return Vec::new();
    };

    ty.all_fields()
        .map(|field| {
            completion_item(
                &field.name,
                COMPLETION_KIND_FIELD,
                &format!("{type_name} field"),
                None,
            )
        })
        .collect()
}

fn type_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    for (label, documentation) in PRIMITIVE_TYPES {
        items.push(completion_item(
            label,
            COMPLETION_KIND_KEYWORD,
            "Primitive type",
            Some(documentation),
        ));
    }
    if let Some(container) = build.schema() {
        for ty in container.all_types() {
            items.push(completion_item(
                &ty.name,
                COMPLETION_KIND_CLASS,
                "CFT type",
                None,
            ));
        }
        for enum_def in container.all_enums() {
            items.push(completion_item(
                &enum_def.name,
                COMPLETION_KIND_ENUM,
                "CFT enum",
                None,
            ));
        }
    } else {
        for document in build.documents.values() {
            if let Some(ast) = &document.ast {
                for item in &ast.items {
                    match item {
                        Item::Type(ty) => items.push(completion_item(
                            &ty.name,
                            COMPLETION_KIND_CLASS,
                            "CFT type",
                            None,
                        )),
                        Item::Enum(enum_def) => items.push(completion_item(
                            &enum_def.name,
                            COMPLETION_KIND_ENUM,
                            "CFT enum",
                            None,
                        )),
                        Item::Const(_) | Item::Check(_) => {}
                    }
                }
            }
        }
    }
    items
}

fn named_type_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(container) = build.schema() {
        for ty in container.all_types() {
            items.push(completion_item(
                &ty.name,
                COMPLETION_KIND_CLASS,
                "CFT type",
                None,
            ));
        }
    }
    items
}

fn const_completion_items(build: &LspBuild) -> Vec<Value> {
    let mut items = Vec::new();
    if let Some(container) = build.schema() {
        for constant in container.all_consts() {
            items.push(completion_item(
                &constant.name,
                COMPLETION_KIND_CONSTANT,
                "CFT constant",
                None,
            ));
        }
    }
    items
}

fn const_completion_items_for_type(build: &LspBuild, ty: &TypeRef) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(container) = build.schema() else {
        return items;
    };
    for constant in container
        .all_consts()
        .filter(|constant| const_value_assignable_to_type(&constant.value, ty))
    {
        items.push(completion_item(
            &constant.name,
            COMPLETION_KIND_CONSTANT,
            "CFT constant",
            None,
        ));
    }
    items
}

fn const_value_assignable_to_type(value: &CftConstValue, ty: &TypeRef) -> bool {
    match (&ty.kind, value) {
        (TypeRefKind::Int, CftConstValue::Int(_))
        | (TypeRefKind::Float, CftConstValue::Float(_))
        | (TypeRefKind::Bool, CftConstValue::Bool(_))
        | (TypeRefKind::String, CftConstValue::String(_)) => true,
        (TypeRefKind::Nullable(inner), value) => const_value_assignable_to_type(value, inner),
        _ => false,
    }
}

fn completion_item(label: &str, kind: u8, detail: &str, documentation: Option<&str>) -> Value {
    let mut item = Map::new();
    item.insert("label".to_string(), json!(label));
    item.insert("kind".to_string(), json!(kind));
    item.insert("detail".to_string(), json!(detail));
    if let Some(documentation) = documentation {
        item.insert("documentation".to_string(), json!(documentation));
    }
    Value::Object(item)
}

fn annotation_completion_item(annotation: &AnnotationCompletion) -> Value {
    let mut item = completion_item(
        annotation.label,
        COMPLETION_KIND_PROPERTY,
        annotation.detail,
        Some(annotation.documentation),
    );
    insert_object_field(&mut item, "insertText", json!(annotation.insert_text));
    insert_object_field(
        &mut item,
        "sortText",
        json!(format!("0_{}", annotation.label)),
    );
    if annotation.insert_text.contains('$') {
        insert_object_field(&mut item, "insertTextFormat", json!(2));
    }
    item
}

fn insert_object_field(object: &mut Value, key: &str, value: Value) {
    if let Value::Object(fields) = object {
        fields.insert(key.to_string(), value);
    }
}

pub(crate) fn annotation_completion_items(scope: CompletionScope) -> Vec<Value> {
    ANNOTATIONS
        .iter()
        .filter(|annotation| annotation_applies_to_scope(annotation.label, scope))
        .map(annotation_completion_item)
        .collect()
}

fn annotation_applies_to_scope(label: &str, scope: CompletionScope) -> bool {
    matches!(label, "@struct" | "@flag" | "@idAsEnum") && scope == CompletionScope::TopLevel
}

pub(crate) fn completion_scope(document: &LspDocument, offset: usize) -> CompletionScope {
    let Some(ast) = &document.ast else {
        return CompletionScope::TopLevel;
    };

    for item in &ast.items {
        match item {
            Item::Enum(enum_def)
                if enum_def.span.start <= offset && offset <= enum_def.span.end =>
            {
                return CompletionScope::EnumBody;
            }
            Item::Type(ty) if ty.span.start <= offset && offset <= ty.span.end => {
                if check_block_contains(ty.check.as_ref(), offset) {
                    return CompletionScope::CheckBlock;
                }
                return CompletionScope::TypeBody;
            }
            Item::Check(check) if check.span.start <= offset && offset <= check.span.end => {
                return CompletionScope::CheckBlock;
            }
            Item::Const(_) | Item::Enum(_) | Item::Type(_) | Item::Check(_) => {}
        }
    }

    CompletionScope::TopLevel
}

fn check_block_contains(
    check: Option<&coflow_cft::syntax::ast::CheckBlock>,
    offset: usize,
) -> bool {
    check.is_some_and(|check| check.span.start <= offset && offset <= check.span.end)
}

pub(crate) fn is_annotation_completion_context(line_prefix: &str) -> bool {
    let Some(index) = line_prefix.rfind('@') else {
        return false;
    };
    line_prefix[index + 1..].chars().all(is_ident_continue)
}

pub(crate) fn is_type_predicate_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(last_word) = last_ident(trimmed) else {
        return false;
    };
    if last_word == "is" {
        return true;
    }
    trimmed[..trimmed.len() - last_word.len()]
        .trim_end()
        .ends_with("is")
}

pub(crate) fn is_type_header_parent_context(line_prefix: &str) -> bool {
    let Some(colon) = line_prefix.rfind(':') else {
        return false;
    };
    let before_colon = &line_prefix[..colon];
    before_colon.contains("type")
}

pub(crate) fn is_value_typeerence_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    let after_colon = &trimmed[colon + 1..];
    !after_colon.contains(';') && !after_colon.contains('=')
}

pub(crate) fn is_const_value_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    trimmed.contains("const ") && trimmed.contains('=') && !trimmed.contains(';')
}

pub(crate) fn is_field_default_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_end();
    let Some(equal) = trimmed.rfind('=') else {
        return false;
    };
    let Some(colon) = trimmed.rfind(':') else {
        return false;
    };
    colon < equal && !trimmed[equal + 1..].contains(';')
}

pub(crate) fn top_level_needs_type_keyword(line_prefix: &str) -> bool {
    matches!(last_ident(line_prefix), Some("abstract" | "sealed"))
}

pub(crate) fn receiver_chain_before_dot(line_prefix: &str) -> Option<Vec<String>> {
    let dot = line_prefix.rfind('.')?;
    let typed = line_prefix[dot + 1..].trim_start();
    if !typed.chars().all(is_ident_continue) {
        return None;
    }
    let receiver = trailing_dotted_ident_chain(&line_prefix[..dot])?;
    parse_dotted_ident_chain(receiver)
}

fn trailing_dotted_ident_chain(text: &str) -> Option<&str> {
    let trimmed_end = text.trim_end().len();
    let bytes = text.as_bytes();
    let mut start = trimmed_end;
    let mut saw_ident = false;
    let mut allow_dot = false;

    while start > 0 {
        let (previous, ch) = previous_char(text, start)?;
        if is_ident_continue(ch) {
            saw_ident = true;
            allow_dot = true;
            start = previous;
            continue;
        }
        if ch == '.' && allow_dot {
            saw_ident = false;
            allow_dot = false;
            start = previous;
            continue;
        }
        if ch.is_whitespace() && !saw_ident && previous + ch.len_utf8() == start {
            start = previous;
            continue;
        }
        break;
    }

    while start < trimmed_end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    (saw_ident && start < trimmed_end).then_some(&text[start..trimmed_end])
}
