use coflow_language::syntax::ast::{Item, TypeRef, TypeRefKind};
use coflow_language::syntax::lexer::{lex, TokenKind};
use coflow_language::{CftCheckBuiltin, CftConstValue, ModuleId};
use serde_json::{json, Map, Value};

use super::documentation::{
    AnnotationCompletion, ANNOTATIONS, KEYWORDS, LITERALS, PRIMITIVE_TYPES,
};
use super::position::{byte_offset_from_position, LspPosition};
use super::{
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

    if let Some(items) = annotation_argument_completion_items(build, line_prefix) {
        return items;
    }

    if let Some(items) = module_path_completion_items(build, line_prefix) {
        return items;
    }

    if is_type_predicate_context(line_prefix) {
        return named_type_completion_items(build);
    }

    if is_annotation_completion_context(line_prefix) {
        return annotation_completion_items(scope);
    }

    if let Some(enum_name) = enum_name_before_double_colon(line_prefix) {
        return enum_variant_completion_items(build, document, enum_name);
    }

    if let Some(chain) = receiver_chain_before_dot(line_prefix) {
        let mut items = dot_completion_items(build, document, offset, &chain);
        if scope == CompletionScope::CheckBlock {
            items.extend(type_of_chain(build, document, offset, &chain).map_or_else(
                function_completion_items,
                |receiver| function_completion_items_for_type(&receiver),
            ));
        }
        return items;
    }

    if top_level_needs_type_keyword(line_prefix) {
        return top_level_completion_items(line_prefix);
    }

    if is_type_header_parent_context(line_prefix) {
        return inheritable_type_completion_items(build, line_prefix);
    }


    if is_const_type_context(line_prefix) || is_type_alias_target_context(line_prefix) {
        return type_completion_items(build);
    }

    match scope {
        CompletionScope::TopLevel => {
            if is_const_value_context(line_prefix) {
                return const_value_completion_items_for_context(build, document, offset);
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
        CompletionScope::EnumBody => enum_member_completion_items(),
    }
}

pub(crate) fn top_level_completion_items(line_prefix: &str) -> Vec<Value> {
    if top_level_needs_type_keyword(line_prefix) {
        return keyword_snippet_completion_item(
            "type",
            "type ${1:Name} {\n\t${2:field}: ${3:string};\n}",
        )
        .into_iter()
        .collect();
    }

    [
        ("namespace", "namespace ${1:project}::${2:module};"),
        ("use", "use ${1:project}::${2:module}::${3:Type};"),
        ("const", "const ${1:NAME}: ${2:int} = ${3:value};"),
        ("enum", "enum ${1:Name} {\n\t${2:Variant},\n}"),
        ("type", "type ${1:Name} {\n\t${2:field}: ${3:string};\n}"),
        (
            "abstract",
            "abstract type ${1:Name} {\n\t${2:field}: ${3:string};\n}",
        ),
        (
            "sealed",
            "sealed type ${1:Name} {\n\t${2:field}: ${3:string};\n}",
        ),
        ("check", "check ${1:Name} {\n\t${2:condition};\n}"),
    ]
    .into_iter()
    .filter_map(|(label, insert_text)| keyword_snippet_completion_item(label, insert_text))
    .collect()
}

fn type_member_completion_items() -> Vec<Value> {
    let mut items = vec![snippet_completion_item(
        "field",
        "${1:field}: ${2:string};",
        "CFT field",
        "Define a typed field.",
    )];
    if let Some(check) =
        keyword_snippet_completion_item("check", "check {\n\t${1:condition};\n}")
    {
        items.push(check);
    }
    items
}

fn enum_member_completion_items() -> Vec<Value> {
    vec![snippet_completion_item(
        "variant",
        "${1:Variant},",
        "CFT enum variant",
        "Define an enum variant.",
    )]
}

pub(crate) fn check_expression_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Vec<Value> {
    if is_records_type_context(&document.source, offset) {
        return named_type_completion_items(build);
    }
    if is_method_completion_context(&document.source, offset) {
        return function_completion_items();
    }

    let mut items = Vec::new();
    items.extend(check_structure_completion_items());
    items.extend(literal_completion_items());
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
    } else {
        items.push(completion_item(
            "records",
            COMPLETION_KIND_FUNCTION,
            "Top-level record set query",
            Some("Return all top-level records assignable to a static object type."),
        ));
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

fn is_records_type_context(source: &str, offset: usize) -> bool {
    let Some(prefix) = source.get(..offset.min(source.len())) else {
        return false;
    };
    let prefix = prefix.trim_end_matches(char::is_whitespace);
    let prefix = prefix.trim_end_matches(is_ident_continue);
    let prefix = prefix.trim_end_matches(char::is_whitespace);
    let Some(prefix) = prefix.strip_suffix('(') else {
        return false;
    };
    last_ident(prefix).is_some_and(|ident| ident == "records")
}

fn literal_completion_items() -> Vec<Value> {
    LITERALS
        .iter()
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
    CftCheckBuiltin::ALL
        .into_iter()
        .map(builtin_completion_item)
        .collect()
}

pub(crate) fn function_completion_items_for_type(receiver: &coflow_language::CftValueType) -> Vec<Value> {
    CftCheckBuiltin::ALL
        .into_iter()
        .filter(|builtin| builtin_supports_receiver(*builtin, receiver))
        .map(builtin_completion_item)
        .collect()
}

fn builtin_completion_item(builtin: CftCheckBuiltin) -> Value {
    let label = builtin.name();
    let mut item = completion_item(
        label,
        COMPLETION_KIND_FUNCTION,
        "CFT built-in function",
        Some(builtin.documentation()),
    );
    let arguments = (1..=builtin.method_arity())
        .map(|index| format!("${{{index}:value}}"))
        .collect::<Vec<_>>()
        .join(", ");
    insert_object_field(
        &mut item,
        "insertText",
        json!(format!("{label}({arguments})")),
    );
    insert_object_field(&mut item, "insertTextFormat", json!(2));
    item
}

fn builtin_supports_receiver(builtin: CftCheckBuiltin, receiver: &coflow_language::CftValueType) -> bool {
    use CftCheckBuiltin::{
        Abs, ApproxEqual, Contains, ContainsKey, ContainsValue, EndsWith, Intersects, IsBlank,
        IsDisjoint, IsFinite, IsSorted, IsStrictlySorted, IsSubsetOf, IsSupersetOf, Keys, Len,
        Matches, Max, Min, StartsWith, Sum, Unique, Values,
    };
    let receiver = TypeRefLike::from(receiver);
    match builtin {
        Len => matches!(receiver, TypeRefLike::String | TypeRefLike::Array | TypeRefLike::Dict),
        Contains => matches!(receiver, TypeRefLike::String | TypeRefLike::Array | TypeRefLike::Dict),
        Unique | Min | Max | Sum | IsSorted | IsStrictlySorted | Intersects | IsDisjoint
        | IsSubsetOf | IsSupersetOf => matches!(receiver, TypeRefLike::Array),
        Keys | Values | ContainsKey | ContainsValue => matches!(receiver, TypeRefLike::Dict),
        Matches | StartsWith | EndsWith | IsBlank => matches!(receiver, TypeRefLike::String),
        Abs => matches!(receiver, TypeRefLike::Int | TypeRefLike::Float),
        IsFinite | ApproxEqual => matches!(receiver, TypeRefLike::Float),
    }
}

enum TypeRefLike {
    Int,
    Float,
    String,
    Array,
    Dict,
    Other,
}

impl<'a> From<&'a coflow_language::CftValueType> for TypeRefLike {
    fn from(value: &'a coflow_language::CftValueType) -> Self {
        match value {
            coflow_language::CftValueType::Int => Self::Int,
            coflow_language::CftValueType::Float => Self::Float,
            coflow_language::CftValueType::String => Self::String,
            coflow_language::CftValueType::Array(_) => Self::Array,
            coflow_language::CftValueType::Dict(_, _) => Self::Dict,
            _ => Self::Other,
        }
    }
}

fn check_structure_completion_items() -> Vec<Value> {
    [
        ("when", "when ${1:condition} {\n\t${2:condition};\n}"),
        (
            "all",
            "all ${1:item} in ${2:items} {\n\t${3:condition};\n}",
        ),
        (
            "any",
            "any ${1:item} in ${2:items} {\n\t${3:condition};\n}",
        ),
        (
            "none",
            "none ${1:item} in ${2:items} {\n\t${3:condition};\n}",
        ),
    ]
    .into_iter()
    .filter_map(|(label, insert_text)| keyword_snippet_completion_item(label, insert_text))
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
    literal_completion_items()
}

fn const_value_completion_items_for_context(
    build: &LspBuild,
    document: &LspDocument,
    offset: usize,
) -> Vec<Value> {
    let ty = document.ast().and_then(|ast| {
        ast.items.iter().find_map(|item| match item {
            Item::Const(constant) if constant.span.start <= offset && offset <= constant.span.end => {
                constant.ty.as_ref()
            }
            _ => None,
        })
    });
    let Some(ty) = ty else {
        let mut items = const_value_completion_items();
        items.extend(const_completion_items(build));
        return items;
    };
    let mut items = Vec::new();
    collect_default_items_for_type(build, ty, &mut items);
    items.extend(const_completion_items_for_type(build, ty));
    items
}

fn field_default_completion_items(
    build: &LspBuild,
    field: Option<&coflow_language::syntax::ast::FieldDef>,
) -> Vec<Value> {
    let mut items = Vec::new();
    let Some(field) = field else {
        items.extend(literal_completion_items());
        items.extend(const_completion_items(build));
        return items;
    };

    collect_default_items_for_type(build, &field.ty, &mut items);
    items.extend(const_completion_items_for_type(build, &field.ty));
    items
}

fn collect_default_items_for_type(build: &LspBuild, ty: &TypeRef, items: &mut Vec<Value>) {
    match &ty.kind {
        TypeRefKind::Bool => items.extend(literal_completion_items()),
        TypeRefKind::Int | TypeRefKind::Float | TypeRefKind::String => {}
        TypeRefKind::Named(name) => {
            if let Some(enum_def) = build
                .schema()
                .and_then(|container| container.resolve_enum(name))
            {
                items.extend(enum_def.variants.iter().map(|variant| {
                    let label = format!("{}::{}", enum_def.name, variant.name);
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
        TypeRefKind::Option(_) => {
            items.push(completion_item(
                "None",
                COMPLETION_KIND_KEYWORD,
                "CFT Option value",
                Some("Option without a value."),
            ));
            items.push(snippet_completion_item(
                "Some",
                "Some(${1:value})",
                "CFT Option constructor",
                "Option containing a value.",
            ));
        }
        TypeRefKind::Result(_, _) => {
            items.push(snippet_completion_item(
                "Ok",
                "Ok(${1:value})",
                "CFT Result constructor",
                "Successful Result value.",
            ));
            items.push(snippet_completion_item(
                "Err",
                "Err(${1:error})",
                "CFT Result constructor",
                "Failed Result value.",
            ));
        }
        TypeRefKind::Function(_, _) | TypeRefKind::Unit => {}
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
        let insert_text = match *label {
            "Option" => Some("Option<${1:string}>"),
            "Result" => Some("Result<${1:string}, ${2:string}>"),
            "fn" => Some("fn(${1:value}: ${2:int}) -> ${3:int}"),
            _ => None,
        };
        let mut item = completion_item(
            label,
            COMPLETION_KIND_KEYWORD,
            "Primitive type",
            Some(documentation),
        );
        if let Some(insert_text) = insert_text {
            insert_object_field(&mut item, "insertText", json!(insert_text));
            insert_object_field(&mut item, "insertTextFormat", json!(2));
        }
        items.push(item);
    }
    items.extend([
        snippet_completion_item(
            "array",
            "[${1:string}]",
            "CFT array type",
            "Array of one value type.",
        ),
        snippet_completion_item(
            "dictionary",
            "{${1:string}: ${2:string}}",
            "CFT dictionary type",
            "Dictionary with key and value types.",
        ),
        snippet_completion_item(
            "reference",
            "&${1:Type}",
            "CFT record reference type",
            "Reference a top-level record by key.",
        ),
        completion_item("()", COMPLETION_KIND_KEYWORD, "CFT unit type", None),
    ]);
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
                        Item::Const(_) | Item::TypeAlias(_) | Item::Check(_) => {}
                    }
                }
            }
        }
    }
    append_type_alias_completion_items(build, &mut items);
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
    append_type_alias_completion_items(build, &mut items);
    items
}

fn inheritable_type_completion_items(build: &LspBuild, line_prefix: &str) -> Vec<Value> {
    let current_name = line_prefix
        .split_once("type")
        .and_then(|(_, suffix)| suffix.trim_start().split_whitespace().next());
    let Some(schema) = build.schema() else {
        return named_type_completion_items(build);
    };
    schema
        .all_types()
        .filter(|candidate| !candidate.is_sealed)
        .filter(|candidate| current_name != Some(candidate.name.as_str()))
        .filter(|candidate| {
            current_name.is_none_or(|current| !type_descends_from(schema, &candidate.name, current))
        })
        .map(|candidate| {
            completion_item(
                &candidate.name,
                COMPLETION_KIND_CLASS,
                "CFT base type",
                None,
            )
        })
        .collect()
}

fn type_descends_from(schema: &coflow_language::CftSchema, candidate: &str, ancestor: &str) -> bool {
    let mut current = schema.resolve_type(candidate);
    while let Some(ty) = current {
        let Some(parent) = ty.parent.as_deref() else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = schema.resolve_type(parent);
    }
    false
}

fn annotation_argument_completion_items(build: &LspBuild, line_prefix: &str) -> Option<Vec<Value>> {
    let trimmed = line_prefix.trim_end();
    let (annotation, detail, labels): (&str, &str, Vec<String>) = if annotation_argument_open(trimmed, "@dimension") {
        (
            "@dimension",
            "Configured dimension",
            build
                .schema()?
                .all_dimensions()
                .map(|dimension| dimension.name.to_string())
                .collect(),
        )
    } else if annotation_argument_open(trimmed, "@idAsEnum") {
        (
            "@idAsEnum",
            "CFT enum",
            build
                .schema()?
                .all_enums()
                .map(|enum_def| enum_def.name.to_string())
                .collect(),
        )
    } else {
        return None;
    };
    Some(
        labels
            .into_iter()
            .map(|label| completion_item(&label, COMPLETION_KIND_ENUM, detail, Some(annotation)))
            .collect(),
    )
}

fn annotation_argument_open(line_prefix: &str, annotation: &str) -> bool {
    let Some(start) = line_prefix.rfind(annotation) else {
        return false;
    };
    let suffix = &line_prefix[start + annotation.len()..];
    suffix.trim_start().starts_with('(') && !suffix.contains(')')
}

fn module_path_completion_items(build: &LspBuild, line_prefix: &str) -> Option<Vec<Value>> {
    let trimmed = line_prefix.trim_start();
    let (keyword, suffix) = if let Some(suffix) = trimmed.strip_prefix("namespace ") {
        ("namespace", suffix)
    } else if let Some(suffix) = trimmed.strip_prefix("use ") {
        ("use", suffix)
    } else {
        return None;
    };
    if suffix.contains(';') || suffix.contains(" as ") {
        return None;
    }
    let mut items = build
        .documents
        .keys()
        .map(|module| {
            completion_item(
                module,
                COMPLETION_KIND_PROPERTY,
                "CFT module",
                None,
            )
        })
        .collect::<Vec<_>>();
    if keyword == "use" {
        if let Some(schema) = build.schema() {
            items.extend(schema.all_types().map(|ty| {
                let label = format!("{}::{}", ty.module, ty.name);
                completion_item(&label, COMPLETION_KIND_CLASS, "Imported CFT type", None)
            }));
            items.extend(schema.all_enums().map(|enum_def| {
                let label = format!("{}::{}", enum_def.module, enum_def.name);
                completion_item(&label, COMPLETION_KIND_ENUM, "Imported CFT enum", None)
            }));
        }
        items.push(completion_item(
            "as",
            COMPLETION_KIND_KEYWORD,
            "Import alias",
            None,
        ));
    }
    Some(items)
}

fn append_type_alias_completion_items(build: &LspBuild, items: &mut Vec<Value>) {
    for document in build.documents.values() {
        let Some(ast) = &document.ast else {
            continue;
        };
        for item in &ast.items {
            if let Item::TypeAlias(alias) = item {
                items.push(completion_item(
                    &alias.name,
                    COMPLETION_KIND_CLASS,
                    "CFT type alias",
                    None,
                ));
            }
        }
    }
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
        (TypeRefKind::Option(_), CftConstValue::OptionNone) => true,
        (TypeRefKind::Option(inner), CftConstValue::OptionSome(value)) => {
            const_value_assignable_to_type(value, inner)
        }
        (TypeRefKind::Result(ok, _), CftConstValue::ResultOk(value)) => {
            const_value_assignable_to_type(value, ok)
        }
        (TypeRefKind::Result(_, error), CftConstValue::ResultErr(value)) => {
            const_value_assignable_to_type(value, error)
        }
        (TypeRefKind::Int, CftConstValue::Int(_))
        | (TypeRefKind::Float, CftConstValue::Float(_))
        | (TypeRefKind::Bool, CftConstValue::Bool(_))
        | (TypeRefKind::String, CftConstValue::String(_)) => true,
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

fn keyword_snippet_completion_item(label: &str, insert_text: &str) -> Option<Value> {
    let documentation = KEYWORDS
        .iter()
        .find_map(|(keyword, documentation)| (*keyword == label).then_some(*documentation))?;
    Some(snippet_completion_item(
        label,
        insert_text,
        "CFT keyword",
        documentation,
    ))
}

fn snippet_completion_item(
    label: &str,
    insert_text: &str,
    detail: &str,
    documentation: &str,
) -> Value {
    let mut item = completion_item(
        label,
        COMPLETION_KIND_FUNCTION,
        detail,
        Some(documentation),
    );
    insert_object_field(&mut item, "insertText", json!(insert_text));
    insert_object_field(&mut item, "insertTextFormat", json!(2));
    item
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
    match scope {
        CompletionScope::TopLevel => matches!(
            label,
            "@struct"
                | "@flag"
                | "@idAsEnum"
                | "@singleton"
                | "@Host"
                | "@label"
                | "@description"
        ),
        CompletionScope::TypeBody => matches!(
            label,
            "@label" | "@description" | "@expand" | "@localized" | "@dimension"
        ),
        CompletionScope::EnumBody => matches!(label, "@label" | "@description"),
        CompletionScope::CheckBlock => false,
    }
}

pub(crate) fn completion_scope(document: &LspDocument, offset: usize) -> CompletionScope {
    if let Some(ast) = &document.ast {
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
                Item::Const(_)
                | Item::Enum(_)
                | Item::Type(_)
                | Item::TypeAlias(_)
                | Item::Check(_) => {}
            }
        }
    }

    inferred_completion_scope(document, offset).unwrap_or(CompletionScope::TopLevel)
}

fn inferred_completion_scope(
    document: &LspDocument,
    offset: usize,
) -> Option<CompletionScope> {
    #[derive(Clone, Copy)]
    enum PendingBody {
        Type,
        Enum,
        Check,
    }

    let prefix = document.source.get(..offset.min(document.source.len()))?;
    let tokens = lex(&ModuleId::new(document.module_id.clone()), prefix).ok()?;
    let mut scopes = vec![CompletionScope::TopLevel];
    let mut pending = None;

    for token in tokens {
        let current = *scopes.last()?;
        match token.kind {
            TokenKind::Type if current == CompletionScope::TopLevel => {
                pending = Some(PendingBody::Type);
            }
            TokenKind::Enum if current == CompletionScope::TopLevel => {
                pending = Some(PendingBody::Enum);
            }
            TokenKind::Check
                if matches!(current, CompletionScope::TopLevel | CompletionScope::TypeBody) =>
            {
                pending = Some(PendingBody::Check);
            }
            TokenKind::LBrace => {
                let scope = match pending.take() {
                    Some(PendingBody::Type) => CompletionScope::TypeBody,
                    Some(PendingBody::Enum) => CompletionScope::EnumBody,
                    Some(PendingBody::Check) => CompletionScope::CheckBlock,
                    None => current,
                };
                scopes.push(scope);
            }
            TokenKind::RBrace => {
                if scopes.len() > 1 {
                    scopes.pop();
                }
                pending = None;
            }
            TokenKind::Equal | TokenKind::Semicolon => pending = None,
            TokenKind::Eof => {}
            _ => {}
        }
    }

    scopes.last().copied()
}

fn check_block_contains(
    check: Option<&coflow_language::syntax::ast::CheckBlock>,
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

fn is_const_type_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_start();
    trimmed.starts_with("const ")
        && trimmed.contains(':')
        && !trimmed.contains('=')
        && !trimmed.contains(';')
}

fn is_type_alias_target_context(line_prefix: &str) -> bool {
    let trimmed = line_prefix.trim_start();
    trimmed.starts_with("type ") && trimmed.contains('=') && !trimmed.contains(';')
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

fn enum_name_before_double_colon(line_prefix: &str) -> Option<&str> {
    let separator = line_prefix.rfind("::")?;
    if !line_prefix[separator + 2..]
        .chars()
        .all(is_ident_continue)
    {
        return None;
    }
    last_ident(line_prefix[..separator].trim_end())
}

fn enum_variant_completion_items(
    build: &LspBuild,
    document: &LspDocument,
    enum_name: &str,
) -> Vec<Value> {
    if let Some(enum_def) = build
        .schema()
        .and_then(|container| container.resolve_enum(enum_name))
    {
        return enum_def
            .variants
            .iter()
            .map(|variant| enum_variant_completion_item(&enum_def.name, &variant.name))
            .collect();
    }

    enum_variants_from_source(document, enum_name)
        .into_iter()
        .map(|variant| enum_variant_completion_item(enum_name, &variant))
        .collect()
}

fn enum_variant_completion_item(enum_name: &str, variant_name: &str) -> Value {
    completion_item(
        variant_name,
        COMPLETION_KIND_ENUM_MEMBER,
        &format!("{enum_name} variant"),
        None,
    )
}

fn enum_variants_from_source(document: &LspDocument, enum_name: &str) -> Vec<String> {
    let Ok(tokens) = lex(
        &ModuleId::new(document.module_id.clone()),
        &document.source,
    ) else {
        return Vec::new();
    };

    let mut index = 0;
    while index + 2 < tokens.len() {
        let is_target = matches!(tokens[index].kind, TokenKind::Enum)
            && matches!(&tokens[index + 1].kind, TokenKind::Ident(name) if name == enum_name)
            && matches!(tokens[index + 2].kind, TokenKind::LBrace);
        if !is_target {
            index += 1;
            continue;
        }

        index += 3;
        let mut variants = Vec::new();
        while index < tokens.len() {
            match &tokens[index].kind {
                TokenKind::At => skip_annotation_tokens(&tokens, &mut index),
                TokenKind::Ident(name) => {
                    variants.push(name.clone());
                    index += 1;
                    while index < tokens.len()
                        && !matches!(tokens[index].kind, TokenKind::Comma | TokenKind::RBrace)
                    {
                        index += 1;
                    }
                    if index < tokens.len() && matches!(tokens[index].kind, TokenKind::Comma) {
                        index += 1;
                    }
                }
                TokenKind::RBrace | TokenKind::Eof => return variants,
                _ => index += 1,
            }
        }
        return variants;
    }
    Vec::new()
}

fn skip_annotation_tokens(tokens: &[coflow_language::syntax::lexer::Token], index: &mut usize) {
    *index += 1;
    if *index < tokens.len() && matches!(tokens[*index].kind, TokenKind::Ident(_)) {
        *index += 1;
    }
    if *index >= tokens.len() || !matches!(tokens[*index].kind, TokenKind::LParen) {
        return;
    }

    let mut depth = 0usize;
    while *index < tokens.len() {
        match tokens[*index].kind {
            TokenKind::LParen => depth += 1,
            TokenKind::RParen => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    *index += 1;
                    return;
                }
            }
            _ => {}
        }
        *index += 1;
    }
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
