use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
struct NoDuplicateValue(Value);

#[derive(Debug, Clone)]
pub struct ProjectConfig {
    pub schema: SchemaConfig,
    /// Paths containing the only supported input format: CFD text.
    pub data: Vec<SourceConfig>,
    /// Code targets are the only published outputs.  The field is deliberately
    /// named `codegen` so data-export concepts cannot reappear in configuration.
    pub codegen: Vec<OutputConfig>,
    pub dimensions: BTreeMap<String, DimensionConfig>,
}

impl Serialize for ProjectConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("schema", &self.schema)?;
        map.serialize_entry("data", &self.data)?;
        map.serialize_entry("codegen", &self.codegen)?;
        if !self.dimensions.is_empty() {
            map.serialize_entry("dimensions", &self.dimensions)?;
        }
        map.end()
    }
}
impl<'de> Deserialize<'de> for ProjectConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let schema = fields
            .remove("schema")
            .ok_or_else(|| de::Error::missing_field("schema"))
            .and_then(|value| config_value(value).map_err(de::Error::custom))?;
        let data = fields
            .remove("data")
            .map(|value| data_value(value).map_err(de::Error::custom))
            .transpose()?
            .unwrap_or_default();
        let codegen_value = fields
            .remove("codegen")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?;
        let dimensions = fields
            .remove("dimensions")
            .map(|value| config_value(value).map_err(de::Error::custom))
            .transpose()?
            .unwrap_or_default();

        if let Some(key) = fields.keys().next() {
            return Err(de::Error::custom(format!("unknown field `{key}`")));
        }

        let codegen = match codegen_value {
            None => Vec::new(),
            Some(Value::Array(values)) => values
                .into_iter()
                .map(|value| serde_json::from_value(value).map_err(de::Error::custom))
                .collect::<Result<Vec<OutputConfig>, _>>()?,
            Some(Value::Object(value)) => {
                vec![serde_json::from_value(Value::Object(value)).map_err(de::Error::custom)?]
            }
            _ => return Err(de::Error::custom("codegen must be an object or a list")),
        };

        Ok(Self {
            schema,
            data,
            codegen,
            dimensions,
        })
    }
}

fn data_value(value: Value) -> Result<Vec<SourceConfig>, String> {
    match value {
        Value::String(path) => Ok(vec![SourceConfig::from_path(PathBuf::from(path))]),
        Value::Array(values) => values
            .into_iter()
            .map(|value| match value {
                Value::String(path) => Ok(SourceConfig::from_path(PathBuf::from(path))),
                _ => Err("data entries must be CFD paths".to_string()),
            })
            .collect(),
        _ => Err("data must be a path or a list of paths".to_string()),
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone)]
pub struct SchemaConfig {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) list_shape: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct SourceConfig {
    location: PathBuf,
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub language: String,
    pub dir: PathBuf,
    options: Value,
}

impl SourceConfig {
    #[must_use]
    pub fn from_path(path: PathBuf) -> Self {
        Self { location: path }
    }

    #[must_use]
    pub const fn location(&self) -> &PathBuf {
        &self.location
    }

    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        &self.location
    }
}

impl SchemaConfig {
    #[must_use]
    pub fn one(path: PathBuf) -> Self {
        Self {
            paths: vec![path],
            list_shape: false,
        }
    }

    #[must_use]
    pub const fn many(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            list_shape: true,
        }
    }

    #[must_use]
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(crate) const fn is_list_shape(&self) -> bool {
        self.list_shape
    }
}

impl Serialize for SchemaConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.list_shape || self.paths.len() != 1 {
            self.paths.serialize(serializer)
        } else {
            self.paths[0].serialize(serializer)
        }
    }
}

impl OutputConfig {
    #[must_use]
    pub const fn new(language: String, dir: PathBuf, options: Value) -> Self {
        Self {
            language,
            dir,
            options,
        }
    }

    #[must_use]
    pub const fn options(&self) -> &Value {
        &self.options
    }
}

impl Serialize for OutputConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2 + self.options.as_object().map_or(0, Map::len)))?;
        map.serialize_entry("language", &self.language)?;
        map.serialize_entry("dir", &self.dir)?;
        if let Some(options) = self.options.as_object() {
            for (key, value) in options {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for SchemaConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::String(path) => Ok(Self::one(PathBuf::from(path))),
            Value::Array(values) => values
                .into_iter()
                .map(path_value)
                .collect::<Result<Vec<_>, _>>()
                .map(Self::many)
                .map_err(de::Error::custom),
            _ => Err(de::Error::custom(
                "schema must be a path or a list of paths",
            )),
        }
    }
}

impl<'de> Deserialize<'de> for OutputConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut fields = no_duplicate_object(deserializer)?;
        let language = fields
            .remove("language")
            .map(string_field("codegen `language`"))
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("codegen target must set `language`"))?;
        let dir = fields
            .remove("dir")
            .map(path_value)
            .transpose()
            .map_err(de::Error::custom)?
            .ok_or_else(|| de::Error::custom("output must set `dir`"))?;
        let options = Value::Object(fields);
        Ok(Self {
            language,
            dir,
            options,
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

fn path_value(value: Value) -> Result<PathBuf, String> {
    let Value::String(value) = value else {
        return Err("source `path` must be a string".to_string());
    };
    Ok(PathBuf::from(value))
}
