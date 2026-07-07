//! Shared schema-aware exporter traversal for Coflow data models.
//!
//! This crate walks an already-built [`CfdDataModel`] according to the compiled
//! CFT schema and delegates concrete value construction to an [`ExportEncoder`].

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

use std::collections::BTreeMap;
use std::fmt;

use coflow_cft::{CftContainer, CftFieldMeta, CftSchemaTypeRef, CftSchemaView};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdObject, CfdRecord, CfdTable, CfdValue};

/// Constructs output values for a concrete export format.
///
/// The traversal controls schema order and type semantics; encoders only build
/// values for their target format.
pub trait ExportEncoder {
    type Error: fmt::Display;
    type Value;

    /// Encodes a null value.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode null.
    fn null(&mut self) -> Result<Self::Value, Self::Error>;

    /// Encodes a boolean value.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the value.
    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error>;

    /// Encodes an integer value.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the value.
    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error>;

    /// Encodes a floating-point value.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the value.
    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error>;

    /// Encodes a string value.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the value.
    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error>;

    /// Encodes an array from already-encoded items.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the array.
    fn array(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, Self::Error>;

    /// Encodes a map from already-encoded entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the target format cannot encode the map.
    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub message: String,
}

impl ExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ExportError {}

/// Exports every concrete table from the data model.
///
/// The returned map key is the CFT type/table name. Values are arrays whose
/// order follows the table's original record order in the data model.
///
/// # Errors
///
/// Returns an error when a model record or field cannot be matched back to the
/// compiled schema, or when the encoder rejects a value.
pub fn export_model_with_encoder<E>(
    schema: &CftContainer,
    model: &CfdDataModel,
    encoder: &mut E,
) -> Result<BTreeMap<String, E::Value>, ExportError>
where
    E: ExportEncoder,
{
    Exporter::new(schema, model, encoder).export()
}

struct Exporter<'a, E> {
    schema: CftSchemaView,
    model: &'a CfdDataModel,
    encoder: &'a mut E,
}

impl<'a, E> Exporter<'a, E>
where
    E: ExportEncoder,
{
    fn new(schema: &'a CftContainer, model: &'a CfdDataModel, encoder: &'a mut E) -> Self {
        Self {
            schema: CftSchemaView::new(schema),
            model,
            encoder,
        }
    }

    fn export(mut self) -> Result<BTreeMap<String, E::Value>, ExportError> {
        let mut out = BTreeMap::new();
        let table_names = self
            .schema
            .types
            .values()
            .filter(|schema_type| !schema_type.is_abstract)
            .map(|schema_type| schema_type.name.clone())
            .collect::<Vec<_>>();
        for table_name in table_names {
            let table = self.model.table(&table_name);
            let value = if let Some(table) = table {
                self.encode_table(table)?
            } else {
                self.encoder.array(Vec::new()).map_err(encoder_error)?
            };
            out.insert(table_name, value);
        }
        Ok(out)
    }

    fn encode_table(&mut self, table: &CfdTable) -> Result<E::Value, ExportError> {
        let mut records = Vec::with_capacity(table.records.len());
        for record_id in &table.records {
            let record = self.model.record(*record_id).ok_or_else(|| {
                ExportError::new(format!(
                    "table `{}` references missing record `{record_id}`",
                    table.type_name
                ))
            })?;
            records.push(self.encode_record(&table.type_name, record, TypeTagMode::Never)?);
        }
        self.encoder.array(records).map_err(encoder_error)
    }

    fn encode_record(
        &mut self,
        declared_type: &str,
        record: &CfdRecord,
        tag_mode: TypeTagMode,
    ) -> Result<E::Value, ExportError> {
        let mut entries = Vec::new();
        if tag_mode == TypeTagMode::Never {
            entries.push((
                "id".to_string(),
                self.encoder.string(record.key()).map_err(encoder_error)?,
            ));
        }
        entries.extend(self.encode_object_entries(declared_type, &record.object, tag_mode)?);
        self.encoder.map(entries).map_err(encoder_error)
    }

    fn encode_object(
        &mut self,
        declared_type: &str,
        object: &CfdObject,
        tag_mode: TypeTagMode,
    ) -> Result<E::Value, ExportError> {
        let entries = self.encode_object_entries(declared_type, object, tag_mode)?;
        self.encoder.map(entries).map_err(encoder_error)
    }

    fn encode_object_entries(
        &mut self,
        declared_type: &str,
        object: &CfdObject,
        tag_mode: TypeTagMode,
    ) -> Result<Vec<(String, E::Value)>, ExportError> {
        let mut entries = Vec::new();
        if tag_mode == TypeTagMode::WhenPolymorphic
            && self.schema.range_is_polymorphic(declared_type)
        {
            entries.push((
                "$type".to_string(),
                self.encoder
                    .string(object.actual_type())
                    .map_err(encoder_error)?,
            ));
        }

        let fields = self
            .schema
            .full_fields(object.actual_type())
            .ok_or_else(|| {
                ExportError::new(format!(
                    "unknown CFT type `{}` during export",
                    object.actual_type()
                ))
            })?
            .to_vec();
        for field in &fields {
            let value = object.fields().get(&field.name).ok_or_else(|| {
                ExportError::new(format!(
                    "record `{}` is missing field `{}`",
                    object.actual_type(),
                    field.name
                ))
            })?;
            let encoded = self.encode_field(field, value)?;
            entries.push((field.name.clone(), encoded));
        }
        Ok(entries)
    }

    fn encode_field(
        &mut self,
        field: &CftFieldMeta,
        value: &CfdValue,
    ) -> Result<E::Value, ExportError> {
        self.encode_value(&field.ty_ref, value)
    }

    fn encode_value(
        &mut self,
        declared_type: &CftSchemaTypeRef,
        value: &CfdValue,
    ) -> Result<E::Value, ExportError> {
        if let CftSchemaTypeRef::Nullable(inner) = declared_type {
            return match value {
                CfdValue::Null => self.encoder.null().map_err(encoder_error),
                other => self.encode_value(inner, other),
            };
        }

        match value {
            CfdValue::Null => self.encoder.null().map_err(encoder_error),
            CfdValue::Bool(value) => self.encoder.bool(*value).map_err(encoder_error),
            CfdValue::Int(value) => self.encoder.int(*value).map_err(encoder_error),
            CfdValue::Float(value) => {
                if value.is_finite() {
                    self.encoder.float(*value).map_err(encoder_error)
                } else {
                    Err(ExportError::new("cannot export non-finite float"))
                }
            }
            CfdValue::String(value) => self.encoder.string(value).map_err(encoder_error),
            CfdValue::Enum(value) => self.encoder.int(value.value).map_err(encoder_error),
            CfdValue::Object(object) => {
                let type_name = match declared_type {
                    CftSchemaTypeRef::Named(type_name) => type_name,
                    other => {
                        return Err(ExportError::new(format!(
                            "object value has non-object declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                self.encode_object(type_name, object, TypeTagMode::WhenPolymorphic)
            }
            CfdValue::Ref(_) => self.encode_ref(value),
            CfdValue::Array(items) => {
                let inner = match declared_type {
                    CftSchemaTypeRef::Array(inner) => inner,
                    other => {
                        return Err(ExportError::new(format!(
                            "array value has non-array declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                let values = items
                    .iter()
                    .map(|item| self.encode_value(inner, item))
                    .collect::<Result<Vec<_>, _>>()?;
                self.encoder.array(values).map_err(encoder_error)
            }
            CfdValue::Dict(entries) => {
                let value_ty = match declared_type {
                    CftSchemaTypeRef::Dict(_, value_ty) => value_ty,
                    other => {
                        return Err(ExportError::new(format!(
                            "dict value has non-dict declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                let mut values = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    values.push((dict_key_string(key), self.encode_value(value_ty, value)?));
                }
                self.encoder.map(values).map_err(encoder_error)
            }
        }
    }

    fn encode_ref(&mut self, value: &CfdValue) -> Result<E::Value, ExportError> {
        match value {
            CfdValue::Null => self.encoder.null().map_err(encoder_error),
            CfdValue::Ref(target_key) => self.encoder.string(target_key).map_err(encoder_error),
            other => Err(ExportError::new(format!(
                "expected ref value, got `{}`",
                value_kind(other)
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTagMode {
    Never,
    WhenPolymorphic,
}

fn dict_key_string(key: &CfdDictKey) -> String {
    match key {
        CfdDictKey::String(value) => value.clone(),
        CfdDictKey::Int(value) => value.to_string(),
        CfdDictKey::Enum(value) => value.value.to_string(),
    }
}

const fn value_kind(value: &CfdValue) -> &'static str {
    match value {
        CfdValue::Null => "null",
        CfdValue::Bool(_) => "bool",
        CfdValue::Int(_) => "int",
        CfdValue::Float(_) => "float",
        CfdValue::String(_) => "string",
        CfdValue::Enum(_) => "enum",
        CfdValue::Object(_) => "object",
        CfdValue::Ref(_) => "ref",
        CfdValue::Array(_) => "array",
        CfdValue::Dict(_) => "dict",
    }
}

fn display_type_ref(ty: &CftSchemaTypeRef) -> String {
    match ty {
        CftSchemaTypeRef::Int => "int".to_string(),
        CftSchemaTypeRef::Float => "float".to_string(),
        CftSchemaTypeRef::Bool => "bool".to_string(),
        CftSchemaTypeRef::String => "string".to_string(),
        CftSchemaTypeRef::Named(name) => name.clone(),
        CftSchemaTypeRef::Ref(name) => format!("&{name}"),
        CftSchemaTypeRef::Array(inner) => format!("[{}]", display_type_ref(inner)),
        CftSchemaTypeRef::Dict(key, value) => {
            format!("{{{}: {}}}", display_type_ref(key), display_type_ref(value))
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", display_type_ref(inner)),
    }
}

fn encoder_error(error: impl fmt::Display) -> ExportError {
    ExportError::new(error.to_string())
}
