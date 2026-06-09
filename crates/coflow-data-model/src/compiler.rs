use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdPath};
use crate::model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdIdValue, CfdInputDictKey, CfdInputRecord,
    CfdInputValue, CfdPolymorphicIndex, CfdRecord, CfdRecordId, CfdTable, CfdValue,
};
use crate::schema_view::{
    id_from_fields, id_matches_type, index_key_from_draft, input_value_kind, type_accepts_default,
    CfdType, CfdValueDraft, FieldMeta, RecordDraft, SchemaView,
};
use coflow_cft::{CftContainer, CftSchemaDefaultValue};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ModelCompiler {
    schema: SchemaView,
    input: Vec<CfdInputRecord>,
    diagnostics: Vec<CfdDiagnostic>,
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
        let mut drafts = Vec::new();
        let input = std::mem::take(&mut self.input);
        for (input_index, record) in input.into_iter().enumerate() {
            let id = CfdRecordId::new(input_index);
            if let Some(draft) = self.validate_record(
                None,
                &record.actual_type,
                &record.fields,
                Some(id),
                CfdPath::root(),
            ) {
                drafts.push(draft);
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let (tables, inheritance_index) = self.build_indexes(&drafts);
        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let mut records = Vec::with_capacity(drafts.len());
        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordId::new(index);
            let Some(fields) = self.resolve_fields(
                &draft.fields,
                Some(record_id),
                &CfdPath::root(),
                &tables,
                &inheritance_index,
            ) else {
                continue;
            };
            records.push(CfdRecord {
                actual_type: draft.actual_type.clone(),
                fields,
            });
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        Ok(CfdDataModel {
            tables,
            inheritance_index,
            records,
        })
    }

    fn validate_record(
        &mut self,
        expected_type: Option<&str>,
        actual_type: &str,
        input_fields: &BTreeMap<String, CfdInputValue>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<RecordDraft> {
        let diagnostic_start = self.diagnostics.len();
        let Some(is_abstract) = self
            .schema
            .types
            .get(actual_type)
            .map(|meta| meta.is_abstract)
        else {
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
            if !self.schema.is_assignable(actual_type, expected) {
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

        let fields = self.schema.full_fields(actual_type);
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
        for field in fields {
            let field_path = path.clone().field(field.name.clone());
            let value = if let Some(value) = input_fields.get(&field.name) {
                self.validate_field_value(&field, value, record, field_path)
            } else if let Some(default) = &field.default {
                self.default_field_value(&field, default, record, field_path)
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
                out.insert(field.name, value);
            }
        }

        if self.diagnostics.len() == diagnostic_start {
            Some(RecordDraft {
                actual_type: actual_type.to_string(),
                fields: out,
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
        if let Some(target_type) = &field.ref_target {
            return self.validate_ref_field(field, target_type, value, record, path);
        }
        self.validate_value(&field.ty, value, record, path)
    }

    fn validate_ref_field(
        &mut self,
        field: &FieldMeta,
        target_type: &str,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CfdInputValue::Null) {
            if field.ty.is_nullable() {
                return Some(CfdValueDraft::Value(CfdValue::Null));
            }
            self.type_mismatch("non-null @ref id", value, record, path);
            return None;
        }

        let id = match value {
            CfdInputValue::Ref(id) => id.clone(),
            CfdInputValue::String(value) => CfdIdValue::String(value.clone()),
            CfdInputValue::Int(value) => CfdIdValue::Int(*value),
            _ => {
                self.type_mismatch("@ref id", value, record, path);
                return None;
            }
        };

        if !id_matches_type(&id, &field.ty) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    "@ref id does not match the field id type",
                )
                .with_primary(record, path),
            );
            return None;
        }

        Some(CfdValueDraft::PendingRef {
            target_type: target_type.to_string(),
            id,
        })
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
            (
                CfdType::Type(expected),
                CfdInputValue::Object {
                    actual_type,
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
                let draft = self.validate_record(Some(expected), &actual, fields, record, path)?;
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
                let mut seen = BTreeMap::<CfdDictKey, CfdPath>::new();
                let mut out = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let key_path = path.clone().dict_key_input(key);
                    let Some(key) = self.validate_dict_key(key_ty, key, record, key_path.clone())
                    else {
                        continue;
                    };
                    let value_path = path.clone().dict_key_value(&key);
                    if let Some(first) = seen.get(&key) {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::DuplicateDictKey,
                                "duplicate dict key",
                            )
                            .with_primary(record, value_path)
                            .with_related(
                                record,
                                first.clone(),
                                "first key is here",
                            ),
                        );
                        continue;
                    }
                    seen.insert(key.clone(), value_path.clone());
                    let Some(value) = self.validate_value(value_ty, value, record, value_path)
                    else {
                        continue;
                    };
                    out.push((key, value));
                }
                Some(CfdValueDraft::Dict(out))
            }
            _ => {
                self.type_mismatch(&ty.display(), value, record, path);
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

    fn default_field_value(
        &mut self,
        field: &FieldMeta,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        let Some(target_type) = &field.ref_target else {
            return self.default_value(&field.ty, value, record, path);
        };

        match value {
            CftSchemaDefaultValue::Null if field.ty.is_nullable() => {
                Some(CfdValueDraft::Value(CfdValue::Null))
            }
            CftSchemaDefaultValue::String(value)
                if id_matches_type(&CfdIdValue::String(value.clone()), &field.ty) =>
            {
                Some(CfdValueDraft::PendingRef {
                    target_type: target_type.clone(),
                    id: CfdIdValue::String(value.clone()),
                })
            }
            CftSchemaDefaultValue::Int(value)
                if id_matches_type(&CfdIdValue::Int(*value), &field.ty) =>
            {
                Some(CfdValueDraft::PendingRef {
                    target_type: target_type.clone(),
                    id: CfdIdValue::Int(*value),
                })
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        "schema default does not match @ref field type",
                    )
                    .with_primary(record, path),
                );
                None
            }
        }
    }

    fn default_value(
        &mut self,
        ty: &CfdType,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
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
            } if type_accepts_default(ty, &CfdType::Enum(enum_name.clone())) => {
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.clone(),
                    variant: Some(variant.clone()),
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray if matches!(ty, CfdType::Array(_)) => {
                CfdValue::Array(Vec::new())
            }
            CftSchemaDefaultValue::EmptyObject if matches!(ty, CfdType::Dict(_, _)) => {
                CfdValue::Dict(BTreeMap::new())
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

    fn build_indexes(
        &mut self,
        drafts: &[RecordDraft],
    ) -> (
        BTreeMap<String, CfdTable>,
        BTreeMap<String, CfdPolymorphicIndex>,
    ) {
        let mut tables = BTreeMap::<String, CfdTable>::new();
        let mut inheritance_index = BTreeMap::<String, CfdPolymorphicIndex>::new();

        for (index, draft) in drafts.iter().enumerate() {
            let record_id = CfdRecordId::new(index);
            let table = tables
                .entry(draft.actual_type.clone())
                .or_insert_with(|| CfdTable {
                    type_name: draft.actual_type.clone(),
                    records: Vec::new(),
                    primary_index: BTreeMap::new(),
                    secondary_indexes: BTreeMap::new(),
                });
            table.records.push(record_id);

            if let Some(id_field) = self.schema.id_field_for_actual(&draft.actual_type) {
                if let Some(id) = id_from_fields(&draft.fields, &id_field.name) {
                    if let Some(first) = table.primary_index.insert(id.clone(), record_id) {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::DuplicateId,
                                format!("duplicate id in table `{}`", draft.actual_type),
                            )
                            .with_primary(
                                Some(record_id),
                                CfdPath::root().field(id_field.name.clone()),
                            )
                            .with_related(
                                Some(first),
                                CfdPath::root().field(id_field.name.clone()),
                                "first id is here",
                            ),
                        );
                    }
                    self.add_polymorphic_ids(
                        &mut inheritance_index,
                        &draft.actual_type,
                        &id,
                        record_id,
                        &id_field.name,
                    );
                } else {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::MissingIdField,
                            format!("record `{}` has no usable @id field", draft.actual_type),
                        )
                        .with_primary(
                            Some(record_id),
                            CfdPath::root().field(id_field.name.clone()),
                        ),
                    );
                }
            }

            for field in self.schema.index_fields_for_actual(&draft.actual_type) {
                let Some(value) = draft.fields.get(&field.name) else {
                    continue;
                };
                let Some(key) = index_key_from_draft(value) else {
                    continue;
                };
                if let Some(table) = tables.get_mut(&draft.actual_type) {
                    table
                        .secondary_indexes
                        .entry(field.name.clone())
                        .or_default()
                        .entry(key)
                        .or_default()
                        .push(record_id);
                }
            }
        }

        (tables, inheritance_index)
    }

    fn add_polymorphic_ids(
        &mut self,
        inheritance_index: &mut BTreeMap<String, CfdPolymorphicIndex>,
        actual_type: &str,
        id: &CfdIdValue,
        record_id: CfdRecordId,
        id_field_name: &str,
    ) {
        for target_type in self.schema.assignable_target_names(actual_type) {
            if !self.schema.range_is_polymorphic(&target_type) {
                continue;
            }
            if !self.schema.range_has_id(&target_type) {
                continue;
            }
            let index = inheritance_index
                .entry(target_type.clone())
                .or_insert_with(|| CfdPolymorphicIndex {
                    records: BTreeMap::new(),
                });
            if let Some(first) = index.records.insert(id.clone(), record_id) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::DuplicatePolymorphicId,
                        format!("duplicate id in polymorphic range `{target_type}`"),
                    )
                    .with_primary(
                        Some(record_id),
                        CfdPath::root().field(id_field_name.to_string()),
                    )
                    .with_related(
                        Some(first),
                        CfdPath::root().field(id_field_name.to_string()),
                        "first id is here",
                    ),
                );
            }
        }
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<String, CfdValueDraft>,
        record: Option<CfdRecordId>,
        path: &CfdPath,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        for (name, value) in fields {
            let value_path = path.clone().field(name.clone());
            let Some(value) =
                self.resolve_value(value, record, value_path, tables, inheritance_index)
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
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<CfdValue> {
        match value {
            CfdValueDraft::Value(value) => Some(value.clone()),
            CfdValueDraft::PendingRef { target_type, id } => {
                let target = self.resolve_ref_target(
                    target_type,
                    id,
                    tables,
                    inheritance_index,
                    record,
                    path,
                )?;
                Some(CfdValue::Ref {
                    id: id.clone(),
                    target,
                })
            }
            CfdValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(
                    &record_draft.fields,
                    record,
                    &path,
                    tables,
                    inheritance_index,
                )?;
                Some(CfdValue::Object(Box::new(CfdRecord {
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
                        tables,
                        inheritance_index,
                    ) else {
                        continue;
                    };
                    out.push(value);
                }
                Some(CfdValue::Array(out))
            }
            CfdValueDraft::Dict(entries) => {
                let mut out = BTreeMap::new();
                for (key, value) in entries {
                    let Some(value) = self.resolve_value(
                        value,
                        record,
                        path.clone().dict_key_value(key),
                        tables,
                        inheritance_index,
                    ) else {
                        continue;
                    };
                    out.insert(key.clone(), value);
                }
                Some(CfdValue::Dict(out))
            }
        }
    }

    fn resolve_ref_target(
        &mut self,
        target_type: &str,
        id: &CfdIdValue,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdRecordId> {
        if !self.schema.range_has_id(target_type) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetHasNoId,
                    format!("ref target `{target_type}` has no @id field"),
                )
                .with_primary(record, path),
            );
            return None;
        }

        let target = if self.schema.range_is_polymorphic(target_type) {
            inheritance_index
                .get(target_type)
                .and_then(|index| index.records.get(id))
                .copied()
        } else {
            tables
                .get(target_type)
                .and_then(|table| table.primary_index.get(id))
                .copied()
        };

        if target.is_none() {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{target_type}` with id was not found"),
                )
                .with_primary(record, path),
            );
        }
        target
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
