mod defaults;
mod indexes;
mod resolve;

use crate::diagnostic::{CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdPath};
use crate::edge_index::{build_ref_indexes, build_spread_indexes};
use crate::model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdInputDictKey, CfdInputRecord, CfdInputValue,
    CfdObject, CfdRecord, CfdRecordId, CfdValue,
};
use crate::origin::RecordOrigin;
use crate::schema_view::{
    input_value_kind, CfdType, CfdValueDraft, FieldMeta, RecordDraft, SchemaView,
    SpreadFieldSource,
};
use coflow_cft::CftContainer;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct ModelCompiler {
    schema: SchemaView,
    input: Vec<CfdInputRecord>,
    diagnostics: Vec<CfdDiagnostic>,
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
        let indexes = indexes::build_indexes(&self.schema, &drafts, &mut self.diagnostics);

        // Phase 2b: singleton validation. We run this even when phase 2 has
        // already collected diagnostics so that singleton-specific codes
        // (SingletonRecordCountInvalid / SingletonKeyMissingOrInvalid /
        // SingletonKeyCollision) are surfaced alongside generic ones; this
        // gives users a complete picture in a single build pass.
        // Localized record-key identifier requirements are already covered by
        // the generic `InvalidRecordKey` path because `record_key_ident_error`
        // and `is_cft_identifier` currently use the same rule set; the spec
        // leaves `LocalizedRecordKeyInvalid` reserved for future divergence.
        indexes::validate_singletons(
            &self.schema,
            &drafts,
            &indexes.tables,
            &mut self.diagnostics,
        );
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
                records.push(CfdRecord {
                    key: draft.key.clone(),
                    object: CfdObject {
                        actual_type: draft.actual_type.clone(),
                        fields,
                    },
                    origin: draft.origin.clone(),
                });
            }
        }

        if !self.diagnostics.is_empty() {
            return Err(CfdDiagnostics::new(self.diagnostics));
        }

        let spread_indexes =
            build_spread_indexes(&drafts, &indexes.record_by_domain_key, &self.schema);
        let ref_indexes = build_ref_indexes(
            &records,
            &indexes.record_by_domain_key,
            &self.schema,
            &spread_indexes.edges,
        );

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
            spread_edges: spread_indexes.edges,
            spread_by_site: spread_indexes.by_site,
            spread_by_source: spread_indexes.by_source,
        })
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
        let mut spread_sources = Vec::new();
        let mut spread_field_sources = BTreeMap::new();
        for spread in input_spreads {
            let spread_origin = top_level_spread_source(actual_type, spread);
            let Some(spread_fields) =
                self.validate_object_spread(actual_type, spread, record, path.clone())
            else {
                continue;
            };
            if let Some(origin) = &spread_origin {
                spread_sources.push(origin.clone());
            }
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
                spread_sources,
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

