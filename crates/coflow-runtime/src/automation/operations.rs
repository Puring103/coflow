use super::{transaction::Candidate, Failure, Result};
use crate::{CfdPathSegment, CfdSource, CfdSourcePath, CfdValue, RecordOrigin};
use coflow_language::{
    cfd::CfdRecord as AstRecord,
    cft::{CftSchema, CftValueType},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

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

struct PreparedField<'a> {
    record: &'a RecordTarget,
    origin: RecordOrigin,
    path: Vec<CfdPathSegment>,
    value: CfdValue,
    top: Option<CfdValue>,
}

impl PreparedField<'_> {
    fn request<'a>(&'a self, schema: &'a CftSchema) -> crate::WriteCellRequest<'a> {
        crate::WriteCellRequest {
            origin: &self.origin,
            record_id: &self.record.id,
            actual_type: &self.record.actual_type,
            field_path: &self.path,
            new_value: &self.value,
            materialized_top_level: self
                .top
                .as_ref()
                .map(crate::api::FieldMaterialization::Value),
            schema,
        }
    }
}

fn unchanged_after_publication_failure(
    sources: &BTreeMap<PathBuf, std::sync::Arc<super::transaction::CandidateSource>>,
    failure: &Failure,
) -> bool {
    // 完整恢复回执和逐文件字节校验缺一不可；冲突或读取失败都不能放行重放。
    failure
        .diagnostics
        .publication
        .as_ref()
        .is_some_and(|report| {
            matches!(
                report.state,
                crate::PersistenceState::NotAttempted | crate::PersistenceState::Restored
            )
        })
        && sources.iter().all(|(path, source)| {
            std::fs::read(path).is_ok_and(|contents| contents == source.text.as_bytes())
        })
}

impl Candidate {
    pub fn apply_operations(&mut self, operations: &[Operation]) -> Result<()> {
        self.capture_typed_references()?;
        let encoded = operations
            .iter()
            .map(super::json)
            .collect::<Result<Vec<_>>>()?;
        let mut index = 0;
        while index < operations.len() {
            let end = index + Self::independent_field_group(&operations[index..]);
            if end > index + 1 {
                match self.apply_field_group(&operations[index..end]) {
                    Ok(true) => {
                        for (offset, operation) in operations[index..end].iter().enumerate() {
                            self.operations.push(json!({"index":index + offset,"status":"success","operation":operation,"details":{}}));
                        }
                        index = end;
                        continue;
                    }
                    Ok(false) => {}
                    Err(failure) => return Err(self.record_failed(index, &encoded, failure)),
                }
            }
            // 不可聚合或准备失败的整个分组仅顺序执行一次，保留首错和候选前缀报告。
            for (offset, operation) in operations[index..end].iter().enumerate() {
                self.operation_details = json!({});
                if let Err(failure) = self.apply_operation(operation) {
                    return Err(self.record_failed(index + offset, &encoded, failure));
                }
                self.operations.push(json!({"index":index + offset,"status":"success","operation":operation,"details":self.operation_details}));
            }
            index = end;
        }
        Ok(())
    }

    fn independent_field_group(operations: &[Operation]) -> usize {
        let mut records = BTreeSet::new();
        for (index, operation) in operations.iter().enumerate() {
            let Operation::SetField { record, .. } = operation else {
                return index.max(1);
            };
            if !records.insert((&record.actual_type, &record.id)) {
                return index.max(1);
            }
        }
        operations.len().max(1)
    }

    fn apply_field_group(&mut self, operations: &[Operation]) -> Result<bool> {
        self.apply_field_group_using(operations, |writer, requests| {
            writer.write_field_batch(requests).is_ok()
        })
    }

    fn apply_field_group_using(
        &mut self,
        operations: &[Operation],
        mut validate: impl FnMut(&crate::cfd_loader::CfdWriter, &[crate::WriteCellRequest<'_>]) -> bool,
    ) -> Result<bool> {
        let session = self.schema_session()?;
        let Some(schema) = session.schema() else {
            return Ok(false);
        };
        let original_references = self.typed_references.clone();
        let materialized_len = self.materialized.len();
        let writer = crate::cfd_loader::CfdWriter::new();
        let mut original_sources = BTreeMap::new();
        let prepared = (|| -> Result<bool> {
            let mut fields = Vec::with_capacity(operations.len());
            for operation in operations {
                let Operation::SetField {
                    record,
                    path,
                    value,
                    expected,
                } = operation
                else {
                    return Ok(false);
                };
                // 每条记录仅出现一次，准备只读取本记录值形状；跨记录引用和模板仍由最终候选连接。
                // 空操作沿用顺序路径，不改变无变化请求的文件发布边界。
                let (source, _) = self.cached_record_source(record)?;
                original_sources
                    .entry(source.absolute.clone())
                    .or_insert(source);
                let Some(field) =
                    self.prepare_field(schema, record, path, value, expected.as_ref())?
                else {
                    return Ok(false);
                };
                fields.push(field);
            }
            let mut files = BTreeMap::<_, Vec<_>>::new();
            for field in &fields {
                let RecordOrigin::File { path, .. } = &field.origin else {
                    return Ok(false);
                };
                files.entry(path).or_default().push(field.request(schema));
            }
            // 每个文件保留原请求顺序并检查所有中间前缀节点预算。
            // 任一文件失败时尚未发布候选，整组原序重放以恢复真正首错及成功前缀。
            Ok(files.values().all(|requests| validate(&writer, requests)))
        })();
        if !matches!(prepared, Ok(true)) {
            self.typed_references = original_references;
            self.materialized.truncate(materialized_len);
            return Ok(false);
        }
        if let Err(failure) = self.publish_writer(&writer) {
            self.typed_references = original_references;
            self.materialized.truncate(materialized_len);
            if unchanged_after_publication_failure(&original_sources, &failure) {
                return Ok(false);
            }
            // 恢复不完整或字节无法证明原样时保留发布错误及回执，不能再次改写候选。
            return Err(failure);
        }
        Ok(true)
    }
    fn prepare_field<'a>(
        &mut self,
        schema: &CftSchema,
        record: &'a RecordTarget,
        path: &[FieldPath],
        value: &InputValue,
        expected: Option<&InputValue>,
    ) -> Result<Option<PreparedField<'a>>> {
        let (source, index) = self.cached_record_source(record)?;
        let ast = &source.ast.records[index];
        let text = &source.text;
        let current_record = path_record(schema, record, ast, text)?;
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
        let current = read_value(schema, record, ast, text, &path)?;
        if let Some(expected) = expected {
            let expected_value = expected.resolve(schema, &ty)?;
            if current.as_ref() != Some(&expected_value) {
                return Err(Failure::conflict("old value precondition failed").with_result(json!({"precondition":{
                "view":"effective","expected":super::model_value::encode(schema,&ty,Some(&expected_value))?,
                "actual":super::model_value::encode(schema,&ty,current.as_ref())?}})));
            }
        }
        // 只有实际存在的赋值才能作为无变化；与默认值相同也必须建立显式数据。
        let explicitly_assigned = path.first().is_some_and(
            |p| matches!(p,CfdPathSegment::Field(name) if ast.fields.iter().any(|f|f.name==*name)),
        );
        if explicitly_assigned && current.as_ref() == Some(&value) {
            return Ok(None);
        }
        let top = materialized_top(schema, record, ast, text, &path, &value)?;
        if !explicitly_assigned {
            if let (Some(CfdPathSegment::Field(field)), Some(top)) = (path.first(), top.as_ref()) {
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
        Ok(Some(PreparedField {
            record,
            origin: RecordOrigin::File {
                path: source.absolute.clone(),
                span: None,
            },
            path,
            value,
            top,
        }))
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
                if let Some(field) =
                    self.prepare_field(schema, record, path, value, expected.as_ref())?
                {
                    writer.write_field(&field.request(schema))?;
                    self.publish_writer(&writer)?;
                }
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
            crate::inspection::query::dimension_destination_for_owner(&project, &owner, dimension)?
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

#[cfg(test)]
mod batch_tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        reason = "回归夹具与逐条执行对照必须完整构造。"
    )]
    use super::*;
    use crate::automation::{Arguments, InputSnapshot};

    fn set(id: &str, field: &[&str], value: &str, expected: Option<&str>) -> Operation {
        Operation::SetField {
            record: RecordTarget {
                actual_type: "Item".into(),
                id: id.into(),
            },
            path: field
                .iter()
                .map(|name| FieldPath::Field {
                    name: (*name).into(),
                })
                .collect(),
            value: InputValue::Expression {
                source: value.into(),
            },
            expected: expected.map(|source| InputValue::Expression {
                source: source.into(),
            }),
        }
    }

    fn sequential(candidate: &mut Candidate, operations: &[Operation]) -> Result<()> {
        candidate.capture_typed_references()?;
        let encoded = operations
            .iter()
            .map(super::super::json)
            .collect::<Result<Vec<_>>>()?;
        for (index, operation) in operations.iter().enumerate() {
            candidate.operation_details = json!({});
            if let Err(failure) = candidate.apply_operation(operation) {
                return Err(candidate.record_failed(index, &encoded, failure));
            }
            candidate.operations.push(json!({"index":index,"status":"success","operation":operation,"details":candidate.operation_details}));
        }
        Ok(())
    }

    fn assert_sequential_equivalence(operations: &[Operation], failed_index: Option<usize>) {
        assert_sequential_equivalence_in_files(operations, failed_index, false);
    }

    fn assert_sequential_equivalence_in_files(
        operations: &[Operation],
        failed_index: Option<usize>,
        split: bool,
    ) {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("coflow.yaml"),
            "schema: schema.cft\ndata: data.cfd\ncodegen: [{language: csharp, dir: generated}]\n",
        )
        .unwrap();
        std::fs::write(root.path().join("schema.cft"), "data Detail { count: int = 3; }\ntable Item { value: int = 1; details: Detail = Detail {}; parent: &Item; label: string = \"default\"; }\n").unwrap();
        let source = "# 保留 Unicode 与注释\na: Item { value: 1, parent: &b, label: \"甲😀\" }\nb: Item { value: 2, parent: &a, details: Detail { count: 5 } }\nc: Item { value: 3, parent: &a }\n";
        let files = if split {
            std::fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: [data.cfd, other.cfd]\ncodegen: [{language: csharp, dir: generated}]\n").unwrap();
            BTreeMap::from([
                (
                    "data.cfd",
                    source.lines().filter(|line| !line.starts_with("b:")).fold(
                        String::new(),
                        |mut text, line| {
                            text.push_str(line);
                            text.push('\n');
                            text
                        },
                    ),
                ),
                (
                    "other.cfd",
                    source.lines().filter(|line| line.starts_with("b:")).fold(
                        String::new(),
                        |mut text, line| {
                            text.push_str(line);
                            text.push('\n');
                            text
                        },
                    ),
                ),
            ])
        } else {
            BTreeMap::from([("data.cfd", source.into())])
        };
        for (name, text) in &files {
            std::fs::write(root.path().join(name), text).unwrap();
        }
        let project = crate::Project::open_schema_only(Some(root.path())).unwrap();
        let snapshot = InputSnapshot::capture(&project).unwrap();
        let mut batch = Candidate::new(&project, &snapshot).unwrap();
        let mut individual = Candidate::new(&project, &snapshot).unwrap();
        let actual = batch.apply_operations(operations);
        let expected = sequential(&mut individual, operations);
        assert_eq!(
            actual
                .as_ref()
                .err()
                .and_then(|failure| failure.result.as_ref())
                .map(|result| result["operation_index"].clone()),
            failed_index.map(|index| json!(index))
        );
        match (actual, expected) {
            (Ok(()), Ok(())) => {
                let (actual, actual_validation) =
                    batch.finish(&Arguments::default(), true).unwrap();
                let (expected, expected_validation) =
                    individual.finish(&Arguments::default(), true).unwrap();
                assert_eq!(actual, expected);
                assert_eq!(
                    actual_validation.diagnostics,
                    expected_validation.diagnostics
                );
            }
            (Err(actual), Err(expected)) => {
                let actual = batch.failure_result(actual, &Arguments::default());
                let expected = individual.failure_result(expected, &Arguments::default());
                assert_eq!(actual.status, expected.status);
                assert_eq!(actual.result, expected.result);
                assert_eq!(actual.diagnostics, expected.diagnostics);
            }
            (actual, expected) => panic!("batch/sequential mismatch: {actual:?} / {expected:?}"),
        }
        assert_eq!(batch.materialized, individual.materialized);
        assert_eq!(
            format!("{:?}", batch.typed_references),
            format!("{:?}", individual.typed_references)
        );
        for (name, text) in files {
            assert_eq!(
                std::fs::read_to_string(root.path().join(name)).unwrap(),
                text
            );
        }
    }

    #[test]
    fn interleaved_files_preserve_results_and_earliest_failure_in_original_order() {
        assert_sequential_equivalence_in_files(
            &[
                set("a", &["details", "count"], "7", Some("3")),
                set("b", &["label"], "f\"{self.parent.value}\"", None),
                set("c", &["parent"], "&Item::b", None),
                // 同记录二次修改必须形成屏障，并读取前一个分组的最新值。
                set("a", &["details", "count"], "8", Some("7")),
            ],
            None,
            true,
        );
        assert_sequential_equivalence_in_files(
            &[
                set("a", &["details", "count"], "7", Some("3")),
                set("b", &["value"], "8", Some("99")),
                set("c", &["value"], "\"invalid\"", None),
            ],
            Some(1),
            true,
        );
        assert_sequential_equivalence_in_files(
            &[
                set("b", &["details", "count"], "7", Some("5")),
                set("a", &["value"], "8", Some("99")),
                set("c", &["value"], "\"invalid\"", None),
            ],
            Some(1),
            true,
        );
    }

    #[test]
    fn replay_after_publication_failure_requires_recovery_receipt_and_unchanged_bytes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("data.cfd");
        let text = "a: Item { value: 1 }\n";
        std::fs::write(&path, text).unwrap();
        let sources = BTreeMap::from([(
            path.clone(),
            std::sync::Arc::new(super::super::transaction::CandidateSource {
                absolute: path.clone(),
                path: "data.cfd".into(),
                text: text.into(),
                ast: coflow_language::cfd::parse_cfd(text).0,
                malformed: false,
            }),
        )]);
        let mut failure = Failure::request("candidate publication failed");
        assert!(!unchanged_after_publication_failure(&sources, &failure));
        for state in [
            crate::PersistenceState::NotAttempted,
            crate::PersistenceState::Restored,
        ] {
            failure.diagnostics.publication = Some(Box::new(crate::PublicationReport {
                state,
                ..Default::default()
            }));
            assert!(unchanged_after_publication_failure(&sources, &failure));
            std::fs::write(&path, "external change").unwrap();
            assert!(!unchanged_after_publication_failure(&sources, &failure));
            std::fs::write(&path, text).unwrap();
        }
        for state in [
            crate::PersistenceState::Partial,
            crate::PersistenceState::Unknown,
            crate::PersistenceState::Committed,
        ] {
            failure.diagnostics.publication = Some(Box::new(crate::PublicationReport {
                state,
                ..Default::default()
            }));
            assert!(!unchanged_after_publication_failure(&sources, &failure));
        }
        failure.diagnostics.publication = Some(Box::new(crate::PublicationReport::default()));
        std::fs::remove_file(&path).unwrap();
        assert!(!unchanged_after_publication_failure(&sources, &failure));
    }

    #[test]
    fn independent_writes_preserve_defaults_templates_references_and_order() {
        assert_sequential_equivalence(
            &[
                set("a", &["value"], "7", Some("1")),
                set("b", &["label"], "f\"{self.parent.value}\"", None),
                set("c", &["details", "count"], "9", Some("3")),
                // 重复记录切开批次，预期值必须读取刚写入的候选结果。
                set("a", &["value"], "8", Some("7")),
                set("b", &["parent"], "&Item::c", None),
                set("c", &["parent"], "&Item::b", None),
            ],
            None,
        );
    }

    #[test]
    fn failed_batch_preserves_first_error_and_successful_candidate_prefix() {
        assert_sequential_equivalence(
            &[
                set("a", &["details", "count"], "7", Some("3")),
                set("b", &["value"], "8", Some("99")),
                set("c", &["value"], "9", None),
            ],
            Some(1),
        );
        assert_sequential_equivalence(
            &[
                set("a", &["parent"], "&Item::c", None),
                set("b", &["value"], "\"invalid\"", None),
                set("c", &["value"], "9", None),
            ],
            Some(1),
        );
    }

    #[test]
    fn noop_and_forward_reference_batches_match_sequential_execution() {
        assert_sequential_equivalence(
            &[
                set("a", &["value"], "1", Some("1")),
                set("b", &["value"], "8", Some("2")),
                set("c", &["details", "count"], "9", None),
            ],
            None,
        );
        assert_sequential_equivalence(
            &[
                set("a", &["parent"], "&Item::future", None),
                set("b", &["value"], "8", None),
                Operation::CreateRecord {
                    record: RecordTarget {
                        actual_type: "Item".into(),
                        id: "future".into(),
                    },
                    file: "data.cfd".into(),
                    fields: BTreeMap::from([(
                        "parent".into(),
                        InputValue::Expression {
                            source: "&Item::a".into(),
                        },
                    )]),
                },
            ],
            None,
        );
        // 类型限定保留至最终连接，批次本身成功不表示悬空引用可通过校验。
        assert_sequential_equivalence(
            &[
                set("a", &["parent"], "&Item::missing", None),
                set("b", &["parent"], "&Item::c", None),
                set("c", &["parent"], "&Item::b", None),
            ],
            None,
        );
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "同一夹具对照跨文件失败首序、已成功前缀及合法预算释放顺序。"
    )]
    fn interleaved_batches_preserve_intermediate_node_budget_and_global_failure_order() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: [first.cfd, second.cfd, last.cfd]\ncodegen: [{language: csharp, dir: generated}]\n").unwrap();
        std::fs::write(
            root.path().join("schema.cft"),
            "table Item { values: [int]; }\n",
        )
        .unwrap();
        let first = "a: Item { values: [] }\nc: Item { values: [1, 2, 3] }\n";
        let second = "b: Item { values: [] }\nd: Item { values: [1, 2, 3] }\n";
        let last = "p: Item { values: [] }\n";
        for (name, text) in [
            ("first.cfd", first),
            ("second.cfd", second),
            ("last.cfd", last),
        ] {
            std::fs::write(root.path().join(name), text).unwrap();
        }
        let project = crate::Project::open_schema_only(Some(root.path())).unwrap();
        let snapshot = InputSnapshot::capture(&project).unwrap();
        let limit = coflow_language::cfd::parse_cfd(first)
            .0
            .structural_node_count()
            + 1;
        let options = coflow_language::cfd::CfdParseOptions {
            structural_limits: coflow_language::limits::StructuralLimits::new(
                256, limit, 10_000_000,
            ),
        };
        let operations = [
            set("p", &["values"], "[9]", None),
            set("b", &["values"], "[1,2,3]", None),
            set("a", &["values"], "[1,2,3]", None),
            set("d", &["values"], "[]", None),
            set("c", &["values"], "[]", None),
        ];
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        candidate.capture_typed_references().unwrap();
        let mut first_failure = None;
        // 按文件校验先看到 first.cfd 的全局第 2 项；不得用该位置覆盖原序第 1 项错误。
        let grouped = candidate
            .apply_field_group_using(&operations, |writer, requests| {
                match writer.write_field_batch_for_test(requests, options) {
                    Ok(_) => true,
                    Err(failure) => {
                        first_failure = Some(requests[failure.index].record_id.to_string());
                        false
                    }
                }
            })
            .unwrap();
        assert!(!grouped);
        assert_eq!(first_failure.as_deref(), Some("a"));
        assert!(candidate.changes().unwrap().is_empty());
        assert!(candidate.materialized.is_empty() && candidate.typed_references.is_empty());
        let encoded = operations
            .iter()
            .map(super::super::json)
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let session = candidate.schema_session().unwrap();
        let schema = session.schema().unwrap();
        let mut failed = None;
        for (index, operation) in operations.iter().enumerate() {
            let Operation::SetField {
                record,
                path,
                value,
                expected,
            } = operation
            else {
                panic!("field operation");
            };
            let field = candidate
                .prepare_field(schema, record, path, value, expected.as_ref())
                .unwrap()
                .unwrap();
            let writer = crate::cfd_loader::CfdWriter::new();
            if let Err(error) = writer.write_field_batch_for_test(&[field.request(schema)], options)
            {
                let failure =
                    candidate.record_failed(index, &encoded, Failure::from(error.diagnostics));
                assert!(failure.diagnostics.contains("nodes limit"));
                assert_eq!(failure.result.unwrap()["operation_index"], 1);
                failed = Some(index);
                break;
            }
            candidate.publish_writer(&writer).unwrap();
            candidate
                .operations
                .push(json!({"index":index,"status":"success","operation":operation,"details":{}}));
        }
        assert_eq!(failed, Some(1));
        assert_eq!(candidate.operations[0]["status"], "success");
        assert_eq!(candidate.operations[1]["status"], "failed");
        assert!(candidate.operations[2..]
            .iter()
            .all(|operation| operation["status"] == "not_executed"));
        let changes = candidate.changes().unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes[0].path.ends_with("last.cfd"));

        // 各文件先释放预算再扩容应成功，文件之间的交错不改变每文件中间预算。
        let reordered = [
            &operations[4],
            &operations[3],
            &operations[0],
            &operations[2],
            &operations[1],
        ]
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        assert!(candidate
            .apply_field_group_using(&reordered, |writer, requests| writer
                .write_field_batch_for_test(requests, options)
                .is_ok())
            .unwrap());
        assert_eq!(candidate.changes().unwrap().len(), 3);
        snapshot.verify(&project).unwrap();
    }
}
