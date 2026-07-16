use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath, CfdPathSegment};
use crate::model::{CfdDictKey, CfdDomainId, CfdObject, CfdRecordId, CfdValue};
use coflow_cft::{FieldName, RecordKey, TypeName};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ValueNode {
    record: CfdRecordId,
    path: CfdPath,
    branch: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct MaterializedShape {
    nodes: u64,
    depth: u64,
}

#[derive(Debug, Clone)]
struct ResolvedMemo {
    value: CfdValue,
    shape: MaterializedShape,
}

impl ValueNode {
    fn field(&self, name: &FieldName) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().field(name.as_str()),
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

pub(super) struct ValueResolver<'a, 'schema> {
    schema: &'a BuildSchema<'schema>,
    drafts: &'a [RecordDraft],
    record_by_domain_key: &'a BTreeMap<CfdDomainId, BTreeMap<RecordKey, CfdRecordId>>,
    diagnostics: &'a mut Vec<CfdDiagnostic>,
    memo: BTreeMap<ValueNode, ResolvedMemo>,
    active: BTreeMap<ValueNode, usize>,
    stack: Vec<ValueNode>,
    reported_cycles: BTreeSet<Vec<ValueNode>>,
    structural_limits: StructuralLimits,
    budget: StructuralBudget,
    budget_exhausted: bool,
}

impl<'a, 'schema> ValueResolver<'a, 'schema> {
    pub(super) fn new(
        schema: &'a BuildSchema<'schema>,
        drafts: &'a [RecordDraft],
        record_by_domain_key: &'a BTreeMap<CfdDomainId, BTreeMap<RecordKey, CfdRecordId>>,
        diagnostics: &'a mut Vec<CfdDiagnostic>,
        structural_limits: StructuralLimits,
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
        self.memo.clear();
        self.active.clear();
        self.stack.clear();
        let drafts = self.drafts;
        let fields = &drafts.get(record.index())?.fields;
        let root = ValueNode {
            record,
            path: CfdPath::root(),
            branch: Vec::new(),
        };
        let cursor = self.enter_node(TraversalCursor::root(), &root, StructureKind::DataValue)?;
        self.resolve_fields(fields, &root, cursor)
    }

    pub(super) fn resolve_dimension_value(
        &mut self,
        record: CfdRecordId,
        value: &ValueDraft,
        path: CfdPath,
    ) -> Option<CfdValue> {
        self.budget = StructuralBudget::new(self.structural_limits);
        self.budget_exhausted = false;
        self.memo.clear();
        self.active.clear();
        self.stack.clear();
        let node = ValueNode {
            record,
            path,
            branch: Vec::new(),
        };
        self.resolve_node(value, node, TraversalCursor::root(), false)
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<FieldName, ValueDraft>,
        parent: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<BTreeMap<FieldName, CfdValue>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        let mut complete = true;
        for (name, value) in fields {
            let Some(value) = self.resolve_node(value, parent.field(name), cursor, false) else {
                complete = false;
                continue;
            };
            out.insert(name.clone(), value);
        }
        (complete && self.diagnostics.len() == diagnostic_start).then_some(out)
    }

    fn resolve_node(
        &mut self,
        value: &ValueDraft,
        node: ValueNode,
        parent: TraversalCursor,
        memoize: bool,
    ) -> Option<CfdValue> {
        if let Some(cycle_start) = self.active.get(&node).copied() {
            self.push_cycle(cycle_start);
            return None;
        }
        let kind = if matches!(
            value,
            ValueDraft::PendingSpreadField { .. } | ValueDraft::DictSpread { .. }
        ) {
            StructureKind::SpreadResolution
        } else {
            StructureKind::DataValue
        };
        let cursor = self.enter_node(parent, &node, kind)?;
        if memoize {
            if let Some(shape) = self.memo.get(&node).map(|memo| memo.shape) {
                self.charge_cached_shape(cursor, &node, shape)?;
                return self.memo.get(&node).map(|memo| memo.value.clone());
            }
        }

        let diagnostic_start = self.diagnostics.len();
        self.active.insert(node.clone(), self.stack.len());
        self.stack.push(node.clone());
        let resolved = self.resolve_value(value, &node, cursor);
        self.stack.pop();
        self.active.remove(&node);

        if self.diagnostics.len() != diagnostic_start {
            return None;
        }
        if let Some(resolved) = resolved {
            if !memoize {
                return Some(resolved);
            }
            self.memo.insert(
                node,
                ResolvedMemo {
                    shape: materialized_shape(&resolved),
                    value: resolved.clone(),
                },
            );
            Some(resolved)
        } else {
            None
        }
    }

    fn resolve_value(
        &mut self,
        value: &ValueDraft,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<CfdValue> {
        match value {
            ValueDraft::Value(value) => Some(value.clone()),
            ValueDraft::PendingRef {
                expected_type: _,
                key,
            } => match RecordKey::new(key.clone()) {
                Ok(key) => Some(CfdValue::Ref(key)),
                Err(error) => {
                    self.diagnostics.push(
                        CfdDiagnostic::error(CfdErrorCode::TypeMismatch, error.to_string())
                            .with_primary(Some(node.record), node.path.clone()),
                    );
                    None
                }
            },
            ValueDraft::PendingSpreadField {
                source_type,
                key,
                field,
            } => self.resolve_spread_field(source_type, key, field, node, cursor),
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
                    let Some(value) = self.resolve_node(item, node.index(index), cursor, false)
                    else {
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
            ValueDraft::DictSpread { spreads, entries } => self
                .resolve_dict_spread(spreads, entries, node, cursor)
                .map(CfdValue::Dict),
        }
    }

    fn resolve_ref_target(
        &mut self,
        expected_type: &TypeName,
        key: &str,
        node: &ValueNode,
    ) -> Option<(CfdRecordId, RecordKey)> {
        let target = self
            .schema
            .type_domain_id(expected_type.as_str())
            .and_then(|domain_id| {
                self.record_by_domain_key
                    .get(&domain_id)?
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
            let Some(value) = self.resolve_node(value, node.dict_key(key), cursor, false) else {
                complete = false;
                continue;
            };
            out.push((key.clone(), value));
        }
        (complete && self.diagnostics.len() == diagnostic_start).then_some(out)
    }

    fn resolve_dict_spread(
        &mut self,
        spreads: &[ValueDraft],
        entries: &[(CfdDictKey, ValueDraft)],
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut merged = BTreeMap::<CfdDictKey, CfdValue>::new();
        let mut complete = true;
        for (index, spread) in spreads.iter().enumerate() {
            let Some(CfdValue::Dict(entries)) =
                self.resolve_node(spread, node.spread_branch(index), cursor, false)
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
            let Some(value) = self.resolve_node(value, node.dict_key(key), cursor, false) else {
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
        source_type: &TypeName,
        key: &str,
        field: &FieldName,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<CfdValue> {
        let (source_id, _) = self.resolve_ref_target(source_type, key, node)?;
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
                path: CfdPath::root().field(field.as_str()),
                branch: Vec::new(),
            },
            cursor,
            true,
        )
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
        let result = self
            .budget
            .enter(parent, kind, 1)
            .and_then(|cursor| self.budget.charge_work(kind, 1).map(|()| cursor));
        match result {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                self.push_budget_error(error.to_string(), node);
                None
            }
        }
    }

    fn charge_cached_shape(
        &mut self,
        cursor: TraversalCursor,
        node: &ValueNode,
        shape: MaterializedShape,
    ) -> Option<()> {
        let additional_depth = shape.depth.saturating_sub(1);
        let additional_nodes = shape.nodes.saturating_sub(1);
        let result = self
            .budget
            .check_additional_depth(cursor, StructureKind::SpreadResolution, additional_depth)
            .and_then(|()| {
                self.budget
                    .charge_nodes(StructureKind::SpreadResolution, additional_nodes)
            })
            .and_then(|()| {
                self.budget
                    .charge_work(StructureKind::SpreadResolution, additional_nodes)
            });
        match result {
            Ok(()) => Some(()),
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
                    let _ = write!(out, "[{index}]");
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

fn materialized_shape(root: &CfdValue) -> MaterializedShape {
    let mut nodes = 0_u64;
    let mut depth = 0_u64;
    let mut pending = vec![(root, 1_u64)];
    while let Some((value, value_depth)) = pending.pop() {
        nodes = nodes.saturating_add(1);
        depth = depth.max(value_depth);
        let child_depth = value_depth.saturating_add(1);
        match value {
            CfdValue::Object(object) => {
                pending.extend(object.fields().values().map(|value| (value, child_depth)));
            }
            CfdValue::Array(items) => {
                pending.extend(items.iter().map(|value| (value, child_depth)));
            }
            CfdValue::Dict(entries) => {
                pending.extend(entries.iter().map(|(_, value)| (value, child_depth)));
            }
            CfdValue::Null
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::Enum(_)
            | CfdValue::Ref(_) => {}
        }
    }
    MaterializedShape { nodes, depth }
}
