use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let (authority, path) = rest.strip_prefix('/').map_or_else(
        || {
            rest.split_once('/').map_or_else(
                || (rest, String::new()),
                |(authority, path)| (authority, format!("/{path}")),
            )
        },
        |stripped| ("", format!("/{stripped}")),
    );
    let authority = percent_decode(authority)?;
    let decoded = percent_decode(&path)?;
    if decoded.is_empty() {
        return None;
    }
    let path = if cfg!(windows) {
        if authority.is_empty() || authority.eq_ignore_ascii_case("localhost") {
            let without_leading_slash = if decoded.len() >= 3
                && decoded.as_bytes()[0] == b'/'
                && decoded.as_bytes()[2] == b':'
            {
                &decoded[1..]
            } else {
                decoded.as_str()
            };
            without_leading_slash.replace('/', "\\")
        } else {
            format!(r"\\{}{}", authority, decoded.replace('/', r"\"))
        }
    } else if authority.is_empty() || authority == "localhost" {
        decoded
    } else {
        format!("//{authority}{decoded}")
    };
    Some(PathBuf::from(path))
}

pub fn path_to_file_uri(path: &Path) -> String {
    let mut path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string();
        }
    }
    if cfg!(windows) && path.len() >= 2 && path.as_bytes()[1] == b':' {
        path.insert(0, '/');
    }
    format!("file://{}", percent_encode_uri_path(&path))
}

pub fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(out).ok()
}

pub const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_encode_uri_path(value: &str) -> String {
    let mut out = String::new();

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                let _ = write!(out, "{byte:02X}");
            }
        }
    }

    out
}
