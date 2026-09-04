use unicode_ident::{is_xid_continue, is_xid_start};

pub(super) fn looks_like_bare_record_key(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty()
        && !matches!(text, "_" | "null")
        && is_type_marker_name(text)
        && !text.starts_with('{')
        && !text.starts_with('[')
        && !text.starts_with('"')
        && !text.contains(',')
        && !text.contains(':')
        && !text.contains('{')
        && !text.contains('}')
        && !text.contains('[')
        && !text.contains(']')
        && text.chars().next().is_some_and(|ch| ch != '@')
}

pub(super) fn is_type_marker_name(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || is_xid_start(first)) && chars.all(|ch| ch == '_' || is_xid_continue(ch))
}
