use crate::compiler_context::{CfdValueDraft, DataModelCompilerContext, RecordDraft};
use crate::diagnostic::{CfdDiagnostic, CfdErrorCode, CfdPath, CfdPathSegment};
use crate::model::{CfdDictKey, CfdDomainId, CfdObject, CfdRecordId, CfdValue};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ValueNode {
    record: CfdRecordId,
    path: CfdPath,
    branch: Vec<usize>,
}

impl ValueNode {
    fn field(&self, name: impl Into<String>) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().field(name),
            branch: self.branch.clone(),
        }
    }

    fn index(&self, index: usize) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().index(index),
            branch: self.branch.clone(),
        }
    }

    fn dict_key(&self, key: &CfdDictKey) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key_value(key),
            branch: self.branch.clone(),
        }
    }

    fn spread_branch(&self, index: usize) -> Self {
        let mut branch = self.branch.clone();
        branch.push(index);
        Self {
            record: self.record,
            path: self.path.clone(),
            branch,
        }
    }
}

pub(super) struct ValueResolver<'a> {
    schema: &'a DataModelCompilerContext,
    drafts: &'a [RecordDraft],
    record_by_domain_key: &'a BTreeMap<(CfdDomainId, String), CfdRecordId>,
    diagnostics: &'a mut Vec<CfdDiagnostic>,
    memo: BTreeMap<ValueNode, CfdValue>,
    active: BTreeMap<ValueNode, usize>,
    stack: Vec<ValueNode>,
    reported_cycles: BTreeSet<Vec<ValueNode>>,
}

impl<'a> ValueResolver<'a> {
    pub(super) fn new(
        schema: &'a DataModelCompilerContext,
        drafts: &'a [RecordDraft],
        record_by_domain_key: &'a BTreeMap<(CfdDomainId, String), CfdRecordId>,
        diagnostics: &'a mut Vec<CfdDiagnostic>,
    ) -> Self {
        Self {
            schema,
            drafts,
            record_by_domain_key,
            diagnostics,
            memo: BTreeMap::new(),
            active: BTreeMap::new(),
            stack: Vec::new(),
            reported_cycles: BTreeSet::new(),
        }
    }

    pub(super) fn resolve_record_fields(
        &mut self,
        record: CfdRecordId,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let drafts = self.drafts;
        let fields = &drafts.get(record.index())?.fields;
        self.resolve_fields(
            fields,
            &ValueNode {
                record,
                path: CfdPath::root(),
                branch: Vec::new(),
            },
        )
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<String, CfdValueDraft>,
        parent: &ValueNode,
    ) -> Option<BTreeMap<String, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        let mut complete = true;
        for (name, value) in fields {
            let Some(value) = self.resolve_node(value, parent.field(name.clone())) else {
                complete = false;
                continue;
            };
            out.insert(name.clone(), value);
        }
        (complete && self.diagnostics.len() == diagnostic_start).then_some(out)
    }

    fn resolve_node(&mut self, value: &CfdValueDraft, node: ValueNode) -> Option<CfdValue> {
        if let Some(value) = self.memo.get(&node) {
            return Some(value.clone());
        }
        if let Some(cycle_start) = self.active.get(&node).copied() {
            self.push_cycle(cycle_start);
            return None;
        }

        let diagnostic_start = self.diagnostics.len();
        self.active.insert(node.clone(), self.stack.len());
        self.stack.push(node.clone());
        let resolved = self.resolve_value(value, &node);
        self.stack.pop();
        self.active.remove(&node);

        if self.diagnostics.len() != diagnostic_start {
            return None;
        }
        if let Some(resolved) = resolved {
            self.memo.insert(node, resolved.clone());
            Some(resolved)
        } else {
            None
        }
    }

    fn resolve_value(&mut self, value: &CfdValueDraft, node: &ValueNode) -> Option<CfdValue> {
        match value {
            CfdValueDraft::Value(value) => Some(value.clone()),
            CfdValueDraft::PendingRef { expected_type, key } => {
                let _ = self.resolve_ref_target(expected_type, key, node)?;
                Some(CfdValue::Ref(key.clone()))
            }
            CfdValueDraft::PendingSpreadField {
                source_type,
                key,
                field,
            } => self.resolve_spread_field(source_type, key, field, node),
            CfdValueDraft::Object(record_draft) => {
                let fields = self.resolve_fields(&record_draft.fields, node)?;
                Some(CfdValue::Object(Box::new(CfdObject {
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                })))
            }
            CfdValueDraft::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                let mut complete = true;
                for (index, item) in items.iter().enumerate() {
                    let Some(value) = self.resolve_node(item, node.index(index)) else {
                        complete = false;
                        continue;
                    };
                    out.push(value);
                }
                complete.then_some(CfdValue::Array(out))
            }
            CfdValueDraft::Dict(entries) => {
                self.resolve_dict_entries(entries, node).map(CfdValue::Dict)
            }
            CfdValueDraft::DictSpread { spreads, entries } => self
                .resolve_dict_spread(spreads, entries, node)
                .map(CfdValue::Dict),
        }
    }

    fn resolve_ref_target(
        &mut self,
        expected_type: &str,
        key: &str,
        node: &ValueNode,
    ) -> Option<CfdRecordId> {
        let target = self
            .schema
            .type_domain_id(expected_type)
            .and_then(|domain_id| self.record_by_domain_key.get(&(domain_id, key.to_string())))
            .copied();

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

        let target_draft = self.drafts.get(target.index())?;
        if !self
            .schema
            .is_assignable(&target_draft.actual_type, expected_type)
        {
            self.diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    format!(
                        "ref target actual type `{}` is not assignable to `{expected_type}`",
                        target_draft.actual_type
                    ),
                )
                .with_primary(Some(node.record), node.path.clone()),
            );
            return None;
        }

        Some(target)
    }

    fn resolve_dict_entries(
        &mut self,
        entries: &[(CfdDictKey, CfdValueDraft)],
        node: &ValueNode,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = Vec::with_capacity(entries.len());
        let mut complete = true;
        for (key, value) in entries {
            let Some(value) = self.resolve_node(value, node.dict_key(key)) else {
                complete = false;
                continue;
            };
            out.push((key.clone(), value));
        }
        (complete && self.diagnostics.len() == diagnostic_start).then_some(out)
    }

    fn resolve_dict_spread(
        &mut self,
        spreads: &[CfdValueDraft],
        entries: &[(CfdDictKey, CfdValueDraft)],
        node: &ValueNode,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut merged = BTreeMap::<CfdDictKey, CfdValue>::new();
        let mut complete = true;
        for (index, spread) in spreads.iter().enumerate() {
            let Some(CfdValue::Dict(entries)) =
                self.resolve_node(spread, node.spread_branch(index))
            else {
                if self.diagnostics.len() == diagnostic_start {
                    self.diagnostics.push(
                        CfdDiagnostic::error(
                            CfdErrorCode::TypeMismatch,
                            "dict spread requires a dict value",
                        )
                        .with_primary(Some(node.record), node.path.clone()),
                    );
                }
                complete = false;
                continue;
            };
            for (key, value) in entries {
                merged.insert(key, value);
            }
        }

        for (key, value) in entries {
            let Some(value) = self.resolve_node(value, node.dict_key(key)) else {
                complete = false;
                continue;
            };
            merged.insert(key.clone(), value);
        }

        (complete && self.diagnostics.len() == diagnostic_start)
            .then(|| merged.into_iter().collect())
    }

    fn resolve_spread_field(
        &mut self,
        source_type: &str,
        key: &str,
        field: &str,
        node: &ValueNode,
    ) -> Option<CfdValue> {
        let source_id = self.resolve_ref_target(source_type, key, node)?;
        let drafts = self.drafts;
        let source_draft = drafts.get(source_id.index())?;
        let Some(value) = source_draft.fields.get(field) else {
            self.diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::UnknownField,
                    format!("spread field `{field}` was not found"),
                )
                .with_primary(Some(node.record), node.path.clone()),
            );
            return None;
        };

        self.resolve_node(
            value,
            ValueNode {
                record: source_id,
                path: CfdPath::root().field(field),
                branch: Vec::new(),
            },
        )
    }

    fn push_cycle(&mut self, cycle_start: usize) {
        let mut nodes = self.stack[cycle_start..].to_vec();
        let displays = nodes
            .iter()
            .map(|node| self.display_node(node))
            .collect::<Vec<_>>();
        let canonical_start = displays
            .iter()
            .enumerate()
            .min_by(|(left_index, left), (right_index, right)| {
                left.cmp(right)
                    .then_with(|| nodes[*left_index].branch.cmp(&nodes[*right_index].branch))
            })
            .map_or(0, |(index, _)| index);
        nodes.rotate_left(canonical_start);
        if !self.reported_cycles.insert(nodes.clone()) {
            return;
        }

        let mut path = nodes
            .iter()
            .map(|node| self.display_node(node))
            .collect::<Vec<_>>();
        if let Some(first) = path.first().cloned() {
            path.push(first);
        }
        let Some(first) = nodes.first() else {
            return;
        };
        let mut diagnostic = CfdDiagnostic::error(
            CfdErrorCode::ValueDependencyCycle,
            format!("data spread dependency cycle: {}", path.join(" -> ")),
        )
        .with_primary(Some(first.record), first.path.clone())
        .with_primary_message("cycle starts here");
        for node in nodes.iter().skip(1) {
            diagnostic = diagnostic.with_related(
                Some(node.record),
                node.path.clone(),
                "cycle continues here",
            );
        }
        self.diagnostics.push(diagnostic);
    }

    fn display_node(&self, node: &ValueNode) -> String {
        let mut out = self.drafts.get(node.record.index()).map_or_else(
            || format!("record#{}", node.record.index()),
            |draft| format!("{}.{}", draft.actual_type, draft.key),
        );
        for segment in &node.path.segments {
            match segment {
                CfdPathSegment::Field(name) => {
                    out.push('.');
                    out.push_str(name);
                }
                CfdPathSegment::Index(index) => {
                    out.push_str(&format!("[{index}]"));
                }
                CfdPathSegment::DictKey(key) => {
                    out.push('[');
                    out.push_str(key);
                    out.push(']');
                }
            }
        }
        out
    }
}
