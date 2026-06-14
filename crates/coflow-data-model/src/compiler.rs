use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdPath};
use crate::model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdInputDictKey, CfdInputRecord, CfdInputRefIndex,
    CfdInputValue, CfdPolymorphicIndex, CfdRecord, CfdRecordId, CfdRefPathSegment, CfdTable,
    CfdValue,
};
use crate::schema_view::{
    input_value_kind, type_accepts_default, CfdType, CfdValueDraft, FieldMeta, RecordDraft,
    SchemaView,
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
        // Phase 1: validate input records into drafts.
        let mut drafts = Vec::new();
        let input = std::mem::take(&mut self.input);
        {
            let mut v = Validator::new(&self.schema, &mut self.diagnostics);
            for (input_index, record) in input.into_iter().enumerate() {
                let id = CfdRecordId::new(input_index);
                if let Some(draft) = v.validate_record(
                    None,
                    &record.key,
                    &record.actual_type,
                    &record.fields,
                    Some(id),
                    CfdPath::root(),
                ) {
                    drafts.push(draft);
                }
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        // Phase 2: build primary / secondary / polymorphic indexes.
        let (tables, inheritance_index) = self.build_indexes(&drafts);
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
                    &tables,
                    &inheritance_index,
                ) else {
                    continue;
                };
                records.push(CfdRecord {
                    key: draft.key.clone(),
                    actual_type: draft.actual_type.clone(),
                    fields,
                });
            }
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
            self.add_polymorphic_ids(
                &mut inheritance_index,
                &draft.actual_type,
                &draft.key,
                record_id,
            );
        }

        (tables, inheritance_index)
    }

    fn add_polymorphic_ids(
        &mut self,
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
            if let Some(first) = index.records.insert(key.to_string(), record_id) {
                self.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::DuplicatePolymorphicId,
                        format!("duplicate key in polymorphic range `{target_type}`"),
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

    fn push(&mut self, diagnostic: CfdDiagnostic) {
        self.diagnostics.push(diagnostic);
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

    fn validate_record(
        &mut self,
        expected_type: Option<&str>,
        key: &str,
        actual_type: &str,
        input_fields: &BTreeMap<String, CfdInputValue>,
        record: Option<CfdRecordId>,
        path: CfdPath,
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
        for field in fields {
            let field_path = path.clone().field(field.name.clone());
            let value = if let Some(value) = input_fields.get(&field.name) {
                self.validate_field_value(field, value, record, field_path)
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
        self.validate_value(&field.ty, value, record, path)
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
            (CfdType::Type(expected), CfdInputValue::RecordRef(key)) => {
                Some(CfdValueDraft::PendingRef {
                    target_type: expected.clone(),
                    key: key.clone(),
                })
            }
            (expected, CfdInputValue::PathRef { root, segments }) => Some(CfdValueDraft::PathRef {
                expected_type: expected.clone(),
                root: root.clone(),
                segments: segments.clone(),
            }),
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
                let draft =
                    self.validate_record(Some(expected), "", &actual, fields, record, path)?;
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
        let draft = self.validate_record(Some(type_name), "", type_name, &fields, record, path)?;
        Some(CfdValueDraft::Object(Box::new(draft)))
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<String, CfdValueDraft>,
        record: Option<CfdRecordId>,
        path: &CfdPath,
        drafts: &[RecordDraft],
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        for (name, value) in fields {
            let value_path = path.clone().field(name.clone());
            let Some(value) =
                self.resolve_value(value, record, value_path, drafts, tables, inheritance_index)
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
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<CfdValue> {
        match value {
            CfdValueDraft::Value(value) => Some(value.clone()),
            CfdValueDraft::PendingRef { target_type, key } => {
                let target = self.resolve_ref_target(
                    target_type,
                    key,
                    tables,
                    inheritance_index,
                    record,
                    path,
                )?;
                Some(CfdValue::Ref {
                    key: key.clone(),
                    target,
                })
            }
            CfdValueDraft::PathRef {
                expected_type,
                root,
                segments,
            } => self.resolve_path_ref(
                expected_type,
                root,
                segments,
                record,
                path,
                drafts,
                tables,
                inheritance_index,
            ),
            CfdValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(
                    &record_draft.fields,
                    record,
                    &path,
                    drafts,
                    tables,
                    inheritance_index,
                )?;
                Some(CfdValue::Object(Box::new(CfdRecord {
                    key: record_draft.key.clone(),
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
                let mut out = Vec::with_capacity(entries.len());
                for (key, value) in entries {
                    let Some(value) = self.resolve_value(
                        value,
                        record,
                        path.clone().dict_key_value(key),
                        drafts,
                        tables,
                        inheritance_index,
                    ) else {
                        continue;
                    };
                    out.push((key.clone(), value));
                }
                Some(CfdValue::Dict(out))
            }
        }
    }

    fn resolve_ref_target(
        &mut self,
        target_type: &str,
        key: &str,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdRecordId> {
        let target = if self.schema.range_is_polymorphic(target_type) {
            inheritance_index
                .get(target_type)
                .and_then(|index| index.records.get(key))
                .copied()
        } else {
            tables
                .get(target_type)
                .and_then(|table| table.primary_index.get(key))
                .copied()
        };

        if target.is_none() {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{target_type}` with key `{key}` was not found"),
                )
                .with_primary(record, path),
            );
        }
        target
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_path_ref(
        &mut self,
        expected_type: &CfdType,
        root: &str,
        segments: &[CfdRefPathSegment],
        record: Option<CfdRecordId>,
        path: CfdPath,
        drafts: &[RecordDraft],
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<CfdValue> {
        let root_id = self.lookup_any_key(root, tables, inheritance_index)?;
        let root_draft = drafts.get(root_id.index())?;
        let mut current_ty = CfdType::Type(root_draft.actual_type.clone());
        let mut current_value = CfdValueDraft::Object(Box::new(root_draft.clone()));
        let mut current_path = path.clone();

        for segment in segments {
            match segment {
                CfdRefPathSegment::Field(name) => {
                    current_path = current_path.field(name.clone());
                    let CfdType::Type(type_name) = non_nullable_type(&current_ty) else {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::TypeMismatch,
                                "path field access requires an object",
                            )
                            .with_primary(record, current_path),
                        );
                        return None;
                    };
                    let field = self
                        .schema
                        .full_fields(type_name)
                        .iter()
                        .find(|field| field.name == *name)?;
                    let CfdValueDraft::Object(record_draft) = &current_value else {
                        return None;
                    };
                    let Some(next) = record_draft.fields.get(name).cloned() else {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::UnknownField,
                                format!("path field `{name}` was not found"),
                            )
                            .with_primary(record, current_path),
                        );
                        return None;
                    };
                    current_ty = field.ty.clone();
                    current_value = next;
                }
                CfdRefPathSegment::Index(index) => match non_nullable_type(&current_ty) {
                    CfdType::Array(inner) => {
                        current_path = match index {
                            CfdInputRefIndex::Int(raw_index) => match usize::try_from(*raw_index) {
                                Ok(i) => current_path.index(i),
                                Err(_) => current_path.index(usize::MAX),
                            },
                            _ => current_path.dict_key(format_ref_index(index)),
                        };
                        let CfdInputRefIndex::Int(raw_index) = index else {
                            self.push(
                                CfdDiagnostic::error(
                                    CfdErrorCode::TypeMismatch,
                                    "array path index must be int",
                                )
                                .with_primary(record, current_path),
                            );
                            return None;
                        };
                        let Ok(item_index) = usize::try_from(*raw_index) else {
                            self.push(
                                CfdDiagnostic::error(
                                    CfdErrorCode::CheckIndexOutOfBounds,
                                    "array path index is out of bounds",
                                )
                                .with_primary(record, current_path),
                            );
                            return None;
                        };
                        let CfdValueDraft::Array(items) = &current_value else {
                            return None;
                        };
                        let Some(next) = items.get(item_index).cloned() else {
                            self.push(
                                CfdDiagnostic::error(
                                    CfdErrorCode::CheckIndexOutOfBounds,
                                    "array path index is out of bounds",
                                )
                                .with_primary(record, current_path),
                            );
                            return None;
                        };
                        current_ty = *inner.clone();
                        current_value = next;
                    }
                    CfdType::Dict(key_ty, value_ty) => {
                        current_path = current_path.dict_key(format_ref_index(index));
                        let Some(key) =
                            self.ref_index_to_dict_key(key_ty, index, record, current_path.clone())
                        else {
                            return None;
                        };
                        let CfdValueDraft::Dict(entries) = &current_value else {
                            return None;
                        };
                        let Some((_, next)) =
                            entries.iter().find(|(entry_key, _)| entry_key == &key)
                        else {
                            self.push(
                                CfdDiagnostic::error(
                                    CfdErrorCode::CheckMissingDictKey,
                                    "dict path key was not found",
                                )
                                .with_primary(record, current_path),
                            );
                            return None;
                        };
                        current_ty = *value_ty.clone();
                        current_value = next.clone();
                    }
                    _ => {
                        self.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::TypeMismatch,
                                "path index access requires an array or dict",
                            )
                            .with_primary(record, current_path),
                        );
                        return None;
                    }
                },
            }
        }

        if !types_compatible(expected_type, &current_ty, self.schema) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    "path ref result type does not match field type",
                )
                .with_primary(record, path),
            );
            return None;
        }

        self.resolve_value(
            &current_value,
            record,
            path,
            drafts,
            tables,
            inheritance_index,
        )
    }

    fn lookup_any_key(
        &self,
        key: &str,
        tables: &BTreeMap<String, CfdTable>,
        inheritance_index: &BTreeMap<String, CfdPolymorphicIndex>,
    ) -> Option<CfdRecordId> {
        tables
            .values()
            .find_map(|table| table.primary_index.get(key).copied())
            .or_else(|| {
                inheritance_index
                    .values()
                    .find_map(|index| index.records.get(key).copied())
            })
    }

    fn ref_index_to_dict_key(
        &mut self,
        ty: &CfdType,
        index: &CfdInputRefIndex,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdDictKey> {
        match (ty, index) {
            (
                CfdType::String,
                CfdInputRefIndex::String(value) | CfdInputRefIndex::Variant(value),
            ) => Some(CfdDictKey::String(value.clone())),
            (CfdType::Int, CfdInputRefIndex::Int(value)) => Some(CfdDictKey::Int(*value)),
            (CfdType::Enum(enum_name), CfdInputRefIndex::Variant(variant)) => {
                let value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdDictKey::Enum(value))
            }
            (CfdType::Enum(expected), CfdInputRefIndex::EnumVariant { enum_name, variant })
                if enum_name == expected =>
            {
                let value = self.resolve_enum_value(enum_name, variant, record, path)?;
                Some(CfdDictKey::Enum(value))
            }
            _ => {
                self.push(
                    CfdDiagnostic::error(CfdErrorCode::TypeMismatch, "dict path key type mismatch")
                        .with_primary(record, path),
                );
                None
            }
        }
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

fn types_compatible(expected: &CfdType, actual: &CfdType, schema: &SchemaView) -> bool {
    match (expected, actual) {
        (CfdType::Nullable(inner), other) | (other, CfdType::Nullable(inner)) => {
            types_compatible(inner, other, schema)
        }
        (CfdType::Type(expected), CfdType::Type(actual)) => schema.is_assignable(actual, expected),
        (CfdType::Array(left), CfdType::Array(right)) => types_compatible(left, right, schema),
        (CfdType::Dict(left_key, left_value), CfdType::Dict(right_key, right_value)) => {
            types_compatible(left_key, right_key, schema)
                && types_compatible(left_value, right_value, schema)
        }
        _ => expected == actual,
    }
}

fn format_ref_index(index: &CfdInputRefIndex) -> String {
    match index {
        CfdInputRefIndex::Int(value) => value.to_string(),
        CfdInputRefIndex::String(value) | CfdInputRefIndex::Variant(value) => value.clone(),
        CfdInputRefIndex::EnumVariant { enum_name, variant } => {
            format!("{enum_name}.{variant}")
        }
    }
}
