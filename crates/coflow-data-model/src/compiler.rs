use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdPath};
use crate::model::{
    CfdDataModel, CfdDictKey, CfdDomainId, CfdEnumValue, CfdInputDictKey, CfdInputRecord,
    CfdInputValue, CfdPolymorphicIndex, CfdRecord, CfdRecordId, CfdTable, CfdTypeId, CfdValue,
    RefEdge, RefEdgeId, RefSite,
};
use crate::origin::RecordOrigin;
use crate::schema_view::{
    input_value_kind, type_accepts_default, CfdType, CfdValueDraft, FieldMeta, RecordDraft,
    SchemaView, SpreadFieldSource,
};
use coflow_cft::{is_cft_identifier, record_key_ident_error, CftContainer, CftSchemaDefaultValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ModelCompiler {
    schema: SchemaView,
    input: Vec<CfdInputRecord>,
    diagnostics: Vec<CfdDiagnostic>,
}

struct ModelIndexes {
    tables: BTreeMap<String, CfdTable>,
    inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    record_by_type_key: BTreeMap<(CfdTypeId, String), CfdRecordId>,
    record_by_domain_key: BTreeMap<(CfdDomainId, String), CfdRecordId>,
}

struct SpreadFieldRef<'a> {
    source_type: &'a str,
    key: &'a str,
    field: &'a str,
}

impl ModelCompiler {
    pub(crate) fn new(schema_source: &CftContainer, input: Vec<CfdInputRecord>) -> Self {
        Self {
            schema: SchemaView::new(schema_source),
            input,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn build(mut self) -> Result<CfdDataModel, CfdDiagnostics> {
        // Phase 1: validate input records into drafts. Capture each record's
        // origin so it can flow through to the final `CfdRecord`.
        let mut drafts = Vec::new();
        let input = std::mem::take(&mut self.input);
        {
            let mut v = Validator::new(&self.schema, &mut self.diagnostics);
            for (input_index, record) in input.into_iter().enumerate() {
                let id = CfdRecordId::new(input_index);
                if let Some(mut draft) = v.validate_record(
                    None,
                    &record.key,
                    &record.actual_type,
                    &record.spreads,
                    &record.fields,
                    Some(id),
                    CfdPath::root(),
                    /*top_level=*/ true,
                ) {
                    // Top-level draft inherits the input's origin.
                    draft.origin = record.origin;
                    drafts.push(draft);
                }
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        // Phase 2: build primary / secondary / polymorphic indexes.
        let indexes = self.build_indexes(&drafts);

        // Phase 2b: singleton validation. We run this even when phase 2 has
        // already collected diagnostics so that singleton-specific codes
        // (SingletonRecordCountInvalid / SingletonKeyMissingOrInvalid /
        // SingletonKeyCollision) are surfaced alongside generic ones; this
        // gives users a complete picture in a single build pass.
        // Localized record-key identifier requirements are already covered by
        // the generic `InvalidRecordKey` path because `record_key_ident_error`
        // and `is_cft_identifier` currently use the same rule set; the spec
        // leaves `LocalizedRecordKeyInvalid` reserved for future divergence.
        self.validate_singletons(&drafts, &indexes.tables);
        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        // Phase 3: resolve PendingRef drafts into concrete CfdValue::Ref.
        let mut records = Vec::with_capacity(drafts.len());
        {
            let mut v = Validator::new(&self.schema, &mut self.diagnostics);
            for (index, draft) in drafts.iter().enumerate() {
                let record_id = CfdRecordId::new(index);
                let Some(fields) = v.resolve_fields(
                    &draft.fields,
                    Some(record_id),
                    &CfdPath::root(),
                    &drafts,
                    &indexes.record_by_domain_key,
                ) else {
                    continue;
                };
                let spread_field_sources = resolve_spread_sources(
                    &self.schema,
                    &draft.spread_field_sources,
                    &indexes.record_by_domain_key,
                );
                records.push(CfdRecord {
                    key: draft.key.clone(),
                    actual_type: draft.actual_type.clone(),
                    fields,
                    origin: draft.origin.clone(),
                    spread_field_sources,
                });
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let ref_indexes = build_ref_indexes(&records, &indexes.record_by_domain_key, &self.schema);

        Ok(CfdDataModel {
            tables: indexes.tables,
            inheritance_index: indexes.inheritance_index,
            domain_index: self.schema.domain_index().clone(),
            record_by_type_key: indexes.record_by_type_key,
            record_by_domain_key: indexes.record_by_domain_key,
            records,
            ref_edges: ref_indexes.edges,
            ref_by_site: ref_indexes.by_site,
            ref_by_host: ref_indexes.by_host,
            ref_by_target: ref_indexes.by_target,
            ref_index: ref_indexes.site_targets,
        })
    }

    fn build_indexes(&mut self, drafts: &[RecordDraft]) -> ModelIndexes {
        let mut tables = BTreeMap::<String, CfdTable>::new();
        let mut inheritance_index = BTreeMap::<String, CfdPolymorphicIndex>::new();
        let mut record_by_type_key = BTreeMap::<(CfdTypeId, String), CfdRecordId>::new();
        let mut record_by_domain_key = BTreeMap::<(CfdDomainId, String), CfdRecordId>::new();

        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordId::new(index);
            let Some(type_id) = self.schema.type_id(&draft.actual_type) else {
                continue;
            };
            let Some(domain_id) = self.schema.type_domain_id(&draft.actual_type) else {
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
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::MissingIdField,
                        format!("record `{}` has an empty key", draft.actual_type),
                    )
                    .with_primary(Some(record_id), CfdPath::root().field("id")),
                );
                continue;
            }
            if let Some(reason) = record_key_ident_error(&draft.key) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::InvalidRecordKey,
                        format!("invalid record key `{}`: {reason}", draft.key),
                    )
                    .with_primary(Some(record_id), CfdPath::root().field("id")),
                );
                continue;
            }

            if let Some(first) = table.primary_index.insert(draft.key.clone(), record_id) {
                self.push(
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
            record_by_type_key.insert((type_id, draft.key.clone()), record_id);
            if let Some(first) =
                record_by_domain_key.insert((domain_id, draft.key.clone()), record_id)
            {
                let first_actual_type = drafts
                    .get(first.index())
                    .map_or("", |first_draft| first_draft.actual_type.as_str());
                if first_actual_type != draft.actual_type {
                    self.push(
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
            self.add_polymorphic_ids(
                &mut inheritance_index,
                &draft.actual_type,
                &draft.key,
                record_id,
            );
        }

        ModelIndexes {
            tables,
            inheritance_index,
            record_by_type_key,
            record_by_domain_key,
        }
    }

    fn add_polymorphic_ids(
        &self,
        inheritance_index: &mut BTreeMap<String, CfdPolymorphicIndex>,
        actual_type: &str,
        key: &str,
        record_id: CfdRecordId,
    ) {
        for target_type in self.schema.assignable_target_names(actual_type) {
            if !self.schema.range_is_polymorphic(&target_type) {
                continue;
            }
            let index = inheritance_index
                .entry(target_type.clone())
                .or_insert_with(|| CfdPolymorphicIndex {
                    records: BTreeMap::new(),
                });
            index.records.entry(key.to_string()).or_insert(record_id);
        }
    }

    fn push(&mut self, diagnostic: CfdDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn validate_singletons(&mut self, drafts: &[RecordDraft], tables: &BTreeMap<String, CfdTable>) {
        let singleton_names: Vec<String> = self
            .schema
            .singleton_types()
            .map(|meta| meta.name.clone())
            .collect();

        let mut seen_keys: BTreeMap<String, (String, CfdRecordId)> = BTreeMap::new();

        for type_name in &singleton_names {
            let Some(table) = tables.get(type_name) else {
                self.push(
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
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::SingletonRecordCountInvalid,
                        format!(
                            "singleton type `{type_name}` has {count} records (must be exactly 1)"
                        ),
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
                self.push(
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
                self.push(
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
}

/// Validation and resolution helper.
///
/// Separating `schema` (a copied `&'s SchemaView` reference) from
/// `diagnostics` (a mutable borrow) lets every method call
/// `schema.full_fields(type)` and obtain a `&'s [FieldMeta]` slice whose
/// lifetime is tied to the outer `SchemaView`, **not** to `self`. The slice
/// can therefore be iterated while `&mut self` methods are called to emit
/// diagnostics — something impossible when the schema is an owned field of
/// the same struct.
struct Validator<'s> {
    schema: &'s SchemaView,
    diagnostics: &'s mut Vec<CfdDiagnostic>,
}

impl<'s> Validator<'s> {
    fn new(schema: &'s SchemaView, diagnostics: &'s mut Vec<CfdDiagnostic>) -> Self {
        Self {
            schema,
            diagnostics,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_record(
        &mut self,
        expected_type: Option<&str>,
        key: &str,
        actual_type: &str,
        input_spreads: &[CfdInputValue],
        input_fields: &BTreeMap<String, CfdInputValue>,
        record: Option<CfdRecordId>,
        path: CfdPath,
        top_level: bool,
    ) -> Option<RecordDraft> {
        // Copy the shared schema reference so that the &'s [FieldMeta] slice
        // obtained below has a lifetime independent of `self`, allowing
        // &mut self methods to be called while iterating over the fields.
        let schema = self.schema;
        let diagnostic_start = self.diagnostics.len();

        let Some(is_abstract) = schema.types.get(actual_type).map(|meta| meta.is_abstract) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::UnknownType,
                    format!("unknown type `{actual_type}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        if is_abstract {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::AbstractRecordType,
                    format!("abstract type `{actual_type}` cannot be instantiated"),
                )
                .with_primary(record, path),
            );
            return None;
        }
        if let Some(expected) = expected_type {
            if !schema.is_assignable(actual_type, expected) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::ObjectTypeMismatch,
                        format!("type `{actual_type}` is not assignable to `{expected}`"),
                    )
                    .with_primary(record, path),
                );
                return None;
            }
        }

        // `fields` has lifetime 's — independent of `self` — so it can be
        // held across calls to &mut self methods below.
        let fields = schema.full_fields(actual_type);
        let known_fields = fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<BTreeSet<_>>();
        for name in input_fields.keys() {
            if !known_fields.contains(name.as_str()) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::UnknownField,
                        format!("unknown field `{name}` on type `{actual_type}`"),
                    )
                    .with_primary(record, path.clone().field(name.clone())),
                );
            }
        }

        let mut out = BTreeMap::new();
        let mut spread_field_sources = BTreeMap::new();
        for spread in input_spreads {
            let spread_origin = top_level_spread_source(actual_type, spread);
            let Some(spread_fields) =
                self.validate_object_spread(actual_type, spread, record, path.clone())
            else {
                continue;
            };
            for name in spread_fields.keys() {
                if let Some(origin) = &spread_origin {
                    spread_field_sources.insert(name.clone(), origin.clone());
                }
            }
            out.extend(spread_fields);
        }
        let _ = top_level;

        for field in fields {
            let field_path = path.clone().field(field.name.clone());
            let value = if let Some(value) = input_fields.get(&field.name) {
                // An explicit field overrides any spread-imported value.
                spread_field_sources.remove(&field.name);
                self.validate_field_value(field, value, record, field_path)
            } else if out.contains_key(&field.name) {
                continue;
            } else if let Some(default) = &field.default {
                self.default_field_value(field, default, record, field_path)
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
                out.insert(field.name.clone(), value);
            }
        }

        if self.diagnostics.len() == diagnostic_start {
            Some(RecordDraft {
                key: key.to_string(),
                actual_type: actual_type.to_string(),
                fields: out,
                origin: RecordOrigin::None,
                spread_field_sources,
            })
        } else {
            None
        }
    }

    fn validate_field_value(
        &mut self,
        field: &FieldMeta,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        self.validate_singleton_ref_only(&field.ty, value, record, path.clone())?;
        self.validate_value(&field.ty, value, record, path)
    }

    fn validate_singleton_ref_only(
        &mut self,
        ty: &CfdType,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<()> {
        if !self.schema.type_contains_singleton(ty) {
            return Some(());
        }

        match (non_nullable_type(ty), value) {
            (_, CfdInputValue::Null) if ty.is_nullable() => Some(()),
            (CfdType::Type(type_name), CfdInputValue::RecordRef(_))
                if self.schema.type_name_is_singleton(type_name) =>
            {
                Some(())
            }
            (CfdType::Ref(type_name), CfdInputValue::RecordRef(_))
                if self.schema.type_name_is_singleton(type_name) =>
            {
                Some(())
            }
            (
                CfdType::Type(type_name),
                CfdInputValue::Object { .. } | CfdInputValue::ObjectSpread { .. },
            ) if self.schema.type_name_is_singleton(type_name) => {
                self.push_singleton_ref_only_diagnostic(record, path);
                None
            }
            (
                CfdType::Ref(type_name),
                CfdInputValue::Object { .. } | CfdInputValue::ObjectSpread { .. },
            ) if self.schema.type_name_is_singleton(type_name) => {
                self.push_singleton_ref_only_diagnostic(record, path);
                None
            }
            (CfdType::Array(inner), CfdInputValue::Array(items)) => {
                let mut ok = true;
                for (index, item) in items.iter().enumerate() {
                    if self
                        .validate_singleton_ref_only(inner, item, record, path.clone().index(index))
                        .is_none()
                    {
                        ok = false;
                    }
                }
                ok.then_some(())
            }
            (
                CfdType::Dict(_, value_ty),
                CfdInputValue::Dict(entries) | CfdInputValue::DictSpread { entries, .. },
            ) => {
                let mut ok = true;
                for (key, item) in entries {
                    if self
                        .validate_singleton_ref_only(
                            value_ty,
                            item,
                            record,
                            path.clone().dict_key_input(key),
                        )
                        .is_none()
                    {
                        ok = false;
                    }
                }
                ok.then_some(())
            }
            _ => Some(()),
        }
    }

    fn push_singleton_ref_only_diagnostic(&mut self, record: Option<CfdRecordId>, path: CfdPath) {
        self.push(
            CfdDiagnostic::error(
                CfdErrorCode::TypeMismatch,
                "singleton fields only allow record references",
            )
            .with_primary(record, path),
        );
    }

    fn validate_value(
        &mut self,
        ty: &CfdType,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if let CfdType::Nullable(inner) = ty {
            return if matches!(value, CfdInputValue::Null) {
                Some(CfdValueDraft::Value(CfdValue::Null))
            } else {
                self.validate_value(inner, value, record, path)
            };
        }

        match (ty, value) {
            (CfdType::Int, CfdInputValue::Int(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Int(*value)))
            }
            (CfdType::Float, CfdInputValue::Float(value)) => {
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
                Some(CfdValueDraft::Value(CfdValue::Float(*value)))
            }
            (CfdType::Bool, CfdInputValue::Bool(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Bool(*value)))
            }
            (CfdType::String, CfdInputValue::String(value)) => {
                Some(CfdValueDraft::Value(CfdValue::String(value.clone())))
            }
            (CfdType::Enum(expected), CfdInputValue::EnumVariant { enum_name, variant }) => {
                if enum_name != expected {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            format!("expected enum `{expected}`, got `{enum_name}`"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                let enum_value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdValueDraft::Value(CfdValue::Enum(enum_value)))
            }
            (CfdType::Ref(expected), CfdInputValue::RecordRef(key)) => {
                Some(CfdValueDraft::PendingRef {
                    expected_type: expected.clone(),
                    key: key.clone(),
                })
            }
            (
                CfdType::Type(expected),
                CfdInputValue::Object {
                    actual_type,
                    fields,
                }
                | CfdInputValue::ObjectSpread {
                    actual_type,
                    spreads: _,
                    fields,
                },
            ) => {
                let actual = if let Some(actual) = actual_type {
                    actual.clone()
                } else if self.schema.range_is_polymorphic(expected) {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::MissingObjectType,
                            format!("field of polymorphic type `{expected}` needs an actual type"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                } else {
                    expected.clone()
                };
                let spreads = match value {
                    CfdInputValue::ObjectSpread { spreads, .. } => spreads.as_slice(),
                    _ => &[],
                };
                let draft = self.validate_record(
                    Some(expected),
                    "",
                    &actual,
                    spreads,
                    fields,
                    record,
                    path,
                    /*top_level=*/ false,
                )?;
                Some(CfdValueDraft::Object(Box::new(draft)))
            }
            (CfdType::Array(inner), CfdInputValue::Array(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(value) =
                        self.validate_value(inner, item, record, path.clone().index(index))
                    else {
                        continue;
                    };
                    out.push(value);
                }
                Some(CfdValueDraft::Array(out))
            }
            (CfdType::Dict(key_ty, value_ty), CfdInputValue::Dict(entries)) => {
                let out = self.validate_dict_entries(key_ty, value_ty, entries, record, &path);
                Some(CfdValueDraft::Dict(out))
            }
            (CfdType::Dict(key_ty, value_ty), CfdInputValue::DictSpread { spreads, entries }) => {
                let mut out_spreads = Vec::with_capacity(spreads.len());
                for spread in spreads {
                    let Some(spread) = self.validate_value(ty, spread, record, path.clone()) else {
                        continue;
                    };
                    out_spreads.push(spread);
                }
                let out_entries =
                    self.validate_dict_entries(key_ty, value_ty, entries, record, &path);
                Some(CfdValueDraft::DictSpread {
                    spreads: out_spreads,
                    entries: out_entries,
                })
            }
            _ => {
                self.type_mismatch(&ty.display(), value, record, path);
                None
            }
        }
    }

    fn validate_object_spread(
        &mut self,
        type_name: &str,
        spread: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<BTreeMap<String, CfdValueDraft>> {
        match spread {
            CfdInputValue::RecordRef(key) => Some(
                self.schema
                    .full_fields(type_name)
                    .iter()
                    .map(|field| {
                        (
                            field.name.clone(),
                            CfdValueDraft::PendingSpreadField {
                                source_type: type_name.to_string(),
                                key: key.clone(),
                                field: field.name.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
            CfdInputValue::Object { .. } | CfdInputValue::ObjectSpread { .. } => {
                let draft = self.validate_value(
                    &CfdType::Type(type_name.to_string()),
                    spread,
                    record,
                    path,
                )?;
                let CfdValueDraft::Object(record_draft) = draft else {
                    return None;
                };
                Some(record_draft.fields)
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        "object spread requires an object value",
                    )
                    .with_primary(record, path),
                );
                None
            }
        }
    }

    fn validate_dict_key(
        &mut self,
        ty: &CfdType,
        key: &CfdInputDictKey,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdDictKey> {
        match (ty, key) {
            (CfdType::String, CfdInputDictKey::String(value)) => {
                Some(CfdDictKey::String(value.clone()))
            }
            (CfdType::Int, CfdInputDictKey::Int(value)) => Some(CfdDictKey::Int(*value)),
            (CfdType::Enum(expected), CfdInputDictKey::EnumVariant { enum_name, variant }) => {
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

    fn validate_dict_entries(
        &mut self,
        key_ty: &CfdType,
        value_ty: &CfdType,
        entries: &[(CfdInputDictKey, CfdInputValue)],
        record: Option<CfdRecordId>,
        path: &CfdPath,
    ) -> Vec<(CfdDictKey, CfdValueDraft)> {
        let mut seen = BTreeMap::<CfdDictKey, CfdPath>::new();
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

    fn default_field_value(
        &mut self,
        field: &FieldMeta,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        self.default_value(&field.ty, value, record, path)
    }

    fn default_value(
        &mut self,
        ty: &CfdType,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            return match non_nullable_type(ty) {
                CfdType::Dict(_, _) => Some(CfdValueDraft::Value(CfdValue::Dict(Vec::new()))),
                CfdType::Type(type_name) => self.default_object_value(type_name, record, path),
                _ => {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            "schema default does not match field type",
                        )
                        .with_primary(record, path),
                    );
                    None
                }
            };
        }

        let out = match value {
            CftSchemaDefaultValue::Null if ty.is_nullable() => CfdValue::Null,
            CftSchemaDefaultValue::Int(value) if type_accepts_default(ty, &CfdType::Int) => {
                CfdValue::Int(*value)
            }
            CftSchemaDefaultValue::Float(value) if type_accepts_default(ty, &CfdType::Float) => {
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
            CftSchemaDefaultValue::Bool(value) if type_accepts_default(ty, &CfdType::Bool) => {
                CfdValue::Bool(*value)
            }
            CftSchemaDefaultValue::String(value) if type_accepts_default(ty, &CfdType::String) => {
                CfdValue::String(value.clone())
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } if matches!(non_nullable_type(ty), CfdType::Enum(name) if name == enum_name) => {
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant: Some(variant.clone()),
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray
                if matches!(non_nullable_type(ty), CfdType::Array(_)) =>
            {
                CfdValue::Array(Vec::new())
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        "schema default does not match field type",
                    )
                    .with_primary(record, path),
                );
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

    fn resolve_fields(
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
                let spread_field_sources = resolve_spread_sources(
                    self.schema,
                    &record_draft.spread_field_sources,
                    record_by_domain_key,
                );
                Some(CfdValue::Object(Box::new(CfdRecord {
                    key: record_draft.key.clone(),
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                    origin: RecordOrigin::None,
                    spread_field_sources,
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

    fn resolve_enum_value(
        &mut self,
        enum_name: &str,
        variant: &str,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdEnumValue> {
        let Some(value) = self.schema.enum_variant_value(enum_name, variant) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::InvalidEnumVariant,
                    format!("unknown enum variant `{enum_name}.{variant}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        Some(CfdEnumValue {
            enum_name: enum_name.to_string(),
            variant: Some(variant.to_string()),
            value,
        })
    }

    fn type_mismatch(
        &mut self,
        expected: &str,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) {
        self.push(
            CfdDiagnostic::error(
                CfdErrorCode::TypeMismatch,
                format!("expected {expected}, got {}", input_value_kind(value)),
            )
            .with_primary(record, path),
        );
    }

    fn push(&mut self, diagnostic: CfdDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}

fn non_nullable_type(ty: &CfdType) -> &CfdType {
    match ty {
        CfdType::Nullable(inner) => non_nullable_type(inner),
        _ => ty,
    }
}

fn top_level_spread_source(
    expected_type: &str,
    spread: &CfdInputValue,
) -> Option<SpreadFieldSource> {
    match spread {
        CfdInputValue::RecordRef(key) => Some(SpreadFieldSource {
            expected_type: expected_type.to_string(),
            key: key.clone(),
        }),
        _ => None,
    }
}

/// Resolve a draft's spread-field-source map (`name → SpreadFieldSource`,
/// which holds the textual expected type+key) into the public-facing
/// `name → CfdRecordId` map stored on `CfdRecord`. Sources that don't
/// resolve are dropped silently — phase 1 already reported them as
/// unresolved record refs if they were truly missing.
fn resolve_spread_sources(
    schema: &SchemaView,
    sources: &BTreeMap<String, SpreadFieldSource>,
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
) -> BTreeMap<String, CfdRecordId> {
    let mut out = BTreeMap::new();
    for (field, source) in sources {
        if let Some(id) = lookup_domain_ref(
            schema,
            record_by_domain_key,
            &source.expected_type,
            &source.key,
        ) {
            out.insert(field.clone(), id);
        }
    }
    out
}

#[derive(Default)]
struct RefIndexes {
    edges: Vec<RefEdge>,
    by_site: BTreeMap<RefSite, RefEdgeId>,
    by_host: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    by_target: BTreeMap<CfdRecordId, Vec<RefEdgeId>>,
    site_targets: BTreeMap<RefSite, CfdRecordId>,
}

fn build_ref_indexes(
    records: &[CfdRecord],
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &SchemaView,
) -> RefIndexes {
    let mut out = RefIndexes::default();
    let context = RefEdgeBuildContext {
        records,
        record_by_domain_key,
        schema,
    };
    for (index, record) in records.iter().enumerate() {
        let host = CfdRecordId::from_index(index);
        let root = CfdPath::root();
        for (name, value) in &record.fields {
            let Some(field) = context
                .schema
                .full_fields(&record.actual_type)
                .iter()
                .find(|field| field.name == *name)
            else {
                continue;
            };
            collect_ref_edges(
                value,
                &field.ty,
                host,
                root.clone().field(name.clone()),
                &context,
                &mut out,
            );
        }
    }
    out
}

struct RefEdgeBuildContext<'a> {
    records: &'a [CfdRecord],
    record_by_domain_key: &'a BTreeMap<(CfdDomainId, String), CfdRecordId>,
    schema: &'a SchemaView,
}

fn collect_ref_edges(
    value: &CfdValue,
    ty: &CfdType,
    host: CfdRecordId,
    path: CfdPath,
    context: &RefEdgeBuildContext<'_>,
    out: &mut RefIndexes,
) {
    match (value, non_nullable_type(ty)) {
        (CfdValue::Ref(key), CfdType::Ref(expected_type)) => {
            let Some(expected_type_id) = context.schema.type_id(expected_type) else {
                return;
            };
            let Some(domain) = context.schema.type_domain_id(expected_type) else {
                return;
            };
            let Some(target) = lookup_domain_ref(
                context.schema,
                context.record_by_domain_key,
                expected_type,
                key,
            ) else {
                return;
            };
            let Some(target_record) = context.records.get(target.index()) else {
                return;
            };
            let Some(target_type) = context.schema.type_id(&target_record.actual_type) else {
                return;
            };
            let site = RefSite::new(host, path.clone());
            let id = RefEdgeId::new(out.edges.len());
            out.edges.push(RefEdge {
                id,
                site: site.clone(),
                host,
                path,
                expected_type: expected_type_id,
                domain,
                key: key.clone(),
                target,
                target_type,
            });
            out.by_site.insert(site.clone(), id);
            out.by_host.entry(host).or_default().push(id);
            out.by_target.entry(target).or_default().push(id);
            out.site_targets.insert(site, target);
        }
        (CfdValue::Object(boxed), CfdType::Type(_)) => {
            for (name, inner) in &boxed.fields {
                let Some(field) = context
                    .schema
                    .full_fields(&boxed.actual_type)
                    .iter()
                    .find(|field| field.name == *name)
                else {
                    continue;
                };
                collect_ref_edges(
                    inner,
                    &field.ty,
                    host,
                    path.clone().field(name.clone()),
                    context,
                    out,
                );
            }
        }
        (CfdValue::Array(items), CfdType::Array(inner_ty)) => {
            for (index, item) in items.iter().enumerate() {
                collect_ref_edges(
                    item,
                    inner_ty,
                    host,
                    path.clone().index(index),
                    context,
                    out,
                );
            }
        }
        (CfdValue::Dict(entries), CfdType::Dict(_, value_ty)) => {
            for (key, item) in entries {
                collect_ref_edges(
                    item,
                    value_ty,
                    host,
                    path.clone().dict_key_value(key),
                    context,
                    out,
                );
            }
        }
        _ => {}
    }
}

fn lookup_domain_ref(
    schema: &SchemaView,
    record_by_domain_key: &BTreeMap<(CfdDomainId, String), CfdRecordId>,
    target_type: &str,
    key: &str,
) -> Option<CfdRecordId> {
    schema
        .type_domain_id(target_type)
        .and_then(|domain_id| record_by_domain_key.get(&(domain_id, key.to_string())))
        .copied()
}
