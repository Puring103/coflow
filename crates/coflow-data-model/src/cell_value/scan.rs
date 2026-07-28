use super::diagnostics::{syntax, CellValueDiagnostics};

pub(super) fn split_top_level(
    input: &str,
    delimiter: char,
) -> Result<Vec<&str>, CellValueDiagnostics> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        state.step(ch)?;
        if state.is_top_level() && ch == delimiter {
            parts.push(input[start..index].trim());
            start = index + ch.len_utf8();
        }
    }
    state.finish()?;
    parts.push(input[start..].trim());
    Ok(parts)
}

pub(super) fn find_top_level_char(
    input: &str,
    target: char,
) -> Result<Option<usize>, CellValueDiagnostics> {
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        state.step(ch)?;
        if state.is_top_level() && ch == target {
            return Ok(Some(index));
        }
    }
    state.finish()?;
    Ok(None)
}

pub(super) fn strip_outer_pair(input: &str, open: char, close: char) -> Option<&str> {
    let input = input.trim();
    if !input.starts_with(open) {
        return None;
    }
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        if state.step(ch).is_err() {
            return None;
        }
        if ch == close && state.is_top_level() {
            let end = index + ch.len_utf8();
            return (end == input.len()).then_some(&input[open.len_utf8()..index]);
        }
    }
    None
}

pub(super) fn find_marker_open_brace(input: &str) -> Option<usize> {
    let mut state = ScanState::default();
    for (index, ch) in input.char_indices() {
        if ch == '{' && state.is_top_level() {
            return (index > 0).then_some(index);
        }
        if state.step(ch).is_err() {
            return None;
        }
    }
    None
}

#[derive(Debug, Default)]
struct ScanState {
    stack: Vec<char>,
    in_string: bool,
    escaped: bool,
}

impl ScanState {
    fn step(&mut self, ch: char) -> Result<(), CellValueDiagnostics> {
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' {
                self.escaped = true;
            } else if ch == '"' {
                self.in_string = false;
            }
            return Ok(());
        }

        match ch {
            '"' => self.in_string = true,
            '{' => self.stack.push('}'),
            '[' => self.stack.push(']'),
            '}' | ']' if self.stack.pop() != Some(ch) => {
                return Err(syntax("mismatched brackets"));
            }
            _ => {}
        }
        Ok(())
    }

    fn is_top_level(&self) -> bool {
        !self.in_string && self.stack.is_empty()
    }

    fn finish(self) -> Result<(), CellValueDiagnostics> {
        if self.in_string {
            return Err(syntax("unterminated string"));
        }
        if !self.stack.is_empty() {
            return Err(syntax("unclosed brackets"));
        }
        Ok(())
    }
}
