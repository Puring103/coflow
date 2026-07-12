use serde_json::Value;
use std::io::BufRead;

use crate::{position::LspPosition, MAX_LSP_CONTENT_LENGTH};

#[derive(Debug)]
pub(crate) struct TextRequest {
    pub(crate) uri: String,
    pub(crate) position: LspPosition,
}

impl TextRequest {
    pub(crate) fn from_params(params: &Value) -> Option<Self> {
        Some(Self {
            uri: text_document_uri(params)?,
            position: LspPosition::from_value(params.get("position")?)?,
        })
    }
}

pub(crate) fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, String> {
    let mut content_length = None;
    let mut saw_header = false;

    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|err| format!("failed to read LSP header: {err}"))?;

        if bytes == 0 {
            return if saw_header {
                Err("unexpected EOF while reading LSP headers".to_string())
            } else {
                Ok(None)
            };
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }

        saw_header = true;
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .map_err(|err| format!("invalid LSP Content-Length: {err}"))?,
                );
            }
        }
    }

    let content_length =
        content_length.ok_or_else(|| "missing LSP Content-Length header".to_string())?;
    if content_length > MAX_LSP_CONTENT_LENGTH {
        return Err(format!(
            "LSP Content-Length {content_length} exceeds maximum {MAX_LSP_CONTENT_LENGTH}"
        ));
    }
    let mut body = vec![0; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|err| format!("failed to read LSP body: {err}"))?;
    Ok(Some(body))
}

pub(crate) fn did_open_document(params: &Value) -> Option<(String, String, Option<i64>)> {
    let document = params.get("textDocument")?;
    Some((
        document.get("uri")?.as_str()?.to_string(),
        document.get("text")?.as_str()?.to_string(),
        document.get("version").and_then(Value::as_i64),
    ))
}

pub(crate) fn did_change_document(params: &Value) -> Option<(String, String, Option<i64>)> {
    let uri = text_document_uri(params)?;
    let version = params
        .get("textDocument")?
        .get("version")
        .and_then(Value::as_i64);
    let text = params
        .get("contentChanges")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|change| change.get("text").and_then(Value::as_str))?
        .to_string();
    Some((uri, text, version))
}

pub(crate) fn did_save_document(params: &Value) -> Option<(String, Option<String>)> {
    Some((
        text_document_uri(params)?,
        params
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
    ))
}

pub(crate) fn did_change_watched_files(params: &Value) -> Vec<String> {
    params
        .get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|change| change.get("uri").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

pub(crate) fn text_document_uri(params: &Value) -> Option<String> {
    params
        .get("textDocument")?
        .get("uri")?
        .as_str()
        .map(str::to_string)
}
