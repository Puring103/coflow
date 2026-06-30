use coflow_cft::{CftAnnotation, CftAnnotationValue};
use unicode_ident::{is_xid_continue, is_xid_start};

pub fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

pub fn annotation_name_arg(annotations: &[CftAnnotation], name: &str) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(value) => Some(value.clone()),
            _ => None,
        })
}

pub fn csharp_ident_error(value: &str) -> Option<String> {
    if value.is_empty() {
        return Some("identifier is empty".to_string());
    }
    if is_csharp_keyword(value) {
        return Some("identifier is a C# keyword".to_string());
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Some("identifier is empty".to_string());
    };
    if !is_csharp_ident_start(first) {
        return Some("identifier must start with `_` or a Unicode identifier start".to_string());
    }
    if chars.any(|ch| !is_csharp_ident_continue(ch)) {
        return Some(
            "identifier must contain only `_` or Unicode identifier characters".to_string(),
        );
    }
    None
}

pub fn csharp_namespace_error(value: &str) -> Option<String> {
    if value.is_empty() {
        return Some("namespace is empty".to_string());
    }
    for part in value.split('.') {
        if let Some(reason) = csharp_ident_error(part) {
            return Some(format!("namespace segment `{part}` {reason}"));
        }
    }
    None
}

pub fn csharp_member_ident_error(value: &str) -> Option<String> {
    let Some(unprefixed) = value.strip_prefix('_') else {
        return csharp_ident_error(value);
    };
    if unprefixed.is_empty() {
        return None;
    }
    if unprefixed.chars().any(|ch| !is_csharp_ident_continue(ch)) {
        return Some(
            "identifier must contain only `_` or Unicode identifier characters".to_string(),
        );
    }
    None
}

pub fn pascal_case(name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in name.chars() {
        if matches!(ch, '_' | '-' | ' ') {
            upper = true;
            continue;
        }
        if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn csharp_type_name(name: &str) -> String {
    pascal_case(name)
}

fn is_csharp_ident_start(ch: char) -> bool {
    ch == '_' || is_xid_start(ch)
}

fn is_csharp_ident_continue(ch: char) -> bool {
    ch == '_' || is_xid_continue(ch)
}

fn is_csharp_keyword(value: &str) -> bool {
    matches!(
        value,
        "abstract"
            | "as"
            | "base"
            | "bool"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "checked"
            | "class"
            | "const"
            | "continue"
            | "decimal"
            | "default"
            | "delegate"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "event"
            | "explicit"
            | "extern"
            | "false"
            | "finally"
            | "fixed"
            | "float"
            | "for"
            | "foreach"
            | "goto"
            | "if"
            | "implicit"
            | "in"
            | "int"
            | "interface"
            | "internal"
            | "is"
            | "lock"
            | "long"
            | "namespace"
            | "new"
            | "null"
            | "object"
            | "operator"
            | "out"
            | "override"
            | "params"
            | "private"
            | "protected"
            | "public"
            | "readonly"
            | "ref"
            | "return"
            | "sbyte"
            | "sealed"
            | "short"
            | "sizeof"
            | "stackalloc"
            | "static"
            | "string"
            | "struct"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "uint"
            | "ulong"
            | "unchecked"
            | "unsafe"
            | "ushort"
            | "using"
            | "virtual"
            | "void"
            | "volatile"
            | "while"
    )
}

pub fn camel_case(name: &str) -> String {
    let pascal = pascal_case(name);
    let mut chars = pascal.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_lowercase().collect::<String>() + chars.as_str()
}

pub fn pluralize(name: &str) -> String {
    if name.ends_with('s') {
        format!("{name}es")
    } else {
        format!("{name}s")
    }
}

pub fn index_param_name(type_name: &str) -> String {
    format!("{}Index", camel_case(type_name))
}

pub fn format_float(value: f64) -> String {
    let mut text = value.to_string();
    if !text.contains('.') && !text.contains('e') && !text.contains('E') {
        text.push_str(".0");
    }
    text
}

pub fn escape_csharp_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
