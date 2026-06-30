use unicode_ident::{is_xid_continue, is_xid_start};

#[must_use]
pub fn is_cft_identifier(name: &str) -> bool {
    record_key_ident_error(name).is_none()
}

#[must_use]
pub fn record_key_ident_error(name: &str) -> Option<String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some("record key is empty".to_string());
    };
    if !(first == '_' || is_xid_start(first)) {
        return Some("record key must start with `_` or a Unicode identifier start".to_string());
    }
    if chars.any(|ch| !(ch == '_' || is_xid_continue(ch))) {
        return Some(
            "record key must contain only `_` or Unicode identifier characters".to_string(),
        );
    }
    if is_cft_reserved_identifier(name) {
        return Some(format!("record key `{name}` is a reserved CFT identifier"));
    }
    None
}

#[must_use]
pub fn is_cft_reserved_identifier(name: &str) -> bool {
    matches!(
        name,
        "_" | "id"
            | "Id"
            | "ID"
            | "const"
            | "enum"
            | "type"
            | "abstract"
            | "sealed"
            | "check"
            | "when"
            | "all"
            | "any"
            | "none"
            | "in"
            | "is"
            | "true"
            | "false"
            | "null"
            | "int"
            | "float"
            | "bool"
            | "string"
            | "len"
            | "contains"
            | "isUnique"
            | "min"
            | "max"
            | "sum"
            | "keys"
            | "values"
            | "matches"
            | "if"
            | "else"
            | "match"
            | "case"
            | "for"
            | "while"
            | "let"
            | "module"
            | "import"
            | "export"
            | "from"
            | "as"
            | "use"
    )
}
