mod dicts;

use crate::build::{BuildSchema, RecordDraft, SpreadFieldSource, ValueDraft};
use crate::diagnostics::RecordOrigin;
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::ingest::LoadedValueDraft;
use crate::model::{CfdEnumValue, CfdRecordId, CfdValue};
use crate::semantics::{CfdValueSemanticContext, ValueValidationMode, ValueValidationRequest};
use coflow_cft::{CftField, CftValueType, FieldName, TypeName};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet};

/// Validation and resolution helper.
///
/// Separating `schema` (a copied `&'s BuildSchema` reference) from
/// `diagnostics` (a mutable borrow) lets every method call
/// `schema.full_fields(type)` and obtain field references whose lifetime is
/// tied to the outer `BuildSchema`, **not** to `self`. The references
/// can therefore be iterated while `&mut self` methods are called to emit
/// diagnostics — something impossible when the schema is an owned field of
/// the same struct.
pub(super) struct Validator<'s, 'schema> {
    pub(super) schema: &'s BuildSchema<'schema>,
    pub(super) diagnostics: &'s mut Vec<CfdDiagnostic>,
    pub(super) default_objects: BTreeMap<String, CachedDefaultObject>,
    structural_limits: StructuralLimits,
    budget: StructuralBudget,
    budget_exhausted: bool,
}

#[derive(Debug, Clone)]
pub(super) struct CachedDefaultObject {
    pub(super) draft: RecordDraft,
    pub(super) nodes: u64,
    pub(super) depth: u64,
}

impl<'s, 'schema> Validator<'s, 'schema> {
    pub(super) fn new(
        schema: &'s BuildSchema<'schema>,
        diagnostics: &'s mut Vec<CfdDiagnostic>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema,
            diagnostics,
            default_objects: BTreeMap::new(),
            structural_limits,
            budget: StructuralBudget::new(structural_limits),
            budget_exhausted: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn validate_top_level_record(
        &mut self,
        expected_type: Option<&str>,
        key: &str,
        actual_type: &str,
        input_spreads: &[LoadedValueDraft],
        input_fields: &BTreeMap<String, LoadedValueDraft>,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<RecordDraft> {
        self.budget = StructuralBudget::new(self.structural_limits);
        self.budget_exhausted = false;
        self.default_objects.clear();
        let cursor = self.enter_value(TraversalCursor::root(), record, &path)?;
        self.validate_record(
            expected_type,
            key,
            actual_type,
            input_spreads,
            input_fields,
            record,
            path,
            cursor,
        )
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub(super) fn validate_record(
        &mut self,
        expected_type: Option<&str>,
        key: &str,
        actual_type: &str,
        input_spreads: &[LoadedValueDraft],
        input_fields: &BTreeMap<String, LoadedValueDraft>,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<RecordDraft> {
        // Copy the shared schema reference so that the shared field slice
        // obtained below has a lifetime independent of `self`, allowing
        // &mut self methods to be called while iterating over the fields.
        let schema = self.schema;
        let diagnostic_start = self.diagnostics.len();

        let Some(actual_type_meta) = schema.resolve_type(actual_type) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::UnknownType,
                    format!("unknown type `{actual_type}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        if expected_type.is_none() && actual_type_meta.is_abstract {
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
            if let Err(error) = crate::semantics::validate_object_type_assignable(
                schema.cft(),
                expected,
                actual_type,
            ) {
                self.push(
                    CfdDiagnostic::error(super::semantic_error_code(error.kind()), error.message())
                        .with_primary(record, path),
                );
                return None;
            }
        }

        // `fields` has lifetime 's, independent of `self`, so it can be held
        // across calls to &mut self methods below.
        let fields = schema.full_fields(actual_type).collect::<Vec<_>>();
        let work = input_fields
            .len()
            .saturating_add(input_spreads.len())
            .saturating_add(fields.len());
        self.charge_work(work, record, &path)?;
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
            let spread_origin = top_level_spread_source(&actual_type_meta.name, spread);
            let Some(spread_fields) = self.validate_object_spread(
                &actual_type_meta.name,
                spread,
                record,
                path.clone(),
                cursor,
            ) else {
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
        for field in fields {
            let field_path = path.clone().field(field.name.as_str());
            let value = if let Some(value) = input_fields.get(field.name.as_str()) {
                // An explicit field overrides any spread-imported value.
                spread_field_sources.remove(field.name.as_str());
                self.validate_field_value(field, value, record, field_path, cursor)
            } else if out.contains_key(field.name.as_str()) {
                continue;
            } else if let Some(default) = &field.default {
                self.default_field_value(field, default, record, field_path, cursor)
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
                actual_type: actual_type_meta.name.clone(),
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
        field: &CftField,
        value: &LoadedValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        self.validate_value(&field.value_type, value, record, path, cursor)
    }

    pub(super) fn validate_value(
        &mut self,
        ty: &CftValueType,
        value: &LoadedValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<ValueDraft> {
        let cursor = self.enter_value(parent, record, &path)?;
        self.validate_value_inner(ty, value, record, path, cursor)
    }

    #[allow(clippy::too_many_lines)]
    fn validate_value_inner(
        &mut self,
        ty: &CftValueType,
        value: &LoadedValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        if let CftValueType::Nullable(inner) = ty {
            return if matches!(value, LoadedValueDraft::Null) {
                Some(ValueDraft::Value(CfdValue::Null))
            } else {
                self.validate_value_inner(inner, value, record, path, cursor)
            };
        }

        match (ty, value) {
            (CftValueType::Int, LoadedValueDraft::Int(value)) => {
                Some(ValueDraft::Value(CfdValue::Int(*value)))
            }
            (CftValueType::Float, LoadedValueDraft::Float(value)) => {
                let value = CfdValue::Float(*value);
                self.validate_materialized_value(ty, &value, record, path)?;
                Some(ValueDraft::Value(value))
            }
            (CftValueType::Bool, LoadedValueDraft::Bool(value)) => {
                Some(ValueDraft::Value(CfdValue::Bool(*value)))
            }
            (CftValueType::String, LoadedValueDraft::String(value)) => {
                Some(ValueDraft::Value(CfdValue::String(value.clone())))
            }
            (
                CftValueType::Enum(expected),
                LoadedValueDraft::EnumVariant { enum_name, variant },
            ) => {
                let enum_value =
                    self.resolve_enum_value(enum_name, variant, record, path.clone())?;
                let value = CfdValue::Enum(enum_value);
                self.validate_materialized_value(
                    &CftValueType::Enum(expected.clone()),
                    &value,
                    record,
                    path,
                )?;
                Some(ValueDraft::Value(value))
            }
            (CftValueType::RecordRef(expected), LoadedValueDraft::RecordRef(key)) => {
                Some(ValueDraft::PendingRef {
                    expected_type: expected.clone(),
                    key: key.clone(),
                })
            }
            (
                CftValueType::Object(expected),
                LoadedValueDraft::Object {
                    actual_type,
                    fields,
                }
                | LoadedValueDraft::ObjectSpread {
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
                    expected.to_string()
                };
                let spreads = match value {
                    LoadedValueDraft::ObjectSpread { spreads, .. } => spreads.as_slice(),
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
                    cursor,
                )?;
                Some(ValueDraft::Object(Box::new(draft)))
            }
            (CftValueType::Array(inner), LoadedValueDraft::Array(items)) => {
                let mut out = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(value) =
                        self.validate_value(inner, item, record, path.clone().index(index), cursor)
                    else {
                        continue;
                    };
                    out.push(value);
                }
                Some(ValueDraft::Array(out))
            }
            (CftValueType::Dict(key_ty, value_ty), LoadedValueDraft::Dict(entries)) => {
                let out =
                    self.validate_dict_entries(key_ty, value_ty, entries, record, &path, cursor);
                Some(ValueDraft::Dict(out))
            }
            (
                CftValueType::Dict(key_ty, value_ty),
                LoadedValueDraft::DictSpread { spreads, entries },
            ) => {
                let mut out_spreads = Vec::with_capacity(spreads.len());
                for spread in spreads {
                    let Some(spread) =
                        self.validate_value(ty, spread, record, path.clone(), cursor)
                    else {
                        continue;
                    };
                    out_spreads.push(spread);
                }
                let out_entries =
                    self.validate_dict_entries(key_ty, value_ty, entries, record, &path, cursor);
                Some(ValueDraft::DictSpread {
                    spreads: out_spreads,
                    entries: out_entries,
                })
            }
            _ => {
                self.type_mismatch(&display_value_type(ty), value, record, path);
                None
            }
        }
    }

    fn validate_object_spread(
        &mut self,
        type_name: &TypeName,
        spread: &LoadedValueDraft,
        record: Option<CfdRecordId>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<BTreeMap<FieldName, ValueDraft>> {
        let cursor = self.enter_value(parent, record, &path)?;
        match spread {
            LoadedValueDraft::RecordRef(key) => Some(
                self.schema
                    .full_fields(type_name.as_str())
                    .map(|field| {
                        (
                            field.name.clone(),
                            ValueDraft::PendingSpreadField {
                                source_type: type_name.clone(),
                                key: key.clone(),
                                field: field.name.clone(),
                            },
                        )
                    })
                    .collect(),
            ),
            LoadedValueDraft::Object { .. } | LoadedValueDraft::ObjectSpread { .. } => {
                let object_type = self.schema.resolve_type(type_name.as_str())?.name.clone();
                let draft = self.validate_value_inner(
                    &CftValueType::Object(object_type),
                    spread,
                    record,
                    path,
                    cursor,
                )?;
                let ValueDraft::Object(record_draft) = draft else {
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
        let Some(value) = self.schema.enum_value(enum_name, variant) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::InvalidEnumVariant,
                    format!("unknown enum variant `{enum_name}.{variant}`"),
                )
                .with_primary(record, path),
            );
            return None;
        };
        Some(value.into())
    }

    fn type_mismatch(
        &mut self,
        expected: &str,
        value: &LoadedValueDraft,
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

    pub(super) fn validate_materialized_value(
        &mut self,
        expected: &CftValueType,
        value: &CfdValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
    ) -> Option<()> {
        let context = SourceValueSemanticContext;
        let request =
            ValueValidationRequest::new(expected, value, ValueValidationMode::SourceFragment);
        match crate::semantics::validate_value_for_schema(self.schema.cft(), &context, request) {
            Ok(()) => Some(()),
            Err(error) => {
                self.push(
                    CfdDiagnostic::error(super::semantic_error_code(error.kind()), error.message())
                        .with_primary(record, path),
                );
                None
            }
        }
    }

    pub(super) fn push(&mut self, diagnostic: CfdDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(super) fn enter_value(
        &mut self,
        parent: TraversalCursor,
        record: Option<CfdRecordId>,
        path: &CfdPath,
    ) -> Option<TraversalCursor> {
        if self.budget_exhausted {
            return None;
        }
        let result = self
            .budget
            .enter(parent, StructureKind::DataValue, 1)
            .and_then(|cursor| {
                self.budget
                    .charge_work(StructureKind::DataValue, 1)
                    .map(|()| cursor)
            });
        match result {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                self.push_budget_error(error.to_string(), record, path.clone());
                None
            }
        }
    }

    fn charge_work(
        &mut self,
        work: usize,
        record: Option<CfdRecordId>,
        path: &CfdPath,
    ) -> Option<()> {
        if self.budget_exhausted {
            return None;
        }
        let work = u64::try_from(work).unwrap_or(u64::MAX);
        match self.budget.charge_work(StructureKind::DataValue, work) {
            Ok(()) => Some(()),
            Err(error) => {
                self.push_budget_error(error.to_string(), record, path.clone());
                None
            }
        }
    }

    pub(super) fn charge_cached_subtree(
        &mut self,
        cursor: TraversalCursor,
        record: Option<CfdRecordId>,
        path: &CfdPath,
        nodes: u64,
        depth: u64,
    ) -> Option<()> {
        if self.budget_exhausted {
            return None;
        }
        let additional_nodes = nodes.saturating_sub(1);
        let result = self
            .budget
            .check_additional_depth(cursor, StructureKind::DefaultValue, depth.saturating_sub(1))
            .and_then(|()| {
                self.budget
                    .charge_nodes(StructureKind::DefaultValue, additional_nodes)
            })
            .and_then(|()| {
                self.budget
                    .charge_work(StructureKind::DefaultValue, additional_nodes)
            });
        match result {
            Ok(()) => Some(()),
            Err(error) => {
                self.push_budget_error(error.to_string(), record, path.clone());
                None
            }
        }
    }

    fn push_budget_error(&mut self, message: String, record: Option<CfdRecordId>, path: CfdPath) {
        self.budget_exhausted = true;
        self.push(
            CfdDiagnostic::error(CfdErrorCode::DataStructureLimitExceeded, message)
                .with_primary(record, path),
        );
    }
}

struct SourceValueSemanticContext;

impl CfdValueSemanticContext for SourceValueSemanticContext {
    fn record_by_domain_key(
        &self,
        _inheritance_root: &TypeName,
        _key: &str,
    ) -> Option<CfdRecordId> {
        None
    }

    fn record_actual_type(&self, _id: CfdRecordId) -> Option<&str> {
        None
    }
}

fn top_level_spread_source(
    expected_type: &TypeName,
    spread: &LoadedValueDraft,
) -> Option<SpreadFieldSource> {
    match spread {
        LoadedValueDraft::RecordRef(key) => Some(SpreadFieldSource {
            expected_type: expected_type.clone(),
            key: key.clone(),
        }),
        _ => None,
    }
}

fn display_value_type(ty: &CftValueType) -> String {
    ty.display_label()
}

fn input_value_kind(value: &LoadedValueDraft) -> &'static str {
    match value {
        LoadedValueDraft::Null => "null",
        LoadedValueDraft::Bool(_) => "bool",
        LoadedValueDraft::Int(_) => "int",
        LoadedValueDraft::Float(_) => "float",
        LoadedValueDraft::String(_) => "string",
        LoadedValueDraft::EnumVariant { .. } => "enum",
        LoadedValueDraft::Object { .. } => "object",
        LoadedValueDraft::ObjectSpread { .. } => "object spread",
        LoadedValueDraft::RecordRef(_) => "record ref",
        LoadedValueDraft::Array(_) => "array",
        LoadedValueDraft::Dict(_) => "dict",
        LoadedValueDraft::DictSpread { .. } => "dict spread",
    }
}
