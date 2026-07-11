use crate::CFD_LOADER_DESCRIPTOR;
use coflow_api::{DecodedSourceOptions, Diagnostic, DiagnosticSet, Label, SourceLocation};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CfdSourceOptions;

pub(crate) fn decode_cfd_source_options(
    raw: &Value,
) -> Result<DecodedSourceOptions, DiagnosticSet> {
    let Some(options) = raw.as_object() else {
        if raw.is_null() {
            return Ok(decoded());
        }
        return Err(option_error([], "cfd source options must be an object"));
    };
    if let Some(key) = options.keys().next() {
        return Err(option_error(
            [key.as_str()],
            if key == "sheets" {
                "CFD sources cannot define `sheets`".to_string()
            } else {
                format!("unknown cfd source option `{key}`")
            },
        ));
    }
    Ok(decoded())
}

fn decoded() -> DecodedSourceOptions {
    DecodedSourceOptions::new(CFD_LOADER_DESCRIPTOR.id, CfdSourceOptions)
}

fn option_error<'a>(
    key_path: impl IntoIterator<Item = &'a str>,
    message: impl Into<String>,
) -> DiagnosticSet {
    DiagnosticSet::one(
        Diagnostic::error("CFD-SOURCE", "CFD", message).with_primary(Label {
            location: SourceLocation::ProjectConfig {
                path: std::path::PathBuf::new(),
                key_path: key_path.into_iter().map(str::to_string).collect(),
            },
            message: None,
        }),
    )
}
