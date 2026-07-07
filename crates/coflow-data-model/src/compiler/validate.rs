mod dicts;

use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdEnumValue, CfdInputValue, CfdRecordId, CfdValue};
use crate::origin::RecordOrigin;
use crate::schema_view::{
    display_type_ref, input_value_kind, CfdValueDraft, RecordDraft, SchemaView, SpreadFieldSource,
};
use coflow_cft::{CftFieldMeta, CftSchemaTypeRef};
use std::collections::{BTreeMap, BTreeSet};

/// Validation and resolution helper.
///
/// Separating `schema` (a copied `&'s SchemaView` reference) from
/// `diagnostics` (a mutable borrow) lets every method call
/// `schema.full_fields(type)` and obtain a `&'s [CftFieldMeta]` slice whose
/// lifetime is tied to the outer `SchemaView`, **not** to `self`. The slice
/// can therefore be iterated while `&mut self` methods are called to emit
/// diagnostics — something impossible when the schema is an owned field of
/// the same struct.
pub(super) struct Validator<'s> {
    pub(super) schema: &'s SchemaView,
    pub(super) diagnostics: &'s mut Vec<CfdDiagnostic>,
}

impl<'s> Validator<'s> {
    pub(super) fn new(schema: &'s SchemaView, diagnostics: &'s mut Vec<CfdDiagnostic>) -> Self {
        Self {
            schema,
            diagnostics,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn validate_record(
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
        // Copy the shared schema reference so that the &'s [CftFieldMeta] slice
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
        field: &CftFieldMeta,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        self.validate_value(&field.ty_ref, value, record, path)
    }

    pub(super) fn validate_value(
        &mut self,
        ty: &CftSchemaTypeRef,
        value: &CfdInputValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<CfdValueDraft> {
        if let CftSchemaTypeRef::Nullable(inner) = ty {
            return if matches!(value, CfdInputValue::Null) {
                Some(CfdValueDraft::Value(CfdValue::Null))
            } else {
                self.validate_value(inner, value, record, path)
            };
        }

        match (ty, value) {
            (CftSchemaTypeRef::Int, CfdInputValue::Int(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Int(*value)))
            }
            (CftSchemaTypeRef::Float, CfdInputValue::Float(value)) => {
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
            (CftSchemaTypeRef::Bool, CfdInputValue::Bool(value)) => {
                Some(CfdValueDraft::Value(CfdValue::Bool(*value)))
            }
            (CftSchemaTypeRef::String, CfdInputValue::String(value)) => {
                Some(CfdValueDraft::Value(CfdValue::String(value.clone())))
            }
            (
                CftSchemaTypeRef::Named(expected),
                CfdInputValue::EnumVariant { enum_name, variant },
            ) if self.schema.is_schema_enum(expected) => {
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
            (CftSchemaTypeRef::Ref(expected), CfdInputValue::RecordRef(key)) => {
                Some(CfdValueDraft::PendingRef {
                    expected_type: expected.clone(),
                    key: key.clone(),
                })
            }
            (
                CftSchemaTypeRef::Named(expected),
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
            (CftSchemaTypeRef::Array(inner), CfdInputValue::Array(items)) => {
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
            (CftSchemaTypeRef::Dict(key_ty, value_ty), CfdInputValue::Dict(entries)) => {
                let out = self.validate_dict_entries(key_ty, value_ty, entries, record, &path);
                Some(CfdValueDraft::Dict(out))
            }
            (
                CftSchemaTypeRef::Dict(key_ty, value_ty),
                CfdInputValue::DictSpread { spreads, entries },
            ) => {
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
                self.type_mismatch(&display_type_ref(ty), value, record, path);
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
                    &CftSchemaTypeRef::Named(type_name.to_string()),
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

    pub(super) fn resolve_enum_value(
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

    pub(super) fn push(&mut self, diagnostic: CfdDiagnostic) {
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
