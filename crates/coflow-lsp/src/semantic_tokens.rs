use coflow_cft::Span;

use crate::position::position_from_byte;

pub(crate) const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace",
    "type",
    "enum",
    "enumMember",
    "property",
    "variable",
    "function",
    "keyword",
    "number",
    "string",
    "comment",
    "operator",
    "decorator",
    "parameter",
];
pub(crate) const SEMANTIC_TOKEN_MODIFIERS: &[&str] =
    &["declaration", "reference", "path", "record", "schema"];

#[cfg(test)]
pub(crate) const SEM_NAMESPACE: u32 = 0;
pub(crate) const SEM_TYPE: u32 = 1;
pub(crate) const SEM_ENUM: u32 = 2;
pub(crate) const SEM_ENUM_MEMBER: u32 = 3;
pub(crate) const SEM_PROPERTY: u32 = 4;
pub(crate) const SEM_VARIABLE: u32 = 5;
pub(crate) const SEM_FUNCTION: u32 = 6;
pub(crate) const SEM_KEYWORD: u32 = 7;
pub(crate) const SEM_NUMBER: u32 = 8;
pub(crate) const SEM_STRING: u32 = 9;
pub(crate) const SEM_COMMENT: u32 = 10;
pub(crate) const SEM_OPERATOR: u32 = 11;
pub(crate) const SEM_DECORATOR: u32 = 12;
pub(crate) const SEM_PARAMETER: u32 = 13;

pub(crate) const MOD_DECLARATION: u32 = 1 << 0;
pub(crate) const MOD_REFERENCE: u32 = 1 << 1;
pub(crate) const MOD_PATH: u32 = 1 << 2;
#[cfg(test)]
pub(crate) const MOD_RECORD: u32 = 1 << 3;
pub(crate) const MOD_SCHEMA: u32 = 1 << 4;

#[derive(Clone)]
pub(crate) struct RawSemanticToken {
    pub(crate) line: usize,
    pub(crate) character: usize,
    pub(crate) length: usize,
    pub(crate) token_type: u32,
    pub(crate) token_modifiers: u32,
}

pub(crate) fn push_semantic_span(
    source: &str,
    span: Span,
    token_type: u32,
    token_modifiers: u32,
    tokens: &mut Vec<RawSemanticToken>,
) {
    if span.end <= span.start {
        return;
    }
    let start = position_from_byte(source, span.start);
    let end = position_from_byte(source, span.end);
    if start.line != end.line || end.character <= start.character {
        return;
    }
    tokens.push(RawSemanticToken {
        line: start.line,
        character: start.character,
        length: end.character - start.character,
        token_type,
        token_modifiers,
    });
}

pub(crate) fn push_semantic_span_plain(
    source: &str,
    span: Span,
    token_type: u32,
    tokens: &mut Vec<RawSemanticToken>,
) {
    push_semantic_span(source, span, token_type, 0, tokens);
}

pub(crate) fn encode_semantic_tokens(mut tokens: Vec<RawSemanticToken>) -> Vec<u32> {
    tokens.sort_by_key(|token| (token.line, token.character, token.length));
    let mut deduped = Vec::new();
    let mut last_end = (0, 0);
    let mut has_last = false;
    for token in tokens {
        if has_last && (token.line, token.character) < last_end {
            continue;
        }
        last_end = (token.line, token.character + token.length);
        has_last = true;
        deduped.push(token);
    }

    let mut data = Vec::with_capacity(deduped.len() * 5);
    let mut previous_line = 0;
    let mut previous_character = 0;
    for token in deduped {
        let delta_line = token.line - previous_line;
        let delta_start = if delta_line == 0 {
            token.character - previous_character
        } else {
            token.character
        };
        data.push(usize_to_u32_saturating(delta_line));
        data.push(usize_to_u32_saturating(delta_start));
        data.push(usize_to_u32_saturating(token.length));
        data.push(token.token_type);
        data.push(token.token_modifiers);
        previous_line = token.line;
        previous_character = token.character;
    }
    data
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
