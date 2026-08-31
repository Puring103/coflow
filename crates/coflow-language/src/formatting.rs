/// Formats CFT source using the language's string, comment, operator, and
/// delimiter rules.
#[must_use]
pub fn format_cft(source: &str) -> String {
    let mut output = String::new();
    let mut delimiters = DelimiterIndent::default();
    let mut continuation = false;
    let mut pending_blank_line = false;
    let mut pending_field_annotation = None;
    let mut annotated_multiline_field = None;
    let mut separate_after_annotation = None;
    for raw_line in source.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            if pending_field_annotation.is_none() {
                pending_blank_line = !output.is_empty();
            }
            continue;
        }
        let formatted = normalize_inline_spacing(trimmed);
        let line_indent = delimiters.depth_after_leading_closers(&formatted);
        let is_field_annotation = line_indent > 0 && formatted.starts_with('@');
        let is_comment = formatted.starts_with('#');
        let is_closing = formatted.starts_with('}') || formatted.starts_with(']');

        if formatted == "{" && attach_opening_brace(&mut output) {
            pending_blank_line = false;
            delimiters.apply_line(&formatted);
            continuation = false;
            if separate_after_annotation == Some(line_indent) {
                separate_after_annotation = None;
                annotated_multiline_field = Some(line_indent);
            }
            continue;
        }

        if pending_blank_line {
            if !is_closing && !last_output_line_opens_block(&output) {
                ensure_blank_line(&mut output);
            }
            pending_blank_line = false;
        }
        if is_field_annotation {
            if pending_field_annotation.is_none() {
                ensure_blank_line(&mut output);
            }
            pending_field_annotation = Some(line_indent);
        } else if !is_comment {
            if separate_after_annotation == Some(line_indent) && !is_closing {
                ensure_blank_line(&mut output);
            }
            separate_after_annotation = None;
        }

        output.push_str(&"  ".repeat(line_indent + usize::from(continuation)));
        output.push_str(&formatted);
        output.push('\n');
        delimiters.apply_line(&formatted);
        continuation = formatted.ends_with(':');

        if !is_field_annotation && !is_comment {
            if let Some(depth) = pending_field_annotation.take() {
                if delimiters.depth() > depth {
                    annotated_multiline_field = Some(depth);
                } else {
                    separate_after_annotation = Some(depth);
                }
            } else if annotated_multiline_field.is_some_and(|depth| delimiters.depth() <= depth) {
                separate_after_annotation = annotated_multiline_field.take();
            }
        }
    }

    output
}

fn attach_opening_brace(output: &mut String) -> bool {
    let Some(without_newline) = output.strip_suffix('\n') else {
        return false;
    };
    let previous_line = without_newline
        .rsplit('\n')
        .next()
        .unwrap_or(without_newline);
    let trimmed = previous_line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.ends_with('{')
        || trimmed.ends_with('[')
    {
        return false;
    }

    output.pop();
    trim_end_spaces(output);
    output.push_str(" {\n");
    true
}

fn ensure_blank_line(output: &mut String) {
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
}

fn last_output_line_opens_block(output: &str) -> bool {
    output
        .trim_end_matches('\n')
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.ends_with('{') || line.ends_with('['))
}

fn normalize_inline_spacing(line: &str) -> String {
    let chars = line.chars().collect::<Vec<_>>();
    let type_header = is_type_header(line);
    let mut output = String::with_capacity(line.len());
    let mut pending_space = false;
    let mut tight_right = false;
    let mut in_string = false;
    let mut escaped = false;
    let mut generic_depth = 0usize;
    let mut inline_brace_spacing = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if ch.is_whitespace() {
            pending_space = !tight_right;
            index += 1;
            continue;
        }
        if ch == '#' {
            trim_end_spaces(&mut output);
            if !output.is_empty() {
                output.push(' ');
            }
            output.extend(chars[index..].iter());
            break;
        }
        if ch == '"' {
            push_pending_space(&mut output, &mut pending_space);
            output.push(ch);
            in_string = true;
            tight_right = false;
            index += 1;
            continue;
        }

        let next = chars.get(index + 1).copied();
        let third = chars.get(index + 2).copied();
        match (ch, next) {
            (':', Some(':')) => {
                trim_end_spaces(&mut output);
                output.push_str("::");
                pending_space = false;
                tight_right = true;
                index += 1;
            }
            ('-', Some('>')) => {
                push_spaced_operator(&mut output, "->");
                pending_space = true;
                tight_right = false;
                index += 1;
            }
            ('=', Some('>')) => {
                push_spaced_operator(&mut output, "=>");
                pending_space = true;
                tight_right = false;
                index += 1;
            }
            ('.', Some('.')) => {
                let operator = if third == Some('=') { "..=" } else { ".." };
                push_spaced_operator(&mut output, operator);
                pending_space = true;
                tight_right = false;
                index += usize::from(third == Some('=')) + 1;
            }
            ('>', Some('>')) if generic_depth > 0 => {
                trim_end_spaces(&mut output);
                output.push('>');
                generic_depth -= 1;
                pending_space = false;
                tight_right = true;
            }
            ('<' | '>' | '=' | '!' | '+' | '-' | '*' | '/' | '%', Some('='))
            | ('&', Some('&' | '='))
            | ('|', Some('|' | '='))
            | ('^', Some('='))
            | ('<' | '>', Some('<' | '>'))
            | ('*' | '/', Some('*' | '/')) => {
                let mut operator = String::with_capacity(2);
                operator.push(ch);
                operator.push(next.unwrap_or_default());
                push_spaced_operator(&mut output, &operator);
                pending_space = true;
                tight_right = false;
                index += 1;
            }
            _ => match ch {
                ':' => {
                    trim_end_spaces(&mut output);
                    if type_header
                        && !chars[..index].contains(&'{')
                        && !chars[..index].contains(&'=')
                    {
                        if !output.is_empty() {
                            output.push(' ');
                        }
                        output.push(':');
                    } else {
                        output.push(':');
                    }
                    pending_space = true;
                    tight_right = false;
                }
                ',' => {
                    trim_end_spaces(&mut output);
                    output.push(',');
                    pending_space = true;
                    tight_right = false;
                }
                ';' => {
                    trim_end_spaces(&mut output);
                    output.push(';');
                    pending_space = false;
                    tight_right = false;
                }
                '(' | '[' => {
                    trim_end_spaces(&mut output);
                    if ch == '['
                        && pending_space
                        && output.chars().next_back().is_some_and(is_separator)
                    {
                        output.push(' ');
                    }
                    output.push(ch);
                    pending_space = false;
                    tight_right = true;
                }
                ')' | ']' => {
                    trim_end_spaces(&mut output);
                    output.push(ch);
                    pending_space = false;
                    tight_right = false;
                }
                '{' => {
                    trim_end_spaces(&mut output);
                    let spaced_inside = output.chars().next_back().is_some_and(|previous| {
                        previous.is_alphanumeric() || matches!(previous, '_' | ')' | ']')
                    });
                    if !output.is_empty() && !output.ends_with(' ') {
                        output.push(' ');
                    }
                    output.push('{');
                    inline_brace_spacing.push(spaced_inside);
                    pending_space = spaced_inside;
                    tight_right = !spaced_inside;
                }
                '}' => {
                    let spaced_inside = inline_brace_spacing.pop().unwrap_or(false);
                    trim_end_spaces(&mut output);
                    if spaced_inside && !output.ends_with('{') {
                        output.push(' ');
                    }
                    output.push('}');
                    pending_space = false;
                    tight_right = false;
                }
                '=' => {
                    push_spaced_operator(&mut output, "=");
                    pending_space = true;
                    tight_right = false;
                }
                '<' if is_generic_open(&output) => {
                    trim_end_spaces(&mut output);
                    output.push('<');
                    generic_depth += 1;
                    pending_space = false;
                    tight_right = true;
                }
                '>' if generic_depth > 0 => {
                    trim_end_spaces(&mut output);
                    output.push('>');
                    generic_depth -= 1;
                    pending_space = false;
                    tight_right = true;
                }
                '<' | '>' => {
                    let mut operator = String::new();
                    operator.push(ch);
                    push_spaced_operator(&mut output, &operator);
                    pending_space = true;
                    tight_right = false;
                }
                '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^'
                    if is_binary_operator(&output, &chars, index) =>
                {
                    let mut operator = String::new();
                    operator.push(ch);
                    push_spaced_operator(&mut output, &operator);
                    pending_space = true;
                    tight_right = false;
                }
                '+' | '-' | '!' | '~' | '&' => {
                    push_pending_space(&mut output, &mut pending_space);
                    output.push(ch);
                    tight_right = true;
                }
                '.' => {
                    trim_end_spaces(&mut output);
                    output.push('.');
                    pending_space = false;
                    tight_right = true;
                }
                _ => {
                    push_pending_space(&mut output, &mut pending_space);
                    output.push(ch);
                    tight_right = false;
                }
            },
        }
        index += 1;
    }

    trim_end_spaces(&mut output);
    output
}

fn is_separator(ch: char) -> bool {
    matches!(
        ch,
        ':' | ',' | '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>'
    )
}

fn is_generic_open(output: &str) -> bool {
    let identifier = output
        .trim_end()
        .rsplit(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .next();
    matches!(identifier, Some("Option" | "Result"))
}

fn is_binary_operator(output: &str, chars: &[char], index: usize) -> bool {
    let previous = output.chars().rev().find(|ch| !ch.is_whitespace());
    let next = chars[index + 1..]
        .iter()
        .copied()
        .find(|ch| !ch.is_whitespace());
    previous.is_some_and(|ch| {
        !matches!(
            ch,
            '(' | '['
                | '{'
                | ':'
                | ','
                | '='
                | '+'
                | '-'
                | '*'
                | '/'
                | '%'
                | '!'
                | '&'
                | '|'
                | '^'
                | '<'
                | '>'
        )
    }) && next.is_some_and(|ch| !matches!(ch, ')' | ']' | '}' | ',' | ';'))
}

fn is_type_header(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("type ")
        || line.starts_with("abstract type ")
        || line.starts_with("sealed type ")
}

fn push_pending_space(output: &mut String, pending_space: &mut bool) {
    if *pending_space && !output.is_empty() && !output.ends_with(' ') {
        output.push(' ');
    }
    *pending_space = false;
}

fn push_spaced_operator(output: &mut String, operator: &str) {
    trim_end_spaces(output);
    if !output.is_empty() {
        output.push(' ');
    }
    output.push_str(operator);
}

fn trim_end_spaces(output: &mut String) {
    while output.ends_with(' ') {
        output.pop();
    }
}

#[derive(Clone, Default)]
struct DelimiterIndent {
    groups: Vec<Vec<char>>,
}

impl DelimiterIndent {
    fn depth(&self) -> usize {
        self.groups.len()
    }

    fn depth_after_leading_closers(&self, line: &str) -> usize {
        let mut projected = self.clone();
        let mut touched_groups = 0usize;
        let mut last_group = None;
        for ch in line.chars().take_while(|ch| matches!(ch, '}' | ']')) {
            let group = projected.groups.len().checked_sub(1);
            if group.is_some() && group != last_group {
                touched_groups += 1;
                last_group = group;
            }
            projected.close(ch);
        }
        self.groups.len().saturating_sub(touched_groups)
    }

    fn apply_line(&mut self, line: &str) {
        let mut replacement_group: Option<usize> = None;
        let mut line_group: Option<usize> = None;
        for ch in delimiter_events(line) {
            match ch {
                '{' | '[' => {
                    let group = line_group
                        .filter(|index| *index < self.groups.len())
                        .or_else(|| replacement_group.filter(|index| *index < self.groups.len()));
                    if let Some(index) = group {
                        self.groups[index].push(ch);
                        line_group = Some(index);
                    } else {
                        self.groups.push(vec![ch]);
                        line_group = Some(self.groups.len() - 1);
                    }
                }
                '}' | ']' => {
                    replacement_group = self.groups.len().checked_sub(1);
                    self.close(ch);
                    if line_group.is_some_and(|index| index >= self.groups.len()) {
                        line_group = None;
                    }
                }
                _ => {}
            }
        }
    }

    fn close(&mut self, _delimiter: char) {
        let Some(group) = self.groups.last_mut() else {
            return;
        };
        group.pop();
        if group.is_empty() {
            self.groups.pop();
        }
    }
}

fn delimiter_events(line: &str) -> Vec<char> {
    let mut delimiters = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
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
            '{' | '[' | '}' | ']' => delimiters.push(ch),
            _ => {}
        }
    }
    delimiters
}
