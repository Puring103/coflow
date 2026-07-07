use coflow_cft::Span;
use serde_json::Value;

use crate::diagnostics::lsp_range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LspPosition {
    pub(crate) line: usize,
    pub(crate) character: usize,
}

impl LspPosition {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            line: usize::try_from(value.get("line")?.as_u64()?).ok()?,
            character: usize::try_from(value.get("character")?.as_u64()?).ok()?,
        })
    }
}

pub(crate) fn full_document_range(source: &str) -> Value {
    let end = position_from_byte(source, source.len());
    lsp_range(0, 0, end.line, end.character)
}

pub(crate) fn byte_range(source: &str, start: usize, end: usize) -> Value {
    let start = position_from_byte(source, start);
    let end = position_from_byte(source, end);
    lsp_range(start.line, start.character, end.line, end.character)
}

pub(crate) fn range_from_span(source: &str, span: Span) -> Value {
    byte_range(source, span.start, span.end.max(span.start + 1))
}

pub(crate) fn byte_offset_from_position(source: &str, position: LspPosition) -> usize {
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if line == position.line && character >= position.character {
            return byte_index;
        }
        if ch == '\n' {
            if line == position.line {
                return byte_index;
            }
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    source.len()
}

pub(crate) fn position_from_byte(source: &str, byte_offset: usize) -> LspPosition {
    let target = byte_offset.min(source.len());
    let mut line = 0;
    let mut character = 0;
    for (byte_index, ch) in source.char_indices() {
        if byte_index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16();
        }
    }
    LspPosition { line, character }
}
