use crate::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};

#[derive(Debug, Clone, Copy)]
pub enum NameTokenKind {
    General,
    Reference,
}

pub(super) struct ScalarToken {
    pub(super) text: String,
    pub(super) span: CfdTextSpan,
}

pub(super) fn parse_name(
    source: &str,
    pos: &mut usize,
    label: &str,
    kind: NameTokenKind,
) -> Result<String, CfdTextDiagnostics> {
    skip_ws_and_comments(source, pos);
    let start = *pos;
    while let Some(ch) = peek_char(source, *pos) {
        if is_name_boundary(ch, kind) {
            break;
        }
        *pos += ch.len_utf8();
    }
    if *pos == start {
        return Err(missing_token(label, start, *pos));
    }
    Ok(source[start..*pos].to_string())
}

pub(super) fn parse_scalar_token(
    source: &str,
    pos: &mut usize,
    label: &str,
) -> Result<ScalarToken, CfdTextDiagnostics> {
    skip_ws_and_comments(source, pos);
    let start = *pos;
    while let Some(ch) = peek_char(source, *pos) {
        if ch.is_whitespace() || matches!(ch, ':' | ',' | ';' | '}' | ']' | '|') {
            break;
        }
        *pos += ch.len_utf8();
    }
    if *pos == start {
        return Err(missing_token(label, start, *pos));
    }
    Ok(ScalarToken {
        text: source[start..*pos].to_string(),
        span: CfdTextSpan { start, end: *pos },
    })
}

pub(super) fn parse_quoted_string(
    source: &str,
    pos: &mut usize,
) -> Result<String, CfdTextDiagnostics> {
    skip_ws_and_comments(source, pos);
    let start = *pos;
    if !eat_char(source, pos, '"') {
        return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            CfdTextErrorCode::Syntax,
            "expected string opening quote",
            CfdTextSpan { start, end: *pos },
        )));
    }
    let mut out = String::new();
    let mut escaped = false;
    while let Some(ch) = peek_char(source, *pos) {
        *pos += ch.len_utf8();
        if escaped {
            match ch {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => {
                    return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                        CfdTextErrorCode::Syntax,
                        format!("unsupported string escape `\\{other}`"),
                        CfdTextSpan { start, end: *pos },
                    )));
                }
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Ok(out);
        } else {
            out.push(ch);
        }
    }
    Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
        CfdTextErrorCode::Syntax,
        "unterminated string",
        CfdTextSpan { start, end: *pos },
    )))
}

pub(super) fn skip_ws_and_comments(source: &str, pos: &mut usize) {
    loop {
        while peek_char(source, *pos).is_some_and(char::is_whitespace) {
            *pos += peek_char(source, *pos).map_or(0, char::len_utf8);
        }
        if source[*pos..].starts_with("//") {
            *pos += 2;
            while peek_char(source, *pos).is_some_and(|ch| ch != '\n') {
                *pos += peek_char(source, *pos).map_or(0, char::len_utf8);
            }
            continue;
        }
        if source[*pos..].starts_with('#') {
            *pos += 1;
            while peek_char(source, *pos).is_some_and(|ch| ch != '\n') {
                *pos += peek_char(source, *pos).map_or(0, char::len_utf8);
            }
            continue;
        }
        break;
    }
}

pub(super) fn eat_char(source: &str, pos: &mut usize, expected: char) -> bool {
    if peek_char(source, *pos) == Some(expected) {
        *pos += expected.len_utf8();
        true
    } else {
        false
    }
}

pub(super) fn eat_spread(source: &str, pos: &mut usize) -> bool {
    skip_ws_and_comments(source, pos);
    if source[*pos..].starts_with("...") {
        *pos += 3;
        true
    } else {
        false
    }
}

pub(super) fn eat_keyword(source: &str, pos: &mut usize, expected: &str) -> bool {
    skip_ws_and_comments(source, pos);
    if !source[*pos..].starts_with(expected) {
        return false;
    }
    let end = *pos + expected.len();
    if source
        .get(end..)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|ch| !is_value_boundary(ch))
    {
        return false;
    }
    *pos = end;
    true
}

pub(super) fn peek_keyword(source: &str, pos: usize, expected: &str) -> bool {
    let mut saved = pos;
    eat_keyword(source, &mut saved, expected)
}

pub(super) fn peek_char(source: &str, pos: usize) -> Option<char> {
    source[pos..].chars().next()
}

pub(super) fn previous_char(source: &str, pos: usize) -> Option<char> {
    source[..pos].chars().next_back()
}

pub(super) fn is_eof(source: &str, pos: usize) -> bool {
    pos >= source.len()
}

pub(super) fn is_value_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | ';' | '}' | ']' | '|' | ':')
}

fn is_name_boundary(ch: char, kind: NameTokenKind) -> bool {
    match kind {
        NameTokenKind::General => {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '=' | ';' | ',' | '{' | '}' | '[' | ']' | '(' | ')' | '@' | '&' | '"'
                )
        }
        NameTokenKind::Reference => {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '.' | '[' | ']' | ',' | ';' | '}' | ')' | ':' | '@' | '&'
                )
        }
    }
}

fn missing_token(label: &str, start: usize, end: usize) -> CfdTextDiagnostics {
    CfdTextDiagnostics::one(CfdTextDiagnostic::error(
        CfdTextErrorCode::Syntax,
        format!("{label} is missing"),
        CfdTextSpan { start, end },
    ))
}
