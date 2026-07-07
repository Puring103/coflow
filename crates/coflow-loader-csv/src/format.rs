/// Parse a CSV document into rows of cell strings. RFC 4180 quoting:
/// - Fields containing `,`, `"`, `\r`, or `\n` may be quoted.
/// - `""` inside a quoted field is an escaped quote.
/// - Lines end with `\n` or `\r\n`.
///
/// # Errors
///
/// Returns an error when a quoted field is unterminated or contains invalid
/// characters after the closing quote.
pub fn parse(source: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut after_closing_quote = false;
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' => {
                    if matches!(chars.peek(), Some('"')) {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                        after_closing_quote = true;
                    }
                }
                _ => field.push(ch),
            }
        } else {
            if after_closing_quote && !matches!(ch, ',' | '\n' | '\r') {
                return Err(format!(
                    "unexpected character `{ch}` after closing quoted field"
                ));
            }
            match ch {
                '"' => {
                    if !field.is_empty() {
                        return Err("unexpected `\"` mid-field".to_string());
                    }
                    in_quotes = true;
                }
                ',' => {
                    row.push(std::mem::take(&mut field));
                    after_closing_quote = false;
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_closing_quote = false;
                }
                '\r' => {
                    if matches!(chars.peek(), Some('\n')) {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                    after_closing_quote = false;
                }
                _ => field.push(ch),
            }
        }
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    if !field.is_empty() || !row.is_empty() || after_closing_quote {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// Serialize rows to a CSV string with RFC 4180 quoting. Each row is written
/// with a single trailing `\n`.
#[must_use]
pub fn write(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_cell(&mut out, cell);
        }
        out.push('\n');
    }
    out
}

fn write_cell(out: &mut String, value: &str) {
    let needs_quote = value
        .chars()
        .any(|ch| matches!(ch, ',' | '"' | '\n' | '\r'));
    if !needs_quote {
        out.push_str(value);
        return;
    }
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
            out.push('"');
        } else {
            out.push(ch);
        }
    }
    out.push('"');
}
