use coflow_language::lexical::{
    is_identifier, tokenize_lossless, LosslessToken, LosslessTokenKind,
};

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
    let mut function_bodies = FunctionScopes::default();
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
            function_bodies.apply_line(&formatted);
            continuation = false;
            if separate_after_annotation == Some(line_indent) {
                separate_after_annotation = None;
                annotated_multiline_field = Some(line_indent);
            }
            continue;
        }

        if pending_blank_line {
            let collapse_use_group = is_use && last_output_line_starts_with(&output, "use ");
            let preserve_function_blank = function_bodies.is_inside();
            if preserve_function_blank
                || (!is_closing && !last_output_line_opens_block(&output) && !collapse_use_group)
            {
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
        function_bodies.apply_line(&formatted);
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

#[derive(Default)]
struct FunctionScopes {
    braces: Vec<bool>,
    pending_signature: bool,
    pending_arrow: bool,
    return_type_started: bool,
}

impl FunctionScopes {
    fn is_inside(&self) -> bool {
        self.braces.iter().any(|is_function| *is_function)
    }

    fn apply_line(&mut self, line: &str) {
        for token in tokenize_lossless(line)
            .into_iter()
            .filter(|token| !token.is_trivia())
        {
            match token.text(line) {
                "fn" if token.kind == LosslessTokenKind::Identifier => {
                    self.pending_signature = true;
                    self.pending_arrow = false;
                    self.return_type_started = false;
                }
                "->" if self.pending_signature => {
                    self.pending_arrow = true;
                    self.return_type_started = false;
                }
                "{" => {
                    let is_function = self.pending_signature
                        && self.pending_arrow
                        && self.return_type_started;
                    self.braces.push(is_function);
                    if is_function {
                        self.pending_signature = false;
                        self.pending_arrow = false;
                        self.return_type_started = false;
                    } else if self.pending_arrow {
                        self.return_type_started = true;
                    }
                }
                "}" => {
                    self.braces.pop();
                }
                ";" if self.pending_signature => {
                    self.pending_signature = false;
                    self.pending_arrow = false;
                    self.return_type_started = false;
                }
                _ if self.pending_arrow => {
                    self.return_type_started = true;
                }
                _ => {}
            }
        }
    }
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
    if let Some(comment) = tokenize_lossless(line)
        .into_iter()
        .find(|token| token.kind == LosslessTokenKind::Comment)
    {
        return line[..comment.span.start].trim().is_empty().then_some("");
    }
    Some(line.trim())
}

fn code_before_comment(line: &str) -> &str {
    for token in tokenize_lossless(line) {
        if token.kind == LosslessTokenKind::Comment {
            return line[..token.span.start].trim();
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
    tokenize_lossless(source)
        .into_iter()
        .find(|token| !token.is_trivia())
        .is_some_and(|token| {
            token.kind == LosslessTokenKind::Identifier && token.text(source) == keyword
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
    prefix.split("::").all(is_identifier)
        || ["type ", "abstract type ", "sealed type "]
            .iter()
            .any(|start| prefix.starts_with(start))
}

fn split_array_object_header(source: &str) -> Option<(&str, &str)> {
    let tokens = tokenize_lossless(source);
    for (index, token) in tokens.iter().enumerate() {
        if token.kind == LosslessTokenKind::Comment {
            return None;
        }
        if token.text(source) == "[" {
            let brace = tokens[index + 1..]
                .iter()
                .find(|candidate| candidate.text(source) == "{")?;
            let after = source[token.span.end..].trim_start();
            let marker = source[token.span.end..brace.span.start].trim();
            if marker.is_empty() || marker.split("::").all(is_identifier) {
                return Some((source[..token.span.end].trim_end(), after));
            }
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
    tokenize_lossless(line)
        .into_iter()
        .skip_while(|token| token.kind == LosslessTokenKind::Whitespace)
        .take_while(|token| token.text(line) == ">")
        .count()
        .min(depth)
}

fn generic_depth_after_line(line: &str, mut depth: usize) -> usize {
    let code = code_before_comment(line);
    let mut prefix = String::new();
    for token in tokenize_lossless(code) {
        let text = token.text(code);
        match text {
            "<" if depth > 0 || is_generic_open(&prefix) => depth += 1,
            ">" if depth > 0 => depth -= 1,
            _ => {}
        }
        prefix.push_str(text);
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
    let tokens = tokenize_lossless(source);
    let mut output = String::with_capacity(source.len() + source.len() / 4);
    let mut braces = Vec::<(BraceKind, usize)>::new();
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut generic_depth = 0usize;
    let mut just_closed_structural = false;

    for (index, token) in tokens.iter().enumerate() {
        let text = token.text(source);
        if !matches!(
            token.kind,
            LosslessTokenKind::Whitespace | LosslessTokenKind::Newline
        ) && !matches!(text, "," | "]")
        {
            just_closed_structural = false;
        }
        match text {
            "(" if token.kind == LosslessTokenKind::Symbol => {
                paren_depth += 1;
                output.push_str(text);
            }
            ")" if token.kind == LosslessTokenKind::Symbol => {
                paren_depth = paren_depth.saturating_sub(1);
                output.push_str(text);
            }
            "[" if token.kind == LosslessTokenKind::Symbol => {
                bracket_depth += 1;
                output.push_str(text);
            }
            "]" if token.kind == LosslessTokenKind::Symbol => {
                if just_closed_structural {
                    push_line_break_before(&mut output);
                }
                bracket_depth = bracket_depth.saturating_sub(1);
                output.push_str(text);
                just_closed_structural = false;
            }
            "<" if token.kind == LosslessTokenKind::Symbol && is_generic_open(&output) => {
                generic_depth += 1;
                output.push_str(text);
            }
            ">" if token.kind == LosslessTokenKind::Symbol && generic_depth > 0 => {
                generic_depth -= 1;
                output.push_str(text);
            }
            "{" if token.kind == LosslessTokenKind::Symbol => {
                let kind = classify_open_brace(current_line_fragment(&output), language);
                braces.push((kind, bracket_depth));
                output.push_str(text);
                if kind != BraceKind::Inline
                    && next_token_on_line(&tokens, source, index)
                        .is_some_and(|next| next != "}")
                {
                    push_line_break(&mut output);
                }
            }
            "}" if token.kind == LosslessTokenKind::Symbol => {
                let kind = braces.pop().map_or(BraceKind::Inline, |entry| entry.0);
                if kind != BraceKind::Inline {
                    push_line_break_before(&mut output);
                }
                output.push_str(text);
                just_closed_structural = kind != BraceKind::Inline;
                if kind != BraceKind::Inline
                    && next_token_on_line(&tokens, source, index).is_some_and(|next| {
                        !matches!(next, "," | ";" | ")" | "]" | "}" | "else")
                    })
                {
                    push_line_break(&mut output);
                }
            }
            ";" if token.kind == LosslessTokenKind::Symbol => {
                output.push_str(text);
                if language == FormatLanguage::Cft
                    && braces.iter().any(|(kind, _)| *kind != BraceKind::Inline)
                    && paren_depth == 0
                    && next_token_on_line(&tokens, source, index)
                        .is_some_and(|next| !next.starts_with('#'))
                {
                    push_line_break(&mut output);
                }
            }
            "," if token.kind == LosslessTokenKind::Symbol => {
                output.push_str(text);
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
                    && next_token_on_line(&tokens, source, index).is_some()
                {
                    push_line_break(&mut output);
                }
                just_closed_structural = false;
            }
            _ => output.push_str(text),
        }
    }
    output
}

fn next_token_on_line<'a>(
    tokens: &[LosslessToken],
    source: &'a str,
    index: usize,
) -> Option<&'a str> {
    for token in &tokens[index + 1..] {
        match token.kind {
            LosslessTokenKind::Whitespace => {}
            LosslessTokenKind::Newline => return None,
            _ => return Some(token.text(source)),
        }
    }
    None
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
            let typed_record = prefix.rsplit_once(':').map_or_else(
                || is_cfd_type_name(prefix),
                |(_, type_name)| is_cfd_type_name(type_name.trim()),
            );
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

fn is_cfd_type_name(value: &str) -> bool {
    !value.is_empty() && value.split("::").all(is_identifier)
}

fn current_line_fragment(output: &str) -> &str {
    output.rsplit('\n').next().unwrap_or(output)
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
        && is_identifier(header)
}

fn normalize_inline_spacing(line: &str) -> String {
    let tokens = tokenize_lossless(line);
    let type_header = is_type_header(line);
    let mut output = String::with_capacity(line.len());
    let mut pending_space = false;
    let mut tight_right = false;
    let mut generic_depth = 0usize;
    let mut inline_brace_spacing = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        let text = token.text(line);
        match token.kind {
            LosslessTokenKind::Whitespace | LosslessTokenKind::Newline => {
                pending_space = !tight_right;
                continue;
            }
            LosslessTokenKind::Comment => {
                trim_end_spaces(&mut output);
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(text);
                break;
            }
            _ => {}
        }

        match text {
            "::" => {
                trim_end_spaces(&mut output);
                output.push_str(text);
                pending_space = false;
                tight_right = true;
            }
            "->" | "=>" | ".." | "..=" | "<=" | ">=" | "==" | "!=" | "&&" | "||"
            | "+=" | "-=" | "*=" | "/=" | "<<" | ">>" | "**" | "//" => {
                push_spaced_operator(&mut output, text);
                pending_space = true;
                tight_right = false;
            }
            ":" => {
                trim_end_spaces(&mut output);
                if type_header
                    && !tokens[..index].iter().any(|token| token.text(line) == "{")
                    && !tokens[..index].iter().any(|token| token.text(line) == "=")
                    && !output.is_empty()
                {
                    output.push(' ');
                }
                output.push(':');
                pending_space = true;
                tight_right = false;
            }
            "," => {
                trim_end_spaces(&mut output);
                output.push(',');
                pending_space = true;
                tight_right = false;
            }
            ";" => {
                trim_end_spaces(&mut output);
                output.push(';');
                pending_space = false;
                tight_right = false;
            }
            "(" | "[" => {
                trim_end_spaces(&mut output);
                if text == "["
                    && pending_space
                    && output.chars().next_back().is_some_and(is_separator)
                {
                    output.push(' ');
                }
                output.push_str(text);
                pending_space = false;
                tight_right = true;
            }
            ")" | "]" => {
                trim_end_spaces(&mut output);
                output.push_str(text);
                pending_space = false;
                tight_right = false;
            }
            "{" => {
                trim_end_spaces(&mut output);
                let spaced_inside = output.chars().next_back().is_some_and(|previous| {
                    coflow_language::lexical::is_identifier_continue(previous)
                        || matches!(previous, ')' | ']')
                });
                if !output.is_empty() && !output.ends_with(' ') {
                    output.push(' ');
                }
                output.push('{');
                inline_brace_spacing.push(spaced_inside);
                pending_space = spaced_inside;
                tight_right = !spaced_inside;
            }
            "}" => {
                let spaced_inside = inline_brace_spacing.pop().unwrap_or(false);
                trim_end_spaces(&mut output);
                if spaced_inside && !output.ends_with('{') {
                    output.push(' ');
                }
                output.push('}');
                pending_space = false;
                tight_right = false;
            }
            "=" => {
                push_spaced_operator(&mut output, text);
                pending_space = true;
                tight_right = false;
            }
            "<" if is_generic_open(&output) => {
                trim_end_spaces(&mut output);
                output.push('<');
                generic_depth += 1;
                pending_space = false;
                tight_right = true;
            }
            ">" if generic_depth > 0 => {
                trim_end_spaces(&mut output);
                output.push('>');
                generic_depth -= 1;
                pending_space = false;
                tight_right = true;
            }
            "<" | ">" => {
                push_spaced_operator(&mut output, text);
                pending_space = true;
                tight_right = false;
            }
            "+" | "-" | "*" | "/" | "%" | "&" | "|" | "^"
                if is_binary_operator_token(&tokens, line, index) =>
            {
                push_spaced_operator(&mut output, text);
                pending_space = true;
                tight_right = false;
            }
            "+" | "-" | "!" | "~" | "&" => {
                push_pending_space(&mut output, &mut pending_space);
                output.push_str(text);
                tight_right = true;
            }
            "." => {
                trim_end_spaces(&mut output);
                output.push('.');
                pending_space = false;
                tight_right = true;
            }
            _ => {
                push_pending_space(&mut output, &mut pending_space);
                output.push_str(text);
                tight_right = false;
            }
        }
    }

    trim_end_spaces(&mut output);
    output
}

fn is_binary_operator_token(tokens: &[LosslessToken], source: &str, index: usize) -> bool {
    let previous = tokens[..index]
        .iter()
        .rev()
        .find(|token| !token.is_trivia())
        .map(|token| token.text(source));
    let next = tokens[index + 1..]
        .iter()
        .find(|token| !token.is_trivia())
        .map(|token| token.text(source));
    previous.is_some_and(|token| {
        !matches!(
            token,
            "(" | "[" | "{" | ":" | "," | "=" | "+" | "-" | "*" | "/" | "%"
                | "!" | "&" | "|" | "^" | "<" | ">" | "->" | "=>" | ".." | "..="
                | "<=" | ">=" | "==" | "!=" | "&&" | "||" | "+=" | "-=" | "*="
                | "/=" | "<<" | ">>" | "**" | "//"
        )
    }) && next.is_some_and(|token| !matches!(token, ")" | "]" | "}" | "," | ";"))
}

fn is_separator(ch: char) -> bool {
    matches!(
        ch,
        ':' | ',' | '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>'
    )
}

fn is_generic_open(output: &str) -> bool {
    tokenize_lossless(output)
        .into_iter()
        .rev()
        .find(|token| !token.is_trivia())
        .is_some_and(|token| {
            token.kind == LosslessTokenKind::Identifier
                && matches!(token.text(output), "Option" | "Result")
        })
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
    tokenize_lossless(line)
        .into_iter()
        .filter(|token| token.kind == LosslessTokenKind::Symbol)
        .filter_map(|token| match token.text(line) {
            "{" => Some('{'),
            "[" => Some('['),
            "(" => Some('('),
            "}" => Some('}'),
            "]" => Some(']'),
            ")" => Some(')'),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::format_cfd;

    #[test]
    fn cfd_formatter_expands_typed_blocks_without_input_spacing() {
        let source = "sword:Item{name:\"Sword\",}";
        let expected = "sword: Item {\n  name: \"Sword\",\n}\n";

        assert_eq!(format_cfd(source), expected);
        assert_eq!(format_cfd(expected), expected);
    }
}
