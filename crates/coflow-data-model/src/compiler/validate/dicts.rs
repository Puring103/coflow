use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdDictKey, CfdInputDictKey, CfdInputValue, CfdRecordId};
use crate::schema_view::CfdValueDraft;
use coflow_cft::CftSchemaTypeRef;

use super::Validator;

impl Validator<'_> {
    pub(super) fn validate_dict_entries(
        &mut self,
        key_ty: &CftSchemaTypeRef,
        value_ty: &CftSchemaTypeRef,
        entries: &[(CfdInputDictKey, CfdInputValue)],
        record: Option<CfdRecordId>,
        path: &CfdPath,
    ) -> Vec<(CfdDictKey, CfdValueDraft)> {
        let mut seen = std::collections::BTreeMap::<CfdDictKey, CfdPath>::new();
        let mut out = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let key_path = path.clone().dict_key_input(key);
            let Some(key) = self.validate_dict_key(key_ty, key, record, key_path) else {
                continue;
            };
            let value_path = path.clone().dict_key_value(&key);
            if let Some(first) = seen.get(&key) {
                self.push(
                    CfdDiagnostic::error(CfdErrorCode::DuplicateDictKey, "duplicate dict key")
                        .with_primary(record, value_path)
                        .with_related(record, first.clone(), "first key is here"),
                );
                continue;
            }
            seen.insert(key.clone(), value_path.clone());
            let Some(value) = self.validate_value(value_ty, value, record, value_path) else {
                continue;
            };
            out.push((key, value));
        }
        out
    }

    fn validate_dict_key(
        &mut self,
        ty: &CftSchemaTypeRef,
        key: &CfdInputDictKey,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdDictKey> {
        match (ty.non_nullable(), key) {
            (CftSchemaTypeRef::String, CfdInputDictKey::String(value)) => {
                Some(CfdDictKey::String(value.clone()))
            }
            (CftSchemaTypeRef::Int, CfdInputDictKey::Int(value)) => Some(CfdDictKey::Int(*value)),
            (
                CftSchemaTypeRef::Named(expected),
                CfdInputDictKey::EnumVariant { enum_name, variant },
            ) if self.schema.is_schema_enum(expected) => {
                if enum_name != expected {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            format!("expected enum key `{expected}`, got `{enum_name}`"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                let value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdDictKey::Enum(value))
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(CfdErrorCode::TypeMismatch, "dict key type mismatch")
                        .with_primary(record, path),
                );
                None
            }
        }
    }
}
