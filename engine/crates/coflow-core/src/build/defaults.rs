use super::Validator;
use crate::build::{RecordDraft, ValueDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath, RecordOrigin};
use crate::model::{CfdEnumValue, CfdRecordId, CfdValue};
use crate::schema::{CftField, CftStaticValue, CftValueType};
use coflow_language::limits::TraversalCursor;
use std::collections::BTreeMap;

impl Validator<'_, '_> {
    pub(super) fn default_field_value(
        &mut self,
        field: &CftField,
        value: &CftStaticValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<ValueDraft> {
        let cursor = self.enter_value(parent, record, &path)?;
        self.default_value(&field.value_type, value, record, path, cursor)
    }

    // Schema 默认值的所有复合变体在此统一递归，保证预算和路径诊断一致。
    #[allow(clippy::too_many_lines)]
    fn default_value(
        &mut self,
        ty: &CftValueType,
        value: &CftStaticValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        let out = match value {
            CftStaticValue::OptionNone if matches!(ty, CftValueType::Option(_)) => {
                CfdValue::OptionNone
            }
            CftStaticValue::OptionSome(value) => {
                let CftValueType::Option(inner) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                return self
                    .default_value(inner, value, record, path, cursor)
                    .map(|value| ValueDraft::OptionSome(Box::new(value)));
            }
            CftStaticValue::Int(value) if type_accepts_default(ty, &CftValueType::Int) => {
                CfdValue::Int(*value)
            }
            CftStaticValue::Float(value)
                if type_accepts_default(ty, &CftValueType::Float) =>
            {
                CfdValue::Float(*value)
            }
            CftStaticValue::Bool(value) if type_accepts_default(ty, &CftValueType::Bool) => {
                CfdValue::Bool(*value)
            }
            CftStaticValue::String(value)
                if type_accepts_default(ty, &CftValueType::String) =>
            {
                CfdValue::String(value.clone())
            }
            CftStaticValue::FormattedString(source)
                if type_accepts_default(ty, &CftValueType::FString) =>
            {
                let parsed = crate::CallableSource::from(source);
                return Some(ValueDraft::FormattedString(parsed));
            }
            CftStaticValue::Function(source)
                if matches!(ty, CftValueType::Function(_, _)) =>
            {
                CfdValue::Function(source.into())
            }
            CftStaticValue::Enum {
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
            CftStaticValue::Array(values) => {
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
            CftStaticValue::Dictionary(entries) => {
                let CftValueType::Dict(key_type, value_type) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                let mut out = Vec::with_capacity(entries.len());
                for (index, (key, value)) in entries.iter().enumerate() {
                    let key_path = path.clone().index(index);
                    let key =
                        self.default_value(key_type, key, record, key_path.clone(), cursor)?;
                    let key = match key {
                        ValueDraft::Value(CfdValue::Int(value)) => crate::CfdDictKey::Int(value),
                        ValueDraft::Value(CfdValue::Bool(value)) => crate::CfdDictKey::Bool(value),
                        ValueDraft::Value(CfdValue::String(value)) => {
                            crate::CfdDictKey::String(value)
                        }
                        ValueDraft::Value(CfdValue::Enum(value)) => crate::CfdDictKey::Enum(value),
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
            CftStaticValue::Object { type_name, fields } => {
                let CftValueType::Object(expected) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                if !self.schema.cft().is_assignable(type_name, expected) {
                    self.push_default_type_mismatch(record, path);
                    return None;
                }
                return self.default_explicit_object_value(type_name, fields, record, path, cursor);
            }
            CftStaticValue::RecordReference { type_name, key } => {
                let CftValueType::RecordRef(expected) = ty else {
                    self.push_default_type_mismatch(record, path);
                    return None;
                };
                return Some(ValueDraft::PendingRef {
                    expected_type: type_name.clone(),
                    required_type: expected.clone(),
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
        type_name: &crate::schema::TypeName,
        supplied: &[(crate::schema::FieldName, CftStaticValue)],
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
        for field in schema.full_fields(type_name) {
            let field_path = path.clone().field(field.name.as_str());
            let value = if let Some(value) = supplied.get(&field.name) {
                self.default_value(&field.value_type, value, record, field_path, cursor)
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

fn type_accepts_default(expected: &CftValueType, actual: &CftValueType) -> bool {
    expected == actual
}
