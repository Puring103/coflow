use super::{SpreadFieldRef, Validator};
use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdDictKey, CfdDomainId, CfdObject, CfdRecordId, CfdValue};
use crate::schema_view::{CfdValueDraft, RecordDraft};
use std::collections::BTreeMap;

impl Validator<'_> {
    pub(super) fn resolve_fields(
        &mut self,
        fields: &BTreeMap<String, CfdValueDraft>,
        record: Option<CfdRecordId>,
        path: &CfdPath,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        for (name, value) in fields {
            let value_path = path.clone().field(name.clone());
            let Some(value) =
                self.resolve_value(value, record, value_path, drafts, record_by_domain_key)
            else {
                continue;
            };
            out.insert(name.clone(), value);
        }
        if self.diagnostics.len() == diagnostic_start {
            Some(out)
        } else {
            None
        }
    }

    fn resolve_value(
        &mut self,
        value: &CfdValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    ) -> Option<CfdValue> {
        match value {
            CfdValueDraft::Value(value) => Some(value.clone()),
            CfdValueDraft::PendingRef { expected_type, key } => {
                // Ref resolution still happens here so we surface
                // RefTargetNotFound diagnostics during build, but the
                // resolved id is stashed in the model's ref edge indexes after
                // all records are built rather than carried in the value.
                let _ = self.resolve_ref_target(
                    expected_type,
                    key,
                    drafts,
                    record_by_domain_key,
                    record,
                    &path,
                )?;
                Some(CfdValue::Ref(key.clone()))
            }
            CfdValueDraft::PendingSpreadField {
                source_type,
                key,
                field,
            } => self.resolve_spread_field(
                &SpreadFieldRef {
                    source_type,
                    key,
                    field,
                },
                record,
                path,
                drafts,
                record_by_domain_key,
            ),
            CfdValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(
                    &record_draft.fields,
                    record,
                    &path,
                    drafts,
                    record_by_domain_key,
                )?;
                Some(CfdValue::Object(Box::new(CfdObject {
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                })))
            }
            CfdValueDraft::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(value) = self.resolve_value(
                        item,
                        record,
                        path.clone().index(index),
                        drafts,
                        record_by_domain_key,
                    ) else {
                        continue;
                    };
                    out.push(value);
                }
                Some(CfdValue::Array(out))
            }
            CfdValueDraft::Dict(entries) => {
                let out = self.resolve_dict_entries(
                    entries,
                    record,
                    &path,
                    drafts,
                    record_by_domain_key,
                )?;
                Some(CfdValue::Dict(out))
            }
            CfdValueDraft::DictSpread { spreads, entries } => {
                let out = self.resolve_dict_spread(
                    spreads,
                    entries,
                    record,
                    &path,
                    drafts,
                    record_by_domain_key,
                )?;
                Some(CfdValue::Dict(out))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_ref_target(
        &mut self,
        expected_type: &str,
        key: &str,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
        record: Option<CfdRecordId>,
        path: &CfdPath,
    ) -> Option<CfdRecordId> {
        let target = self
            .schema
            .type_domain_id(expected_type)
            .and_then(|domain_id| record_by_domain_key.get(&(domain_id, key.to_string())))
            .copied();

        let Some(target) = target else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{expected_type}` with key `{key}` was not found"),
                )
                .with_primary(record, path.clone()),
            );
            return None;
        };

        let target_draft = drafts.get(target.index())?;
        if !self
            .schema
            .is_assignable(&target_draft.actual_type, expected_type)
        {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    format!(
                        "ref target actual type `{}` is not assignable to `{expected_type}`",
                        target_draft.actual_type
                    ),
                )
                .with_primary(record, path.clone()),
            );
            return None;
        }

        Some(target)
    }

    fn resolve_dict_entries(
        &mut self,
        entries: &[(CfdDictKey, CfdValueDraft)],
        record: Option<CfdRecordId>,
        path: &CfdPath,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let Some(value) = self.resolve_value(
                value,
                record,
                path.clone().dict_key_value(key),
                drafts,
                record_by_domain_key,
            ) else {
                continue;
            };
            out.push((key.clone(), value));
        }
        if self.diagnostics.len() == diagnostic_start {
            Some(out)
        } else {
            None
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_dict_spread(
        &mut self,
        spreads: &[CfdValueDraft],
        entries: &[(CfdDictKey, CfdValueDraft)],
        record: Option<CfdRecordId>,
        path: &CfdPath,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut merged = BTreeMap::<CfdDictKey, CfdValue>::new();
        for spread in spreads {
            let Some(CfdValue::Dict(entries)) =
                self.resolve_value(spread, record, path.clone(), drafts, record_by_domain_key)
            else {
                if self.diagnostics.len() == diagnostic_start {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            "dict spread requires a dict value",
                        )
                        .with_primary(record, path.clone()),
                    );
                }
                continue;
            };
            for (key, value) in entries {
                merged.insert(key, value);
            }
        }

        for (key, value) in entries {
            let Some(value) = self.resolve_value(
                value,
                record,
                path.clone().dict_key_value(key),
                drafts,
                record_by_domain_key,
            ) else {
                continue;
            };
            merged.insert(key.clone(), value);
        }

        if self.diagnostics.len() == diagnostic_start {
            Some(merged.into_iter().collect())
        } else {
            None
        }
    }

    fn resolve_spread_field(
        &mut self,
        spread: &SpreadFieldRef<'_>,
        record: Option<CfdRecordId>,
        path: CfdPath,
        drafts: &[RecordDraft],
        record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    ) -> Option<CfdValue> {
        let source_id = self.resolve_ref_target(
            spread.source_type,
            spread.key,
            drafts,
            record_by_domain_key,
            record,
            &path,
        )?;
        let source_draft = drafts.get(source_id.index())?;
        let Some(value) = source_draft.fields.get(spread.field) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::UnknownField,
                    format!("spread field `{}` was not found", spread.field),
                )
                .with_primary(record, path),
            );
            return None;
        };

        self.resolve_value(value, record, path, drafts, record_by_domain_key)
    }
}
