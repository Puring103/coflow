use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath};
use crate::model::{CfdDictKey, CfdFormattedString, CfdObject, CfdRecordId, CfdValue};
use crate::schema::{FieldName, RecordKey, TypeName};
use crate::LoadedFormattedString;
use coflow_language::limits::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ValueNode {
    record: CfdRecordId,
    path: CfdPath,
}

impl ValueNode {
    fn field(&self, name: &FieldName) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().field(name.as_str()),
        }
    }

    fn index(&self, index: usize) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().index(index),
        }
    }

    fn dict_key(&self, key: &CfdDictKey) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key_value(key),
        }
    }
}

pub(super) struct ValueResolver<'a, 'schema> {
    schema: &'a BuildSchema<'schema>,
    drafts: &'a [RecordDraft],
    record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
    diagnostics: &'a mut Vec<CfdDiagnostic>,
    structural_limits: StructuralLimits,
    budget: StructuralBudget,
    budget_exhausted: bool,
}

impl<'a, 'schema> ValueResolver<'a, 'schema> {
    pub(super) fn new(
        schema: &'a BuildSchema<'schema>,
        drafts: &'a [RecordDraft],
        record_by_domain_key: &'a BTreeMap<TypeName, BTreeMap<RecordKey, CfdRecordId>>,
        diagnostics: &'a mut Vec<CfdDiagnostic>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema,
            drafts,
            record_by_domain_key,
            diagnostics,
            structural_limits,
            budget: StructuralBudget::new(structural_limits),
            budget_exhausted: false,
        }
    }

    pub(super) fn resolve_record_fields(
        &mut self,
        record: CfdRecordId,
    ) -> Option<BTreeMap<FieldName, CfdValue>> {
        self.budget = StructuralBudget::new(self.structural_limits);
        self.budget_exhausted = false;
        let drafts = self.drafts;
        let fields = &drafts.get(record.index())?.fields;
        let root = ValueNode {
            record,
            path: CfdPath::root(),
        };
        let cursor = self.enter_node(TraversalCursor::root(), &root, StructureKind::DataValue)?;
        self.resolve_fields(fields, &root, cursor)
    }

    pub(super) fn diagnostic_count(&self) -> usize {
        self.diagnostics.len()
    }

    pub(super) fn resolve_dimension_value(
        &mut self,
        record: CfdRecordId,
        value: &ValueDraft,
        path: &CfdPath,
    ) -> Option<CfdValue> {
        self.budget = StructuralBudget::new(self.structural_limits);
        self.budget_exhausted = false;
        let node = ValueNode {
            record,
            path: path.clone(),
        };
        self.resolve_node(value, &node, TraversalCursor::root())
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<FieldName, ValueDraft>,
        parent: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<BTreeMap<FieldName, CfdValue>> {
        let mut out = BTreeMap::new();
        let mut complete = true;
        for (name, value) in fields {
            let Some(value) = self.resolve_node(value, &parent.field(name), cursor) else {
                complete = false;
                continue;
            };
            out.insert(name.clone(), value);
        }
        complete.then_some(out)
    }

    // 静态构建只遍历存储结构；记录引用保持身份，模板不在构建期求值。
    fn resolve_node(
        &mut self,
        value: &ValueDraft,
        node: &ValueNode,
        parent: TraversalCursor,
    ) -> Option<CfdValue> {
        let cursor = self.enter_node(parent, node, StructureKind::DataValue)?;
        self.resolve_value(value, node, cursor)
    }

    fn resolve_value(
        &mut self,
        value: &ValueDraft,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<CfdValue> {
        match value {
            ValueDraft::Value(value) => Some(value.clone()),
            ValueDraft::OptionSome(value) => self
                .resolve_value(value, node, cursor)
                .map(|value| CfdValue::OptionSome(Box::new(value))),
            ValueDraft::FormattedString(value) => {
                self.resolve_formatted_string(value, node, cursor)
            }
            ValueDraft::PendingRef {
                expected_type,
                required_type,
                key,
            } => {
                // 编辑模型保留悬空引用及诊断；完整 Runtime 仍由构建入口拒绝。
                if let Some((target_id, _)) = self.resolve_ref_target(expected_type, key, node) {
                    let target = self.drafts.get(target_id.index())?;
                    if !self
                        .schema
                        .is_assignable(target.actual_type.as_str(), required_type.as_str())
                    {
                        self.diagnostics.push(
                            CfdDiagnostic::error(
                                CfdErrorCode::RefTargetTypeMismatch,
                                "record reference target does not match declared field type",
                            )
                            .with_primary(Some(node.record), node.path.clone()),
                        );
                    }
                }
                match RecordKey::new(key.clone()) {
                    Ok(key) => Some(CfdValue::Ref(key)),
                    Err(error) => {
                        self.diagnostics.push(
                            CfdDiagnostic::error(CfdErrorCode::TypeMismatch, error.to_string())
                                .with_primary(Some(node.record), node.path.clone()),
                        );
                        None
                    }
                }
            }
            ValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(&record_draft.fields, node, cursor)?;
                Some(CfdValue::Object(Box::new(CfdObject {
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                })))
            }
            ValueDraft::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                let mut complete = true;
                for (index, item) in items.iter().enumerate() {
                    let Some(value) = self.resolve_node(item, &node.index(index), cursor) else {
                        complete = false;
                        continue;
                    };
                    out.push(value);
                }
                complete.then_some(CfdValue::Array(out))
            }
            ValueDraft::Dict(entries) => self
                .resolve_dict_entries(entries, node, cursor)
                .map(CfdValue::Dict),
        }
    }

    fn resolve_formatted_string(
        &mut self,
        value: &LoadedFormattedString,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<CfdValue> {
        // 构建只保存模板，动态读取由未来的 VM 负责。
        let _ = (node, cursor);
        Some(CfdValue::FormattedString(CfdFormattedString {
            from_default: value.from_default,
            location: value.location.clone(),
            imports: value.imports.clone(),
            constant_origin: value.constant_origin.clone(),
            source: value.source.clone(),
        }))
    }

    fn resolve_ref_target(
        &mut self,
        expected_type: &TypeName,
        key: &str,
        node: &ValueNode,
    ) -> Option<(CfdRecordId, RecordKey)> {
        let target = self
            .schema
            .inheritance_root(expected_type.as_str())
            .and_then(|inheritance_root| {
                self.record_by_domain_key
                    .get(inheritance_root)?
                    .get_key_value(key)
                    .map(|(key, id)| (*id, key.clone()))
            });

        let Some(target) = target else {
            self.diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{expected_type}` with key `{key}` was not found"),
                )
                .with_primary(Some(node.record), node.path.clone()),
            );
            return None;
        };

        let target_draft = self.drafts.get(target.0.index())?;
        if !self
            .schema
            .is_assignable(target_draft.actual_type.as_str(), expected_type.as_str())
        {
            self.diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetTypeMismatch,
                    format!(
                        "ref target actual type `{}` is not assignable to `{expected_type}`",
                        target_draft.actual_type
                    ),
                )
                .with_primary(Some(node.record), node.path.clone()),
            );
            return None;
        }

        Some((target.0, target.1))
    }

    fn resolve_dict_entries(
        &mut self,
        entries: &[(CfdDictKey, ValueDraft)],
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = Vec::with_capacity(entries.len());
        let mut complete = true;
        for (key, value) in entries {
            let Some(value) = self.resolve_node(value, &node.dict_key(key), cursor) else {
                complete = false;
                continue;
            };
            out.push((key.clone(), value));
        }
        (complete && self.diagnostics.len() == diagnostic_start).then_some(out)
    }

    fn enter_node(
        &mut self,
        parent: TraversalCursor,
        node: &ValueNode,
        kind: StructureKind,
    ) -> Option<TraversalCursor> {
        if self.budget_exhausted {
            return None;
        }
        let result = self.budget.enter(parent, kind, 1);
        match result {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                self.push_budget_error(error.to_string(), node);
                None
            }
        }
    }

    fn push_budget_error(&mut self, message: String, node: &ValueNode) {
        self.budget_exhausted = true;
        self.diagnostics.push(
            CfdDiagnostic::error(CfdErrorCode::DataStructureLimitExceeded, message)
                .with_primary(Some(node.record), node.path.clone()),
        );
    }
}
