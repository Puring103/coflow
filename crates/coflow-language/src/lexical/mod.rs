//! Shared lexical rules used by CFT, CFD, and embedded function syntax.

mod trivia;

pub use trivia::{tokenize_lossless, LosslessToken, LosslessTokenKind};
pub(crate) use trivia::{
    decode_simple_escape, scan_balanced_delimiter, scan_number_literal, scan_string_literal,
    scan_trivia, validate_formatted_string_literal, validate_number_literal, DelimiterNesting,
    NumberLiteralError, StringLiteralError,
};

use unicode_ident::{is_xid_continue, is_xid_start};

/// Returns whether `ch` may start a Coflow identifier.
#[must_use]
pub fn is_identifier_start(ch: char) -> bool {
    ch == '_' || is_xid_start(ch)
}

/// Returns whether `ch` may continue a Coflow identifier.
#[must_use]
pub fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || is_xid_continue(ch)
}

#[must_use]
pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_identifier_start) && chars.all(is_identifier_continue)
}

#[must_use]
pub fn is_cft_identifier(name: &str) -> bool {
    is_identifier(name) && !is_cft_reserved_identifier(name)
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
    if !is_identifier_start(first) {
        return Some(IdentifierIssue::InvalidStart);
    }
    if chars.any(|ch| !is_identifier_continue(ch)) {
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
            | "fn"
            | "var"
            | "return"
            | "break"
            | "continue"
            | "Host"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "Option"
            | "Result"
            | "alert"
            | "records"
    )
}

#[cfg(test)]
mod tests {
    use super::{is_cft_identifier, is_identifier, is_identifier_continue, is_identifier_start};

    #[test]
    fn uses_unicode_xid_for_all_identifier_positions() {
        assert!(is_identifier_start('\u{53D8}'));
        assert!(is_identifier_continue('\u{0301}'));
        assert!(is_identifier("\u{53D8}\u{91CF}\u{0301}"));
        assert!(is_cft_identifier("\u{53D8}\u{91CF}\u{0301}"));
        assert!(!is_identifier_start('\u{0301}'));
        assert!(!is_cft_identifier("\u{0301}value"));
    }

    #[test]
    fn rejects_all_target_language_reserved_identifiers() {
        for name in [
            "fn", "var", "return", "if", "else", "match", "for", "while", "break",
            "continue", "None", "Some", "Ok", "Err", "Option",
            "Result", "Host", "alert", "records",
        ] {
            assert!(!is_cft_identifier(name), "`{name}` must be reserved");
        }
        for name in ["namespace", "use", "as"] {
            assert!(is_cft_identifier(name), "`{name}` is an ordinary identifier");
        }
    }
}
