use super::validate::CachedDefaultObject;
use super::Validator;
use crate::data_model::build::{RecordDraft, ValueDraft};
use crate::data_model::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath, RecordOrigin};
use crate::data_model::model::{CfdEnumValue, CfdRecordId, CfdValue};
use coflow_language::limits::TraversalCursor;
use coflow_language::{CftField, CftSchemaDefaultValue, CftValueType};
use std::collections::BTreeMap;

impl Validator<'_, '_> {
    pub(super) fn default_field_value(
        &mut self,
        field: &CftField,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<ValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            if let CftValueType::Object(type_name) = &field.value_type {
                if let Some(cycle) = crate::data_model::dependencies::schema_default_cycle(
                    self.schema.cft(),
                    type_name,
                ) {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::ValueDependencyCycle,
                            format!("schema default dependency cycle: {cycle}"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
            }
        }
        let cursor = self.enter_value(parent, record, &path)?;
        self.default_value(&field.value_type, value, record, path, cursor)
    }

    fn default_value(
        &mut self,
        ty: &CftValueType,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            return match ty {
                CftValueType::Dict(_, _) => Some(ValueDraft::Value(CfdValue::Dict(Vec::new()))),
                CftValueType::Object(type_name) => {
                    self.default_object_value(type_name, record, path, cursor)
                }
                _ => {
                    self.push_default_type_mismatch(record, path);
                    None
                }
            };
        }

        let out = match value {
            CftSchemaDefaultValue::OptionNone if matches!(ty, CftValueType::Option(_)) => {
                CfdValue::OptionNone
            }
            CftSchemaDefaultValue::OptionSome(value) => {
                let CftValueType::Option(inner) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                return self
                    .default_value(inner, value, record, path, cursor)
                    .map(|value| ValueDraft::OptionSome(Box::new(value)));
            }
            CftSchemaDefaultValue::ResultOk(value) => {
                let CftValueType::Result(ok, _) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                return self
                    .default_value(ok, value, record, path, cursor)
                    .map(|value| ValueDraft::ResultOk(Box::new(value)));
            }
            CftSchemaDefaultValue::ResultErr(value) => {
                let CftValueType::Result(_, error) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                return self
                    .default_value(error, value, record, path, cursor)
                    .map(|value| ValueDraft::ResultErr(Box::new(value)));
            }
            CftSchemaDefaultValue::Int(value) if type_accepts_default(ty, &CftValueType::Int) => {
                CfdValue::Int(*value)
            }
            CftSchemaDefaultValue::Float(value)
                if type_accepts_default(ty, &CftValueType::Float) =>
            {
                CfdValue::Float(*value)
            }
            CftSchemaDefaultValue::Bool(value) if type_accepts_default(ty, &CftValueType::Bool) => {
                CfdValue::Bool(*value)
            }
            CftSchemaDefaultValue::String(value)
                if type_accepts_default(ty, &CftValueType::String) =>
            {
                CfdValue::String(value.clone())
            }
            CftSchemaDefaultValue::FormattedString(source)
                if type_accepts_default(ty, &CftValueType::String) =>
            {
                let parsed = match crate::data_model::cell_value::parse_automatic_formatted_string(source) {
                    Ok(Some(value)) => value,
                    _ => {
                        self.push_default_type_mismatch(record, path);
                        return None;
                    }
                };
                return Some(ValueDraft::FormattedString(parsed));
            }
            CftSchemaDefaultValue::Function(source)
                if matches!(ty, CftValueType::Function(_, _)) =>
            {
                CfdValue::Function(crate::data_model::CfdFunction {
                    source: source.clone(),
                })
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } if matches!(ty, CftValueType::Enum(name) if name == enum_name) => {
                let variant = (!self
                    .schema
                    .cft()
                    .resolve_enum(enum_name)
                    .is_some_and(|schema_enum| schema_enum.is_flag))
                .then(|| variant.clone());
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant,
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray
                if matches!(ty, CftValueType::Array(_)) =>
            {
                CfdValue::Array(Vec::new())
            }
            CftSchemaDefaultValue::Array(values) => {
                let CftValueType::Array(inner) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                let mut out = Vec::with_capacity(values.len());
                for (index, value) in values.iter().enumerate() {
                    let value = self.default_value(
                        inner,
                        value,
                        record,
                        path.clone().index(index),
                        cursor,
                    )?;
                    out.push(value);
                }
                return Some(ValueDraft::Array(out));
            }
            CftSchemaDefaultValue::Dictionary(entries) => {
                let CftValueType::Dict(key_type, value_type) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                let mut out = Vec::with_capacity(entries.len());
                for (index, (key, value)) in entries.iter().enumerate() {
                    let key_path = path.clone().index(index);
                    let key = self.default_value(
                        key_type,
                        key,
                        record,
                        key_path.clone(),
                        cursor,
                    )?;
                    let key = match key {
                        ValueDraft::Value(CfdValue::Int(value)) => {
                            crate::data_model::CfdDictKey::Int(value)
                        }
                        ValueDraft::Value(CfdValue::String(value)) => {
                            crate::data_model::CfdDictKey::String(value)
                        }
                        ValueDraft::Value(CfdValue::Enum(value)) => {
                            crate::data_model::CfdDictKey::Enum(value)
                        }
                        _ => {
                            self.push_default_type_mismatch(record, key_path);
                            return None;
                        }
                    };
                    let value = self.default_value(
                        value_type,
                        value,
                        record,
                        path.clone().index(index),
                        cursor,
                    )?;
                    out.push((key, value));
                }
                return Some(ValueDraft::Dict(out));
            }
            CftSchemaDefaultValue::Object { type_name, fields } => {
                let CftValueType::Object(expected) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                if !self.schema.cft().is_assignable(type_name, expected) {
                    self.push_default_type_mismatch(record, path);
                    return None;
                }
                return self.default_explicit_object_value(
                    type_name,
                    fields,
                    record,
                    path,
                    cursor,
                );
            }
            CftSchemaDefaultValue::RecordReference { type_name, key } => {
                let CftValueType::RecordRef(expected) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                if !self.schema.cft().is_assignable(type_name, expected) {
                    self.push_default_type_mismatch(record, path);
                    return None;
                }
                return Some(ValueDraft::PendingRef {
                    expected_type: expected.clone(),
                    key: key.clone(),
                });
            }
            _ => {
                self.push_default_type_mismatch(record, path);
                return None;
            }
        };
        self.validate_materialized_value(ty, &out, record, path)?;
        Some(ValueDraft::Value(out))
    }

    fn default_explicit_object_value(
        &mut self,
        type_name: &coflow_language::TypeName,
        supplied: &[(coflow_language::FieldName, CftSchemaDefaultValue)],
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        let schema = self.schema;
        let Some(schema_type) = schema.resolve_type(type_name) else {
            self.push_default_type_mismatch(record, path);
            return None;
        };
        if schema_type.is_abstract {
            self.push_default_type_mismatch(record, path);
            return None;
        }
        let supplied = supplied
            .iter()
            .map(|(name, value)| (name, value))
            .collect::<BTreeMap<_, _>>();
        let mut fields = BTreeMap::new();
        for field in schema.full_fields(type_name).collect::<Vec<_>>() {
            let field_path = path.clone().field(field.name.as_str());
            let value = if let Some(value) = supplied.get(&field.name) {
                self.default_value(
                    &field.value_type,
                    value,
                    record,
                    field_path,
                    cursor,
                )
            } else if let Some(default) = &field.default {
                self.default_field_value(field, default, record, field_path, cursor)
            } else {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::MissingRequiredField,
                        format!("missing required field `{}`", field.name),
                    )
                    .with_primary(record, field_path),
                );
                None
            };
            if let Some(value) = value {
                fields.insert(field.name.clone(), value);
            }
        }
        Some(ValueDraft::Object(Box::new(RecordDraft {
            key: String::new(),
            actual_type: type_name.clone(),
            fields,
            origin: RecordOrigin::None,
        })))
    }

    fn default_object_value(
        &mut self,
        type_name: &str,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        if let Some((nodes, depth)) = self
            .default_objects
            .get(type_name)
            .map(|cached| (cached.nodes, cached.depth))
        {
            self.charge_cached_subtree(cursor, record, &path, nodes, depth)?;
            return self
                .default_objects
                .get(type_name)
                .map(|cached| ValueDraft::Object(Box::new(cached.draft.clone())));
        }
        if let Some(cycle) =
            crate::data_model::dependencies::schema_default_cycle(self.schema.cft(), type_name)
        {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::ValueDependencyCycle,
                    format!("schema default dependency cycle: {cycle}"),
                )
                .with_primary(record, path),
            );
            return None;
        }
        let fields = BTreeMap::new();
        let draft = self.validate_record(
            Some(type_name),
            "",
            type_name,
            &fields,
            record,
            path,
            cursor,
        )?;
        let (nodes, depth) = draft_shape(&draft);
        self.default_objects.insert(
            type_name.to_string(),
            CachedDefaultObject {
                draft: draft.clone(),
                nodes,
                depth,
            },
        );
        Some(ValueDraft::Object(Box::new(draft)))
    }

    fn push_default_type_mismatch(&mut self, record: Option<CfdRecordId>, path: CfdPath) {
        self.push(
            CfdDiagnostic::error(
                CfdErrorCode::TypeMismatch,
                "schema default does not match field type",
            )
            .with_primary(record, path),
        );
    }
}

fn draft_shape(root: &RecordDraft) -> (u64, u64) {
    enum DraftNode<'a> {
        Record(&'a RecordDraft),
        Value(&'a ValueDraft),
    }

    let mut nodes = 0_u64;
    let mut depth = 0_u64;
    let mut pending = vec![(DraftNode::Record(root), 1_u64)];
    while let Some((node, node_depth)) = pending.pop() {
        nodes = nodes.saturating_add(1);
        depth = depth.max(node_depth);
        let child_depth = node_depth.saturating_add(1);
        match node {
            DraftNode::Record(record) => {
                pending.extend(
                    record
                        .fields
                        .values()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(ValueDraft::Object(record)) => {
                pending.extend(
                    record
                        .fields
                        .values()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(
                ValueDraft::OptionSome(value)
                | ValueDraft::ResultOk(value)
                | ValueDraft::ResultErr(value),
            ) => pending.push((DraftNode::Value(value), child_depth)),
            DraftNode::Value(ValueDraft::Array(items)) => {
                pending.extend(
                    items
                        .iter()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(ValueDraft::Dict(entries)) => {
                pending.extend(
                    entries
                        .iter()
                        .map(|(_, value)| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(
                ValueDraft::Value(_)
                | ValueDraft::FormattedString(_)
                | ValueDraft::PendingRef { .. },
            ) => {}
        }
    }
    (nodes, depth)
}

fn type_accepts_default(expected: &CftValueType, actual: &CftValueType) -> bool {
    expected == actual
}
