#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatLanguage {
    Cft,
    Cfd,
}

/// Formats CFT source using the language's declaration, block, spacing, and
/// delimiter rules.
#[must_use]
pub fn format_cft(source: &str) -> String {
    format_source(source, FormatLanguage::Cft)
}

/// Formats CFD source using record-aware block, spacing, and delimiter rules.
#[must_use]
pub fn format_cfd(source: &str) -> String {
    format_source(source, FormatLanguage::Cfd)
}

fn format_source(source: &str, language: FormatLanguage) -> String {
    let collapsed = collapse_logical_lines(source);
    let expanded = expand_structural_lines(&collapsed, language);
    let mut output = String::new();
    let mut delimiters = DelimiterIndent::default();
    let mut generic_continuation_depth = 0usize;
    let mut continuation = false;
    let mut pending_blank_line = false;
    let mut pending_field_annotation = None;
    let mut annotated_multiline_field = None;
    let mut separate_after_annotation = None;
    for raw_line in expanded.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            if pending_field_annotation.is_none() {
                pending_blank_line = !output.is_empty();
            }
            continue;
        }
        let formatted = normalize_inline_spacing(trimmed);
        let generic_closers = leading_generic_closers(&formatted, generic_continuation_depth);
        let line_indent = delimiters.depth_after_leading_closers(&formatted)
            + generic_continuation_depth.saturating_sub(generic_closers);
        let is_field_annotation = line_indent > 0 && formatted.starts_with('@');
        let is_top_level_annotation = line_indent == 0 && formatted.starts_with('@');
        let is_top_level_definition = line_indent == 0
            && match language {
                FormatLanguage::Cft => is_definition_start(&formatted),
                FormatLanguage::Cfd => is_cfd_record_start(&formatted),
            };
        let is_grouped_cfd_record = language == FormatLanguage::Cfd
            && line_indent == 1
            && is_grouped_record_start(&formatted);
        let is_type_check = language == FormatLanguage::Cft
            && line_indent > 0
            && (formatted == "check {" || formatted.starts_with("check "));
        let is_use = line_indent == 0 && formatted.starts_with("use ");
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
            let collapse_use_group = is_use && last_output_line_starts_with(&output, "use ");
            if !is_closing && !last_output_line_opens_block(&output) && !collapse_use_group {
                ensure_blank_line(&mut output);
            }
            pending_blank_line = false;
        }
        if is_use && last_output_line_starts_with(&output, "namespace ") {
            ensure_blank_line(&mut output);
        }
        if (is_top_level_annotation || is_top_level_definition)
            && !last_output_line_is_annotation(&output)
        {
            ensure_blank_line(&mut output);
        }
        if (is_grouped_cfd_record || is_type_check) && !last_output_line_opens_block(&output) {
            ensure_blank_line(&mut output);
        }
        if is_field_annotation {
            if pending_field_annotation.is_none() {
                ensure_blank_line(&mut output);
            }
            separate_after_annotation = None;
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
        generic_continuation_depth =
            generic_depth_after_line(&formatted, generic_continuation_depth);
        if !is_comment {
            continuation = line_requires_continuation(&formatted);
        }

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

fn collapse_logical_lines(source: &str) -> String {
    let lines = source.lines().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;
    while index < lines.len() {
        let mut line = lines[index].trim().to_string();

        loop {
            let Some(code) = uncommented_code(&line) else {
                break;
            };
            let join_field_value = joinable_colon_prefix(code);
            let join_arrow_result = code.ends_with("->");
            let join_closing_arrow = code.ends_with(')');
            let join_else = code.ends_with('}');
            if !join_field_value && !join_arrow_result && !join_closing_arrow && !join_else {
                break;
            }

            let Some((next_index, next)) = next_joinable_line(&lines, index) else {
                break;
            };
            let next_code = uncommented_code(next).unwrap_or_default();
            let should_join = join_field_value
                || (join_arrow_result && !next_code.is_empty())
                || (join_closing_arrow && next_code.starts_with("->"))
                || (join_else && starts_with_keyword(next_code, "else"));
            if !should_join {
                break;
            }

            line.push(' ');
            line.push_str(next.trim());
            index = next_index;
        }

        if let Some((before, after)) = split_array_object_header(&line) {
            output.push_str(before);
            output.push('\n');
            line = after.to_string();
        }
        output.push_str(&line);
        output.push('\n');
        index += 1;
    }
    output
}

fn uncommented_code(line: &str) -> Option<&str> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
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
            '#' => return line[..index].trim().is_empty().then_some(""),
            _ => {}
        }
    }
    Some(line.trim())
}

fn code_before_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
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
            '#' => return line[..index].trim(),
            _ => {}
        }
    }
    line.trim()
}

fn next_joinable_line<'a>(lines: &'a [&str], index: usize) -> Option<(usize, &'a str)> {
    for (offset, line) in lines.get(index + 1..)?.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        return uncommented_code(line)
            .filter(|code| !code.is_empty())
            .map(|_| (index + offset + 1, *line));
    }
    None
}

fn starts_with_keyword(source: &str, keyword: &str) -> bool {
    source.strip_prefix(keyword).is_some_and(|rest| {
        rest.chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
    })
}

fn joinable_colon_prefix(source: &str) -> bool {
    let Some(prefix) = source.strip_suffix(':').map(str::trim) else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    if prefix.starts_with('"') && prefix.ends_with('"') {
        return true;
    }
    prefix
        .chars()
        .all(|ch| ch == '_' || ch == ':' || ch.is_alphanumeric())
        || ["type ", "abstract type ", "sealed type "]
            .iter()
            .any(|start| prefix.starts_with(start))
}

fn split_array_object_header(source: &str) -> Option<(&str, &str)> {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in source.char_indices() {
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
            '#' => return None,
            '[' => {
                let after = source[index + 1..].trim_start();
                let brace = after.find('{')?;
                let marker = after[..brace].trim();
                if marker.is_empty()
                    || marker
                        .chars()
                        .all(|ch| ch == '_' || ch == ':' || ch.is_alphanumeric())
                {
                    return Some((source[..index + 1].trim_end(), after));
                }
            }
            _ => {}
        }
    }
    None
}

fn line_requires_continuation(line: &str) -> bool {
    let code = code_before_comment(line);
    if code
        .strip_suffix('<')
        .is_some_and(|prefix| is_generic_open(prefix.trim_end()))
    {
        return false;
    }
    [
        ":", "=", "->", "=>", "+", "-", "*", "/", "%", "&&", "||", "&", "|", "^",
        "==", "!=", "<", ">", "<=", ">=", ".",
    ]
    .iter()
    .any(|operator| code.ends_with(operator))
}

fn leading_generic_closers(line: &str, depth: usize) -> usize {
    line.chars().take_while(|ch| *ch == '>').count().min(depth)
}

fn generic_depth_after_line(line: &str, mut depth: usize) -> usize {
    let code = code_before_comment(line);
    let mut prefix = String::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in code.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            prefix.push(ch);
            continue;
        }
        match ch {
            '"' => in_string = true,
            '<' if depth > 0 || is_generic_open(&prefix) => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ => {}
        }
        prefix.push(ch);
    }
    depth
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BraceKind {
    Structural,
    Enum,
    Inline,
}

fn expand_structural_lines(source: &str, language: FormatLanguage) -> String {
    let chars = source.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(source.len() + source.len() / 4);
    let mut braces = Vec::<(BraceKind, usize)>::new();
    let mut in_string = false;
    let mut escaped = false;
    let mut in_comment = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut generic_depth = 0usize;
    let mut just_closed_structural = false;

    for (index, ch) in chars.iter().copied().enumerate() {
        if in_comment {
            output.push(ch);
            if ch == '\n' {
                in_comment = false;
            }
            continue;
        }
        if in_string {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if !ch.is_whitespace() && !matches!(ch, ',' | ']') {
            just_closed_structural = false;
        }
        match ch {
            '"' => {
                in_string = true;
                output.push(ch);
            }
            '#' => {
                in_comment = true;
                output.push(ch);
            }
            '(' => {
                paren_depth += 1;
                output.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                output.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                output.push(ch);
            }
            ']' => {
                if just_closed_structural {
                    push_line_break_before(&mut output);
                }
                bracket_depth = bracket_depth.saturating_sub(1);
                output.push(ch);
                just_closed_structural = false;
            }
            '<' if is_generic_open(&output) => {
                generic_depth += 1;
                output.push(ch);
            }
            '>' if generic_depth > 0 => {
                generic_depth -= 1;
                output.push(ch);
            }
            '{' => {
                let kind = classify_open_brace(current_line_fragment(&output), language);
                braces.push((kind, bracket_depth));
                output.push(ch);
                if kind != BraceKind::Inline
                    && next_non_whitespace_on_line(&chars, index + 1).is_some_and(|ch| ch != '}')
                {
                    push_line_break(&mut output);
                }
            }
            '}' => {
                let kind = braces.pop().map_or(BraceKind::Inline, |entry| entry.0);
                if kind != BraceKind::Inline {
                    push_line_break_before(&mut output);
                }
                output.push(ch);
                just_closed_structural = kind != BraceKind::Inline;
                if kind != BraceKind::Inline
                    && next_non_whitespace_on_line(&chars, index + 1).is_some_and(|ch| {
                        !matches!(ch, ',' | ';' | ')' | ']' | '}')
                    })
                    && !next_word_is(&chars, index + 1, "else")
                {
                    push_line_break(&mut output);
                }
            }
            ';' => {
                output.push(ch);
                if language == FormatLanguage::Cft
                    && braces.iter().any(|(kind, _)| *kind != BraceKind::Inline)
                    && paren_depth == 0
                    && next_non_whitespace_on_line(&chars, index + 1)
                        .is_some_and(|ch| ch != '#')
                {
                    push_line_break(&mut output);
                }
            }
            ',' => {
                output.push(ch);
                let split_cft_enum = language == FormatLanguage::Cft
                    && generic_depth == 0
                    && braces.last().is_some_and(|(kind, depth)| {
                        *kind == BraceKind::Enum && *depth == bracket_depth
                    })
                    && paren_depth == 0;
                let split_cfd_field = language == FormatLanguage::Cfd
                    && generic_depth == 0
                    && braces.last().is_some_and(|(kind, depth)| {
                        *kind != BraceKind::Inline && *depth == bracket_depth
                    })
                    && paren_depth == 0;
                if (split_cft_enum || split_cfd_field || just_closed_structural)
                    && next_non_whitespace_on_line(&chars, index + 1).is_some()
                {
                    push_line_break(&mut output);
                }
                just_closed_structural = false;
            }
            _ => output.push(ch),
        }
    }
    output
}

fn classify_open_brace(prefix: &str, language: FormatLanguage) -> BraceKind {
    let prefix = prefix.trim();
    match language {
        FormatLanguage::Cft => {
            if prefix.starts_with("enum ") {
                BraceKind::Enum
            } else if prefix.starts_with("type ")
                || prefix.starts_with("abstract type ")
                || prefix.starts_with("sealed type ")
                || prefix == "check"
                || prefix.starts_with("check ")
                || prefix.starts_with("when ")
                || prefix.starts_with("all ")
                || prefix.starts_with("any ")
                || prefix.starts_with("none ")
                || prefix.starts_with("if ")
                || prefix == "else"
                || prefix.ends_with(" else")
                || (prefix.contains("fn") && prefix.contains("->"))
            {
                BraceKind::Structural
            } else {
                BraceKind::Inline
            }
        }
        FormatLanguage::Cfd => {
            let last = prefix.split_whitespace().next_back().unwrap_or_default();
            let typed_record = !last.is_empty()
                && last.chars().all(|ch| ch == '_' || ch.is_alphanumeric())
                && (prefix.contains(':') || prefix.split_whitespace().count() == 1);
            if typed_record
                || prefix.starts_with("if ")
                || prefix == "else"
                || prefix.ends_with(" else")
                || prefix.starts_with("match ")
                || (prefix.contains("fn") && prefix.contains("->"))
            {
                BraceKind::Structural
            } else {
                BraceKind::Inline
            }
        }
    }
}

fn current_line_fragment(output: &str) -> &str {
    output.rsplit('\n').next().unwrap_or(output)
}

fn next_non_whitespace_on_line(chars: &[char], start: usize) -> Option<char> {
    chars
        .get(start..)?
        .iter()
        .copied()
        .take_while(|ch| *ch != '\n' && *ch != '\r')
        .find(|ch| !ch.is_whitespace())
}

fn next_word_is(chars: &[char], start: usize, word: &str) -> bool {
    chars
        .get(start..)
        .map(|suffix| suffix.iter().copied().skip_while(|ch| ch.is_whitespace()).take(word.len()).collect::<String>())
        .is_some_and(|candidate| candidate == word)
}

fn push_line_break(output: &mut String) {
    trim_end_spaces(output);
    if !output.ends_with('\n') {
        output.push('\n');
    }
}

fn push_line_break_before(output: &mut String) {
    trim_end_spaces(output);
    if !output.ends_with('\n') && !output.ends_with('{') {
        output.push('\n');
    }
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

fn last_output_line_is_annotation(output: &str) -> bool {
    output
        .trim_end_matches('\n')
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.trim_start().starts_with('@'))
}

fn last_output_line_starts_with(output: &str, prefix: &str) -> bool {
    output
        .trim_end_matches('\n')
        .rsplit('\n')
        .next()
        .is_some_and(|line| line.trim_start().starts_with(prefix))
}

fn is_definition_start(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("type ")
        || line.starts_with("abstract type ")
        || line.starts_with("sealed type ")
        || line.starts_with("enum ")
        || line.starts_with("const ")
        || line.starts_with("check ")
}

fn is_cfd_record_start(line: &str) -> bool {
    let line = line.trim();
    line.ends_with('{')
        && !line.starts_with("namespace ")
        && !line.starts_with("use ")
        && !line.starts_with('#')
}

fn is_grouped_record_start(line: &str) -> bool {
    let line = line.trim();
    let header = line.strip_suffix('{').map(str::trim).unwrap_or_default();
    !header.contains(':')
        && !header.is_empty()
        && header
            .chars()
            .all(|ch| ch == '_' || ch.is_alphanumeric())
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
        for ch in line
            .chars()
            .take_while(|ch| matches!(ch, '}' | ']' | ')'))
        {
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
                '{' | '[' | '(' => {
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
                '}' | ']' | ')' => {
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
            '{' | '[' | '(' | '}' | ']' | ')' => delimiters.push(ch),
            _ => {}
        }
    }
    delimiters
}
