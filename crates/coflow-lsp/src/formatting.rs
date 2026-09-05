use serde_json::{json, Value};
use similar::{DiffTag, TextDiff};

use super::position::byte_range;


pub(crate) fn formatting_edits(source: &str, formatted: &str) -> Vec<Value> {
    if source == formatted {
        return Vec::new();
    }

    let source_offsets = character_offsets(source);
    let formatted_offsets = character_offsets(formatted);
    TextDiff::from_chars(source, formatted)
        .ops()
        .iter()
        .filter(|operation| operation.tag() != DiffTag::Equal)
        .filter_map(|operation| {
            let old = operation.old_range();
            let new = operation.new_range();
            let old_start = source_offsets[old.start];
            let old_end = source_offsets[old.end];
            let new_start = formatted_offsets[new.start];
            let new_end = formatted_offsets[new.end];
            let (start, end, new_text) =
                shrink_edit(source, old_start, old_end, &formatted[new_start..new_end]);
            (start != end || !new_text.is_empty()).then(|| {
                json!({
                    "range": byte_range(source, start, end),
                    "newText": new_text,
                })
            })
        })
        .collect()
}

fn character_offsets(source: &str) -> Vec<usize> {
    let mut offsets = source
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    offsets.push(source.len());
    offsets
}

fn shrink_edit<'a>(
    source: &str,
    mut start: usize,
    mut end: usize,
    mut new_text: &'a str,
) -> (usize, usize, &'a str) {
    let old_text = &source[start..end];
    let prefix = common_prefix_bytes(old_text, new_text);
    start += prefix;
    new_text = &new_text[prefix..];

    let old_text = &source[start..end];
    let suffix = common_suffix_bytes(old_text, new_text);
    end -= suffix;
    new_text = &new_text[..new_text.len() - suffix];
    (start, end, new_text)
}

fn common_prefix_bytes(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum()
}

fn common_suffix_bytes(left: &str, right: &str) -> usize {
    left.chars()
        .rev()
        .zip(right.chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum()
}
