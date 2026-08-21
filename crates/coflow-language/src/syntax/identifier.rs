use unicode_ident::{is_xid_continue, is_xid_start};

#[must_use]
pub fn is_cft_identifier(name: &str) -> bool {
    identifier_issue(name).is_none()
}

#[must_use]
pub fn record_key_ident_error(name: &str) -> Option<String> {
    Some(match identifier_issue(name)? {
        IdentifierIssue::Empty => "record key is empty".to_string(),
        IdentifierIssue::InvalidStart => {
            "record key must start with `_` or a Unicode identifier start".to_string()
        }
        IdentifierIssue::InvalidContinue => {
            "record key must contain only `_` or Unicode identifier characters".to_string()
        }
        IdentifierIssue::Reserved => format!("record key `{name}` is a reserved CFT identifier"),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentifierIssue {
    Empty,
    InvalidStart,
    InvalidContinue,
    Reserved,
}

fn identifier_issue(name: &str) -> Option<IdentifierIssue> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Some(IdentifierIssue::Empty);
    };
    if !(first == '_' || is_xid_start(first)) {
        return Some(IdentifierIssue::InvalidStart);
    }
    if chars.any(|ch| !(ch == '_' || is_xid_continue(ch))) {
        return Some(IdentifierIssue::InvalidContinue);
    }
    if is_cft_reserved_identifier(name) {
        return Some(IdentifierIssue::Reserved);
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
