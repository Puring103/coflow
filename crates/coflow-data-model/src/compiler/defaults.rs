use super::Validator;
use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdEnumValue, CfdRecordId, CfdValue};
use crate::schema_view::{type_accepts_default, CfdValueDraft};
use coflow_cft::{CftFieldMeta, CftSchemaDefaultValue, CftSchemaTypeRef};
use std::collections::BTreeMap;

impl<'s> Validator<'s> {
    pub(super) fn default_field_value(
        &mut self,
        field: &CftFieldMeta,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        self.default_value(&field.ty_ref, value, record, path)
    }

    fn default_value(
        &mut self,
        ty: &CftSchemaTypeRef,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            return match non_nullable_type(ty) {
                CftSchemaTypeRef::Dict(_, _) => {
                    Some(CfdValueDraft::Value(CfdValue::Dict(Vec::new())))
                }
                CftSchemaTypeRef::Named(type_name) if !self.schema.is_schema_enum(type_name) => {
                    self.default_object_value(type_name, record, path)
                }
                _ => {
                    self.push_default_type_mismatch(record, path);
                    None
                }
            };
        }

        let out = match value {
            CftSchemaDefaultValue::Null if ty.is_nullable() => CfdValue::Null,
            CftSchemaDefaultValue::Int(value)
                if type_accepts_default(ty, &CftSchemaTypeRef::Int) =>
            {
                CfdValue::Int(*value)
            }
            CftSchemaDefaultValue::Float(value)
                if type_accepts_default(ty, &CftSchemaTypeRef::Float) =>
            {
                if !value.is_finite() {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            "float value must be finite",
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                CfdValue::Float(*value)
            }
            CftSchemaDefaultValue::Bool(value)
                if type_accepts_default(ty, &CftSchemaTypeRef::Bool) =>
            {
                CfdValue::Bool(*value)
            }
            CftSchemaDefaultValue::String(value)
                if type_accepts_default(ty, &CftSchemaTypeRef::String) =>
            {
                CfdValue::String(value.clone())
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } if matches!(non_nullable_type(ty), CftSchemaTypeRef::Named(name) if name == enum_name && self.schema.is_schema_enum(name)) => {
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant: Some(variant.clone()),
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray
                if matches!(non_nullable_type(ty), CftSchemaTypeRef::Array(_)) =>
            {
                CfdValue::Array(Vec::new())
            }
            _ => {
                self.push_default_type_mismatch(record, path);
                return None;
            }
        };
        Some(CfdValueDraft::Value(out))
    }

    fn default_object_value(
        &mut self,
        type_name: &str,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        let fields = BTreeMap::new();
        let draft = self.validate_record(
            Some(type_name),
            "",
            type_name,
            &[],
            &fields,
            record,
            path,
            /*top_level=*/ false,
        )?;
        Some(CfdValueDraft::Object(Box::new(draft)))
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

fn non_nullable_type(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable_type(inner),
        _ => ty,
    }
}
