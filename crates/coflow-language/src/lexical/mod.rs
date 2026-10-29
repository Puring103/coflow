//! Shared lexical rules used by CFT and CFD.

mod trivia;

pub(crate) use trivia::{
    decode_simple_escape, lossless_tokens, scan_number_literal, scan_string_literal, scan_trivia,
    DelimiterNesting, NumberLiteralError, StringLiteralError,
};
pub use trivia::{tokenize_lossless, LosslessToken, LosslessTokenKind};

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
pub fn record_id_ident_error(name: &str) -> Option<String> {
    Some(match identifier_issue(name)? {
        IdentifierIssue::Empty => "record ID is empty".to_string(),
        IdentifierIssue::InvalidStart => {
            "record ID must start with `_` or a Unicode identifier start".to_string()
        }
        IdentifierIssue::InvalidContinue => {
            "record ID must contain only `_` or Unicode identifier characters".to_string()
        }
        IdentifierIssue::Reserved => format!("record ID `{name}` is a reserved CFT identifier"),
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
    // 只保留当前语法的结构词与字面量；方法名和目标语言关键字不占用业务名称。
    matches!(
        name,
        "_" | "const"
            | "enum"
            | "type"
            | "table"
            | "data"
            | "singleton"
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
            | "None"
            | "Some"
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
    fn reserves_only_current_syntax_and_literals() {
        for name in [
            "check", "when", "all", "any", "none", "None", "Some", "table", "in",
        ] {
            assert!(!is_cft_identifier(name), "`{name}` must be reserved");
        }
        for name in [
            "min",
            "max",
            "keys",
            "values",
            "records",
            "for",
            "return",
            "fn",
            "var",
            "id",
            "Id",
            "ID",
            "namespace",
            "null",
            "fstring",
        ] {
            assert!(
                is_cft_identifier(name),
                "`{name}` is an ordinary identifier"
            );
        }
    }
}
