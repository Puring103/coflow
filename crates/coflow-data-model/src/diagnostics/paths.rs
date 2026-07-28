use crate::ingest::LoadedDictKeyDraft;
use crate::model::CfdDictKey;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, serde::Serialize, serde::Deserialize,
)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct CfdPath {
    pub segments: Vec<CfdPathSegment>,
}

impl CfdPath {
    #[must_use]
    pub fn root() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::Field(name.into()));
        self
    }

    #[must_use]
    pub fn index(mut self, index: usize) -> Self {
        self.segments.push(CfdPathSegment::Index(index));
        self
    }

    #[must_use]
    pub fn dict_key(mut self, key: impl Into<String>) -> Self {
        self.segments.push(CfdPathSegment::DictKey(key.into()));
        self
    }

    #[must_use]
    pub fn dict_key_value(mut self, key: &CfdDictKey) -> Self {
        self.segments
            .push(CfdPathSegment::DictKey(format_cfd_dict_key(key)));
        self
    }

    #[must_use]
    pub fn dict_key_input(mut self, key: &LoadedDictKeyDraft) -> Self {
        self.segments
            .push(CfdPathSegment::DictKey(format_input_dict_key(key)));
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CfdPathSegment {
    Field(String),
    Index(usize),
    DictKey(String),
}

#[must_use]
pub fn format_cfd_dict_key(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => quote_cfd_string(value),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value.variant.as_deref().map_or_else(
            || format!("{}({})", value.enum_name, value.value),
            |variant| format!("{}.{}", value.enum_name, variant),
        ),
    }
}

fn format_input_dict_key(key: &LoadedDictKeyDraft) -> String {
    match key {
        LoadedDictKeyDraft::String(value) => quote_cfd_string(value),
        LoadedDictKeyDraft::Int(value) => value.to_string(),
        LoadedDictKeyDraft::EnumVariant { enum_name, variant } => {
            format!("{enum_name}.{variant}")
        }
    }
}

fn quote_cfd_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
