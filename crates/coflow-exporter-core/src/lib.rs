//! Shared path-aware streaming traversal for Coflow data exporters.

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

use coflow_cft::{CftField, CftSchema, CftSchemaTypeRef};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdObject, CfdRecord, CfdTable, CfdValue};
use std::borrow::Cow;
use std::fmt;
use std::fmt::Write as _;

/// Receives one ordered event stream for each exported table.
///
/// Container lengths are known before their contents, so binary sinks can
/// write headers directly without buffering child values.
pub trait ExportEventSink {
    type Error: fmt::Display;

    /// Starts one table event stream.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the table header cannot be written.
    fn begin_table(&mut self, name: &str, records: usize) -> Result<(), Self::Error>;
    /// Finishes the current table event stream.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the table cannot be finalized.
    fn end_table(&mut self) -> Result<(), Self::Error>;
    /// Starts an array with its known element count.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the array header cannot be written.
    fn begin_array(&mut self, len: usize) -> Result<(), Self::Error>;
    /// Finishes the current array.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the array cannot be finalized.
    fn end_array(&mut self) -> Result<(), Self::Error>;
    /// Starts a map with its known entry count.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the map header cannot be written.
    fn begin_map(&mut self, len: usize) -> Result<(), Self::Error>;
    /// Writes the next map key.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the key cannot be written.
    fn map_key(&mut self, key: &str) -> Result<(), Self::Error>;
    /// Finishes the current map.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the map cannot be finalized.
    fn end_map(&mut self) -> Result<(), Self::Error>;
    /// Writes a null scalar.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the scalar cannot be written.
    fn null(&mut self) -> Result<(), Self::Error>;
    /// Writes a Boolean scalar.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the scalar cannot be written.
    fn bool(&mut self, value: bool) -> Result<(), Self::Error>;
    /// Writes an integer scalar.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the scalar cannot be written.
    fn int(&mut self, value: i64) -> Result<(), Self::Error>;
    /// Writes a floating-point scalar.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the scalar cannot be written.
    fn float(&mut self, value: f64) -> Result<(), Self::Error>;
    /// Writes a string scalar.
    ///
    /// # Errors
    ///
    /// Returns the sink-specific error when the scalar cannot be written.
    fn string(&mut self, value: &str) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportError {
    pub location: String,
    pub message: String,
}

impl ExportError {
    fn at(location: &ExportLocation<'_>, message: impl Into<String>) -> Self {
        Self {
            location: location.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

impl std::error::Error for ExportError {}

/// Streams every concrete table from the model into `sink`.
///
/// # Errors
///
/// Returns a location-bearing error when model/schema state is inconsistent or
/// when the sink rejects an event.
pub fn export_model_to_sink<S>(
    schema: &CftSchema,
    model: &CfdDataModel,
    sink: &mut S,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    for schema_type in schema
        .all_types()
        .filter(|schema_type| !schema_type.is_abstract)
    {
        let table = model.table(&schema_type.name);
        let record_count = table.map_or(0, |table| table.records.len());
        let mut location = ExportLocation::new(&schema_type.name);
        sink_event(&location, sink.begin_table(&schema_type.name, record_count))?;
        if let Some(table) = table {
            encode_table(schema, model, sink, table, &mut location)?;
        }
        sink_event(&location, sink.end_table())?;
    }
    export_dimension_tables(schema, model, sink)?;
    Ok(())
}

fn export_dimension_tables<S>(
    schema: &CftSchema,
    model: &CfdDataModel,
    sink: &mut S,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    for dimension in schema.all_dimensions() {
        for field in &dimension.fields {
            let source_type =
                schema
                    .resolve_type(&field.declaring_type)
                    .ok_or_else(|| ExportError {
                        location: format!("{}.{}", field.declaring_type, field.name),
                        message: "dimension field has unknown declaring type".to_string(),
                    })?;
            let table_name = format!("{}_{}Variants", field.declaring_type, field.name);
            let record_count = model.records_assignable_to(&field.declaring_type).count();
            let mut location = ExportLocation::new(&table_name);
            sink_event(&location, sink.begin_table(&table_name, record_count))?;
            for (_, record) in model.records_assignable_to(&field.declaring_type) {
                location.record = Some(if source_type.is_singleton {
                    field.name.to_string()
                } else {
                    record.key().to_string()
                });
                let result = encode_dimension_record(
                    schema,
                    sink,
                    dimension,
                    field,
                    source_type.is_singleton,
                    record,
                    &mut location,
                );
                location.record = None;
                result?;
            }
            sink_event(&location, sink.end_table())?;
        }
    }
    Ok(())
}

fn encode_dimension_record<S>(
    schema: &CftSchema,
    sink: &mut S,
    dimension: &coflow_cft::CftDimension,
    field: &CftField,
    is_singleton: bool,
    record: &CfdRecord,
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    sink_event(location, sink.begin_map(dimension.variants.len() + 2))?;

    let checkpoint = location.enter_field("id");
    let result = (|| {
        sink_event(location, sink.map_key("id"))?;
        sink_event(
            location,
            sink.string(if is_singleton {
                field.name.as_str()
            } else {
                record.key()
            }),
        )
    })();
    location.exit(checkpoint);
    result?;

    let checkpoint = location.enter_field("default");
    let result = (|| {
        let value = record.field(&field.name).ok_or_else(|| {
            ExportError::at(
                location,
                format!(
                    "record `{}` is missing dimension source field `{}`",
                    record.actual_type(),
                    field.name
                ),
            )
        })?;
        sink_event(location, sink.map_key("default"))?;
        encode_value(schema, sink, &field.ty_ref, value, location)
    })();
    location.exit(checkpoint);
    result?;

    let overlay = record.dimension_field(&field.name);
    if overlay.is_some_and(|values| values.dimension != dimension.name) {
        return Err(ExportError::at(
            location,
            format!(
                "dimension source field `{}` contains values for a different dimension",
                field.name
            ),
        ));
    }
    for variant in &dimension.variants {
        let checkpoint = location.enter_field(variant);
        let result = (|| {
            sink_event(location, sink.map_key(variant))?;
            if let Some(value) = overlay.and_then(|values| values.variants.get(variant)) {
                encode_value(schema, sink, &field.ty_ref, &value.value, location)
            } else {
                sink_event(location, sink.null())
            }
        })();
        location.exit(checkpoint);
        result?;
    }

    sink_event(location, sink.end_map())
}

fn encode_table<S>(
    schema: &CftSchema,
    model: &CfdDataModel,
    sink: &mut S,
    table: &CfdTable,
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    for record_id in &table.records {
        let record = model.record(*record_id).ok_or_else(|| {
            ExportError::at(
                location,
                format!(
                    "table `{}` references missing record `{record_id}`",
                    table.type_name
                ),
            )
        })?;
        location.record = Some(record.key().to_string());
        let result = encode_record(schema, sink, &table.type_name, record, location);
        location.record = None;
        result?;
    }
    Ok(())
}

fn encode_record<S>(
    schema: &CftSchema,
    sink: &mut S,
    declared_type: &str,
    record: &CfdRecord,
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    let fields = object_fields(schema, &record.object, location)?;
    sink_event(location, sink.begin_map(fields.len() + 1))?;

    let checkpoint = location.enter_field("id");
    let result = (|| {
        sink_event(location, sink.map_key("id"))?;
        sink_event(location, sink.string(record.key()))
    })();
    location.exit(checkpoint);
    result?;

    encode_object_members(
        schema,
        sink,
        declared_type,
        &record.object,
        TypeTagMode::Never,
        &fields,
        location,
    )?;
    sink_event(location, sink.end_map())
}

fn encode_object<S>(
    schema: &CftSchema,
    sink: &mut S,
    declared_type: &str,
    object: &CfdObject,
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    let fields = object_fields(schema, object, location)?;
    let tag_mode = TypeTagMode::WhenPolymorphic;
    let has_type_tag = schema.range_is_polymorphic(declared_type);
    sink_event(
        location,
        sink.begin_map(fields.len() + usize::from(has_type_tag)),
    )?;
    encode_object_members(
        schema,
        sink,
        declared_type,
        object,
        tag_mode,
        &fields,
        location,
    )?;
    sink_event(location, sink.end_map())
}

fn object_fields<'a>(
    schema: &'a CftSchema,
    object: &CfdObject,
    location: &ExportLocation<'_>,
) -> Result<Vec<&'a CftField>, ExportError> {
    schema
        .resolve_type(object.actual_type())
        .map(|ty| ty.all_fields().collect())
        .ok_or_else(|| {
            ExportError::at(
                location,
                format!("unknown CFT type `{}` during export", object.actual_type()),
            )
        })
}

fn encode_object_members<S>(
    schema: &CftSchema,
    sink: &mut S,
    declared_type: &str,
    object: &CfdObject,
    tag_mode: TypeTagMode,
    fields: &[&CftField],
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    if tag_mode == TypeTagMode::WhenPolymorphic && schema.range_is_polymorphic(declared_type) {
        let checkpoint = location.enter_field("$type");
        let result = (|| {
            sink_event(location, sink.map_key("$type"))?;
            sink_event(location, sink.string(object.actual_type()))
        })();
        location.exit(checkpoint);
        result?;
    }

    for field in fields {
        let checkpoint = location.enter_field(&field.name);
        let result = (|| {
            let value = object.fields().get(field.name.as_str()).ok_or_else(|| {
                ExportError::at(
                    location,
                    format!(
                        "record `{}` is missing field `{}`",
                        object.actual_type(),
                        field.name
                    ),
                )
            })?;
            sink_event(location, sink.map_key(&field.name))?;
            encode_value(schema, sink, &field.ty_ref, value, location)
        })();
        location.exit(checkpoint);
        result?;
    }
    Ok(())
}

fn encode_value<S>(
    schema: &CftSchema,
    sink: &mut S,
    declared_type: &CftSchemaTypeRef,
    value: &CfdValue,
    location: &mut ExportLocation<'_>,
) -> Result<(), ExportError>
where
    S: ExportEventSink,
{
    if let CftSchemaTypeRef::Nullable(inner) = declared_type {
        return match value {
            CfdValue::Null => sink_event(location, sink.null()),
            other => encode_value(schema, sink, inner, other, location),
        };
    }

    match value {
        CfdValue::Null => sink_event(location, sink.null()),
        CfdValue::Bool(value) => sink_event(location, sink.bool(*value)),
        CfdValue::Int(value) => sink_event(location, sink.int(*value)),
        CfdValue::Float(value) => {
            if value.is_finite() {
                sink_event(location, sink.float(*value))
            } else {
                Err(ExportError::at(location, "cannot export non-finite float"))
            }
        }
        CfdValue::String(value) => sink_event(location, sink.string(value)),
        CfdValue::Enum(value) => sink_event(location, sink.int(value.value)),
        CfdValue::Object(object) => {
            let type_name = match declared_type {
                CftSchemaTypeRef::Object(type_name) => type_name,
                other => {
                    return Err(ExportError::at(
                        location,
                        format!(
                            "object value has non-object declared type `{}`",
                            display_type_ref(other)
                        ),
                    ))
                }
            };
            encode_object(schema, sink, type_name, object, location)
        }
        CfdValue::Ref(target_key) => sink_event(location, sink.string(target_key)),
        CfdValue::Array(items) => {
            let inner = match declared_type {
                CftSchemaTypeRef::Array(inner) => inner,
                other => {
                    return Err(ExportError::at(
                        location,
                        format!(
                            "array value has non-array declared type `{}`",
                            display_type_ref(other)
                        ),
                    ))
                }
            };
            sink_event(location, sink.begin_array(items.len()))?;
            for (index, item) in items.iter().enumerate() {
                let checkpoint = location.enter_index(index);
                let result = encode_value(schema, sink, inner, item, location);
                location.exit(checkpoint);
                result?;
            }
            sink_event(location, sink.end_array())
        }
        CfdValue::Dict(entries) => {
            let value_ty = match declared_type {
                CftSchemaTypeRef::Dict(_, value_ty) => value_ty,
                other => {
                    return Err(ExportError::at(
                        location,
                        format!(
                            "dict value has non-dict declared type `{}`",
                            display_type_ref(other)
                        ),
                    ))
                }
            };
            sink_event(location, sink.begin_map(entries.len()))?;
            for (key, value) in entries {
                let key = dict_key_string(key);
                let checkpoint = location.enter_dict_key(&key);
                let result = (|| {
                    sink_event(location, sink.map_key(&key))?;
                    encode_value(schema, sink, value_ty, value, location)
                })();
                location.exit(checkpoint);
                result?;
            }
            sink_event(location, sink.end_map())
        }
    }
}

fn sink_event<T>(
    location: &ExportLocation<'_>,
    result: Result<T, impl fmt::Display>,
) -> Result<T, ExportError> {
    result.map_err(|error| ExportError::at(location, error.to_string()))
}

#[derive(Debug)]
struct ExportLocation<'a> {
    table: &'a str,
    record: Option<String>,
    suffix: String,
}

impl<'a> ExportLocation<'a> {
    const fn new(table: &'a str) -> Self {
        Self {
            table,
            record: None,
            suffix: String::new(),
        }
    }

    fn enter_field(&mut self, field: &str) -> usize {
        let checkpoint = self.suffix.len();
        self.suffix.push('.');
        self.suffix.push_str(field);
        checkpoint
    }

    fn enter_index(&mut self, index: usize) -> usize {
        let checkpoint = self.suffix.len();
        let _ = write!(self.suffix, "[{index}]");
        checkpoint
    }

    fn enter_dict_key(&mut self, key: &str) -> usize {
        let checkpoint = self.suffix.len();
        let _ = write!(self.suffix, "[{key:?}]");
        checkpoint
    }

    fn exit(&mut self, checkpoint: usize) {
        self.suffix.truncate(checkpoint);
    }
}

impl fmt::Display for ExportLocation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.table)?;
        if let Some(record) = self.record.as_deref() {
            write!(f, "[{record:?}]")?;
        }
        f.write_str(&self.suffix)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeTagMode {
    Never,
    WhenPolymorphic,
}

fn dict_key_string(key: &CfdDictKey) -> Cow<'_, str> {
    match key {
        CfdDictKey::String(value) => Cow::Borrowed(value),
        CfdDictKey::Int(value) => Cow::Owned(value.to_string()),
        CfdDictKey::Enum(value) => Cow::Owned(value.value.to_string()),
    }
}

fn display_type_ref(ty: &CftSchemaTypeRef) -> String {
    ty.display_label()
}
