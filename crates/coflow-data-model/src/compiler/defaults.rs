use super::validate::CachedDefaultObject;
use super::Validator;
use crate::compiler_context::{type_accepts_default, CfdValueDraft, RecordDraft};
use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdEnumValue, CfdRecordId, CfdValue};
use coflow_cft::{CftField, CftSchemaDefaultValue, CftValueType};
use coflow_structure::TraversalCursor;
use std::collections::BTreeMap;

impl Validator<'_, '_> {
    pub(super) fn default_field_value(
        &mut self,
        field: &CftField,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            if let CftValueType::Object(type_name) = non_nullable_type(&field.value_type) {
                if let Some(cycle) = self.schema.schema_default_cycle(type_name) {
                    self.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::ValueDependencyCycle,
                            format!("schema default dependency cycle: {cycle}"),
                        )
                        .with_primary(record, path),
                    );
                    return None;
                }
            }
        }
        let cursor = self.enter_value(parent, record, &path)?;
        self.default_value(&field.value_type, value, record, path, cursor)
    }

    fn default_value(
        &mut self,
        ty: &CftValueType,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordId>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<CfdValueDraft> {
        if matches!(value, CftSchemaDefaultValue::EmptyObject) {
            return match non_nullable_type(ty) {
                CftValueType::Dict(_, _) => {
                    Some(CfdValueDraft::Value(CfdValue::Dict(Vec::new())))
                }
                CftValueType::Object(type_name) => {
                    self.default_object_value(type_name, record, path, cursor)
                }
                _ => {
                    self.push_default_type_mismatch(record, path);
                    None
                }
            };
        }

        let out = match value {
            CftSchemaDefaultValue::Null if ty.is_nullable() => CfdValue::Null,
            CftSchemaDefaultValue::Int(value)
                if type_accepts_default(ty, &CftValueType::Int) =>
            {
                CfdValue::Int(*value)
            }
            CftSchemaDefaultValue::Float(value)
                if type_accepts_default(ty, &CftValueType::Float) =>
            {
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
            CftSchemaDefaultValue::Bool(value)
                if type_accepts_default(ty, &CftValueType::Bool) =>
            {
                CfdValue::Bool(*value)
            }
            CftSchemaDefaultValue::String(value)
                if type_accepts_default(ty, &CftValueType::String) =>
            {
                CfdValue::String(value.clone())
            }
            CftSchemaDefaultValue::Enum {
                enum_name,
                variant,
                value,
            } if matches!(non_nullable_type(ty), CftValueType::Enum(name) if name == enum_name) => {
                CfdValue::Enum(CfdEnumValue {
                    enum_name: enum_name.to_string(),
                    variant: Some(variant.to_string()),
                    value: *value,
                })
            }
            CftSchemaDefaultValue::EmptyArray
                if matches!(non_nullable_type(ty), CftValueType::Array(_)) =>
            {
                CfdValue::Array(Vec::new())
            }
            _ => {
                self.push_default_type_mismatch(record, path);
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
        cursor: TraversalCursor,
    ) -> Option<CfdValueDraft> {
        if let Some((nodes, depth)) = self
            .default_objects
            .get(type_name)
            .map(|cached| (cached.nodes, cached.depth))
        {
            self.charge_cached_subtree(cursor, record, &path, nodes, depth)?;
            return self
                .default_objects
                .get(type_name)
                .map(|cached| CfdValueDraft::Object(Box::new(cached.draft.clone())));
        }
        if let Some(cycle) = self.schema.schema_default_cycle(type_name) {
            self.push(
                CfdDiagnostic::error(
                    CfdErrorCode::ValueDependencyCycle,
                    format!("schema default dependency cycle: {cycle}"),
                )
                .with_primary(record, path),
            );
            return None;
        }
        let fields = BTreeMap::new();
        let draft = self.validate_record(
            Some(type_name),
            "",
            type_name,
            &[],
            &fields,
            record,
            path,
            cursor,
        )?;
        let (nodes, depth) = draft_shape(&draft);
        self.default_objects.insert(
            type_name.to_string(),
            CachedDefaultObject {
                draft: draft.clone(),
                nodes,
                depth,
            },
        );
        Some(CfdValueDraft::Object(Box::new(draft)))
    }

    fn push_default_type_mismatch(&mut self, record: Option<CfdRecordId>, path: CfdPath) {
        self.push(
            CfdDiagnostic::error(
                CfdErrorCode::TypeMismatch,
                "schema default does not match field type",
            )
            .with_primary(record, path),
        );
    }
}

fn draft_shape(root: &RecordDraft) -> (u64, u64) {
    enum DraftNode<'a> {
        Record(&'a RecordDraft),
        Value(&'a CfdValueDraft),
    }

    let mut nodes = 0_u64;
    let mut depth = 0_u64;
    let mut pending = vec![(DraftNode::Record(root), 1_u64)];
    while let Some((node, node_depth)) = pending.pop() {
        nodes = nodes.saturating_add(1);
        depth = depth.max(node_depth);
        let child_depth = node_depth.saturating_add(1);
        match node {
            DraftNode::Record(record) => {
                pending.extend(
                    record
                        .fields
                        .values()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(CfdValueDraft::Object(record)) => {
                pending.extend(
                    record
                        .fields
                        .values()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(CfdValueDraft::Array(items)) => {
                pending.extend(
                    items
                        .iter()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(CfdValueDraft::Dict(entries)) => {
                pending.extend(
                    entries
                        .iter()
                        .map(|(_, value)| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(CfdValueDraft::DictSpread { spreads, entries }) => {
                pending.extend(
                    spreads
                        .iter()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
                pending.extend(
                    entries
                        .iter()
                        .map(|(_, value)| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(
                CfdValueDraft::Value(_)
                | CfdValueDraft::PendingRef { .. }
                | CfdValueDraft::PendingSpreadField { .. },
            ) => {}
        }
    }
    (nodes, depth)
}

fn non_nullable_type(ty: &CftValueType) -> &CftValueType {
    match ty {
        CftValueType::Nullable(inner) => non_nullable_type(inner),
        _ => ty,
    }
}
