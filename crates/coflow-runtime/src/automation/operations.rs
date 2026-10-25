use super::{transaction::Candidate, Failure, Result};
use crate::{CfdPathSegment, CfdSource, CfdSourcePath, CfdValue, RecordOrigin};
use coflow_language::{
    cfd::CfdRecord as AstRecord,
    cft::{CftSchema, CftValueType},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordTarget {
    #[serde(rename = "type")]
    pub actual_type: String,
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldPath {
    Field { name: String },
    Index { index: usize },
    DictKey { key: super::ModelValue },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputValue {
    Expression { source: String },
    Model { value: super::ModelValue },
}
pub type RecordFields = BTreeMap<String, InputValue>;
impl InputValue {
    fn resolve(&self, schema: &CftSchema, expected: &CftValueType) -> Result<CfdValue> {
        match self {
            Self::Expression { source } => super::values::expression(schema, expected, source),
            Self::Model { value } => value.resolve(schema, expected),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    CreateRecord {
        record: RecordTarget,
        file: String,
        #[serde(default)]
        fields: RecordFields,
    },
    SetField {
        record: RecordTarget,
        path: Vec<FieldPath>,
        value: InputValue,
        #[serde(default)]
        expected: Option<InputValue>,
    },
    UnsetField {
        record: RecordTarget,
        path: Vec<FieldPath>,
    },
    RenameRecord {
        record: RecordTarget,
        new_id: String,
    },
    DeleteRecord {
        record: RecordTarget,
    },
    TransferRecord {
        record: RecordTarget,
        to_file: String,
    },
    SetDimensionValue {
        record: RecordTarget,
        dimension: String,
        variant: String,
        field: String,
        #[serde(default)]
        path: Vec<FieldPath>,
        value: InputValue,
        #[serde(default)]
        file: Option<String>,
    },
    ClearDimensionValue {
        record: RecordTarget,
        dimension: String,
        variant: String,
        field: String,
    },
    ArrayInsert {
        record: RecordTarget,
        path: Vec<FieldPath>,
        index: usize,
        value: InputValue,
    },
    ArrayRemove {
        record: RecordTarget,
        path: Vec<FieldPath>,
        index: usize,
    },
    ArrayMove {
        record: RecordTarget,
        path: Vec<FieldPath>,
        from: usize,
        to: usize,
    },
    DictSet {
        record: RecordTarget,
        path: Vec<FieldPath>,
        key: InputValue,
        value: InputValue,
    },
    DictRemove {
        record: RecordTarget,
        path: Vec<FieldPath>,
        key: InputValue,
    },
    SetWhere {
        selector: super::Selector,
        path: Vec<FieldPath>,
        value: InputValue,
        expected_count: usize,
    },
}

impl Candidate {
    pub fn apply_operations(&mut self, operations: &[Operation]) -> Result<()> {
        self.capture_typed_references()?;
        let encoded = operations
            .iter()
            .map(super::json)
            .collect::<Result<Vec<_>>>()?;
        for (index, operation) in operations.iter().enumerate() {
            self.operation_details = json!({});
            if let Err(failure) = self.apply_operation(operation) {
                return Err(self.record_failed(index, &encoded, failure));
            }
            self.operations.push(json!({"index":index,"status":"success","operation":operation,"details":self.operation_details}));
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    fn apply_operation(&mut self, operation: &Operation) -> Result<()> {
        let schema_session = self.schema_session()?;
        let schema = schema_session
            .schema()
            .ok_or_else(|| Failure::from(schema_session.diagnostics().as_set().clone()))?;
        let writer = crate::cfd_loader::CfdWriter::new();
        match operation {
            Operation::CreateRecord {
                record,
                file,
                fields,
            } => {
                if find_record(self, record).is_ok() {
                    return Err(Failure::conflict("record already exists"));
                }
                let meta = schema
                    .resolve_type(&record.actual_type)
                    .ok_or_else(|| Failure::missing("unknown record type"))?;
                if meta.is_abstract || meta.is_data() {
                    return Err(Failure::request(
                        "record requires a concrete table or singleton type",
                    ));
                }
                let path = self.resolve(file, true)?;
                self.track(&path)?;
                if !self.shadow(&path).exists() {
                    self.write(&path, b"")?;
                }
                let source = CfdSource {
                    location: CfdSourcePath::new(self.shadow(&path)),
                    display_name: file.clone(),
                };
                for (name, value) in fields {
                    let expected = &schema
                        .field(&record.actual_type, name)
                        .ok_or_else(|| Failure::missing("unknown field"))?
                        .value_type;
                    self.replace_typed_references(
                        schema,
                        record,
                        None,
                        &[CfdPathSegment::Field(name.clone())],
                        Some(value),
                        expected,
                    )?;
                }
                let fields = fields
                    .iter()
                    .map(|(name, value)| {
                        let ty = &schema
                            .field(&record.actual_type, name)
                            .ok_or_else(|| Failure::missing(format!("unknown field `{name}`")))?
                            .value_type;
                        Ok((name.clone(), value.resolve(schema, ty)?))
                    })
                    .collect::<Result<_>>()?;
                writer.insert_record(&crate::InsertRecordRequest {
                    source: &source,
                    record_id: &record.id,
                    actual_type: &record.actual_type,
                    fields: &fields,
                    schema,
                    before: None,
                })?;
                self.publish_writer(&writer)?;
                self.include(&path)?;
            }
            Operation::SetField {
                record,
                path,
                value,
                expected,
            } => {
                let (file, ast, text) = find_record(self, record)?;
                let current_record = path_record(schema, record, &ast, &text)?;
                let path = internal_path_for_record(schema, &current_record, path, Vec::new())?;
                let ty = crate::write_rules::expected_type_for_record_path(
                    schema,
                    &current_record,
                    &path,
                    "CLI-PATH",
                    "CLI",
                )?;
                self.replace_typed_references(schema, record, None, &path, Some(value), &ty)?;
                let value = value.resolve(schema, &ty)?;
                let current = read_value(schema, record, &ast, &text, &path)?;
                if let Some(expected) = expected {
                    let expected_value = expected.resolve(schema, &ty)?;
                    if current.as_ref() != Some(&expected_value) {
                        return Err(Failure::conflict("old value precondition failed").with_result(json!({"precondition":{
                        "view":"effective","expected":super::model_value::encode(schema,&ty,Some(&expected_value))?,
                        "actual":super::model_value::encode(schema,&ty,current.as_ref())?}})));
                    }
                }
                // 只有实际存在的赋值才能作为无变化；与默认值相同也必须建立显式数据。
                let explicitly_assigned=path.first().is_some_and(|p|matches!(p,CfdPathSegment::Field(name) if ast.fields.iter().any(|f|f.name==*name)));
                if explicitly_assigned && current.as_ref() == Some(&value) {
                    return Ok(());
                }
                let top = materialized_top(schema, record, &ast, &text, &path, &value)?;
                if !explicitly_assigned {
                    if let (Some(CfdPathSegment::Field(field)), Some(top)) =
                        (path.first(), top.as_ref())
                    {
                        let meta = schema
                            .field(&record.actual_type, field)
                            .ok_or_else(|| Failure::missing("unknown top-level field"))?;
                        let location = schema
                            .resolve_type(&meta.declaring_type)
                            .and_then(|ty| schema.source(&ty.module))
                            .map(|source| -> Result<_> {
                                Ok(super::schema_queries::location(
                                    self.original.root_dir(),
                                    &self.unshadow(&source.path)?,
                                    &source.source,
                                    meta.span,
                                ))
                            })
                            .transpose()?;
                        // 未赋值的父字段只有显式声明默认值后才可物化。
                        self.materialized.push(
                            json!({"record":record,"field":field,"origin":"schema_default",
                            "location":location,
                            "value":super::ModelValue::from_model(schema,&meta.value_type,top)?}),
                        );
                    }
                }
                writer.write_field(&crate::WriteCellRequest {
                    origin: &RecordOrigin::File {
                        path: file,
                        span: None,
                    },
                    record_id: &record.id,
                    actual_type: &record.actual_type,
                    field_path: &path,
                    new_value: &value,
                    materialized_top_level: top
                        .as_ref()
                        .map(crate::api::FieldMaterialization::Value),
                    schema,
                })?;
                self.publish_writer(&writer)?;
            }
            Operation::UnsetField { record, path } => {
                let (file, ast, text) = find_record(self, record)?;
                let current_record = path_record(schema, record, &ast, &text)?;
                let internal = internal_path_for_record(schema, &current_record, path, Vec::new())?;
                let expected = crate::write_rules::expected_type_for_record_path(
                    schema,
                    &current_record,
                    &internal,
                    "CLI-PATH",
                    "CLI",
                )?;
                self.replace_typed_references(schema, record, None, &internal, None, &expected)?;
                writer.unset_field(
                    &RecordOrigin::File {
                        path: file,
                        span: None,
                    },
                    &record.id,
                    &record.actual_type,
                    &internal,
                    schema,
                )?;
                self.publish_writer(&writer)?;
            }
            Operation::RenameRecord { record, new_id } => {
                let (file, _, _) = find_record(self, record)?;
                if find_record(
                    self,
                    &RecordTarget {
                        actual_type: record.actual_type.clone(),
                        id: new_id.clone(),
                    },
                )
                .is_ok()
                {
                    return Err(Failure::conflict("new record id already exists"));
                }
                super::rewrite::rename_references(self, schema, record, new_id)?;
                writer.rename_record(&crate::RenameRecordRequest {
                    origin: &RecordOrigin::File {
                        path: file,
                        span: None,
                    },
                    old_id: &record.id,
                    new_id,
                    actual_type: &record.actual_type,
                })?;
                rewrite_dimensions(self, &writer, schema, record, Some(new_id))?;
                self.publish_writer(&writer)?;
                super::rewrite::rename_mapping(self, schema, record, new_id)?;
                self.rename_typed_references(schema, record, new_id);
            }
            Operation::DeleteRecord { record } => {
                let (file, _, _) = find_record(self, record)?;
                writer.delete_record(&crate::DeleteRecordRequest {
                    origin: &RecordOrigin::File {
                        path: file,
                        span: None,
                    },
                    record_id: &record.id,
                    actual_type: &record.actual_type,
                })?;
                rewrite_dimensions(self, &writer, schema, record, None)?;
                self.publish_writer(&writer)?;
            }
            Operation::TransferRecord { record, to_file } => {
                let (file, _, _) = find_record(self, record)?;
                let target = self.resolve(to_file, true)?;
                self.track(&target)?;
                if self.shadow(&target) == file {
                    return Ok(());
                }
                if !self.shadow(&target).exists() {
                    self.write(&target, b"")?;
                }
                writer.transfer_record(
                    &crate::WriteRecordRef {
                        origin: &RecordOrigin::File {
                            path: file,
                            span: None,
                        },
                        record_id: &record.id,
                        actual_type: &record.actual_type,
                    },
                    &CfdSource {
                        location: CfdSourcePath::new(self.shadow(&target)),
                        display_name: to_file.clone(),
                    },
                    None,
                )?;
                self.publish_writer(&writer)?;
                self.include(&target)?;
            }
            Operation::SetDimensionValue {
                record,
                dimension,
                variant,
                field,
                path,
                value,
                file,
            } => {
                self.dimension_write(
                    schema,
                    record,
                    dimension,
                    variant,
                    field,
                    path,
                    Some(value),
                    file.as_deref(),
                )?;
            }
            Operation::ClearDimensionValue {
                record,
                dimension,
                variant,
                field,
            } => {
                self.dimension_write(schema, record, dimension, variant, field, &[], None, None)?;
            }
            Operation::SetWhere {
                selector,
                path,
                value,
                expected_count,
            } => {
                let session = self.data_session()?;
                if session.queries().has_diagnostics() {
                    return Err(session.queries().diagnostics().as_set().clone().into());
                }
                let records = super::query::selected_records(&session, selector)?;
                if records.len() != *expected_count {
                    return Err(Failure::conflict(format!(
                        "expected {expected_count} records, matched {}",
                        records.len()
                    )).with_result(json!({"precondition":{"kind":"count","expected":expected_count,"actual":records.len()}})));
                }
                self.operation_details = json!({"matched_count":records.len(),"targets":records.iter().map(|record|
                    json!({"type":record.actual_type(),"id":record.id()})).collect::<Vec<_>>()});
                for record in records {
                    self.apply_operation(&Operation::SetField {
                        record: RecordTarget {
                            actual_type: record.actual_type().into(),
                            id: record.id().into(),
                        },
                        path: path.clone(),
                        value: value.clone(),
                        expected: None,
                    })?;
                }
            }
            other => self.collection(schema, other)?,
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    fn collection(&mut self, schema: &CftSchema, operation: &Operation) -> Result<()> {
        let (Operation::ArrayInsert { record, path, .. }
        | Operation::ArrayRemove { record, path, .. }
        | Operation::ArrayMove { record, path, .. }
        | Operation::DictSet { record, path, .. }
        | Operation::DictRemove { record, path, .. }) = operation
        else {
            return Err(Failure::request("operation is not a collection edit"));
        };
        let (_, ast, text) = find_record(self, record)?;
        let current_record = path_record(schema, record, &ast, &text)?;
        let internal = internal_path_for_record(schema, &current_record, path, Vec::new())?;
        let expected = crate::write_rules::expected_type_for_record_path(
            schema,
            &current_record,
            &internal,
            "CLI-PATH",
            "CLI",
        )?;
        let mut retained = self.collection_typed_references(schema, record, &internal, operation);
        if let Operation::DictSet { key, .. } | Operation::DictRemove { key, .. } = operation {
            if let CftValueType::Dict(key_type, _) = &expected {
                let key = crate::data_model::format_cfd_dict_key(&as_key(
                    key.resolve(schema, key_type)?,
                )?);
                let mut replaced = internal.clone();
                replaced.push(CfdPathSegment::DictKey(key));
                retained.retain(|reference| !reference.path.starts_with(&replaced));
            }
        }
        let mut value = read_value(schema, record, &ast, &text, &internal)?
            .ok_or_else(|| Failure::missing("collection value is missing"))?;
        match (operation, &mut value, &expected) {
            (
                Operation::ArrayInsert { index, value, .. },
                CfdValue::Array(items),
                CftValueType::Array(inner),
            ) => {
                if *index > items.len() {
                    return Err(Failure::request("array index out of bounds"));
                }
                items.insert(*index, value.resolve(schema, inner)?);
            }
            (Operation::ArrayRemove { index, .. }, CfdValue::Array(items), _) => {
                if *index >= items.len() {
                    return Err(Failure::request("array index out of bounds"));
                }
                items.remove(*index);
            }
            (Operation::ArrayMove { from, to, .. }, CfdValue::Array(items), _) => {
                if *from >= items.len() || *to >= items.len() {
                    return Err(Failure::request("array index out of bounds"));
                }
                let item = items.remove(*from);
                items.insert(*to, item);
            }
            (
                Operation::DictSet { key, value, .. },
                CfdValue::Dict(items),
                CftValueType::Dict(key_ty, value_ty),
            ) => {
                let key = as_key(key.resolve(schema, key_ty)?)?;
                let value = value.resolve(schema, value_ty)?;
                if let Some((_, v)) = items.iter_mut().find(|(k, _)| k == &key) {
                    *v = value;
                } else {
                    items.push((key, value));
                }
            }
            (
                Operation::DictRemove { key, .. },
                CfdValue::Dict(items),
                CftValueType::Dict(key_ty, _),
            ) => {
                let key = as_key(key.resolve(schema, key_ty)?)?;
                let index = items
                    .iter()
                    .position(|(k, _)| k == &key)
                    .ok_or_else(|| Failure::missing("dictionary key does not exist"))?;
                items.remove(index);
            }
            _ => {
                return Err(Failure::request(
                    "collection operation does not match the field type",
                ))
            }
        }
        self.apply_operation(&Operation::SetField {
            record: record.clone(),
            path: path.clone(),
            value: InputValue::Model {
                value: super::ModelValue::from_model(schema, &expected, &value)?,
            },
            expected: None,
        })?;
        self.typed_references.extend(retained);
        match (operation, &expected) {
            (Operation::ArrayInsert { index, value, .. }, CftValueType::Array(inner)) => {
                let mut path = internal;
                path.push(CfdPathSegment::Index(*index));
                self.replace_typed_references(schema, record, None, &path, Some(value), inner)?;
            }
            (Operation::DictSet { key, value, .. }, CftValueType::Dict(key_type, value_type)) => {
                let mut path = internal;
                path.push(CfdPathSegment::DictKey(
                    crate::data_model::format_cfd_dict_key(&as_key(
                        key.resolve(schema, key_type)?,
                    )?),
                ));
                self.replace_typed_references(
                    schema,
                    record,
                    None,
                    &path,
                    Some(value),
                    value_type,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "写入入口显式区分主体、字段、维度、来源及前置条件。"
    )]
    fn dimension_write(
        &mut self,
        schema: &CftSchema,
        record: &RecordTarget,
        dimension: &str,
        variant: &str,
        field: &str,
        path: &[FieldPath],
        value: Option<&InputValue>,
        file: Option<&str>,
    ) -> Result<()> {
        let (owner, _, _) = find_record(self, record)?;
        let source_type = schema
            .resolve_type(&record.actual_type)
            .ok_or_else(|| Failure::missing("unknown record type"))?;
        let source_field = schema
            .field(&record.actual_type, field)
            .ok_or_else(|| Failure::missing("unknown field"))?;
        if source_field
            .dimension
            .as_ref()
            .is_none_or(|d| d.dimension.as_str() != dimension)
        {
            return Err(Failure::request("field is not bound to this dimension"));
        }
        let dimension_schema = schema
            .all_dimensions()
            .find(|d| d.name.as_str() == dimension)
            .ok_or_else(|| Failure::missing("unknown dimension"))?;
        let project = self.project()?;
        let mut existing = None;
        for source in self.data_sources()? {
            let text = &source.text;
            let ast = &source.ast;
            for block in ast
                .dimensions
                .iter()
                .filter(|b| b.dimension == dimension && b.variant == variant)
            {
                for target in block.records.iter().filter(|r| {
                    r.id == record.id
                        && schema.inheritance_root(&r.type_name)
                            == schema.inheritance_root(&record.actual_type)
                }) {
                    if let Some(f) = target.fields.iter().find(|f| f.name == field) {
                        if existing.is_some() {
                            return Err(Failure::conflict("dimension field has multiple sources"));
                        }
                        let span = super::query::ast_value_span(&f.value);
                        let value = super::values::expression(
                            schema,
                            &source_field.value_type,
                            &text[span.start..span.end],
                        )?;
                        existing = Some((source.absolute.clone(), value));
                    }
                }
            }
        }
        if existing.is_none() && value.is_none() {
            return Ok(());
        }
        let current_record = record_with_fields(
            record,
            existing
                .as_ref()
                .map(|(_, value)| (field.to_string(), value.clone()))
                .into_iter()
                .collect(),
        )?;
        let constraint_path = internal_path_for_record(
            schema,
            &current_record,
            path,
            vec![CfdPathSegment::Field(field.into())],
        )?;
        let constraint_type = crate::write_rules::expected_type_for_record_path(
            schema,
            &current_record,
            &constraint_path,
            "CLI-PATH",
            "CLI",
        )?;
        self.replace_typed_references(
            schema,
            record,
            Some(&(dimension.into(), variant.into())),
            &constraint_path,
            value,
            &constraint_type,
        )?;
        let destination = if let Some((path, _)) = &existing {
            path.clone()
        } else if let Some(file) = file {
            let original = self.resolve(file, true)?;
            self.track(&original)?;
            self.shadow(&original)
        } else {
            crate::query::dimension_destination_for_owner(&project, &owner, dimension)?
        };
        let new_value = if let Some(value) = value {
            if path.is_empty() {
                Some(value.resolve(schema, &source_field.value_type)?)
            } else {
                let mut current = existing.as_ref().map(|(_, v)| v.clone()).ok_or_else(|| {
                    Failure::missing(
                        "set the complete dimension field before editing a nested value",
                    )
                })?;
                replace_nested(
                    &mut current,
                    &constraint_path[1..],
                    value.resolve(schema, &constraint_type)?,
                )?;
                Some(current)
            }
        } else {
            None
        };
        if existing.as_ref().map(|(_, v)| v) == new_value.as_ref() {
            return Ok(());
        }
        let original = self.unshadow(&destination)?;
        self.track(&original)?;
        let writer = crate::cfd_loader::CfdWriter::new();
        writer.write_dimension_value(&crate::WriteDimensionValueRequest {
            source: &CfdSource {
                location: CfdSourcePath::new(destination),
                display_name: field.into(),
            },
            schema: crate::DimensionSourceSchema {
                schema,
                dimension: dimension_schema,
                source_type,
                source_field,
            },
            source_id: &crate::RecordId::new(&record.id)
                .map_err(|e| Failure::request(e.to_string()))?,
            variant: &crate::VariantName::new(variant)
                .map_err(|e| Failure::request(e.to_string()))?,
            new_value: new_value.as_ref(),
        })?;
        self.publish_writer(&writer)?;
        self.include(&original)?;
        Ok(())
    }
}

fn as_key(value: CfdValue) -> Result<crate::CfdDictKey> {
    Ok(match value {
        CfdValue::Int(v) => crate::CfdDictKey::Int(v),
        CfdValue::String(v) => crate::CfdDictKey::String(v),
        CfdValue::Enum(v) => crate::CfdDictKey::Enum(v),
        _ => return Err(Failure::request("invalid dictionary key type")),
    })
}
fn record_with_fields(
    record: &RecordTarget,
    fields: BTreeMap<String, CfdValue>,
) -> Result<crate::CfdRecord> {
    Ok(crate::CfdRecord {
        id: crate::RecordId::new(&record.id)
            .map_err(|error| Failure::request(error.to_string()))?,
        object: crate::CfdObject::try_new(&record.actual_type, fields)
            .map_err(|error| Failure::request(error.to_string()))?
            .into(),
        origin: RecordOrigin::None,
        dimension_fields: BTreeMap::new().into(),
    })
}
fn path_record(
    schema: &CftSchema,
    record: &RecordTarget,
    ast: &AstRecord,
    text: &str,
) -> Result<crate::CfdRecord> {
    // 只补齐值形状，不连接引用；同批前向引用仍由最终候选校验处理。
    let ty = schema
        .resolve_type(&record.actual_type)
        .ok_or_else(|| Failure::missing("unknown record type"))?;
    let mut fields = BTreeMap::new();
    for field in ty.all_fields() {
        if let Some(value) = read_value(
            schema,
            record,
            ast,
            text,
            &[CfdPathSegment::Field(field.name.to_string())],
        )? {
            fields.insert(field.name.to_string(), value);
        }
    }
    record_with_fields(record, fields)
}
pub(crate) fn internal_path_for_record(
    schema: &CftSchema,
    record: &crate::CfdRecord,
    path: &[FieldPath],
    prefix: Vec<CfdPathSegment>,
) -> Result<Vec<CfdPathSegment>> {
    internal_path_from(schema, record.actual_type(), Some(record), path, prefix)
}
pub fn internal_path(
    schema: &CftSchema,
    actual_type: &str,
    path: &[FieldPath],
) -> Result<Vec<CfdPathSegment>> {
    internal_path_from(schema, actual_type, None, path, Vec::new())
}
fn internal_path_from(
    schema: &CftSchema,
    actual_type: &str,
    record: Option<&crate::CfdRecord>,
    path: &[FieldPath],
    mut out: Vec<CfdPathSegment>,
) -> Result<Vec<CfdPathSegment>> {
    let expected = |path: &[CfdPathSegment]| -> Result<CftValueType> {
        Ok(match record {
            Some(record) => crate::write_rules::expected_type_for_record_path(
                schema, record, path, "CLI-PATH", "CLI",
            )?,
            None => crate::write_rules::expected_type_for_cfd_path(
                schema,
                actual_type,
                path,
                "CLI-PATH",
                "CLI",
            )?,
        })
    };
    for segment in path {
        let mut parent = if out.is_empty() {
            CftValueType::Object(
                crate::TypeName::new(actual_type).map_err(|e| Failure::request(e.to_string()))?,
            )
        } else {
            expected(&out)?
        };
        while let CftValueType::Option(inner) = parent {
            parent = *inner;
        }
        let next = match segment {
            FieldPath::Field { name } => CfdPathSegment::Field(name.clone()),
            FieldPath::Index { index } => match &parent {
                // 文本路径的数字下标根据声明区分数组下标与整数键。
                CftValueType::Dict(key_type, _) => {
                    let key = super::ModelValue::Int {
                        value: index.to_string(),
                    }
                    .resolve(schema, key_type)?;
                    CfdPathSegment::DictKey(crate::data_model::format_cfd_dict_key(&as_key(key)?))
                }
                _ => CfdPathSegment::Index(*index),
            },
            FieldPath::DictKey { key } => {
                let CftValueType::Dict(key_type, _) = &parent else {
                    return Err(Failure::request("dictionary path requires a dictionary"));
                };
                let key = as_key(key.resolve(schema, key_type)?)?;
                CfdPathSegment::DictKey(crate::data_model::format_cfd_dict_key(&key))
            }
        };
        out.push(next);
        expected(&out)?;
    }
    Ok(out)
}

pub fn find_record(
    candidate: &Candidate,
    record: &RecordTarget,
) -> Result<(PathBuf, AstRecord, String)> {
    candidate.cached_record(record)
}

pub fn read_value(
    schema: &CftSchema,
    record: &RecordTarget,
    ast: &AstRecord,
    text: &str,
    path: &[CfdPathSegment],
) -> Result<Option<CfdValue>> {
    let Some(CfdPathSegment::Field(name)) = path.first() else {
        return Err(Failure::request("path must start with a field"));
    };
    let field = schema
        .field(&record.actual_type, name)
        .ok_or_else(|| Failure::missing("unknown field"))?;
    let mut value = if let Some(f) = ast.fields.iter().find(|f| &f.name == name) {
        let span = super::query::ast_value_span(&f.value);
        super::values::expression(schema, &field.value_type, &text[span.start..span.end])?
    } else {
        let Some(value) = crate::mutation::defaults::declared_field_default(schema, field)? else {
            return Ok(None);
        };
        value
    };
    crate::mutation::defaults::complete_declared_defaults(schema, &mut value)?;
    for segment in &path[1..] {
        while let CfdValue::OptionSome(inner) = value {
            value = *inner;
        }
        value = match (segment, &value) {
            (CfdPathSegment::Field(name), CfdValue::Object(v)) => v.field(name).cloned(),
            (CfdPathSegment::Index(i), CfdValue::Array(v)) => v.get(*i).cloned(),
            (CfdPathSegment::DictKey(key), CfdValue::Dict(v)) => v
                .iter()
                .find(|(k, _)| crate::data_model::format_cfd_dict_key(k) == *key)
                .map(|(_, v)| v.clone()),
            _ => None,
        }
        .ok_or_else(|| Failure::missing("nested value is unreachable"))?;
    }
    Ok(Some(value))
}
fn materialized_top(
    schema: &CftSchema,
    record: &RecordTarget,
    ast: &AstRecord,
    text: &str,
    path: &[CfdPathSegment],
    value: &CfdValue,
) -> Result<Option<CfdValue>> {
    if path.len() < 2 {
        return Ok(None);
    }
    let mut top = read_value(schema, record, ast, text, &path[..1])?
        .ok_or_else(|| Failure::missing("top-level value is missing"))?;
    replace_nested(&mut top, &path[1..], value.clone())?;
    Ok(Some(top))
}
pub fn replace_nested(
    current: &mut CfdValue,
    path: &[CfdPathSegment],
    value: CfdValue,
) -> Result<()> {
    if path.is_empty() {
        *current = value;
        return Ok(());
    }
    if let CfdValue::OptionSome(inner) = current {
        return replace_nested(inner, path, value);
    }
    let next = match (&path[0], current) {
        (CfdPathSegment::Field(name), CfdValue::Object(v)) => v.fields.get_mut(name.as_str()),
        (CfdPathSegment::Index(i), CfdValue::Array(v)) => v.get_mut(*i),
        (CfdPathSegment::DictKey(key), CfdValue::Dict(v)) => v
            .iter_mut()
            .find(|(k, _)| crate::data_model::format_cfd_dict_key(k) == *key)
            .map(|(_, v)| v),
        _ => None,
    }
    .ok_or_else(|| Failure::missing("nested value is unreachable"))?;
    replace_nested(next, &path[1..], value)
}

fn rewrite_dimensions(
    candidate: &mut Candidate,
    writer: &crate::cfd_loader::CfdWriter,
    schema: &CftSchema,
    record: &RecordTarget,
    new_id: Option<&str>,
) -> Result<()> {
    let old = crate::RecordId::new(&record.id).map_err(|e| Failure::request(e.to_string()))?;
    let new = new_id
        .map(crate::RecordId::new)
        .transpose()
        .map_err(|e| Failure::request(e.to_string()))?;
    for file in candidate.data_sources()? {
        let ast = &file.ast;
        let count = ast
            .dimensions
            .iter()
            .flat_map(|block| &block.records)
            .filter(|entry| {
                entry.id == record.id
                    && schema.inheritance_root(&record.actual_type)
                        == schema.inheritance_root(&entry.type_name)
            })
            .count();
        if count == 0 {
            continue;
        }
        candidate.automatic_updates.push(json!({"kind":"dimension_overrides","record":record,"new_id":new_id,"count":count,
            "file":crate::project_path(candidate.original.root_dir(),&candidate.unshadow(&file.absolute)?)}));
        writer.rewrite_dimension_record(&crate::RewriteDimensionRecordRequest {
            source: &CfdSource {
                location: CfdSourcePath::new(file.absolute.clone()),
                display_name: file.path.clone(),
            },
            schema,
            actual_type: &record.actual_type,
            old_id: &old,
            new_id: new.as_ref(),
        })?;
    }
    Ok(())
}
