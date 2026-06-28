use std::collections::BTreeMap;

use coflow_api::{
    Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry, RecordOrigin, Severity,
    WriteFieldPathSegment,
};
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdDictKey, CfdEnumValue, CfdRecord, CfdValue};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPatchRequest {
    #[serde(default = "default_true")]
    pub check_after_write: bool,
    #[serde(default = "default_true")]
    pub stop_on_write_error: bool,
    pub ops: Vec<DataPatchOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DataPatchOp {
    InsertRecord {
        file: String,
        #[serde(default)]
        sheet: Option<String>,
        #[serde(rename = "type")]
        actual_type: String,
        key: String,
        #[serde(default)]
        fields: BTreeMap<String, Value>,
    },
    SetField {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
        path: Vec<PatchPathSegment>,
        value: Value,
    },
    DeleteRecord {
        record: PatchRecordSelector,
        #[serde(default)]
        file: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRecordSelector {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PatchPathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchReport {
    pub write_ok: bool,
    pub check_ok: bool,
    pub applied: Vec<DataPatchAppliedOp>,
    pub failed: Vec<DataPatchFailedOp>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchAppliedOp {
    pub index: usize,
    pub op: String,
    pub record: Option<RecordCoordinate>,
    pub file: Option<String>,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataPatchFailedOp {
    pub index: usize,
    pub op: String,
    pub diagnostics: Vec<FlatDiagnostic>,
}

const fn default_true() -> bool {
    true
}

impl ProjectSession {
    /// Apply a batch patch through the provider writer layer and report the
    /// rebuilt session diagnostics.
    ///
    /// # Errors
    ///
    /// This method reserves `Err` for unrecoverable patch execution failures.
    /// Per-operation write and coercion failures are represented in the
    /// returned [`DataPatchReport`] so callers can inspect the failed op.
    pub fn apply_data_patch(
        &mut self,
        registry: &ProviderRegistry,
        request: DataPatchRequest,
    ) -> Result<DataPatchReport, DiagnosticSet> {
        let DataPatchRequest {
            check_after_write,
            stop_on_write_error,
            ops,
        } = request;
        let mut applied = Vec::new();
        let mut failed = Vec::new();
        let mut failure_diagnostics = Vec::new();
        let mut write_ok = true;

        for (index, op) in ops.iter().enumerate() {
            match apply_one(self, registry, op) {
                Ok(applied_op) => applied.push(DataPatchAppliedOp {
                    index,
                    ..applied_op
                }),
                Err(err) => {
                    write_ok = false;
                    let diagnostics = err.diagnostics();
                    let flat = flat_diagnostics(diagnostics);
                    let failed_op = DataPatchFailedOp {
                        index,
                        op: op_name(op).to_string(),
                        diagnostics: flat.clone(),
                    };
                    failure_diagnostics.extend(flat);
                    failed.push(failed_op);
                    if stop_on_write_error || err.is_terminal() {
                        failure_diagnostics.extend(session_flat_diagnostics(self));
                        return Ok(DataPatchReport {
                            write_ok: false,
                            check_ok: false,
                            applied,
                            failed,
                            diagnostics: failure_diagnostics,
                        });
                    }
                }
            }
        }

        let mut diagnostics = failure_diagnostics;
        diagnostics.extend(session_flat_diagnostics(self));
        let check_ok = write_ok
            && (!check_after_write
                || diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.severity != "error"));
        Ok(DataPatchReport {
            write_ok,
            check_ok,
            applied,
            failed,
            diagnostics,
        })
    }
}

fn apply_one(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    op: &DataPatchOp,
) -> Result<DataPatchAppliedOp, PatchApplyError> {
    match op {
        DataPatchOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
        } => {
            ensure_source_file(session, file).map_err(PatchApplyError::Recoverable)?;
            ensure_type_can_insert(session, actual_type).map_err(PatchApplyError::Recoverable)?;
            let values = coerce_insert_fields(session, actual_type, fields)
                .map_err(PatchApplyError::Recoverable)?;
            let outcome = session
                .insert_record(registry, file, sheet.as_deref(), key, actual_type, &values)
                .map_err(PatchApplyError::Terminal)?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "insert_record".to_string(),
                record: Some(RecordCoordinate::new(actual_type, key)),
                file: Some(file.clone()),
                outcome,
            })
        }
        DataPatchOp::SetField {
            record,
            file,
            path,
            value,
        } => {
            let coordinate = RecordCoordinate::new(&record.actual_type, &record.key);
            let expected = expected_type_for_path(session, &coordinate, path)
                .map_err(PatchApplyError::Recoverable)?;
            let write_file = effective_write_file_for_set_field(session, &coordinate, path)
                .map_err(PatchApplyError::Recoverable)?;
            ensure_file_guard_for_file(&coordinate, &write_file, file.as_deref())
                .map_err(PatchApplyError::Recoverable)?;
            let write_path =
                patch_path_to_write_path(path).map_err(PatchApplyError::Recoverable)?;
            let new_value =
                coerce_value(session, &expected, value).map_err(PatchApplyError::Recoverable)?;
            let outcome = session
                .write_field(
                    registry,
                    &coordinate.actual_type,
                    &coordinate.key,
                    &write_path,
                    &new_value,
                )
                .map_err(PatchApplyError::Terminal)?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "set_field".to_string(),
                record: Some(coordinate),
                file: Some(write_file),
                outcome,
            })
        }
        DataPatchOp::DeleteRecord { record, file } => {
            let coordinate = RecordCoordinate::new(&record.actual_type, &record.key);
            ensure_file_guard(session, &coordinate, file.as_deref())
                .map_err(PatchApplyError::Recoverable)?;
            let report_file = file
                .clone()
                .or_else(|| record_file(session, &coordinate).map(ToOwned::to_owned));
            let outcome = session
                .delete_record(registry, &coordinate.actual_type, &coordinate.key)
                .map_err(PatchApplyError::Terminal)?;
            Ok(DataPatchAppliedOp {
                index: 0,
                op: "delete_record".to_string(),
                record: Some(coordinate),
                file: report_file,
                outcome,
            })
        }
    }
}

#[derive(Debug)]
enum PatchApplyError {
    Recoverable(DiagnosticSet),
    Terminal(DiagnosticSet),
}

impl PatchApplyError {
    const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    const fn diagnostics(&self) -> &DiagnosticSet {
        match self {
            Self::Recoverable(diagnostics) | Self::Terminal(diagnostics) => diagnostics,
        }
    }
}

fn ensure_source_file(session: &ProjectSession, file: &str) -> Result<(), DiagnosticSet> {
    if session.files.source_files().contains(file) {
        return Ok(());
    }
    Err(one_patch_error(
        "PATCH-FILE",
        format!("file `{file}` is not a loaded data source"),
    ))
}

fn ensure_file_guard(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    file: Option<&str>,
) -> Result<(), DiagnosticSet> {
    let Some(expected_file) = file else {
        return Ok(());
    };
    let Some(actual_file) = record_file(session, coordinate) else {
        return Err(one_patch_error(
            "PATCH-FILE-GUARD",
            format!(
                "record `{}.{}` was not found for file guard `{expected_file}`",
                coordinate.actual_type, coordinate.key
            ),
        ));
    };
    if actual_file == expected_file {
        return Ok(());
    }
    Err(one_patch_error(
        "PATCH-FILE-GUARD",
        format!(
            "record `{}.{}` belongs to `{actual_file}`, not `{expected_file}`",
            coordinate.actual_type, coordinate.key
        ),
    ))
}

fn ensure_file_guard_for_file(
    coordinate: &RecordCoordinate,
    actual_file: &str,
    expected_file: Option<&str>,
) -> Result<(), DiagnosticSet> {
    let Some(expected_file) = expected_file else {
        return Ok(());
    };
    if actual_file == expected_file {
        return Ok(());
    }
    Err(one_patch_error(
        "PATCH-FILE-GUARD",
        format!(
            "record `{}.{}` writes to `{actual_file}`, not `{expected_file}`",
            coordinate.actual_type, coordinate.key
        ),
    ))
}

fn ensure_type_can_insert(
    session: &ProjectSession,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = session.schema.resolve_type(actual_type) else {
        return Err(one_patch_error(
            "PATCH-TYPE",
            format!("unknown insert type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_patch_error(
            "PATCH-TYPE",
            format!("abstract type `{actual_type}` cannot be inserted"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_patch_error(
            "PATCH-TYPE",
            format!("singleton type `{actual_type}` cannot be inserted"),
        ));
    }
    Ok(())
}

fn coerce_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let Some(schema_type) = session.schema.resolve_type(actual_type) else {
        return Err(one_patch_error(
            "PATCH-TYPE",
            format!("unknown insert type `{actual_type}`"),
        ));
    };
    let mut values = BTreeMap::new();
    for (name, value) in fields {
        let field = schema_type
            .all_fields
            .iter()
            .find(|field| field.name == *name)
            .ok_or_else(|| {
                one_patch_error(
                    "PATCH-PATH",
                    format!("unknown field `{name}` on type `{actual_type}`"),
                )
            })?;
        values.insert(name.clone(), coerce_value(session, &field.ty_ref, value)?);
    }
    Ok(values)
}

fn expected_type_for_path(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[PatchPathSegment],
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_patch_error(
            "PATCH-PATH",
            "patch path must not be empty",
        ));
    }

    let mut current = CftSchemaTypeRef::Named(coordinate.actual_type.clone());
    for segment in path {
        current = match segment {
            PatchPathSegment::Field(field) => {
                field_type_for_path_segment(session, &current, field)?
            }
            PatchPathSegment::Index(index) => array_item_type_for_path_segment(&current, *index)?,
        };
    }
    Ok(current)
}

fn field_type_for_path_segment(
    session: &ProjectSession,
    current: &CftSchemaTypeRef,
    field_name: &str,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    match non_nullable(current) {
        CftSchemaTypeRef::Named(type_name) if session.schema.has_type(type_name) => {
            let schema_type = session.schema.resolve_type(type_name).ok_or_else(|| {
                one_patch_error("PATCH-PATH", format!("unknown type `{type_name}`"))
            })?;
            schema_type
                .all_fields
                .iter()
                .find(|field| field.name == field_name)
                .map(|field| field.ty_ref.clone())
                .ok_or_else(|| {
                    one_patch_error(
                        "PATCH-PATH",
                        format!("unknown field `{field_name}` on type `{type_name}`"),
                    )
                })
        }
        CftSchemaTypeRef::Dict(_, _) => Err(one_patch_error(
            "PATCH-PATH",
            "dict-key field writes are not supported",
        )),
        _ => Err(one_patch_error(
            "PATCH-PATH",
            format!("field `{field_name}` cannot be selected from this value"),
        )),
    }
}

fn array_item_type_for_path_segment(
    current: &CftSchemaTypeRef,
    index: usize,
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    match non_nullable(current) {
        CftSchemaTypeRef::Array(inner) => Ok((**inner).clone()),
        CftSchemaTypeRef::Dict(_, _) => Err(one_patch_error(
            "PATCH-PATH",
            "dict-key index writes are not supported",
        )),
        _ => Err(one_patch_error(
            "PATCH-PATH",
            format!("array index `{index}` cannot be selected from this value"),
        )),
    }
}

fn patch_path_to_write_path(
    path: &[PatchPathSegment],
) -> Result<Vec<WriteFieldPathSegment>, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_patch_error(
            "PATCH-PATH",
            "patch path must not be empty",
        ));
    }
    Ok(path
        .iter()
        .map(|segment| match segment {
            PatchPathSegment::Field(field) => WriteFieldPathSegment::Field(field.clone()),
            PatchPathSegment::Index(index) => WriteFieldPathSegment::Index(*index),
        })
        .collect())
}

fn effective_write_file_for_set_field(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[PatchPathSegment],
) -> Result<String, DiagnosticSet> {
    let record_ref = session
        .records
        .get_by_coordinate(&coordinate.actual_type, &coordinate.key)
        .ok_or_else(|| {
            one_patch_error(
                "PATCH-PATH",
                format!(
                    "record `{}.{}` was not found",
                    coordinate.actual_type, coordinate.key
                ),
            )
        })?;
    let Some(PatchPathSegment::Field(top_field)) = path.first() else {
        return Ok(record_ref.display_path.clone());
    };
    let record = session.model.record(record_ref.id).ok_or_else(|| {
        one_patch_error(
            "PATCH-PATH",
            format!(
                "record `{}.{}` was not found in the data model",
                coordinate.actual_type, coordinate.key
            ),
        )
    })?;
    let Some(source_id) = record.spread_source_for_field(top_field) else {
        return Ok(record_ref.display_path.clone());
    };
    session
        .records
        .get(source_id)
        .map(|source_ref| source_ref.display_path.clone())
        .ok_or_else(|| {
            one_patch_error(
                "PATCH-PATH",
                format!("spread source for field `{top_field}` is no longer indexed"),
            )
        })
}

fn coerce_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    match expected {
        CftSchemaTypeRef::Int => value
            .as_i64()
            .map(CfdValue::Int)
            .ok_or_else(|| one_value_error("expected int")),
        CftSchemaTypeRef::Float => value
            .as_f64()
            .map(CfdValue::Float)
            .ok_or_else(|| one_value_error("expected float")),
        CftSchemaTypeRef::Bool => value
            .as_bool()
            .map(CfdValue::Bool)
            .ok_or_else(|| one_value_error("expected bool")),
        CftSchemaTypeRef::String => value
            .as_str()
            .map(|text| CfdValue::String(text.to_string()))
            .ok_or_else(|| one_value_error("expected string")),
        CftSchemaTypeRef::Nullable(_) if value.is_null() => Ok(CfdValue::Null),
        CftSchemaTypeRef::Nullable(inner) => coerce_value(session, inner, value),
        CftSchemaTypeRef::Array(inner) => {
            let items = value
                .as_array()
                .ok_or_else(|| one_value_error("expected array"))?;
            items
                .iter()
                .map(|item| coerce_value(session, inner, item))
                .collect::<Result<Vec<_>, _>>()
                .map(CfdValue::Array)
        }
        CftSchemaTypeRef::Dict(key, item) => coerce_dict_value(session, key, item, value),
        CftSchemaTypeRef::Named(name) if session.schema.has_enum(name) => {
            let variant = value
                .as_str()
                .ok_or_else(|| one_value_error(format!("expected enum variant for `{name}`")))?;
            enum_value(session, name, variant).map(CfdValue::Enum)
        }
        CftSchemaTypeRef::Named(name) => coerce_named_value(session, name, value),
    }
}

fn coerce_dict_value(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    item_type: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Err(one_value_error("expected dict object"));
    };
    if let Some(entries) = object.get("$dict") {
        if object.len() != 1 {
            return Err(one_value_error("`$dict` object cannot include other keys"));
        }
        return coerce_special_dict(session, key_type, item_type, entries);
    }

    object
        .iter()
        .map(|(key, entry_value)| {
            let key_value = Value::String(key.clone());
            Ok((
                coerce_dict_key(session, key_type, &key_value)?,
                coerce_value(session, item_type, entry_value)?,
            ))
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()
        .map(CfdValue::Dict)
}

fn coerce_special_dict(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    item_type: &CftSchemaTypeRef,
    entries: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let entries = entries
        .as_array()
        .ok_or_else(|| one_value_error("`$dict` must be an array"))?;
    entries
        .iter()
        .map(|entry| {
            let object = entry
                .as_object()
                .ok_or_else(|| one_value_error("`$dict` entries must be objects"))?;
            let key = object
                .get("key")
                .ok_or_else(|| one_value_error("`$dict` entry is missing `key`"))?;
            let value = object
                .get("value")
                .ok_or_else(|| one_value_error("`$dict` entry is missing `value`"))?;
            Ok((
                coerce_dict_key(session, key_type, key)?,
                coerce_value(session, item_type, value)?,
            ))
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()
        .map(CfdValue::Dict)
}

fn coerce_dict_key(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    value: &Value,
) -> Result<CfdDictKey, DiagnosticSet> {
    match key_type {
        CftSchemaTypeRef::String => value
            .as_str()
            .map(|text| CfdDictKey::String(text.to_string()))
            .ok_or_else(|| one_value_error("expected string dict key")),
        CftSchemaTypeRef::Int => coerce_int_dict_key(value),
        CftSchemaTypeRef::Named(enum_name) if session.schema.has_enum(enum_name) => {
            let variant = value.as_str().ok_or_else(|| {
                one_value_error(format!("expected enum dict key for `{enum_name}`"))
            })?;
            enum_value(session, enum_name, variant).map(CfdDictKey::Enum)
        }
        CftSchemaTypeRef::Nullable(inner) => coerce_dict_key(session, inner, value),
        _ => Err(one_value_error(
            "dict keys support only string, int, and enum types",
        )),
    }
}

fn coerce_int_dict_key(value: &Value) -> Result<CfdDictKey, DiagnosticSet> {
    if let Some(number) = value.as_i64() {
        return Ok(CfdDictKey::Int(number));
    }
    let text = value
        .as_str()
        .ok_or_else(|| one_value_error("expected int dict key"))?;
    let number = text
        .parse::<i64>()
        .map_err(|_| one_value_error(format!("expected int dict key, got `{text}`")))?;
    Ok(CfdDictKey::Int(number))
}

fn coerce_named_value(
    session: &ProjectSession,
    expected_type: &str,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    if let Some(reference) = coerce_ref_value(session, expected_type, value)? {
        return Ok(reference);
    }
    let object = value.as_object().ok_or_else(|| {
        one_value_error(format!("expected object or `$ref` for `{expected_type}`"))
    })?;
    let actual_type = actual_object_type(object, expected_type)?;
    ensure_object_type_assignable(session, expected_type, &actual_type)?;
    let fields = coerce_object_fields(session, &actual_type, object)?;
    Ok(CfdValue::Object(Box::new(CfdRecord {
        key: String::new(),
        actual_type,
        fields,
        origin: RecordOrigin::None,
        spread_field_sources: BTreeMap::new(),
    })))
}

fn actual_object_type(
    object: &Map<String, Value>,
    expected_type: &str,
) -> Result<String, DiagnosticSet> {
    object.get("$type").map_or_else(
        || Ok(expected_type.to_string()),
        |value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| one_value_error("`$type` must be a string"))
        },
    )
}

fn ensure_object_type_assignable(
    session: &ProjectSession,
    expected_type: &str,
    actual_type: &str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = session.schema.resolve_type(actual_type) else {
        return Err(one_value_error(format!(
            "unknown object type `{actual_type}`"
        )));
    };
    if schema_type.is_abstract {
        return Err(one_value_error(format!(
            "abstract object type `{actual_type}` cannot be instantiated"
        )));
    }
    if !session.schema.is_assignable(actual_type, expected_type) {
        return Err(one_value_error(format!(
            "type `{actual_type}` is not assignable to `{expected_type}`"
        )));
    }
    Ok(())
}

fn coerce_object_fields(
    session: &ProjectSession,
    actual_type: &str,
    object: &Map<String, Value>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let Some(schema_type) = session.schema.resolve_type(actual_type) else {
        return Err(one_value_error(format!(
            "unknown object type `{actual_type}`"
        )));
    };
    let mut fields = BTreeMap::new();
    for (field_name, field_value) in object {
        if field_name == "$type" {
            continue;
        }
        if field_name.starts_with('$') {
            return Err(one_value_error(format!(
                "unsupported object form key `{field_name}`"
            )));
        }
        let field = schema_type
            .all_fields
            .iter()
            .find(|field| field.name == *field_name)
            .ok_or_else(|| {
                one_value_error(format!(
                    "unknown field `{field_name}` on type `{actual_type}`"
                ))
            })?;
        fields.insert(
            field_name.clone(),
            coerce_value(session, &field.ty_ref, field_value)?,
        );
    }
    Ok(fields)
}

fn coerce_ref_value(
    session: &ProjectSession,
    expected_type: &str,
    value: &Value,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let Some(raw_ref) = object.get("$ref") else {
        return Ok(None);
    };
    if object.len() != 1 {
        return Err(one_ref_error("`$ref` object cannot include other keys"));
    }
    let (target_type, target_key) = parse_ref_target(raw_ref)?;
    if target_key.is_empty() {
        return Err(one_ref_error("reference key must not be empty"));
    }
    if !session.schema.has_type(&target_type) {
        return Err(one_ref_error(format!(
            "unknown reference type `{target_type}`"
        )));
    }
    if !session.schema.is_assignable(&target_type, expected_type) {
        return Err(one_ref_error(format!(
            "reference type `{target_type}` is not assignable to `{expected_type}`"
        )));
    }
    Ok(Some(CfdValue::Ref {
        target_type,
        target_key,
    }))
}

fn parse_ref_target(value: &Value) -> Result<(String, String), DiagnosticSet> {
    if let Some(text) = value.as_str() {
        let Some((target_type, target_key)) = text.split_once('.') else {
            return Err(one_ref_error("string `$ref` must be written as `Type.key`"));
        };
        if target_type.is_empty() || target_key.is_empty() {
            return Err(one_ref_error("string `$ref` must be written as `Type.key`"));
        }
        return Ok((target_type.to_string(), target_key.to_string()));
    }
    let object = value
        .as_object()
        .ok_or_else(|| one_ref_error("`$ref` must be a string or object"))?;
    let target_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| one_ref_error("object `$ref` is missing string `type`"))?;
    let target_key = object
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| one_ref_error("object `$ref` is missing string `key`"))?;
    Ok((target_type.to_string(), target_key.to_string()))
}

fn enum_value(
    session: &ProjectSession,
    enum_name: &str,
    raw_variant: &str,
) -> Result<CfdEnumValue, DiagnosticSet> {
    let variant = raw_variant
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(raw_variant);
    let int_value = session
        .schema
        .enum_variant_value(enum_name, variant)
        .ok_or_else(|| one_value_error(format!("unknown enum variant `{enum_name}.{variant}`")))?;
    Ok(CfdEnumValue {
        enum_name: enum_name.to_string(),
        variant: Some(variant.to_string()),
        value: int_value,
    })
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn record_file<'a>(session: &'a ProjectSession, coordinate: &RecordCoordinate) -> Option<&'a str> {
    session.file_for_record(&coordinate.actual_type, &coordinate.key)
}

const fn op_name(op: &DataPatchOp) -> &'static str {
    match op {
        DataPatchOp::InsertRecord { .. } => "insert_record",
        DataPatchOp::SetField { .. } => "set_field",
        DataPatchOp::DeleteRecord { .. } => "delete_record",
    }
}

fn one_value_error(message: impl Into<String>) -> DiagnosticSet {
    one_patch_error("PATCH-VALUE", message)
}

fn one_ref_error(message: impl Into<String>) -> DiagnosticSet {
    one_patch_error("PATCH-REF", message)
}

fn one_patch_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(patch_diag(code, message))
}

fn patch_diag(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: code.to_string(),
        stage: "PATCH".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    }
}

fn session_flat_diagnostics(session: &ProjectSession) -> Vec<FlatDiagnostic> {
    flat_diagnostics(session.diagnostics.as_set())
}

fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}
