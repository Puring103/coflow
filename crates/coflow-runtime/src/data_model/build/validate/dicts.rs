use crate::data_model::build::ValueDraft;
use crate::data_model::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::data_model::ingest::{LoadedDictKeyDraft, LoadedValueDraft};
use crate::data_model::model::{CfdDictKey, CfdRecordId};
use coflow_language::limits::TraversalCursor;
use coflow_language::CftValueType;

use super::Validator;

impl Validator<'_, '_> {
    pub(super) fn validate_dict_entries(
        &mut self,
        key_ty: &CftValueType,
        value_ty: &CftValueType,
        entries: &[(LoadedDictKeyDraft, LoadedValueDraft)],
        record: Option<CfdRecordId>,
        path: &CfdPath,
        cursor: TraversalCursor,
    ) -> Vec<(CfdDictKey, ValueDraft)> {
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
            let Some(value) = self.validate_value(value_ty, value, record, value_path, cursor)
            else {
                continue;
            };
            out.push((key, value));
        }
        out
    }

    fn validate_dict_key(
        &mut self,
        ty: &CftValueType,
        key: &LoadedDictKeyDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdDictKey> {
        let value = match key {
            LoadedDictKeyDraft::String(value) => CfdDictKey::String(value.clone()),
            LoadedDictKeyDraft::Int(value) => CfdDictKey::Int(*value),
            LoadedDictKeyDraft::EnumVariant { enum_name, variant } => CfdDictKey::Enum(
                self.resolve_enum_key_value(enum_name, variant, record, path.clone())?,
            ),
        };
        match crate::data_model::semantics::validate_dict_key_for_schema(
            self.schema.cft(),
            ty,
            &value,
        ) {
            Ok(()) => Some(value),
            Err(error) => {
                self.push(
                    CfdDiagnostic::error(
                        super::super::semantic_error_code(error.kind()),
                        error.message(),
                    )
                    .with_primary(record, path),
                );
                None
            }
        }
    }
}
