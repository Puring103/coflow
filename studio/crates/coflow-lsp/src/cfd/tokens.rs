//! CFD tokens 能力。
use super::{
    incomplete_group_keys, is_identifier_continue, tokenize_cfd, CfdAst, CfdBitExpr,
    CfdBitExprKind, CfdValue, CftSchema, LosslessTokenKind, SemanticTokens, Span,
    CFD_FUNCTION_KEYWORDS, CFD_FUNCTION_TYPES, MOD_DECLARATION, MOD_RECORD, MOD_REFERENCE,
    MOD_SCHEMA, SEM_COMMENT, SEM_ENUM_MEMBER, SEM_FUNCTION, SEM_KEYWORD, SEM_NUMBER, SEM_OPERATOR,
    SEM_PARAMETER, SEM_PROPERTY, SEM_RECORD_KEY, SEM_STRING, SEM_TYPE, SEM_VARIABLE,
};

pub fn semantic_tokens(source: &str, ast: &CfdAst, schema: Option<&CftSchema>) -> SemanticTokens {
    let mut collector = TokenCollector::new(source);

    // Lex all comment spans.
    collect_comment_tokens(source, &mut collector);

    // Walk the AST for structured tokens.
    for record in &ast.records {
        collector.add(
            record.key_span,
            SEM_RECORD_KEY,
            MOD_DECLARATION | MOD_RECORD,
        );
        collector.add(record.type_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
        for field in &record.fields {
            collector.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
            collect_value_tokens(&field.value, &mut collector);
        }
    }
    if let Some(schema) = schema {
        for (_, span, _) in incomplete_group_keys(source, schema) {
            collector.add(span, SEM_RECORD_KEY, MOD_DECLARATION | MOD_RECORD);
        }
    }

    collector.into_lsp_data()
}

pub(super) fn collect_value_tokens(value: &CfdValue, c: &mut TokenCollector<'_>) {
    match value {
        CfdValue::Scalar(text, span) => {
            collect_scalar_token(text, *span, c);
        }
        CfdValue::BitExpr(expr) => collect_bit_expr_tokens(expr, c),
        CfdValue::QuotedString(_, span) => c.add_multiline_plain(*span, SEM_STRING),
        CfdValue::FormattedString(value) => collect_formatted_string_tokens(value, c),
        CfdValue::Function(value) => collect_function_tokens(value.span, &value.source, c),
        CfdValue::OptionNone(span) => c.add_plain(*span, SEM_KEYWORD),
        CfdValue::Block(block) => {
            if let Some((_, span)) = &block.type_marker {
                c.add(*span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
            }
            for field in &block.fields {
                c.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
                collect_value_tokens(&field.value, c);
            }
        }
        CfdValue::Dimension(dimension) => {
            for field in &dimension.fields {
                c.add(field.name_span, SEM_PROPERTY, MOD_DECLARATION | MOD_SCHEMA);
                collect_value_tokens(&field.value, c);
            }
        }
        CfdValue::Array(items, _) => {
            for item in items {
                collect_value_tokens(item, c);
            }
        }
        CfdValue::Ref(r) => {
            c.add_plain(Span::new(r.span.start, r.span.start + 1), SEM_OPERATOR);
            if let Some((_, type_span)) = &r.type_name {
                c.add(*type_span, SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA);
            }
            c.add(r.key.1, SEM_RECORD_KEY, MOD_REFERENCE | MOD_RECORD);
        }
    }
}

pub(super) fn collect_formatted_string_tokens(
    value: &coflow_language::cfd::CfdFormattedString,
    collector: &mut TokenCollector<'_>,
) {
    collector.add_multiline_plain(value.span, SEM_STRING);
}

pub(super) fn collect_bit_expr_tokens(expr: &CfdBitExpr, c: &mut TokenCollector<'_>) {
    match &expr.kind {
        CfdBitExprKind::Value(text) => collect_scalar_token(text, expr.span, c),
        CfdBitExprKind::Binary { lhs, rhs, .. } => {
            collect_bit_expr_tokens(lhs, c);
            collect_bit_expr_tokens(rhs, c);
        }
    }
}

pub(super) fn collect_scalar_token(text: &str, span: Span, c: &mut TokenCollector<'_>) {
    if matches!(text, "None" | "true" | "false") {
        c.add_plain(span, SEM_KEYWORD);
    } else if text
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_digit() || b == b'-')
    {
        c.add_plain(span, SEM_NUMBER);
    } else if text.bytes().next().is_some_and(|b| b.is_ascii_uppercase()) {
        c.add(span, SEM_ENUM_MEMBER, MOD_REFERENCE | MOD_SCHEMA);
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn collect_function_tokens(
    span: Span,
    function_source: &str,
    c: &mut TokenCollector<'_>,
) {
    visit_function_semantic_tokens(
        function_source,
        span.start,
        |span, token_type, modifiers, multiline| {
            if multiline {
                c.add_multiline(span, token_type, modifiers);
            } else {
                c.add(span, token_type, modifiers);
            }
        },
    );
}

pub(crate) fn visit_function_semantic_tokens(
    function_source: &str,
    base: usize,
    mut visit: impl FnMut(Span, u32, u32, bool),
) {
    for token in tokenize_cfd(function_source) {
        let text = token.text(function_source);
        let span = offset_span(base, token.span.start, token.span.end);
        match token.kind {
            LosslessTokenKind::Comment => visit(span, SEM_COMMENT, 0, false),
            LosslessTokenKind::String => visit(span, SEM_STRING, 0, true),
            LosslessTokenKind::Number => visit(span, SEM_NUMBER, 0, false),
            LosslessTokenKind::Identifier => {
                // `$field` 在共享 token 流中是一个整体；语义高亮只标记字段名部分。
                if let Some(field) = text.strip_prefix('$') {
                    let field_start = token.span.end - field.len();
                    visit(
                        offset_span(base, field_start, token.span.end),
                        SEM_PROPERTY,
                        MOD_REFERENCE | MOD_SCHEMA,
                        false,
                    );
                    continue;
                }
                let previous = previous_non_whitespace(function_source, token.span.start);
                let following = next_non_whitespace(function_source, token.span.end);
                let (token_type, modifiers) = if CFD_FUNCTION_KEYWORDS.contains(&text) {
                    (SEM_KEYWORD, 0)
                } else if CFD_FUNCTION_TYPES.contains(&text) {
                    (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
                } else if previous == Some('&') {
                    if function_source[token.span.end..].starts_with("::") {
                        (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
                    } else {
                        (SEM_RECORD_KEY, MOD_REFERENCE | MOD_RECORD)
                    }
                } else if previous == Some('.') {
                    if following == Some('(') {
                        (SEM_FUNCTION, MOD_REFERENCE)
                    } else {
                        (SEM_PROPERTY, MOD_REFERENCE | MOD_SCHEMA)
                    }
                } else if previous_ends_double_colon(function_source, token.span.start) {
                    if type_key_chain_is_reference(function_source, token.span.start) {
                        (SEM_RECORD_KEY, MOD_REFERENCE | MOD_RECORD)
                    } else {
                        (SEM_ENUM_MEMBER, MOD_REFERENCE | MOD_SCHEMA)
                    }
                } else if following == Some(':') {
                    (SEM_PARAMETER, MOD_DECLARATION)
                } else if matches!(
                    previous_function_ident(function_source, token.span.start),
                    Some("var" | "as")
                ) {
                    (SEM_VARIABLE, MOD_DECLARATION)
                } else if following == Some('(') {
                    (SEM_FUNCTION, MOD_REFERENCE)
                } else if text.chars().next().is_some_and(char::is_uppercase) {
                    (SEM_TYPE, MOD_REFERENCE | MOD_SCHEMA)
                } else {
                    (SEM_VARIABLE, MOD_REFERENCE)
                };
                visit(span, token_type, modifiers, false);
            }
            LosslessTokenKind::Symbol if is_function_operator(text) => {
                visit(span, SEM_OPERATOR, 0, false);
            }
            LosslessTokenKind::Whitespace
            | LosslessTokenKind::Newline
            | LosslessTokenKind::Symbol
            | LosslessTokenKind::Unknown => {}
        }
    }
}

pub(super) fn is_function_operator(text: &str) -> bool {
    matches!(
        text,
        "..="
            | "->"
            | "::"
            | "//"
            | "=="
            | "!="
            | "<="
            | ">="
            | "&&"
            | "||"
            | "=>"
            | "**"
            | "<<"
            | ">>"
            | ".."
            | "+="
            | "-="
            | "*="
            | "/="
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "<"
            | ">"
            | "="
            | "!"
            | "~"
            | "&"
            | "|"
            | "^"
            | "?"
            | "."
            | ":"
            | "$"
    )
}

const fn offset_span(base: usize, start: usize, end: usize) -> Span {
    Span::new(base + start, base + end)
}

pub(super) fn previous_non_whitespace(source: &str, offset: usize) -> Option<char> {
    source[..offset]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
}

pub(super) fn previous_function_ident(source: &str, offset: usize) -> Option<&str> {
    let prefix = source[..offset].trim_end();
    let start = prefix
        .char_indices()
        .rev()
        .find_map(|(index, ch)| (!is_identifier_continue(ch)).then_some(index + ch.len_utf8()))
        .unwrap_or(0);
    prefix.get(start..).filter(|ident| !ident.is_empty())
}

pub(super) fn next_non_whitespace(source: &str, offset: usize) -> Option<char> {
    source[offset..].chars().find(|ch| !ch.is_whitespace())
}

pub(super) fn previous_ends_double_colon(source: &str, offset: usize) -> bool {
    source[..offset].trim_end().ends_with("::")
}

pub(super) fn type_key_chain_is_reference(source: &str, offset: usize) -> bool {
    let prefix = source[..offset].trim_end_matches(':');
    let chain_start = prefix
        .char_indices()
        .rev()
        .find_map(|(index, ch)| {
            (!is_identifier_continue(ch) && ch != ':' && ch != '&').then_some(index + ch.len_utf8())
        })
        .unwrap_or(0);
    prefix[chain_start..].starts_with('&')
}

pub(super) fn collect_comment_tokens(source: &str, c: &mut TokenCollector<'_>) {
    for token in tokenize_cfd(source) {
        if token.kind == LosslessTokenKind::Comment {
            c.add_plain(token.span, SEM_COMMENT);
        }
    }
}

pub(super) struct TokenCollector<'a> {
    source: &'a str,
    // 行索引只构建一次；逐 token 从文件头扫描会让语义着色退化成二次复杂度。
    line_index: coflow_project::LineIndex,
    tokens: Vec<(usize, usize, u32, u32)>, // (byte_start, byte_end, token_type, modifiers)
}

impl<'a> TokenCollector<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self {
            source,
            line_index: coflow_project::LineIndex::new(source),
            tokens: Vec::new(),
        }
    }

    pub(super) fn add(&mut self, span: Span, token_type: u32, modifiers: u32) {
        if span.start < span.end {
            self.tokens
                .push((span.start, span.end, token_type, modifiers));
        }
    }

    fn add_plain(&mut self, span: Span, token_type: u32) {
        self.add(span, token_type, 0);
    }

    fn add_multiline_plain(&mut self, span: Span, token_type: u32) {
        self.add_multiline(span, token_type, 0);
    }

    fn add_multiline(&mut self, span: Span, token_type: u32, modifiers: u32) {
        let mut start = span.start;
        for line in self.source[span.start..span.end.min(self.source.len())].split_inclusive('\n') {
            let content_len = line.trim_end_matches(['\r', '\n']).len();
            if content_len != 0 {
                self.add(Span::new(start, start + content_len), token_type, modifiers);
            }
            start += line.len();
        }
    }

    pub(super) fn into_lsp_data(mut self) -> SemanticTokens {
        // Sort by start, then remove same-start duplicates and overlapping tokens.
        self.tokens.sort_by_key(|&(start, _, _, _)| start);
        self.tokens.dedup_by_key(|t| t.0);

        let mut data: Vec<u32> = Vec::new();
        let mut prev_line = 0usize;
        let mut prev_char = 0usize;
        let mut prev_end = 0usize; // track end of last emitted token to skip overlaps

        for (start, end, token_type, modifiers) in self.tokens {
            // Skip tokens that overlap with the previous one.
            if start < prev_end {
                continue;
            }
            prev_end = end;
            let position = self.line_index.position(self.source, start);
            let (line, character) = (position.line, position.character);
            let length_utf16 = self.source[start..end.min(self.source.len())]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>();

            let delta_line = line - prev_line;
            let delta_char = if delta_line == 0 {
                character - prev_char
            } else {
                character
            };

            #[allow(clippy::cast_possible_truncation)]
            {
                data.push(delta_line as u32);
                data.push(delta_char as u32);
                data.push(length_utf16 as u32);
            }
            data.push(token_type);
            data.push(modifiers);

            prev_line = line;
            prev_char = character;
        }

        SemanticTokens {
            data,
            syntax_valid: true,
        }
    }
}
