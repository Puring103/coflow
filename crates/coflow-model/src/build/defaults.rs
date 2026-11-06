use super::validate::CachedDefaultObject;
use super::Validator;
use crate::build::{RecordDraft, ValueDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::CfdRecordHandle;
use coflow_language::cft::{CftField, CftSchemaDefaultValue, CftValueType};
use coflow_language::limits::TraversalCursor;
use std::collections::BTreeMap;
use std::sync::Arc;

impl<'schema> Validator<'_, 'schema> {
    pub(super) fn default_field_value(
        &mut self,
        field: &'schema CftField,
        value: &CftSchemaDefaultValue,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
        parent: TraversalCursor,
    ) -> Option<ValueDraft> {
        if let (CftValueType::Object(type_name), CftSchemaDefaultValue::EmptyObject) =
            (&field.value_type, value)
        {
            let cursor = self.enter_value(parent, record, &path)?;
            return self.default_object_value(type_name, record, path, cursor);
        }
        // 复合默认值仅降低一次，缓存不包含缺省展开、引用连接或模板求值结果。
        let cache_key = matches!(
            value,
            CftSchemaDefaultValue::OptionSome(_)
                | CftSchemaDefaultValue::Array(_)
                | CftSchemaDefaultValue::Dictionary(_)
                | CftSchemaDefaultValue::Object { .. }
        )
        .then_some((&field.declaring_type, &field.name));
        if let Some(loaded) = cache_key
            .and_then(|key| self.default_inputs.get(&key))
            .cloned()
        {
            // 命中输入缓存仍逐节点验证，预算边界、诊断路径与当前记录上下文保持一致。
            return self.validate_value(&field.value_type, &loaded, record, path, parent);
        }
        let loaded = match crate::lower_schema_default(self.schema.cft(), &field.value_type, value)
        {
            Ok(loaded) => loaded,
            Err(message) => {
                self.push(
                    CfdDiagnostic::error(CfdErrorCode::TypeMismatch, message)
                        .with_primary(record, path),
                );
                return None;
            }
        };
        if let Some(key) = cache_key {
            let loaded = Arc::new(loaded);
            self.default_inputs.insert(key, Arc::clone(&loaded));
            return self.validate_value(&field.value_type, &loaded, record, path, parent);
        }
        self.validate_value(&field.value_type, &loaded, record, path, parent)
    }

    fn default_object_value(
        &mut self,
        type_name: &str,
        record: Option<CfdRecordHandle>,
        path: CfdPath,
        cursor: TraversalCursor,
    ) -> Option<ValueDraft> {
        if let Some((nodes, depth)) = self
            .default_objects
            .get(type_name)
            .map(|cached| (cached.nodes, cached.depth))
        {
            self.charge_cached_subtree(cursor, record, &path, nodes, depth)?;
            return self
                .default_objects
                .get(type_name)
                .map(|cached| ValueDraft::Object(Box::new(cached.draft.clone())));
        }
        if let Some(cycle) = crate::dependencies::schema_default_cycle(self.schema.cft(), type_name)
        {
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
        Some(ValueDraft::Object(Box::new(draft)))
    }
}

fn draft_shape(root: &RecordDraft) -> (u64, u64) {
    enum DraftNode<'a> {
        Record(&'a RecordDraft),
        Value(&'a ValueDraft),
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
            DraftNode::Value(ValueDraft::Object(record)) => {
                pending.extend(
                    record
                        .fields
                        .values()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(ValueDraft::OptionSome(value)) => {
                pending.push((DraftNode::Value(value), child_depth));
            }
            DraftNode::Value(ValueDraft::Array(items)) => {
                pending.extend(
                    items
                        .iter()
                        .map(|value| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(ValueDraft::Dict(entries)) => {
                pending.extend(
                    entries
                        .iter()
                        .map(|(_, value)| (DraftNode::Value(value), child_depth)),
                );
            }
            DraftNode::Value(
                ValueDraft::Value(_)
                | ValueDraft::FormattedString(_)
                | ValueDraft::PendingRef { .. },
            ) => {}
        }
    }
    (nodes, depth)
}
