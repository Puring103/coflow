//! C# 标识符、命名空间和字符串转义。
pub(super) fn identifier(source: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "abstract",
        "as",
        "base",
        "bool",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "checked",
        "class",
        "const",
        "continue",
        "decimal",
        "default",
        "delegate",
        "do",
        "double",
        "else",
        "enum",
        "event",
        "explicit",
        "extern",
        "false",
        "finally",
        "fixed",
        "float",
        "for",
        "foreach",
        "goto",
        "if",
        "implicit",
        "in",
        "int",
        "interface",
        "internal",
        "is",
        "lock",
        "long",
        "namespace",
        "new",
        "null",
        "object",
        "operator",
        "out",
        "override",
        "params",
        "private",
        "protected",
        "public",
        "readonly",
        "ref",
        "return",
        "sbyte",
        "sealed",
        "short",
        "sizeof",
        "stackalloc",
        "static",
        "string",
        "struct",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "uint",
        "ulong",
        "unchecked",
        "unsafe",
        "ushort",
        "using",
        "virtual",
        "void",
        "volatile",
        "while",
    ];
    if KEYWORDS.contains(&source) {
        format!("@{source}")
    } else {
        source.to_string()
    }
}

pub(super) fn name(source: &str) -> String {
    source
        .split("::")
        .map(identifier)
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn qualified(root: &str, source: &str) -> String {
    format!("global::{}.{}", root, name(source))
}
pub(super) fn namespace(root: &str, source: &str) -> String {
    source.rsplit_once("::").map_or_else(
        || root.to_string(),
        |(ns, _)| format!("{root}.{}", name(ns)),
    )
}
pub(super) fn short(source: &str) -> &str {
    source.rsplit("::").next().unwrap_or(source)
}
pub(super) fn quoted(source: &str) -> String {
    format!(
        "\"{}\"",
        source
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
