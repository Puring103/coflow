#[derive(Clone, Debug)]
pub(crate) struct WordAt {
    pub(crate) text: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn is_trivia_position(source: &str, offset: usize) -> bool {
    let line_prefix = line_prefix_at(source, offset);
    if is_after_line_comment(line_prefix) {
        return true;
    }
    is_inside_string(source, offset)
}

pub(crate) fn is_inside_string(source: &str, offset: usize) -> bool {
    let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    let mut in_string = false;
    let mut escaped = false;
    for ch in source[line_start..offset].chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
        }
    }
    in_string
}

pub(crate) fn is_after_line_comment(line_prefix: &str) -> bool {
    let mut in_string = false;
    let mut escaped = false;
    for ch in line_prefix.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_string && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if !in_string && ch == '#' {
            return true;
        }
    }
    false
}

pub(crate) fn dotted_chain_at(source: &str, word: &WordAt) -> Option<Vec<String>> {
    let line_start = source[..word.start]
        .rfind('\n')
        .map_or(0, |index| index + 1);
    let line_end = source[word.end..]
        .find('\n')
        .map_or(source.len(), |index| word.end + index);
    let mut start = word.start;
    while start > line_start {
        let previous = previous_char(source, start)?;
        if previous.1 == '.' || previous.1.is_whitespace() || is_ident_continue(previous.1) {
            start = previous.0;
        } else {
            break;
        }
    }
    let mut end = word.end;
    while end < line_end {
        let Some(ch) = source[end..].chars().next() else {
            break;
        };
        if ch == '.' || ch.is_whitespace() || is_ident_continue(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    parse_dotted_ident_chain(&source[start..end])
}

pub(crate) fn word_at(source: &str, offset: usize) -> Option<WordAt> {
    let mut start = offset.min(source.len());
    if start == source.len()
        || !source[start..]
            .chars()
            .next()
            .is_some_and(is_ident_continue)
    {
        if let Some((previous, ch)) = previous_char(source, start) {
            if is_ident_continue(ch) {
                start = previous;
            }
        }
    }
    while let Some((previous, ch)) = previous_char(source, start) {
        if is_ident_continue(ch) {
            start = previous;
        } else {
            break;
        }
    }

    let mut end = start;
    while end < source.len() {
        let Some(ch) = source[end..].chars().next() else {
            break;
        };
        if is_ident_continue(ch) {
            end += ch.len_utf8();
        } else {
            break;
        }
    }
    (end > start).then(|| WordAt {
        text: source[start..end].to_string(),
        start,
        end,
    })
}

pub(crate) fn parse_dotted_ident_chain(text: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    for part in text.split('.') {
        let trimmed = part.trim();
        if trimmed.is_empty() || !trimmed.chars().all(is_ident_continue) {
            return None;
        }
        parts.push(trimmed.to_string());
    }
    (!parts.is_empty()).then_some(parts)
}

pub(crate) fn previous_char(source: &str, offset: usize) -> Option<(usize, char)> {
    source[..offset].char_indices().next_back()
}

pub(crate) fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

pub(crate) fn last_ident(text: &str) -> Option<&str> {
    let mut start = text.trim_end().len();
    let end = start;
    while let Some((previous, ch)) = previous_char(text, start) {
        if is_ident_continue(ch) {
            start = previous;
        } else {
            break;
        }
    }
    (start < end).then_some(&text[start..end])
}

pub(crate) fn line_prefix_at(source: &str, offset: usize) -> &str {
    let start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    &source[start..offset]
}
