mod dicts;

use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::RecordOrigin;
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::ingest::LoadedValueDraft;
use crate::model::{CfdEnumValue, CfdRecordHandle, CfdValue};
use crate::semantics::{CfdValueSemanticContext, ValueValidationMode, ValueValidationRequest};
use coflow_language::cft::{CftField, CftValueType, TypeName};
use coflow_language::limits::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

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
        id: &str,
        actual_type: &str,
        input_fields: &BTreeMap<String, LoadedValueDraft>,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
    ) -> Option<RecordDraft> {
        self.budget = StructuralBudget::new(self.structural_limits);
        self.budget_exhausted = false;
        self.default_objects.clear();
        let cursor = self.enter_value(TraversalCursor::root(), record, &path)?;
        self.validate_record(
            expected_type,
            id,
            actual_type,
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
        id: &str,
        actual_type: &str,
        input_fields: &BTreeMap<String, LoadedValueDraft>,
        record: Option<CfdRecordHandle>,
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
        if expected_type.is_none() && actual_type_meta.is_data() {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    "data cannot be a top-level record",
                )
                .with_primary(record, path),
            );
            return None;
        }
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

        // 每条记录复用 schema 的字段索引；字段顺序仍由声明顺序决定。
        for name in input_fields.keys() {
            if actual_type_meta.field(name).is_none() {
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
        for field in actual_type_meta.all_fields() {
            let field_path = path.clone().field(field.name.as_str());
            let value = if let Some(value) = input_fields.get(field.name.as_str()) {
                self.validate_field_value(field, value, record, field_path, cursor)
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

        let diagnostics = &self.diagnostics[diagnostic_start..];
        if diagnostics.iter().all(|diagnostic| {
            matches!(
                diagnostic.code,
                CfdErrorCode::MissingRequiredField | CfdErrorCode::InvalidEnumVariant
            )
        }) {
            Some(RecordDraft {
                id: id.to_string(),
                actual_type: actual_type_meta.name.clone(),
                fields: out,
                origin: RecordOrigin::None,
            })
        } else {
            None
        }
    }

    fn validate_field_value(
        &mut self,
        field: &CftField,
        value: &LoadedValueDraft,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        self.validate_value(&field.value_type, value, record, path, cursor)
    }

    pub(super) fn validate_value(
        &mut self,
        ty: &CftValueType,
        value: &LoadedValueDraft,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<ValueDraft> {
        let cursor = self.enter_value(parent, record, &path)?;
        self.validate_value_inner(ty, value, record, path, cursor)
    }

    #[allow(clippy::too_many_lines)]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "语言的 float 明确按 IEEE 754 binary32 舍入，转换后由范围校验处理非有限值。"
    )]
    fn validate_value_inner(
        &mut self,
        ty: &CftValueType,
        value: &LoadedValueDraft,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        match (ty, value) {
            (CftValueType::Option(_), LoadedValueDraft::OptionNone) => {
                Some(ValueDraft::Value(CfdValue::OptionNone))
            }
            (CftValueType::Option(inner), LoadedValueDraft::OptionSome(value)) => self
                .validate_value_inner(inner, value, record, path, cursor)
                .map(|value| ValueDraft::OptionSome(Box::new(value))),
            (CftValueType::Option(inner), value) => self
                .validate_value_inner(inner, value, record, path, cursor)
                .map(|value| ValueDraft::OptionSome(Box::new(value))),
            (CftValueType::Int | CftValueType::Long, LoadedValueDraft::Int(value)) => {
                let value = CfdValue::Int(*value);
                self.validate_materialized_value(ty, &value, record, path)?;
                Some(ValueDraft::Value(value))
            }
            (CftValueType::Float | CftValueType::Double, LoadedValueDraft::Float(value)) => {
                // 模型只保存目标精度下的值，读取、编辑和插值共享相同结果。
                let value = CfdValue::Float(if *ty == CftValueType::Float {
                    f64::from(*value as f32)
                } else {
                    *value
                });
                self.validate_materialized_value(ty, &value, record, path)?;
                Some(ValueDraft::Value(value))
            }
            (CftValueType::Bool, LoadedValueDraft::Bool(value)) => {
                Some(ValueDraft::Value(CfdValue::Bool(*value)))
            }
            (CftValueType::String, LoadedValueDraft::String(value)) => {
                Some(ValueDraft::Value(CfdValue::String(value.clone())))
            }
            (CftValueType::String, LoadedValueDraft::FormattedString(value)) => {
                Some(ValueDraft::FormattedString(value.clone()))
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
            (CftValueType::Enum(expected), LoadedValueDraft::EnumValue { enum_name, value }) => {
                let enum_value =
                    self.resolve_enum_int_value(enum_name, *value, record, path.clone())?;
                let value = CfdValue::Enum(enum_value);
                self.validate_materialized_value(
                    &CftValueType::Enum(expected.clone()),
                    &value,
                    record,
                    path,
                )?;
                Some(ValueDraft::Value(value))
            }
            (CftValueType::RecordRef(_), LoadedValueDraft::RecordRef(id)) => {
                Some(ValueDraft::PendingRef {
                    explicit_type: None,
                    id: id.clone(),
                })
            }
            (
                CftValueType::RecordRef(expected),
                LoadedValueDraft::TypedRecordRef { type_name, id },
            ) => {
                if !self
                    .schema
                    .cft()
                    .is_assignable(type_name, expected.as_str())
                {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            "explicit reference type must be assignable to the declared type",
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
                Some(ValueDraft::PendingRef {
                    explicit_type: Some(TypeName::new(type_name).ok()?),
                    id: id.clone(),
                })
            }
            (
                CftValueType::Object(expected),
                LoadedValueDraft::Object {
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
                    expected.to_string()
                };
                let draft = self.validate_record(
                    Some(expected),
                    "",
                    &actual,
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
            _ => {
                self.type_mismatch(&display_value_type(ty), value, record, path);
                None
            }
        }
    }

    pub(super) fn resolve_enum_value(
        &mut self,
        enum_name: &str,
        variant: &str,
        record: Option<CfdRecordHandle>,
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

    pub(super) fn resolve_enum_key_value(
        &mut self,
        enum_name: &str,
        variant: &str,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
    ) -> Option<CfdEnumValue> {
        let Some(value) = self.schema.enum_key_value(enum_name, variant) else {
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

    fn resolve_enum_int_value(
        &mut self,
        enum_name: &str,
        value: i64,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
    ) -> Option<CfdEnumValue> {
        let Some(value) = self.schema.enum_value_from_int(enum_name, value) else {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::InvalidEnumVariant,
                    format!("unknown enum `{enum_name}`"),
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
        record: Option<CfdRecordHandle>,
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
        record: Option<CfdRecordHandle>,
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
        record: Option<CfdRecordHandle>,
        path: &CfdPath,
    ) -> Option<TraversalCursor> {
        if self.budget_exhausted {
            return None;
        }
        let result = self.budget.enter(parent, StructureKind::DataValue, 1);
        match result {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                self.push_budget_error(error.to_string(), record, path.clone());
                None
            }
        }
    }

    pub(super) fn charge_cached_subtree(
        &mut self,
        cursor: TraversalCursor,
        record: Option<CfdRecordHandle>,
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
            });
        match result {
            Ok(()) => Some(()),
            Err(error) => {
                self.push_budget_error(error.to_string(), record, path.clone());
                None
            }
        }
    }

    fn push_budget_error(
        &mut self,
        message: String,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
    ) {
        self.budget_exhausted = true;
        self.push(
            CfdDiagnostic::error(CfdErrorCode::DataStructureLimitExceeded, message)
                .with_primary(record, path),
        );
    }
}

struct SourceValueSemanticContext;

impl CfdValueSemanticContext for SourceValueSemanticContext {
    fn record_by_domain_id(
        &self,
        _inheritance_root: &TypeName,
        _key: &str,
    ) -> Option<CfdRecordHandle> {
        None
    }

    fn record_actual_type(&self, _id: CfdRecordHandle) -> Option<&str> {
        None
    }
}

fn display_value_type(ty: &CftValueType) -> String {
    ty.display_label()
}

fn input_value_kind(value: &LoadedValueDraft) -> &'static str {
    match value {
        LoadedValueDraft::OptionNone => "None",
        LoadedValueDraft::OptionSome(_) => "Some",
        LoadedValueDraft::Bool(_) => "bool",
        LoadedValueDraft::Int(_) => "int",
        LoadedValueDraft::Float(_) => "float",
        LoadedValueDraft::String(_) => "string",
        LoadedValueDraft::FormattedString(_) => "formatted string",

        LoadedValueDraft::EnumVariant { .. } | LoadedValueDraft::EnumValue { .. } => "enum",
        LoadedValueDraft::Object { .. } => "object",
        LoadedValueDraft::RecordRef(_) | LoadedValueDraft::TypedRecordRef { .. } => "record ref",
        LoadedValueDraft::Array(_) => "array",
        LoadedValueDraft::Dict(_) => "dict",
    }
}
