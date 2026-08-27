use unicode_ident::{is_xid_continue, is_xid_start};

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
    if value.split('.').next() == Some("Coflow") {
        return Some("namespace root `Coflow` is reserved by the Runtime entry type".to_string());
    }
    for part in value.split('.') {
        if let Some(reason) = csharp_ident_error(part) {
            return Some(format!("namespace segment `{part}` {reason}"));
        }
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
    pascal_case(cft_short_name(name))
}

pub fn cft_short_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

pub fn cft_namespace(name: &str) -> Option<&str> {
    name.rsplit_once("::").map(|(namespace, _)| namespace)
}

pub fn csharp_declaration_namespace(root: &str, name: &str) -> String {
    cft_namespace(name).map_or_else(
        || root.to_string(),
        |namespace| format!("{}.{}", root, namespace.replace("::", ".")),
    )
}

pub fn csharp_qualified_type_name(root: &str, name: &str) -> String {
    format!(
        "global::{}.{}",
        csharp_declaration_namespace(root, name),
        csharp_type_name(name)
    )
}

pub fn csharp_relative_type_path(name: &str) -> String {
    cft_namespace(name).map_or_else(
        || format!("{}.cs", csharp_type_name(name)),
        |namespace| {
            format!(
                "{}/{}.cs",
                namespace.replace("::", "/"),
                csharp_type_name(name)
            )
        },
    )
}

pub fn metadata_identifier(name: &str) -> String {
    let mut out = String::from("Cft_");
    for byte in name.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02X}");
    }
    out
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
