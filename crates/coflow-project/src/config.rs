use coflow_api::SourceLocationSpec;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
struct NoDuplicateValue(Value);

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub schema: SchemaConfig,
    pub sources: Vec<SourceConfig>,
    pub outputs: OutputsConfig,
    pub dimensions: BTreeMap<String, DimensionConfig>,
}
impl<'de> Deserialize<'de> for ProjectConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        if fields.contains_key("localization") {
            return Err(de::Error::custom(
                "PROJECT-CONFIG-LOCALIZATION-REMOVED: `localization` has been removed; use `dimensions.language` instead.",
            ));
        }

        let schema = fields
            .remove("schema")
            .ok_or_else(|| de::Error::missing_field("schema"))
            .and_then(|value| config_value(value).map_err(de::Error::custom))?;
        let sources = fields
            .remove("sources")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?
            .unwrap_or_default();
        let outputs = fields
            .remove("outputs")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?
            .unwrap_or_default();
        let dimensions = fields
            .remove("dimensions")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?
            .unwrap_or_default();

        if let Some(key) = fields.keys().next() {
            return Err(de::Error::custom(format!("unknown field `{key}`")));
        }

        Ok(Self {
            schema,
            sources,
            outputs,
            dimensions,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct DimensionConfig {
    #[serde(default)]
    pub variants: Vec<String>,
    pub out_dir: Option<PathBuf>,
    /// Human-readable label for this dimension. The editor falls back to a
    /// built-in mapping (e.g. `"language" → "本地化"`) when missing, and to
    /// the raw dimension name otherwise.
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SchemaConfig {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub source_type: Option<String>,
    pub location: SourceLocationSpec,
    pub options: Value,
}

#[derive(Debug, Clone, Default)]
pub struct OutputsConfig {
    targets: Vec<OutputTargetConfig>,
    legacy_shape: bool,
}

#[derive(Debug, Clone)]
pub struct OutputTargetConfig {
    pub data: OutputConfig,
    pub code: Option<OutputConfig>,
    pub loader: Option<LoaderConfig>,
}

#[derive(Debug, Clone)]
pub struct LoaderConfig {
    pub loader_type: String,
    options: Value,
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub output_type: String,
    pub dir: PathBuf,
    pub options: Value,
}

impl SourceConfig {
    #[must_use]
    pub const fn location(&self) -> &SourceLocationSpec {
        &self.location
    }

    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl OutputConfig {
    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl OutputsConfig {
    #[must_use]
    pub fn new(targets: Vec<OutputTargetConfig>) -> Self {
        Self {
            targets,
            legacy_shape: false,
        }
    }

    #[must_use]
    pub fn targets(&self) -> &[OutputTargetConfig] {
        &self.targets
    }

    pub fn targets_mut(&mut self) -> &mut [OutputTargetConfig] {
        &mut self.targets
    }

    #[must_use]
    pub const fn is_legacy_shape(&self) -> bool {
        self.legacy_shape
    }
}

impl OutputTargetConfig {
    #[must_use]
    pub fn loader_options(&self) -> &Value {
        static EMPTY_OPTIONS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        self.loader.as_ref().map_or_else(
            || EMPTY_OPTIONS.get_or_init(|| Value::Object(Map::new())),
            LoaderConfig::options,
        )
    }
}

impl LoaderConfig {
    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl<'de> Deserialize<'de> for SourceConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        reject_removed_source_fields(&fields).map_err(de::Error::custom)?;
        if fields.contains_key("url") {
            return Err(de::Error::custom("unknown field `url`"));
        }
        let source_type = fields
            .remove("type")
            .map(string_field("source `type`"))
            .transpose()
            .map_err(de::Error::custom)?;
        let path = fields.remove("path");
        let path = path.ok_or_else(|| de::Error::custom("source must set `path`"))?;
        let location = SourceLocationSpec::Path(path_value(path).map_err(de::Error::custom)?);
        let options = Value::Object(fields);
        Ok(Self {
            source_type,
            location,
            options,
        })
    }
}

impl<'de> Deserialize<'de> for OutputConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let output_type = fields
            .remove("type")
            .map(string_field("output `type`"))
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("output must set `type`"))?;
        let dir = fields
            .remove("dir")
            .map(path_value)
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("output must set `dir`"))?;
        let options = Value::Object(fields);
        Ok(Self {
            output_type,
            dir,
            options,
        })
    }
}

impl<'de> Deserialize<'de> for OutputsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::Object(mut fields) => {
                let data = fields
                    .remove("data")
                    .map(|value| config_value(value).map_err(de::Error::custom))
                    .transpose()?;
                let code = fields
                    .remove("code")
                    .map(|value| config_value(value).map_err(de::Error::custom))
                    .transpose()?;
                let loader = fields
                    .remove("loader")
                    .map(|value| config_value(value).map_err(de::Error::custom))
                    .transpose()?;
                if let Some(key) = fields.keys().next() {
                    return Err(de::Error::custom(format!("unknown field `{key}`")));
                }
                let targets = match data {
                    Some(data) => vec![OutputTargetConfig { data, code, loader }],
                    None if code.is_some() || loader.is_some() => {
                        return Err(de::Error::custom(
                            "coflow.yaml missing outputs.data; legacy outputs.code and outputs.loader require outputs.data",
                        ));
                    }
                    None => Vec::new(),
                };
                Ok(Self {
                    targets,
                    legacy_shape: true,
                })
            }
            Value::Array(values) => values
                .into_iter()
                .map(config_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::new)
                .map_err(de::Error::custom),
            _ => Err(de::Error::custom(
                "outputs must be an object or a list of output targets",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for OutputTargetConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let data = fields
            .remove("data")
            .ok_or_else(|| de::Error::missing_field("data"))
            .and_then(|value| config_value(value).map_err(de::Error::custom))?;
        let code = fields
            .remove("code")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?;
        let loader = fields
            .remove("loader")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?;
        if let Some(key) = fields.keys().next() {
            return Err(de::Error::custom(format!("unknown field `{key}`")));
        }
        Ok(Self { data, code, loader })
    }
}

impl<'de> Deserialize<'de> for LoaderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let loader_type = fields
            .remove("type")
            .map(string_field("loader `type`"))
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::missing_field("type"))?;
        Ok(Self {
            loader_type,
            options: Value::Object(fields),
        })
    }
}

fn no_duplicate_object<'de, D>(deserializer: D) -> Result<Map<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    let NoDuplicateValue(Value::Object(fields)) = NoDuplicateValue::deserialize(deserializer)?
    else {
        return Err(de::Error::custom("expected an object"));
    };
    Ok(fields)
}

impl<'de> Deserialize<'de> for NoDuplicateValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(NoDuplicateValueVisitor)
    }
}

struct NoDuplicateValueVisitor;

impl<'de> Visitor<'de> for NoDuplicateValueVisitor {
    type Value = NoDuplicateValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a YAML value without duplicate mapping keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| E::custom("non-finite numbers are not supported"))?;
        Ok(NoDuplicateValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(NoDuplicateValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        NoDuplicateValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(NoDuplicateValue(value)) = seq.next_element()? {
            values.push(value);
        }
        Ok(NoDuplicateValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
            let NoDuplicateValue(value) = map.next_value()?;
            object.insert(key, value);
        }
        Ok(NoDuplicateValue(Value::Object(object)))
    }
}

fn string_field(label: &'static str) -> impl FnOnce(Value) -> Result<String, String> {
    move |value| {
        let Value::String(value) = value else {
            return Err(format!("{label} must be a string"));
        };
        Ok(value)
    }
}

fn config_value<T>(value: Value) -> Result<T, String>
where
    T: de::DeserializeOwned,
{
    serde_json::from_value(value).map_err(|err| err.to_string())
}

fn reject_removed_source_fields(fields: &Map<String, Value>) -> Result<(), String> {
    for key in ["file", "dir"] {
        if fields.contains_key(key) {
            return Err(format!("unknown field `{key}`"));
        }
    }
    Ok(())
}

fn path_value(value: Value) -> Result<PathBuf, String> {
    let Value::String(value) = value else {
        return Err("source `path` must be a string".to_string());
    };
    Ok(PathBuf::from(value))
}
