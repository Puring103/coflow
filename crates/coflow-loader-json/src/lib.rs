//! Schema-guided JSON source provider.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_api::{
    DecodedSourceOptions, Diagnostic, DiagnosticSet, Label, LoadedSource, ProbeResult,
    ProjectSourceRef, ProviderBundle, ProviderRegistrationError, ResolvedSource, SourceLoadContext,
    SourceLocation, SourceProvider, SourceProviderDescriptor, SourceResolveContext,
};
use coflow_cft::{CftSchema, CftValueType};
use coflow_data_model::{LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft, RecordOrigin};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const JSON_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "json",
    display_name: "JSON file",
    extensions: &["json"],
    option_keys: &["record_type"],
};

#[derive(Debug, Default, Clone, Copy)]
pub struct JsonLoader;

#[derive(Debug, Clone)]
struct JsonSourceOptions {
    record_type: Option<String>,
}

/// Declares the JSON source provider role.
///
/// # Errors
///
/// Returns an error if the JSON provider id is already present in the bundle.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_source_provider(JsonLoader)?;
    Ok(bundle)
}

impl SourceProvider for JsonLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &JSON_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some(JSON_LOADER_DESCRIPTOR.id) {
            return ProbeResult::certain();
        }
        if is_json_path(source.location.path()) {
            ProbeResult::likely()
        } else {
            ProbeResult::none()
        }
    }

    fn decode_options(&self, options: &Value) -> Result<DecodedSourceOptions, DiagnosticSet> {
        let Some(fields) = options.as_object() else {
            return Err(config_error("JSON source options must be an object"));
        };
        if let Some(key) = fields.keys().find(|key| key.as_str() != "record_type") {
            return Err(config_error(format!("unknown JSON source option `{key}`")));
        }
        let record_type = fields
            .get("record_type")
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| config_error("JSON `record_type` must be a string"))
            })
            .transpose()?;
        Ok(DecodedSourceOptions::new(
            JSON_LOADER_DESCRIPTOR.id,
            JsonSourceOptions { record_type },
        ))
    }

    fn resolve(
        &self,
        _ctx: SourceResolveContext<'_>,
        source: &ResolvedSource,
    ) -> Result<Vec<ResolvedSource>, DiagnosticSet> {
        if is_json_path(source.location.path()) {
            let mut resolved = source.clone();
            resolved.provider_id = JSON_LOADER_DESCRIPTOR.id.to_string();
            Ok(vec![resolved])
        } else {
            Err(file_error(
                source.location.path(),
                format!(
                    "JSON source `{}` must use the `.json` extension",
                    source.display_name
                ),
            ))
        }
    }

    fn load(
        &self,
        ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let options = source.options::<JsonSourceOptions>(JSON_LOADER_DESCRIPTOR.id)?;
        load_json_source(ctx.schema, source.location.path(), options)
            .map(|records| LoadedSource { records })
    }
}

fn load_json_source(
    schema: &CftSchema,
    path: &Path,
    options: &JsonSourceOptions,
) -> Result<Vec<LoadedRecordDraft>, DiagnosticSet> {
    let text = fs::read_to_string(path).map_err(|error| {
        file_error(
            path,
            format!("failed to read JSON source `{}`: {error}", path.display()),
        )
    })?;
    let source_map = JsonSourceMap::build(&text);
    let root =
        serde_json::from_str::<JsonNode>(&text).map_err(|error| parse_error(path, &error))?;
    let declared_type = options.record_type.clone().or_else(|| {
        path.file_stem()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    });
    let Some(declared_type) = declared_type else {
        return Err(file_error(
            path,
            "JSON source cannot determine its record type",
        ));
    };
    let Some(type_meta) = schema.resolve_type(&declared_type) else {
        return Err(file_error(
            path,
            format!("JSON source has unknown record type `{declared_type}`"),
        ));
    };
    if type_meta.is_abstract {
        return Err(file_error(
            path,
            format!("JSON source record type `{declared_type}` is abstract"),
        ));
    }
    let JsonNode::Array(items) = root else {
        return Err(file_error(
            path,
            "JSON source root must be an array of records",
        ));
    };
    let mut records = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        records.push(lower_record(
            schema,
            path,
            &source_map,
            &declared_type,
            item,
            index,
        )?);
    }
    Ok(records)
}

fn lower_record(
    schema: &CftSchema,
    path: &Path,
    source_map: &JsonSourceMap,
    declared_type: &str,
    node: JsonNode,
    index: usize,
) -> Result<LoadedRecordDraft, DiagnosticSet> {
    let JsonNode::Object(mut object) = node else {
        return Err(path_error(
            path,
            Some(source_map),
            format!("$[{index}]"),
            "record must be an object",
        ));
    };
    let key = take_string(&mut object, "id").ok_or_else(|| {
        path_error(
            path,
            Some(source_map),
            format!("$[{index}].id"),
            "record requires a string `id`",
        )
    })?;
    let actual_type =
        take_string(&mut object, "$type").unwrap_or_else(|| declared_type.to_string());
    if !schema.is_assignable(&actual_type, declared_type) {
        return Err(path_error(
            path,
            Some(source_map),
            format!("$[{index}].$type"),
            format!("type `{actual_type}` is not assignable to `{declared_type}`"),
        ));
    }
    let fields = lower_object_fields(
        schema,
        path,
        source_map,
        &actual_type,
        object,
        &format!("$[{index}]"),
    )?;
    Ok(
        LoadedRecordDraft::new(key, actual_type, fields).with_origin(RecordOrigin::File {
            path: path.to_path_buf(),
            span: None,
        }),
    )
}

fn lower_object_fields(
    schema: &CftSchema,
    path: &Path,
    source_map: &JsonSourceMap,
    actual_type: &str,
    object: BTreeMap<String, JsonNode>,
    json_path: &str,
) -> Result<BTreeMap<String, LoadedValueDraft>, DiagnosticSet> {
    let Some(type_meta) = schema.resolve_type(actual_type) else {
        return Err(path_error(
            path,
            Some(source_map),
            json_path,
            format!("unknown object type `{actual_type}`"),
        ));
    };
    let mut fields = BTreeMap::new();
    for (name, value) in object {
        let Some(field) = type_meta.field(&name) else {
            return Err(path_error(
                path,
                Some(source_map),
                format!("{json_path}.{name}"),
                format!("type `{actual_type}` has no field `{name}`"),
            ));
        };
        fields.insert(
            name.clone(),
            lower_value(
                schema,
                path,
                source_map,
                &field.value_type,
                value,
                &format!("{json_path}.{name}"),
            )?,
        );
    }
    Ok(fields)
}

fn lower_value(
    schema: &CftSchema,
    path: &Path,
    source_map: &JsonSourceMap,
    ty: &CftValueType,
    node: JsonNode,
    json_path: &str,
) -> Result<LoadedValueDraft, DiagnosticSet> {
    if let CftValueType::Nullable(inner) = ty {
        return if matches!(node, JsonNode::Null) {
            Ok(LoadedValueDraft::Null)
        } else {
            lower_value(schema, path, source_map, inner, node, json_path)
        };
    }
    match (ty, node) {
        (CftValueType::Int, JsonNode::Int(value)) => Ok(LoadedValueDraft::Int(value)),
        (CftValueType::Float, JsonNode::Int(value)) => Ok(LoadedValueDraft::Float(value as f64)),
        (CftValueType::Float, JsonNode::Float(value)) => Ok(LoadedValueDraft::Float(value)),
        (CftValueType::Bool, JsonNode::Bool(value)) => Ok(LoadedValueDraft::Bool(value)),
        (CftValueType::String, JsonNode::String(value)) => Ok(LoadedValueDraft::String(value)),
        (CftValueType::Enum(enum_name), JsonNode::String(value)) => {
            parse_enum_text(enum_name, &value).ok_or_else(|| {
                path_error(
                    path,
                    Some(source_map),
                    json_path,
                    format!("invalid `{enum_name}` value `{value}`"),
                )
            })
        }
        (CftValueType::Enum(enum_name), JsonNode::Int(value)) => Ok(LoadedValueDraft::EnumValue {
            enum_name: enum_name.to_string(),
            value,
        }),
        (CftValueType::RecordRef(_), JsonNode::String(value)) => {
            Ok(LoadedValueDraft::RecordRef(value))
        }
        (CftValueType::Array(inner), JsonNode::Array(values)) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                lower_value(
                    schema,
                    path,
                    source_map,
                    inner,
                    value,
                    &format!("{json_path}[{index}]"),
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map(LoadedValueDraft::Array),
        (CftValueType::Dict(key_ty, value_ty), JsonNode::Object(values)) => values
            .into_iter()
            .map(|(key, value)| {
                Ok((
                    lower_dict_key(key_ty, &key).ok_or_else(|| {
                        path_error(
                            path,
                            Some(source_map),
                            json_path,
                            format!("invalid dictionary key `{key}`"),
                        )
                    })?,
                    lower_value(
                        schema,
                        path,
                        source_map,
                        value_ty,
                        value,
                        &format!("{json_path}[{key:?}]"),
                    )?,
                ))
            })
            .collect::<Result<Vec<_>, DiagnosticSet>>()
            .map(LoadedValueDraft::Dict),
        (CftValueType::Object(declared), JsonNode::Object(mut object)) => {
            let actual = take_string(&mut object, "$type").unwrap_or_else(|| declared.to_string());
            if !schema.is_assignable(&actual, declared) {
                return Err(path_error(
                    path,
                    Some(source_map),
                    json_path,
                    format!("type `{actual}` is not assignable to `{declared}`"),
                ));
            }
            let fields = lower_object_fields(schema, path, source_map, &actual, object, json_path)?;
            Ok(LoadedValueDraft::Object {
                actual_type: (actual != declared.as_str()).then_some(actual),
                fields,
            })
        }
        (expected, actual) => Err(path_error(
            path,
            Some(source_map),
            json_path,
            format!("expected `{expected}`, found {}", actual.kind()),
        )),
    }
}

fn parse_enum_text(enum_name: &str, value: &str) -> Option<LoadedValueDraft> {
    if let Some(variant) = value.strip_prefix(&format!("{enum_name}.")) {
        return Some(LoadedValueDraft::EnumVariant {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
        });
    }
    let numeric = value
        .strip_prefix(&format!("{enum_name}("))?
        .strip_suffix(')')?
        .parse::<i64>()
        .ok()?;
    Some(LoadedValueDraft::EnumValue {
        enum_name: enum_name.to_string(),
        value: numeric,
    })
}

fn lower_dict_key(ty: &CftValueType, value: &str) -> Option<LoadedDictKeyDraft> {
    match ty.non_nullable() {
        CftValueType::String => Some(LoadedDictKeyDraft::String(value.to_string())),
        CftValueType::Int => value.parse().ok().map(LoadedDictKeyDraft::Int),
        CftValueType::Enum(enum_name) => match parse_enum_text(enum_name, value)? {
            LoadedValueDraft::EnumVariant { enum_name, variant } => {
                Some(LoadedDictKeyDraft::EnumVariant { enum_name, variant })
            }
            LoadedValueDraft::EnumValue { enum_name, value } => {
                Some(LoadedDictKeyDraft::EnumValue { enum_name, value })
            }
            _ => None,
        },
        _ => None,
    }
}

fn take_string(object: &mut BTreeMap<String, JsonNode>, key: &str) -> Option<String> {
    match object.remove(key) {
        Some(JsonNode::String(value)) => Some(value),
        _ => None,
    }
}

fn is_json_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension == "json")
}

fn config_error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("JSON-SOURCE", "JSON", message))
}

fn file_error(path: &Path, message: impl Into<String>) -> DiagnosticSet {
    path_error(path, None, "$", message)
}

fn path_error(
    path: &Path,
    source_map: Option<&JsonSourceMap>,
    json_path: impl Into<String>,
    message: impl Into<String>,
) -> DiagnosticSet {
    let json_path = json_path.into();
    let (start_line, start_character, end_line, end_character) = source_map
        .and_then(|source_map| source_map.location(&json_path))
        .unwrap_or((0, 0, 0, 1));
    DiagnosticSet::one(
        Diagnostic::error("JSON-SOURCE", "JSON", message).with_primary(Label {
            location: SourceLocation::FileSpan {
                path: PathBuf::from(path),
                start_line,
                start_character,
                end_line,
                end_character,
            },
            message: Some(json_path),
        }),
    )
}

fn parse_error(path: &Path, error: &serde_json::Error) -> DiagnosticSet {
    let line = error.line().saturating_sub(1);
    let character = error.column().saturating_sub(1);
    DiagnosticSet::one(
        Diagnostic::error(
            "JSON-SOURCE",
            "JSON",
            format!("failed to parse JSON source `{}`: {error}", path.display()),
        )
        .with_primary(Label {
            location: SourceLocation::FileSpan {
                path: path.to_path_buf(),
                start_line: line,
                start_character: character,
                end_line: line,
                end_character: character.saturating_add(1),
            },
            message: Some("$".to_string()),
        }),
    )
}

struct JsonSourceMap {
    text: String,
    line_starts: Vec<usize>,
    spans: BTreeMap<String, (usize, usize)>,
}

impl JsonSourceMap {
    fn build(text: &str) -> Self {
        let mut parser = JsonSpanParser {
            text,
            offset: 0,
            spans: BTreeMap::new(),
        };
        let _ = parser.parse_value("$");
        let mut line_starts = vec![0];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Self {
            text: text.to_string(),
            line_starts,
            spans: parser.spans,
        }
    }

    fn location(&self, path: &str) -> Option<(usize, usize, usize, usize)> {
        let mut candidate = path;
        let (start, end) = loop {
            if let Some(span) = self.spans.get(candidate) {
                break *span;
            }
            let dot = candidate.rfind('.');
            let bracket = candidate.rfind('[');
            let truncate = match (dot, bracket) {
                (Some(left), Some(right)) => left.max(right),
                (Some(index), None) | (None, Some(index)) => index,
                (None, None) => return None,
            };
            candidate = &candidate[..truncate];
        };
        let (start_line, start_character) = self.line_character(start);
        let (end_line, end_character) = self.line_character(end);
        Some((start_line, start_character, end_line, end_character))
    }

    fn line_character(&self, offset: usize) -> (usize, usize) {
        let line = self
            .line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        let line_start = self.line_starts[line];
        let character = self.text[line_start..offset.min(self.text.len())]
            .chars()
            .count();
        (line, character)
    }
}

struct JsonSpanParser<'a> {
    text: &'a str,
    offset: usize,
    spans: BTreeMap<String, (usize, usize)>,
}

impl JsonSpanParser<'_> {
    fn parse_value(&mut self, path: &str) -> Option<()> {
        self.skip_whitespace();
        let start = self.offset;
        match self.peek()? {
            b'{' => self.parse_object(path)?,
            b'[' => self.parse_array(path)?,
            b'"' => {
                self.parse_string()?;
            }
            _ => {
                while self.peek().is_some_and(|byte| {
                    !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}')
                }) {
                    self.offset += 1;
                }
            }
        }
        self.spans.insert(path.to_string(), (start, self.offset));
        Some(())
    }

    fn parse_object(&mut self, path: &str) -> Option<()> {
        self.offset += 1;
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.offset += 1;
            return Some(());
        }
        loop {
            self.skip_whitespace();
            let key_start = self.offset;
            self.parse_string()?;
            let key = serde_json::from_str::<String>(&self.text[key_start..self.offset]).ok()?;
            self.skip_whitespace();
            if self.peek()? != b':' {
                return None;
            }
            self.offset += 1;
            let field_path = format!("{path}.{key}");
            self.parse_value(&field_path)?;
            if let Some(span) = self.spans.get(&field_path).copied() {
                self.spans.insert(format!("{path}[{key:?}]"), span);
            }
            self.skip_whitespace();
            match self.peek()? {
                b',' => self.offset += 1,
                b'}' => {
                    self.offset += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }

    fn parse_array(&mut self, path: &str) -> Option<()> {
        self.offset += 1;
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.offset += 1;
            return Some(());
        }
        let mut index = 0usize;
        loop {
            self.parse_value(&format!("{path}[{index}]"))?;
            index += 1;
            self.skip_whitespace();
            match self.peek()? {
                b',' => self.offset += 1,
                b']' => {
                    self.offset += 1;
                    return Some(());
                }
                _ => return None,
            }
        }
    }

    fn parse_string(&mut self) -> Option<()> {
        if self.peek()? != b'"' {
            return None;
        }
        self.offset += 1;
        while let Some(byte) = self.peek() {
            self.offset += 1;
            match byte {
                b'"' => return Some(()),
                b'\\' => {
                    self.offset += 1;
                }
                _ => {}
            }
        }
        None
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.offset += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.offset).copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum JsonNode {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Self>),
    Object(BTreeMap<String, Self>),
}

impl JsonNode {
    fn kind(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }
}

impl<'de> Deserialize<'de> for JsonNode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonNodeVisitor)
    }
}

struct JsonNodeVisitor;

impl<'de> Visitor<'de> for JsonNodeVisitor {
    type Value = JsonNode;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(JsonNode::Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(JsonNode::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonNode::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonNode::Int(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value)
            .map(JsonNode::Int)
            .map_err(|_| E::custom("JSON integer exceeds i64::MAX"))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(JsonNode::Float(value))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(JsonNode::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(JsonNode::String(value))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element()? {
            values.push(value);
        }
        Ok(JsonNode::Array(values))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, JsonNode>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!(
                    "duplicate JSON property `{key}`"
                )));
            }
        }
        Ok(JsonNode::Object(values))
    }
}

#[cfg(test)]
mod tests {
    use super::{load_json_source, JsonSourceOptions};
    use coflow_api::SourceLocation;
    use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
    use coflow_data_model::{CfdDataModel, CfdValue};
    use std::fs;
    use tempfile::tempdir;

    type TestResult = Result<(), String>;

    #[test]
    fn loads_export_shape_and_symbolic_enums() -> TestResult {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "enum Rarity { Common = 0, Rare = 7, } type Item { name: string; rarity: Rarity; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default())
            .map_err(|error| format!("schema: {error:?}"))?;
        let dir = tempdir().map_err(|error| error.to_string())?;
        let path = dir.path().join("Item.json");
        fs::write(
            &path,
            r#"[{"id":"sword","name":"Sword","rarity":"Rarity.Rare"}]"#,
        )
        .map_err(|error| error.to_string())?;
        let records = load_json_source(&schema, &path, &JsonSourceOptions { record_type: None })
            .map_err(|error| format!("load: {error:?}"))?;
        let mut builder = CfdDataModel::builder(&schema);
        for record in records {
            builder.add_loaded_record(record);
        }
        let model = builder
            .build()
            .map_err(|error| format!("model: {error:?}"))?;
        let record_id = model
            .record_by_type_key("Item", "sword")
            .ok_or_else(|| "missing record".to_string())?;
        let record = model
            .record(record_id)
            .ok_or_else(|| "missing record value".to_string())?;
        assert!(matches!(record.field("rarity"), Some(CfdValue::Enum(value)) if value.value == 7));
        Ok(())
    }

    #[test]
    fn rejects_duplicate_properties() -> TestResult {
        let error = serde_json::from_str::<super::JsonNode>(r#"{"id":"a","id":"b"}"#)
            .expect_err("duplicate property");
        assert!(error.to_string().contains("duplicate JSON property `id`"));
        Ok(())
    }

    #[test]
    fn semantic_diagnostics_point_to_the_json_value() -> TestResult {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "type Item { count: int; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default())
            .map_err(|error| format!("schema: {error:?}"))?;
        let dir = tempdir().map_err(|error| error.to_string())?;
        let path = dir.path().join("Item.json");
        fs::write(
            &path,
            "[\n  {\n    \"id\": \"x\",\n    \"count\": \"bad\"\n  }\n]",
        )
        .map_err(|error| error.to_string())?;

        let error = load_json_source(&schema, &path, &JsonSourceOptions { record_type: None })
            .expect_err("invalid count");
        let primary = error.diagnostics[0].primary.as_ref().ok_or("primary")?;
        assert_eq!(primary.message.as_deref(), Some("$[0].count"));
        assert!(matches!(
            primary.location,
            SourceLocation::FileSpan {
                start_line: 3,
                start_character: 13,
                end_line: 3,
                end_character: 18,
                ..
            }
        ));
        Ok(())
    }
}
