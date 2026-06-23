//! Minimal RFC 4180 CSV reader/writer used by the localization module.
//!
//! Scope is intentionally narrow — we only need to round-trip the table
//! format documented in `docs/spec/13-localization.md` §4. Cell values are
//! treated as opaque strings; field-typed serialization is handled by the
//! caller.

use std::fmt::Write as _;

/// Parse a CSV document into rows of cell strings. RFC 4180 quoting:
/// - Fields containing `,`, `"`, `\r`, or `\n` may be quoted.
/// - `""` inside a quoted field is an escaped quote.
/// - Lines end with `\n` or `\r\n`.
pub fn parse(source: &str) -> Result<Vec<Vec<String>>, String> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
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
                    }
                }
                _ => field.push(ch),
            }
        } else {
            match ch {
                '"' => {
                    if !field.is_empty() {
                        return Err("unexpected `\"` mid-field".to_string());
                    }
                    in_quotes = true;
                }
                ',' => {
                    row.push(std::mem::take(&mut field));
                }
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                '\r' => {
                    if matches!(chars.peek(), Some('\n')) {
                        chars.next();
                    }
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(ch),
            }
        }
    }
    if in_quotes {
        return Err("unterminated quoted field".to_string());
    }
    // Trailing field without newline.
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

/// Serialize rows to a CSV string with RFC 4180 quoting. Each row is written
/// with a single trailing `\n`.
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
    let _ = write!(out, ""); // silence unused import on some platforms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let csv = "key,default,en\nItem/potion/name,治疗药水,Potion\n";
        let rows = parse(csv).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["key", "default", "en"]);
        assert_eq!(rows[1], vec!["Item/potion/name", "治疗药水", "Potion"]);
        let written = write(&rows);
        assert_eq!(written, csv);
    }

    #[test]
    fn handles_quoted_commas_and_quotes() {
        let csv = "k,v\nfoo,\"a, b\"\n\"with\"\"quote\",bar\n";
        let rows = parse(csv).unwrap();
        assert_eq!(rows[1], vec!["foo", "a, b"]);
        assert_eq!(rows[2], vec![r#"with"quote"#, "bar"]);
    }
}
