use std::collections::{BTreeMap, BTreeSet};

use coflow_api::{Diagnostic, DiagnosticSet, FlatDiagnostic, ProviderRegistry, Severity};
use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef};
use coflow_data_model::{
    CfdDictKey, CfdEnumValue, CfdObject, CfdPath, CfdPathSegment, CfdRecord, CfdValue, RecordOrigin,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::write_rules;
use crate::{ProjectSession, RecordCoordinate, WriteOutcome};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationRequest {
    #[serde(default = "default_true")]
    pub check_after_write: bool,
    #[serde(default = "default_true")]
    pub stop_on_write_error: bool,
    pub ops: Vec<MutationOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MutationOp {
    InsertRecord {
        file: String,
        #[serde(default)]
        sheet: Option<String>,
        #[serde(rename = "type")]
        actual_type: String,
        key: String,
        #[serde(default)]
        fields: MutationFields,
        #[serde(default)]
        materialization: DefaultMaterialization,
    },
    SetField {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
        path: Vec<CfdPathSegment>,
        value: MutationValue,
    },
    RenameRecord {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
        new_key: String,
    },
    DeleteRecord {
        record: RecordCoordinate,
        #[serde(default)]
        file: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MutationValue {
    Json(Value),
    Cfd(CfdValue),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MutationFields {
    #[default]
    Empty,
    Json(BTreeMap<String, Value>),
    Cfd(BTreeMap<String, CfdValue>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMaterialization {
    #[default]
    Minimal,
    EditableShape,
}

#[derive(Debug, Clone)]
pub struct PreparedMutation {
    check_after_write: bool,
    stop_on_write_error: bool,
    ops: Vec<PreparedMutationOp>,
}

#[derive(Debug, Clone)]
enum PreparedMutationOp {
    Pending {
        op: MutationOp,
    },
    InsertRecord {
        file: String,
        sheet: Option<String>,
        actual_type: String,
        key: String,
        fields: BTreeMap<String, CfdValue>,
    },
    SetField {
        record: RecordCoordinate,
        write_file: String,
        path: Vec<coflow_api::WriteFieldPathSegment>,
        value: CfdValue,
    },
    RenameRecord {
        record: RecordCoordinate,
        new_key: String,
        report_file: Option<String>,
    },
    DeleteRecord {
        record: RecordCoordinate,
        report_file: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationReport {
    pub write_ok: bool,
    pub check_ok: bool,
    pub applied: Vec<MutationAppliedOp>,
    pub failed: Vec<MutationFailedOp>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationAppliedOp {
    pub index: usize,
    pub op: String,
    pub record: Option<RecordCoordinate>,
    pub file: Option<String>,
    pub outcome: WriteOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationFailedOp {
    pub index: usize,
    pub op: String,
    pub diagnostics: Vec<FlatDiagnostic>,
}

const fn default_true() -> bool {
    true
}

impl ProjectSession {
    /// Prepare a mutation request for later execution.
    ///
    /// # Errors
    ///
    /// This function currently reserves `Err` for future whole-request
    /// validation failures. Individual operations stay pending until apply
    /// time so each op can be validated against the latest session state
    /// after earlier ops in the same batch have run.
    pub fn prepare_mutation(
        &self,
        request: MutationRequest,
    ) -> Result<PreparedMutation, DiagnosticSet> {
        let MutationRequest {
            check_after_write,
            stop_on_write_error,
            ops,
        } = request;
        let prepared_ops = ops
            .into_iter()
            .map(|op| PreparedMutationOp::Pending { op })
            .collect();
        Ok(PreparedMutation {
            check_after_write,
            stop_on_write_error,
            ops: prepared_ops,
        })
    }

    /// Execute a prepared mutation request through provider writers.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when execution cannot produce a report.
    /// Per-operation validation and writer failures are represented in the
    /// returned [`MutationReport`].
    pub fn apply_prepared_mutation(
        &mut self,
        registry: &ProviderRegistry,
        prepared: PreparedMutation,
    ) -> Result<MutationReport, DiagnosticSet> {
        let PreparedMutation {
            check_after_write,
            stop_on_write_error,
            ops,
        } = prepared;
        let mut applied = Vec::new();
        let mut failed = Vec::new();
        let mut failure_diagnostics = Vec::new();
        let mut write_ok = true;

        for (index, op) in ops.iter().enumerate() {
            match apply_prepared_one(self, registry, op) {
                Ok(applied_op) => applied.push(MutationAppliedOp {
                    index,
                    ..applied_op
                }),
                Err(err) => {
                    write_ok = false;
                    let diagnostics = err.diagnostics();
                    let flat = flat_diagnostics(diagnostics);
                    failed.push(MutationFailedOp {
                        index,
                        op: prepared_op_name(op),
                        diagnostics: flat.clone(),
                    });
                    failure_diagnostics.extend(flat);
                    if stop_on_write_error || err.is_terminal() {
                        failure_diagnostics.extend(session_flat_diagnostics(self));
                        return Ok(MutationReport {
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
        Ok(MutationReport {
            write_ok,
            check_ok,
            applied,
            failed,
            diagnostics,
        })
    }

    /// Prepare and execute a mutation request.
    ///
    /// # Errors
    ///
    /// Returns diagnostics only when mutation execution cannot produce a
    /// report. Per-operation validation and writer failures are represented in
    /// the returned [`MutationReport`].
    pub fn apply_mutation(
        &mut self,
        registry: &ProviderRegistry,
        request: MutationRequest,
    ) -> Result<MutationReport, DiagnosticSet> {
        let prepared = self.prepare_mutation(request)?;
        self.apply_prepared_mutation(registry, prepared)
    }

    /// Build a schema-shaped default record value.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when `type_name` is not known in the compiled
    /// schema.
    pub fn default_record_value(
        &self,
        type_name: &str,
        materialization: DefaultMaterialization,
    ) -> Result<CfdValue, DiagnosticSet> {
        let record = default_record_for_type(&self.schema, type_name, materialization)?;
        Ok(CfdValue::Object(Box::new(record.object)))
    }
}

fn prepare_one(
    session: &ProjectSession,
    op: MutationOp,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    match op {
        MutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
            materialization,
        } => {
            ensure_source_file(session, &file)?;
            ensure_type_can_insert(session, &actual_type)?;
            ensure_record_key_can_insert(session, &actual_type, &key, None)?;
            let fields = prepare_insert_fields(session, &actual_type, fields, materialization)?;
            Ok(PreparedMutationOp::InsertRecord {
                file,
                sheet,
                actual_type,
                key,
                fields,
            })
        }
        MutationOp::SetField {
            record,
            file,
            path,
            value,
        } => {
            let expected = expected_value_for_path(session, &record, &path)?;
            let (write_file, write_path) =
                effective_write_target_for_set_field(session, &record, &path)?;
            ensure_file_guard_for_file(&record, &write_file, file.as_deref())?;
            let path = cfd_path_to_write_path(&write_path)?;
            let value = coerce_mutation_value(session, &expected.ty, value)?;
            Ok(PreparedMutationOp::SetField {
                record,
                write_file,
                path,
                value,
            })
        }
        MutationOp::RenameRecord {
            record,
            file,
            new_key,
        } => {
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = file.or_else(|| record_file(session, &record).map(ToOwned::to_owned));
            Ok(PreparedMutationOp::RenameRecord {
                record,
                new_key,
                report_file,
            })
        }
        MutationOp::DeleteRecord { record, file } => {
            ensure_file_guard(session, &record, file.as_deref())?;
            let report_file = file.or_else(|| record_file(session, &record).map(ToOwned::to_owned));
            Ok(PreparedMutationOp::DeleteRecord {
                record,
                report_file,
            })
        }
    }
}

fn apply_prepared_one(
    session: &mut ProjectSession,
    registry: &ProviderRegistry,
    op: &PreparedMutationOp,
) -> Result<MutationAppliedOp, MutationApplyError> {
    match op {
        PreparedMutationOp::Pending { op } => {
            let prepared = prepare_one(session, op.clone())
                .map_err(|diagnostics| classify_prepare_error(op, diagnostics))?;
            apply_prepared_one(session, registry, &prepared)
        }
        PreparedMutationOp::InsertRecord {
            file,
            sheet,
            actual_type,
            key,
            fields,
        } => {
            let outcome = session
                .insert_record(registry, file, sheet.as_deref(), key, actual_type, fields)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "insert_record".to_string(),
                record: Some(RecordCoordinate::new(actual_type, key)),
                file: Some(file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::SetField {
            record,
            write_file,
            path,
            value,
            ..
        } => {
            let outcome = session
                .write_field(registry, &record.actual_type, &record.key, path, value)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "set_field".to_string(),
                record: Some(record.clone()),
                file: Some(write_file.clone()),
                outcome,
            })
        }
        PreparedMutationOp::RenameRecord {
            record,
            new_key,
            report_file,
        } => {
            let outcome = session
                .rename_record_key(registry, &record.actual_type, &record.key, new_key)
                .map_err(MutationApplyError::Terminal)?;
            let record = outcome.renamed.as_ref().map_or_else(
                || RecordCoordinate::new(&record.actual_type, new_key),
                |(_, new)| new.clone(),
            );
            Ok(MutationAppliedOp {
                index: 0,
                op: "rename_record".to_string(),
                record: Some(record),
                file: report_file.clone(),
                outcome,
            })
        }
        PreparedMutationOp::DeleteRecord {
            record,
            report_file,
            ..
        } => {
            let outcome = session
                .delete_record(registry, &record.actual_type, &record.key)
                .map_err(MutationApplyError::Terminal)?;
            Ok(MutationAppliedOp {
                index: 0,
                op: "delete_record".to_string(),
                record: Some(record.clone()),
                file: report_file.clone(),
                outcome,
            })
        }
    }
}

#[derive(Debug)]
enum MutationApplyError {
    Recoverable(DiagnosticSet),
    Terminal(DiagnosticSet),
}

impl MutationApplyError {
    const fn is_terminal(&self) -> bool {
        matches!(self, Self::Terminal(_))
    }

    const fn diagnostics(&self) -> &DiagnosticSet {
        match self {
            Self::Recoverable(diagnostics) | Self::Terminal(diagnostics) => diagnostics,
        }
    }
}

fn prepare_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: MutationFields,
    materialization: DefaultMaterialization,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let provided = prepare_provided_insert_fields(session, actual_type, fields)?;
    let provided_names = provided.keys().cloned().collect::<BTreeSet<_>>();
    let mut out = default_missing_fields_for_type(
        &session.schema,
        actual_type,
        materialization,
        &provided_names,
    )?;
    out.extend(provided);
    Ok(out)
}

fn prepare_provided_insert_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: MutationFields,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let mut out = BTreeMap::new();
    match fields {
        MutationFields::Empty => {}
        MutationFields::Json(fields) => {
            for (name, value) in fields {
                let field = schema_field(session, actual_type, &name)?;
                out.insert(name, coerce_json_field_value(session, field, &value)?);
            }
        }
        MutationFields::Cfd(fields) => {
            for (name, value) in fields {
                let field = schema_field(session, actual_type, &name)?;
                out.insert(name, coerce_cfd_field_value(session, field, value)?);
            }
        }
    }
    Ok(out)
}

fn default_record_for_type(
    schema: &CftContainer,
    type_name: &str,
    materialization: DefaultMaterialization,
) -> Result<CfdRecord, DiagnosticSet> {
    ensure_type_can_materialize(schema, type_name)?;
    let mut stack = BTreeSet::new();
    let fields =
        default_fields_for_type_inner(schema, type_name, materialization, &mut stack, None)?;
    Ok(CfdRecord {
        key: String::new(),
        object: CfdObject::new(type_name, fields),
        origin: RecordOrigin::None,
    })
}

fn default_missing_fields_for_type(
    schema: &CftContainer,
    type_name: &str,
    materialization: DefaultMaterialization,
    provided_names: &BTreeSet<String>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let mut stack = BTreeSet::new();
    default_fields_for_type_inner(
        schema,
        type_name,
        materialization,
        &mut stack,
        Some(provided_names),
    )
}

fn default_fields_for_type_inner(
    schema: &CftContainer,
    type_name: &str,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
    skip_fields: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("abstract object type `{type_name}` cannot be default materialized"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("singleton object type `{type_name}` cannot be default materialized"),
        ));
    }
    if !stack.insert(type_name.to_string()) {
        return if materialization == DefaultMaterialization::Minimal {
            Err(one_mutation_error(
                "MUTATION-DEFAULT",
                format!("required inline object type `{type_name}` is recursive"),
            ))
        } else {
            Ok(BTreeMap::new())
        };
    }
    let mut fields = BTreeMap::new();
    for field in &schema_type.all_fields {
        if skip_fields.is_some_and(|skip_fields| skip_fields.contains(&field.name)) {
            continue;
        }
        let value = match materialization {
            DefaultMaterialization::Minimal => default_minimal_for_field(schema, field, stack)?,
            DefaultMaterialization::EditableShape => Some(default_value_for_ty(
                schema,
                &field.ty_ref,
                field.default.as_ref(),
                materialization,
                stack,
            )?),
        };
        if let Some(value) = value {
            fields.insert(field.name.clone(), value);
        }
    }
    stack.remove(type_name);
    Ok(fields)
}

fn default_minimal_for_field(
    schema: &CftContainer,
    field: &coflow_cft::CftSchemaField,
    stack: &mut BTreeSet<String>,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    if field.default.is_some() {
        return Ok(None);
    }
    match non_nullable(&field.ty_ref) {
        CftSchemaTypeRef::Ref(name) => Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!(
                "field `{}` of type `&{name}` has no schema default; provide an explicit value",
                field.name
            ),
        )),
        CftSchemaTypeRef::Named(name) if schema.has_type(name) => {
            ensure_type_can_materialize(schema, name)?;
            let fields = default_fields_for_type_inner(
                schema,
                name,
                DefaultMaterialization::Minimal,
                stack,
                None,
            )?;
            Ok(Some(CfdValue::Object(Box::new(CfdObject::new(
                name.clone(),
                fields,
            )))))
        }
        CftSchemaTypeRef::Named(name) => Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!(
                "field `{}` of type `{name}` has no schema default; provide an explicit value",
                field.name
            ),
        )),
        _ => default_zero_for_ty(schema, &field.ty_ref).map(Some),
    }
}

fn default_value_for_ty(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    declared_default: Option<&CftSchemaDefaultValue>,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    if let Some(default) = declared_default {
        return default_from_schema_default(schema, ty, default, materialization, stack);
    }
    default_zero_for_ty_inner(schema, ty, stack)
}

fn default_from_schema_default(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    default: &CftSchemaDefaultValue,
    materialization: DefaultMaterialization,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    match default {
        CftSchemaDefaultValue::Null => Ok(CfdValue::Null),
        CftSchemaDefaultValue::Int(value) => Ok(CfdValue::Int(*value)),
        CftSchemaDefaultValue::Float(value) => Ok(CfdValue::Float(*value)),
        CftSchemaDefaultValue::Bool(value) => Ok(CfdValue::Bool(*value)),
        CftSchemaDefaultValue::String(value) => Ok(CfdValue::String(value.clone())),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => Ok(CfdValue::Enum(CfdEnumValue {
            enum_name: enum_name.clone(),
            variant: Some(variant.clone()),
            value: *value,
        })),
        CftSchemaDefaultValue::EmptyArray => Ok(CfdValue::Array(Vec::new())),
        CftSchemaDefaultValue::EmptyObject => match non_nullable(ty) {
            CftSchemaTypeRef::Named(name) if schema.has_type(name) => {
                let fields =
                    default_fields_for_type_inner(schema, name, materialization, stack, None)?;
                Ok(CfdValue::Object(Box::new(CfdObject::new(
                    name.clone(),
                    fields,
                ))))
            }
            CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
            _ => default_zero_for_ty_inner(schema, ty, stack),
        },
    }
}

fn default_zero_for_ty(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
) -> Result<CfdValue, DiagnosticSet> {
    let mut stack = BTreeSet::new();
    default_zero_for_ty_inner(schema, ty, &mut stack)
}

fn default_zero_for_ty_inner(
    schema: &CftContainer,
    ty: &CftSchemaTypeRef,
    stack: &mut BTreeSet<String>,
) -> Result<CfdValue, DiagnosticSet> {
    match ty {
        CftSchemaTypeRef::Int => Ok(CfdValue::Int(0)),
        CftSchemaTypeRef::Float => Ok(CfdValue::Float(0.0)),
        CftSchemaTypeRef::Bool => Ok(CfdValue::Bool(false)),
        CftSchemaTypeRef::String => Ok(CfdValue::String(String::new())),
        CftSchemaTypeRef::Ref(_) | CftSchemaTypeRef::Nullable(_) => Ok(CfdValue::Null),
        CftSchemaTypeRef::Array(_) => Ok(CfdValue::Array(Vec::new())),
        CftSchemaTypeRef::Dict(_, _) => Ok(CfdValue::Dict(Vec::new())),
        CftSchemaTypeRef::Named(name) if schema.has_enum(name) => {
            let value = schema
                .resolve_enum(name)
                .and_then(|enm| enm.variants.first());
            Ok(value.map_or_else(
                || {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: None,
                        value: 0,
                    })
                },
                |variant| {
                    CfdValue::Enum(CfdEnumValue {
                        enum_name: name.clone(),
                        variant: Some(variant.name.clone()),
                        value: variant.value,
                    })
                },
            ))
        }
        CftSchemaTypeRef::Named(name) => {
            ensure_type_can_materialize(schema, name)?;
            let fields = default_fields_for_type_inner(
                schema,
                name,
                DefaultMaterialization::EditableShape,
                stack,
                None,
            )?;
            Ok(CfdValue::Object(Box::new(CfdObject::new(
                name.clone(),
                fields,
            ))))
        }
    }
}

fn ensure_type_can_materialize(
    schema: &CftContainer,
    type_name: &str,
) -> Result<(), DiagnosticSet> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{type_name}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("abstract object type `{type_name}` cannot be default materialized"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-DEFAULT",
            format!("singleton object type `{type_name}` cannot be default materialized"),
        ));
    }
    Ok(())
}

fn coerce_mutation_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: MutationValue,
) -> Result<CfdValue, DiagnosticSet> {
    let value = match value {
        MutationValue::Json(value) => coerce_json_value(session, expected, &value),
        MutationValue::Cfd(value) => coerce_cfd_value(session, expected, value),
    }?;
    validate_value_for_write(session, expected, &value)?;
    Ok(value)
}

fn coerce_json_field_value(
    session: &ProjectSession,
    field: &coflow_cft::CftSchemaField,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let value = coerce_json_value(session, &field.ty_ref, value)?;
    validate_value_for_write(session, &field.ty_ref, &value)?;
    Ok(value)
}

fn coerce_cfd_field_value(
    session: &ProjectSession,
    field: &coflow_cft::CftSchemaField,
    value: CfdValue,
) -> Result<CfdValue, DiagnosticSet> {
    let value = coerce_cfd_value(session, &field.ty_ref, value)?;
    validate_value_for_write(session, &field.ty_ref, &value)?;
    Ok(value)
}

fn coerce_json_value(
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
        CftSchemaTypeRef::Nullable(inner) => coerce_json_value(session, inner, value),
        CftSchemaTypeRef::Array(inner) => {
            let items = value
                .as_array()
                .ok_or_else(|| one_value_error("expected array"))?;
            items
                .iter()
                .map(|item| coerce_json_value(session, inner, item))
                .collect::<Result<Vec<_>, _>>()
                .map(CfdValue::Array)
        }
        CftSchemaTypeRef::Dict(key, item) => coerce_json_dict_value(session, key, item, value),
        CftSchemaTypeRef::Ref(target_type) => json_ref_key(value)
            .map(|key| CfdValue::Ref(key.to_string()))
            .ok_or_else(|| one_value_error(format!("expected record key for `&{target_type}`"))),
        CftSchemaTypeRef::Named(name) if session.schema.has_enum(name) => {
            let variant = value
                .as_str()
                .ok_or_else(|| one_value_error(format!("expected enum variant for `{name}`")))?;
            enum_value(session, name, variant).map(CfdValue::Enum)
        }
        CftSchemaTypeRef::Named(name) => coerce_json_named_value(session, name, value),
    }
}

fn json_ref_key(value: &Value) -> Option<&str> {
    if let Some(key) = value.as_str() {
        return Some(key);
    }
    let object = value.as_object()?;
    if object.len() != 1 {
        return None;
    }
    object.get("$ref")?.as_str()
}

fn coerce_cfd_value(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: CfdValue,
) -> Result<CfdValue, DiagnosticSet> {
    if let CftSchemaTypeRef::Nullable(inner) = expected {
        return if matches!(value, CfdValue::Null) {
            Ok(CfdValue::Null)
        } else {
            coerce_cfd_value(session, inner, value)
        };
    }
    match (expected, value) {
        (CftSchemaTypeRef::Int, value @ CfdValue::Int(_))
        | (CftSchemaTypeRef::Float, value @ CfdValue::Float(_))
        | (CftSchemaTypeRef::Bool, value @ CfdValue::Bool(_))
        | (CftSchemaTypeRef::String, value @ CfdValue::String(_)) => Ok(value),
        (CftSchemaTypeRef::Array(inner), CfdValue::Array(items)) => items
            .into_iter()
            .map(|item| coerce_cfd_value(session, inner, item))
            .collect::<Result<Vec<_>, DiagnosticSet>>()
            .map(CfdValue::Array),
        (CftSchemaTypeRef::Dict(key_type, item_type), CfdValue::Dict(entries)) => entries
            .into_iter()
            .map(|(key, item)| {
                Ok((
                    coerce_cfd_dict_key(session, key_type, key)?,
                    coerce_cfd_value(session, item_type, item)?,
                ))
            })
            .collect::<Result<Vec<_>, DiagnosticSet>>()
            .map(CfdValue::Dict),
        (CftSchemaTypeRef::Named(name), CfdValue::Enum(enum_value))
            if session.schema.has_enum(name) =>
        {
            coerce_cfd_enum_value(session, name, enum_value).map(CfdValue::Enum)
        }
        (CftSchemaTypeRef::Named(expected_type), CfdValue::Object(record)) => {
            ensure_object_type_assignable(session, expected_type, record.actual_type())?;
            let mut record = *record;
            let actual_type = record.actual_type().to_string();
            record.fields = coerce_cfd_object_fields(
                session,
                &actual_type,
                std::mem::take(&mut record.fields),
            )?;
            Ok(CfdValue::Object(Box::new(record)))
        }
        (CftSchemaTypeRef::Ref(_expected_type), CfdValue::Ref(target_key)) => {
            if target_key.is_empty() {
                return Err(one_value_error("reference key must not be empty"));
            }
            Ok(CfdValue::Ref(target_key))
        }
        (CftSchemaTypeRef::Named(_), CfdValue::Ref(_)) => Err(one_value_error(
            "inline object fields do not accept record refs",
        )),
        _ => Err(one_value_error("value does not match expected schema type")),
    }
}

fn coerce_cfd_object_fields(
    session: &ProjectSession,
    actual_type: &str,
    fields: BTreeMap<String, CfdValue>,
) -> Result<BTreeMap<String, CfdValue>, DiagnosticSet> {
    fields
        .into_iter()
        .map(|(name, value)| {
            let field = schema_field(session, actual_type, &name)?;
            Ok((name, coerce_cfd_field_value(session, field, value)?))
        })
        .collect()
}

fn validate_value_for_write(
    session: &ProjectSession,
    expected: &CftSchemaTypeRef,
    value: &CfdValue,
) -> Result<(), DiagnosticSet> {
    write_rules::validate_value_for_write(session, expected, value, "MUTATION-SHAPE", "MUTATION")
}

fn coerce_cfd_dict_key(
    session: &ProjectSession,
    key_type: &CftSchemaTypeRef,
    key: CfdDictKey,
) -> Result<CfdDictKey, DiagnosticSet> {
    match (key_type, key) {
        (CftSchemaTypeRef::Nullable(inner), key) => coerce_cfd_dict_key(session, inner, key),
        (CftSchemaTypeRef::String, key @ CfdDictKey::String(_))
        | (CftSchemaTypeRef::Int, key @ CfdDictKey::Int(_)) => Ok(key),
        (CftSchemaTypeRef::Named(enum_name), CfdDictKey::Enum(value))
            if session.schema.has_enum(enum_name) =>
        {
            coerce_cfd_enum_value(session, enum_name, value).map(CfdDictKey::Enum)
        }
        _ => Err(one_value_error(
            "dict key does not match expected schema type",
        )),
    }
}

fn coerce_cfd_enum_value(
    session: &ProjectSession,
    enum_name: &str,
    mut value: CfdEnumValue,
) -> Result<CfdEnumValue, DiagnosticSet> {
    if value.enum_name != enum_name {
        return Err(one_value_error(format!(
            "expected enum `{enum_name}`, got `{}`",
            value.enum_name
        )));
    }
    if let Some(variant) = value.variant.as_ref() {
        // The variant name is authoritative — the backing int on the wire
        // may be stale (e.g. the editor picks a new variant but reuses the
        // previously-selected int). Re-derive the int from the schema
        // instead of forcing callers to keep the two in sync.
        let expected_value = session
            .schema
            .enum_variant_value(enum_name, variant)
            .ok_or_else(|| {
                one_value_error(format!("unknown enum variant `{enum_name}.{variant}`"))
            })?;
        value.value = expected_value;
    }
    Ok(value)
}

fn coerce_json_dict_value(
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
        return coerce_json_special_dict(session, key_type, item_type, entries);
    }

    object
        .iter()
        .map(|(key, entry_value)| {
            let key_value = Value::String(key.clone());
            Ok((
                coerce_dict_key(session, key_type, &key_value)?,
                coerce_json_value(session, item_type, entry_value)?,
            ))
        })
        .collect::<Result<Vec<_>, DiagnosticSet>>()
        .map(CfdValue::Dict)
}

fn coerce_json_special_dict(
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
                coerce_json_value(session, item_type, value)?,
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

fn coerce_json_named_value(
    session: &ProjectSession,
    expected_type: &str,
    value: &Value,
) -> Result<CfdValue, DiagnosticSet> {
    let object = value
        .as_object()
        .ok_or_else(|| one_value_error(format!("expected object for `{expected_type}`")))?;
    let actual_type = actual_object_type(object, expected_type)?;
    ensure_object_type_assignable(session, expected_type, &actual_type)?;
    let fields = coerce_json_object_fields(session, &actual_type, object)?;
    Ok(CfdValue::Object(Box::new(CfdObject::new(
        actual_type,
        fields,
    ))))
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
    write_rules::ensure_object_type_assignable(
        &session.schema,
        expected_type,
        actual_type,
        "MUTATION-VALUE",
        "MUTATION",
    )
}

fn coerce_json_object_fields(
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
            coerce_json_field_value(session, field, field_value)?,
        );
    }
    Ok(fields)
}

#[derive(Debug, Clone)]
struct ExpectedValue {
    ty: CftSchemaTypeRef,
}

fn expected_value_for_path(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[CfdPathSegment],
) -> Result<ExpectedValue, DiagnosticSet> {
    let current = write_rules::expected_type_for_cfd_path(
        &session.schema,
        &coordinate.actual_type,
        path,
        "MUTATION-PATH",
        "MUTATION",
    )?;
    Ok(ExpectedValue { ty: current })
}

fn cfd_path_to_write_path(
    path: &[CfdPathSegment],
) -> Result<Vec<coflow_api::WriteFieldPathSegment>, DiagnosticSet> {
    if path.is_empty() {
        return Err(one_path_error("mutation path must not be empty"));
    }
    Ok(write_rules::cfd_path_to_write_path(path))
}

fn effective_write_target_for_set_field(
    session: &ProjectSession,
    coordinate: &RecordCoordinate,
    path: &[CfdPathSegment],
) -> Result<(String, Vec<CfdPathSegment>), DiagnosticSet> {
    let record_ref = session
        .records
        .get_by_coordinate(&coordinate.actual_type, &coordinate.key)
        .ok_or_else(|| {
            one_path_error(format!(
                "record `{}.{}` was not found",
                coordinate.actual_type, coordinate.key
            ))
        })?;
    let Some(CfdPathSegment::Field(top_field)) = path.first() else {
        return Ok((record_ref.display_path.clone(), path.to_vec()));
    };
    let _record = session.model.record(record_ref.id).ok_or_else(|| {
        one_path_error(format!(
            "record `{}.{}` was not found in the data model",
            coordinate.actual_type, coordinate.key
        ))
    })?;
    let cfd_path = CfdPath {
        segments: path.to_vec(),
    };
    let Some((source_id, source_path)) = session.model.spread_source_path(record_ref.id, &cfd_path)
    else {
        return Ok((record_ref.display_path.clone(), path.to_vec()));
    };
    session
        .records
        .get(source_id)
        .map(|source_ref| {
            (
                source_ref.display_path.clone(),
                source_path.segments.clone(),
            )
        })
        .ok_or_else(|| {
            one_path_error(format!(
                "spread source for field `{top_field}` is no longer indexed"
            ))
        })
}

fn schema_field<'a>(
    session: &'a ProjectSession,
    actual_type: &str,
    field_name: &str,
) -> Result<&'a coflow_cft::CftSchemaField, DiagnosticSet> {
    let Some(schema_type) = session.schema.resolve_type(actual_type) else {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{actual_type}`"),
        ));
    };
    schema_type
        .all_fields
        .iter()
        .find(|field| field.name == field_name)
        .ok_or_else(|| {
            one_path_error(format!(
                "unknown field `{field_name}` on type `{actual_type}`"
            ))
        })
}

fn ensure_source_file(session: &ProjectSession, file: &str) -> Result<(), DiagnosticSet> {
    if session.files.source_files().contains(file) {
        return Ok(());
    }
    Err(one_mutation_error(
        "MUTATION-FILE",
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
        return Err(one_mutation_error(
            "MUTATION-FILE-GUARD",
            format!(
                "record `{}.{}` was not found for file guard `{expected_file}`",
                coordinate.actual_type, coordinate.key
            ),
        ));
    };
    if actual_file == expected_file {
        return Ok(());
    }
    Err(one_mutation_error(
        "MUTATION-FILE-GUARD",
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
    Err(one_mutation_error(
        "MUTATION-FILE-GUARD",
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
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown insert type `{actual_type}`"),
        ));
    };
    if schema_type.is_abstract {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("abstract type `{actual_type}` cannot be inserted"),
        ));
    }
    if schema_type.is_singleton {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("singleton type `{actual_type}` cannot be inserted"),
        ));
    }
    Ok(())
}

fn ensure_record_key_can_insert(
    session: &ProjectSession,
    actual_type: &str,
    key: &str,
    current_record: Option<coflow_data_model::CfdRecordId>,
) -> Result<(), DiagnosticSet> {
    write_rules::ensure_record_key_available(
        session,
        actual_type,
        key,
        current_record,
        "MUTATION-INSERT",
        "MUTATION",
    )
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

fn prepared_op_name(op: &PreparedMutationOp) -> String {
    match op {
        PreparedMutationOp::Pending { op } => mutation_op_name(op).to_string(),
        PreparedMutationOp::InsertRecord { .. } => "insert_record".to_string(),
        PreparedMutationOp::SetField { .. } => "set_field".to_string(),
        PreparedMutationOp::RenameRecord { .. } => "rename_record".to_string(),
        PreparedMutationOp::DeleteRecord { .. } => "delete_record".to_string(),
    }
}

const fn mutation_op_name(op: &MutationOp) -> &'static str {
    match op {
        MutationOp::InsertRecord { .. } => "insert_record",
        MutationOp::SetField { .. } => "set_field",
        MutationOp::RenameRecord { .. } => "rename_record",
        MutationOp::DeleteRecord { .. } => "delete_record",
    }
}

fn classify_prepare_error(op: &MutationOp, diagnostics: DiagnosticSet) -> MutationApplyError {
    let terminal_insert_conflict = matches!(op, MutationOp::InsertRecord { .. })
        && diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "MUTATION-INSERT" && diagnostic.message.contains("already exists")
        });
    if terminal_insert_conflict {
        MutationApplyError::Terminal(diagnostics)
    } else {
        MutationApplyError::Recoverable(diagnostics)
    }
}

fn one_path_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-PATH", message)
}

fn one_value_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-VALUE", message)
}

fn one_mutation_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: "MUTATION".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}

fn session_flat_diagnostics(session: &ProjectSession) -> Vec<FlatDiagnostic> {
    session
        .diagnostics
        .as_set()
        .diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let location = session.diagnostics.logical_location(index);
            let actual_type = location.and_then(|l| l.actual_type.clone());
            let record_key = location.and_then(|l| l.record_key.clone());
            let field_path = location.and_then(|l| l.field_path.clone());
            diagnostic.flat_view(actual_type, record_key, field_path)
        })
        .collect()
}

fn flat_diagnostics(diagnostics: &DiagnosticSet) -> Vec<FlatDiagnostic> {
    diagnostics
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.flat_view(None, None, None))
        .collect()
}
