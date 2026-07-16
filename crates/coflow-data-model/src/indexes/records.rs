use crate::build::{BuildSchema, RecordDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdRecordId, CfdTable};
use coflow_cft::{is_cft_identifier, record_key_ident_error, RecordKey, TypeName};
use std::collections::BTreeMap;

pub(crate) struct ModelIndexes {
    pub(crate) tables: BTreeMap<TypeName, CfdTable>,
    pub(crate) record_by_type_key: BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    pub(crate) record_by_domain_key: BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
}

pub(crate) fn build_indexes(
    schema: BuildSchema<'_>,
    drafts: &[RecordDraft],
    diagnostics: &mut Vec<CfdDiagnostic>,
) -> ModelIndexes {
    let mut tables = BTreeMap::<TypeName, CfdTable>::new();
    let mut record_by_type_key = BTreeMap::<TypeName, BTreeMap<RecordKey, CfdRecordId>>::new();
    let mut record_by_domain_key = BTreeMap::<TypeName, BTreeMap<RecordKey, CfdRecordId>>::new();

    for (index, draft) in drafts.iter().enumerate() {
        let record_id = CfdRecordId::new(index);
        let Some(inheritance_root) = schema.inheritance_root(draft.actual_type.as_str()) else {
            continue;
        };
        let table = tables
            .entry(draft.actual_type.clone())
            .or_insert_with(|| CfdTable {
                type_name: draft.actual_type.clone(),
                records: Vec::new(),
                primary_index: BTreeMap::new(),
            });
        table.records.push(record_id);

        if draft.key.is_empty() {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::MissingIdField,
                    format!("record `{}` has an empty key", draft.actual_type),
                )
                .with_primary(Some(record_id), CfdPath::root().field("id")),
            );
            continue;
        }
        if let Some(reason) = record_key_ident_error(&draft.key) {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::InvalidRecordKey,
                    format!("invalid record key `{}`: {reason}", draft.key),
                )
                .with_primary(Some(record_id), CfdPath::root().field("id")),
            );
            continue;
        }
        let Ok(key) = RecordKey::new(draft.key.clone()) else {
            continue;
        };

        if let Some(first) = table.primary_index.insert(key.clone(), record_id) {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::DuplicateId,
                    format!("duplicate key in table `{}`", draft.actual_type),
                )
                .with_primary(Some(record_id), CfdPath::root().field("id"))
                .with_related(
                    Some(first),
                    CfdPath::root().field("id"),
                    "first key is here",
                ),
            );
        }
        record_by_type_key
            .entry(draft.actual_type.clone())
            .or_default()
            .insert(key.clone(), record_id);
        if let Some(first) = record_by_domain_key
            .entry(inheritance_root.clone())
            .or_default()
            .insert(key.clone(), record_id)
        {
            let first_actual_type = drafts
                .get(first.index())
                .map_or("", |first_draft| first_draft.actual_type.as_str());
            if first_actual_type != draft.actual_type.as_str() {
                diagnostics.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::DuplicatePolymorphicId,
                        "duplicate key in inheritance domain",
                    )
                    .with_primary(Some(record_id), CfdPath::root().field("id"))
                    .with_related(
                        Some(first),
                        CfdPath::root().field("id"),
                        "first key is here",
                    ),
                );
            }
        }
    }

    ModelIndexes {
        tables,
        record_by_type_key,
        record_by_domain_key,
    }
}

pub(crate) fn validate_singletons(
    schema: BuildSchema<'_>,
    drafts: &[RecordDraft],
    tables: &BTreeMap<TypeName, CfdTable>,
    diagnostics: &mut Vec<CfdDiagnostic>,
) {
    let singleton_names: Vec<TypeName> = schema
        .singleton_types()
        .map(|meta| meta.name.clone())
        .collect();

    let mut seen_keys: BTreeMap<String, (TypeName, CfdRecordId)> = BTreeMap::new();

    for type_name in &singleton_names {
        let Some(table) = tables.get(type_name) else {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::SingletonRecordCountInvalid,
                    format!("singleton type `{type_name}` has 0 records (must be exactly 1)"),
                )
                .with_primary(None, CfdPath::root()),
            );
            continue;
        };
        if table.records.len() != 1 {
            let count = table.records.len();
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::SingletonRecordCountInvalid,
                    format!("singleton type `{type_name}` has {count} records (must be exactly 1)"),
                )
                .with_primary(table.records.first().copied(), CfdPath::root()),
            );
            continue;
        }
        let record_id = table.records[0];
        let Some(draft) = drafts.get(record_id.index()) else {
            continue;
        };
        if draft.key.is_empty() || !is_cft_identifier(&draft.key) {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::SingletonKeyMissingOrInvalid,
                    format!(
                        "singleton type `{type_name}` record key `{}` is missing or not a valid CFT identifier",
                        draft.key
                    ),
                )
                .with_primary(Some(record_id), CfdPath::root().field("id")),
            );
            continue;
        }
        if let Some((other_type, first_id)) = seen_keys.get(&draft.key) {
            diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::SingletonKeyCollision,
                    format!(
                        "singleton record key `{}` collides between `{type_name}` and `{other_type}`",
                        draft.key
                    ),
                )
                .with_primary(Some(record_id), CfdPath::root().field("id"))
                .with_related(
                    Some(*first_id),
                    CfdPath::root().field("id"),
                    "first occurrence is here",
                ),
            );
        } else {
            seen_keys.insert(draft.key.clone(), (type_name.clone(), record_id));
        }
    }
}
