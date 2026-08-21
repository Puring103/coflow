pub(crate) fn format_cft(source: &str) -> String {
    let mut output = String::new();
    let mut indent = 0usize;
    let mut continuation = false;
    let ended_with_newline = source.ends_with('\n');

    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            output.push('\n');
            continue;
        }
        if starts_with_closing_delimiter(trimmed) {
            indent = indent.saturating_sub(1);
        }
        output.push_str(&"  ".repeat(indent + usize::from(continuation)));
        output.push_str(trimmed);
        output.push('\n');
        indent = adjusted_indent(indent, trimmed);
        continuation = trimmed.ends_with(':');
    }

    if !ended_with_newline && output.ends_with('\n') {
        output.pop();
    }
    output
}

pub(crate) fn starts_with_closing_delimiter(line: &str) -> bool {
    line.starts_with('}') || line.starts_with(']')
}

pub(crate) fn adjusted_indent(mut indent: usize, line: &str) -> usize {
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '#' => break,
            '{' | '[' => indent += 1,
            '}' | ']' => indent = indent.saturating_sub(1),
            _ => {}
        }
    }
    indent
}
