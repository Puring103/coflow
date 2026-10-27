use unicode_general_category::{get_general_category, GeneralCategory};

pub fn csharp_ident_error(value: &str) -> Option<String> {
    if value.is_empty() {
        return Some("identifier is empty".to_string());
    }
    // Roslyn 按 UTF-16 字符识别名称，补充平面字母也不能直接成为 C# 标识符。
    if value.chars().any(|ch| ch.len_utf16() != 1) {
        return Some(
            "identifier contains a supplementary Unicode character unsupported by C#".into(),
        );
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Some("identifier is empty".to_string());
    };
    if !is_csharp_ident_start(first) {
        return Some("identifier must start with `_` or a C# letter character".to_string());
    }
    if chars.any(|ch| !is_csharp_ident_continue(ch)) {
        return Some("identifier contains a character not supported by C# identifiers".to_string());
    }
    None
}

// CFT 的合法业务名可以是 C# 关键字；逐段转义，不扩大全语言保留字集合。
pub fn csharp_identifier(value: &str) -> String {
    value
        .split('.')
        .map(|part| {
            if is_csharp_keyword(part) {
                format!("@{part}")
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
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
    // C# 使用 Unicode 字符类别，不能用范围不同的 Coflow XID 规则替代。
    ch == '_'
        || matches!(
            get_general_category(ch),
            GeneralCategory::UppercaseLetter
                | GeneralCategory::LowercaseLetter
                | GeneralCategory::TitlecaseLetter
                | GeneralCategory::ModifierLetter
                | GeneralCategory::OtherLetter
                | GeneralCategory::LetterNumber
        )
}

fn is_csharp_ident_continue(ch: char) -> bool {
    is_csharp_ident_start(ch)
        || matches!(
            get_general_category(ch),
            GeneralCategory::NonspacingMark
                | GeneralCategory::SpacingMark
                | GeneralCategory::DecimalNumber
                | GeneralCategory::ConnectorPunctuation
        )
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
