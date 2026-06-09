//! JSON exporter for validated Coflow data models.
//!
//! This crate converts an already-built [`CfdDataModel`] into table-oriented
//! JSON values. It deliberately does not load files or run checks.

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

use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftContainer, CftSchemaField, CftSchemaTypeRef,
};
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdIdValue, CfdRecord, CfdTable, CfdValue};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonExportError {
    pub message: String,
}

impl JsonExportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for JsonExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for JsonExportError {}

/// Converts every table in the data model into one JSON array value.
///
/// The returned map key is the CFT type/table name. Values are arrays whose
/// order follows the table's original record order in the data model.
///
/// # Errors
///
/// Returns an error when a model record or field cannot be matched back to the
/// compiled schema. A `CfdDataModel` built from the same `CftContainer` should
/// not hit these errors.
pub fn export_json_model(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Value>, JsonExportError> {
    JsonExporter::new(schema, model).export()
}

struct JsonExporter<'a> {
    schema: SchemaView<'a>,
    model: &'a CfdDataModel,
}

impl<'a> JsonExporter<'a> {
    fn new(schema: &'a CftContainer, model: &'a CfdDataModel) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
        }
    }

    fn export(&self) -> Result<BTreeMap<String, Value>, JsonExportError> {
        let mut out = BTreeMap::new();
        for schema_type in self.schema.schema.all_types() {
            if schema_type.is_abstract {
                continue;
            }
            let has_id_field = schema_type
                .all_fields
                .iter()
                .any(|field| has_annotation(&field.annotations, "id"));
            if !has_id_field {
                continue;
            }

            let table = self.model.table(&schema_type.name);
            let value = if let Some(table) = table {
                self.encode_table(table)?
            } else {
                Value::Array(Vec::new())
            };
            out.insert(schema_type.name.clone(), value);
        }
        Ok(out)
    }

    fn encode_table(&self, table: &CfdTable) -> Result<Value, JsonExportError> {
        let mut records = Vec::with_capacity(table.records.len());
        for record_id in &table.records {
            let record = self.model.record(*record_id).ok_or_else(|| {
                JsonExportError::new(format!(
                    "table `{}` references missing record `{record_id}`",
                    table.type_name
                ))
            })?;
            records.push(self.encode_record(&table.type_name, record, TypeTagMode::Never)?);
        }
        Ok(Value::Array(records))
    }

    fn encode_record(
        &self,
        declared_type: &str,
        record: &CfdRecord,
        tag_mode: TypeTagMode,
    ) -> Result<Value, JsonExportError> {
        let mut object = Map::new();
        if tag_mode == TypeTagMode::WhenPolymorphic
            && self.schema.range_is_polymorphic(declared_type)
        {
            object.insert(
                "$type".to_string(),
                Value::String(record.actual_type.clone()),
            );
        }

        for field in self.schema.full_fields(&record.actual_type)? {
            let value = record.fields.get(&field.name).ok_or_else(|| {
                JsonExportError::new(format!(
                    "record `{}` is missing field `{}`",
                    record.actual_type, field.name
                ))
            })?;
            object.insert(field.name.clone(), self.encode_field(&field, value)?);
        }
        Ok(Value::Object(object))
    }

    fn encode_field(&self, field: &FieldMeta, value: &CfdValue) -> Result<Value, JsonExportError> {
        if field.ref_target.is_some() {
            return Self::encode_ref(value);
        }
        self.encode_value(&field.ty, value)
    }

    fn encode_value(
        &self,
        declared_type: &CftSchemaTypeRef,
        value: &CfdValue,
    ) -> Result<Value, JsonExportError> {
        if let CftSchemaTypeRef::Nullable(inner) = declared_type {
            return match value {
                CfdValue::Null => Ok(Value::Null),
                other => self.encode_value(inner, other),
            };
        }

        match value {
            CfdValue::Null => Ok(Value::Null),
            CfdValue::Bool(value) => Ok(Value::Bool(*value)),
            CfdValue::Int(value) => Ok(Value::Number(Number::from(*value))),
            CfdValue::Float(value) => Number::from_f64(*value)
                .map(Value::Number)
                .ok_or_else(|| JsonExportError::new("cannot export non-finite float")),
            CfdValue::String(value) => Ok(Value::String(value.clone())),
            CfdValue::Enum(value) => Ok(Value::Number(Number::from(value.value))),
            CfdValue::Object(record) => {
                let type_name = match declared_type {
                    CftSchemaTypeRef::Named(type_name) => type_name,
                    other => {
                        return Err(JsonExportError::new(format!(
                            "object value has non-object declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                self.encode_record(type_name, record, TypeTagMode::WhenPolymorphic)
            }
            CfdValue::Ref { .. } => Self::encode_ref(value),
            CfdValue::Array(items) => {
                let inner = match declared_type {
                    CftSchemaTypeRef::Array(inner) => inner,
                    other => {
                        return Err(JsonExportError::new(format!(
                            "array value has non-array declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                items
                    .iter()
                    .map(|item| self.encode_value(inner, item))
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            }
            CfdValue::Dict(entries) => {
                let value_ty = match declared_type {
                    CftSchemaTypeRef::Dict(_, value_ty) => value_ty,
                    other => {
                        return Err(JsonExportError::new(format!(
                            "dict value has non-dict declared type `{}`",
                            display_type_ref(other)
                        )))
                    }
                };
                let mut object = Map::new();
                for (key, value) in entries {
                    object.insert(dict_key_string(key), self.encode_value(value_ty, value)?);
                }
                Ok(Value::Object(object))
            }
        }
    }

    fn encode_ref(value: &CfdValue) -> Result<Value, JsonExportError> {
        match value {
            CfdValue::Null => Ok(Value::Null),
            CfdValue::Ref { id, .. } => Ok(id_to_json(id)),
            other => Err(JsonExportError::new(format!(
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

struct SchemaView<'a> {
    schema: &'a CftContainer,
    children_by_parent: BTreeMap<String, Vec<String>>,
}

impl<'a> SchemaView<'a> {
    fn new(schema: &'a CftContainer) -> Self {
        let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
        for schema_type in schema.all_types() {
            if let Some(parent) = &schema_type.parent {
                children_by_parent
                    .entry(parent.clone())
                    .or_default()
                    .push(schema_type.name.clone());
            }
        }
        Self {
            schema,
            children_by_parent,
        }
    }

    fn full_fields(&self, type_name: &str) -> Result<Vec<FieldMeta>, JsonExportError> {
        let mut out = Vec::new();
        self.fill_fields(type_name, &mut out, &mut BTreeSet::new())?;
        Ok(out)
    }

    fn fill_fields(
        &self,
        type_name: &str,
        out: &mut Vec<FieldMeta>,
        seen: &mut BTreeSet<String>,
    ) -> Result<(), JsonExportError> {
        if !seen.insert(type_name.to_string()) {
            return Ok(());
        }
        let schema_type = self.schema.resolve_type(type_name).ok_or_else(|| {
            JsonExportError::new(format!("unknown CFT type `{type_name}` during JSON export"))
        })?;
        if let Some(parent) = &schema_type.parent {
            self.fill_fields(parent, out, seen)?;
        }
        for field in &schema_type.fields {
            out.push(FieldMeta::from_schema(field));
        }
        Ok(())
    }

    fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.schema
            .resolve_type(type_name)
            .is_some_and(|schema_type| schema_type.is_abstract)
            || self
                .children_by_parent
                .get(type_name)
                .is_some_and(|children| !children.is_empty())
    }
}

#[derive(Debug, Clone)]
struct FieldMeta {
    name: String,
    ty: CftSchemaTypeRef,
    ref_target: Option<String>,
}

impl FieldMeta {
    fn from_schema(field: &CftSchemaField) -> Self {
        Self {
            name: field.name.clone(),
            ty: field.ty_ref.clone(),
            ref_target: ref_target(&field.annotations),
        }
    }
}

fn ref_target(annotations: &[CftAnnotation]) -> Option<String> {
    annotations
        .iter()
        .find(|annotation| annotation.name == "ref")
        .and_then(|annotation| annotation.args.first())
        .and_then(|arg| match arg {
            CftAnnotationValue::Name(name) => Some(name.clone()),
            _ => None,
        })
}

fn has_annotation(annotations: &[CftAnnotation], name: &str) -> bool {
    annotations.iter().any(|annotation| annotation.name == name)
}

fn id_to_json(id: &CfdIdValue) -> Value {
    match id {
        CfdIdValue::String(value) => Value::String(value.clone()),
        CfdIdValue::Int(value) => Value::Number(Number::from(*value)),
    }
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
        CfdValue::Ref { .. } => "ref",
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
        CftSchemaTypeRef::Array(inner) => format!("[{}]", display_type_ref(inner)),
        CftSchemaTypeRef::Dict(key, value) => {
            format!("{{{}: {}}}", display_type_ref(key), display_type_ref(value))
        }
        CftSchemaTypeRef::Nullable(inner) => format!("{}?", display_type_ref(inner)),
    }
}
