use super::draft::RecordDraftSource;
use crate::build::{BuildSchema, RecordDraft, ValueDraft};
use crate::diagnostics::{CfdDiagnostic, CfdErrorCode, CfdPath, CfdPathSegment};
use crate::model::{CfdDictKey, CfdFormattedString, CfdObject, CfdRecordHandle, CfdValue};
use crate::{stringify_value, LoadedFieldReference, LoadedFormatSegment, LoadedFormattedString};
use coflow_language::cft::{
    CftValueType, DimensionName, FieldName, RecordId, TypeName, VariantName,
};
use coflow_language::limits::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ValueNode {
    record: CfdRecordHandle,
    path: CfdPath,
    // 维度身份属于求值节点，缓存与循环检测不能仅用变体名区分。
    context: Option<(DimensionName, VariantName)>,
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
            context: self.context.clone(),
        }
    }

    fn index(&self, index: usize) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().index(index),
            context: self.context.clone(),
        }
    }

    fn dict_key(&self, key: &CfdDictKey) -> Self {
        Self {
            record: self.record,
            path: self.path.clone().dict_key_value(key),
            context: self.context.clone(),
        }
    }
}

pub(super) struct ValueResolver<'a, 'schema, S: RecordDraftSource + ?Sized> {
    schema: &'a BuildSchema<'schema>,
    drafts: &'a S,
    record_by_domain_id: &'a BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
    diagnostics: &'a mut Vec<CfdDiagnostic>,
    dimension_values:
        BTreeMap<(CfdRecordHandle, FieldName, DimensionName, VariantName), ValueDraft>,
    reference_diagnostics: Vec<CfdDiagnostic>,
    memo: BTreeMap<ValueNode, ResolvedMemo>,
    active: BTreeMap<ValueNode, usize>,
    stack: Vec<ValueNode>,
    reported_cycles: BTreeSet<Vec<ValueNode>>,
    structural_limits: StructuralLimits,
    budget: StructuralBudget,
    budget_exhausted: bool,
    root_record: CfdRecordHandle,
    reads: BTreeMap<CfdRecordHandle, BTreeSet<CfdRecordHandle>>,
}

impl<'a, 'schema, S: RecordDraftSource + ?Sized> ValueResolver<'a, 'schema, S> {
    pub(super) fn new(
        schema: &'a BuildSchema<'schema>,
        drafts: &'a S,
        record_by_domain_id: &'a BTreeMap<TypeName, BTreeMap<RecordId, CfdRecordHandle>>,
        diagnostics: &'a mut Vec<CfdDiagnostic>,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema,
            drafts,
            record_by_domain_id,
            diagnostics,
            reference_diagnostics: Vec::new(),
            dimension_values: BTreeMap::new(),
            memo: BTreeMap::new(),
            active: BTreeMap::new(),
            stack: Vec::new(),
            reported_cycles: BTreeSet::new(),
            structural_limits,
            budget: StructuralBudget::new(structural_limits),
            budget_exhausted: false,
            root_record: CfdRecordHandle::new(0),
            reads: BTreeMap::new(),
        }
    }

    pub(super) fn resolve_record_fields(
        &mut self,
        record: CfdRecordHandle,
        context: Option<(DimensionName, VariantName)>,
    ) -> Option<BTreeMap<FieldName, CfdValue>> {
        self.root_record = record;
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
            context,
        };
        let cursor = self.enter_node(TraversalCursor::root(), &root, StructureKind::DataValue)?;
        // 顶层保留已成功的字段，供容错加载使用；严格构建仍会拒绝全部求值诊断。
        Some(
            self.resolve_fields_partial(fields, &root, cursor, drafts.get(record.index())?)
                .0,
        )
    }

    pub(super) fn take_reference_diagnostics(&mut self) -> Vec<CfdDiagnostic> {
        std::mem::take(&mut self.reference_diagnostics)
    }

    pub(super) fn take_reads(&mut self) -> BTreeMap<CfdRecordHandle, BTreeSet<CfdRecordHandle>> {
        std::mem::take(&mut self.reads)
    }

    pub(super) fn diagnostic_count(&self) -> usize {
        self.diagnostics.len()
    }

    pub(super) fn set_dimension_values(&mut self, values: &[super::ValidatedDimensionValue]) {
        self.dimension_values = values
            .iter()
            .map(|(record, input, draft, _)| {
                (
                    (
                        *record,
                        input.field.clone(),
                        input.dimension.clone(),
                        input.variant.clone(),
                    ),
                    draft.clone(),
                )
            })
            .collect();
    }

    fn resolve_fields(
        &mut self,
        fields: &BTreeMap<FieldName, ValueDraft>,
        parent: &ValueNode,
        cursor: TraversalCursor,
        owner: &RecordDraft,
    ) -> Option<BTreeMap<FieldName, CfdValue>> {
        let (fields, complete) = self.resolve_fields_partial(fields, parent, cursor, owner);
        complete.then_some(fields)
    }

    fn resolve_fields_partial(
        &mut self,
        fields: &BTreeMap<FieldName, ValueDraft>,
        parent: &ValueNode,
        cursor: TraversalCursor,
        owner: &RecordDraft,
    ) -> (BTreeMap<FieldName, CfdValue>, bool) {
        let diagnostic_start = self.diagnostics.len();
        let mut out = BTreeMap::new();
        let mut complete = true;
        for (name, value) in fields {
            let selected = self.dimension_override(parent, name).cloned();
            let value = selected.as_ref().unwrap_or(value);
            let Some(value) =
                self.resolve_node(value, parent.field(name), cursor, false, owner, parent)
            else {
                complete = false;
                continue;
            };
            out.insert(name.clone(), value);
        }
        (out, complete && self.diagnostics.len() == diagnostic_start)
    }

    fn dimension_override(&self, owner: &ValueNode, field: &FieldName) -> Option<&ValueDraft> {
        if !owner.path.segments.is_empty() {
            return None;
        }
        let (dimension, variant) = owner.context.as_ref()?;
        self.dimension_values
            .get(&(
                owner.record,
                field.clone(),
                dimension.clone(),
                variant.clone(),
            ))
            .or_else(|| {
                self.drafts
                    .dimension_value(owner.record, field, dimension, variant)
            })
    }

    fn resolve_node(
        &mut self,
        value: &ValueDraft,
        node: ValueNode,
        parent: TraversalCursor,
        memoize: bool,
        owner: &RecordDraft,
        owner_node: &ValueNode,
    ) -> Option<CfdValue> {
        // 叶子值与裸引用不会递归读取其他值节点；保留结构计费，省去路径克隆、
        // 活动栈和 memo 树操作。循环检测仍覆盖所有模板与容器递归。
        if matches!(value, ValueDraft::Value(_) | ValueDraft::PendingRef { .. }) {
            let cursor = self.enter_node(parent, &node, StructureKind::DataValue)?;
            return self.resolve_value(value, &node, cursor, owner, owner_node);
        }
        if let Some(cycle_start) = self.active.get(&node).copied() {
            self.push_cycle(cycle_start);
            return None;
        }
        let cursor = self.enter_node(parent, &node, StructureKind::DataValue)?;
        if memoize {
            if let Some(shape) = self.memo.get(&node).map(|memo| memo.shape) {
                self.charge_cached_shape(cursor, &node, shape)?;
                return self.memo.get(&node).map(|memo| memo.value.clone());
            }
        }

        let diagnostic_start = self.diagnostics.len();
        self.active.insert(node.clone(), self.stack.len());
        self.stack.push(node.clone());
        let resolved = self.resolve_value(value, &node, cursor, owner, owner_node);
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
        owner: &RecordDraft,
        owner_node: &ValueNode,
    ) -> Option<CfdValue> {
        match value {
            ValueDraft::Value(value) => Some(value.clone()),
            ValueDraft::OptionSome(value) => self
                .resolve_value(value, node, cursor, owner, owner_node)
                .map(|value| CfdValue::OptionSome(Box::new(value))),
            ValueDraft::FormattedString(value) => {
                self.resolve_formatted_string(value, node, cursor, owner, owner_node)
            }
            ValueDraft::PendingRef {
                explicit_type,
                id: key,
            } => {
                if let Some(explicit) = explicit_type {
                    if let Some(target) = self
                        .schema
                        .inheritance_root(explicit.as_str())
                        .and_then(|root| self.record_by_domain_id.get(root))
                        .and_then(|records| records.get(key.as_str()))
                        .and_then(|id| self.drafts.actual_type(id.index()))
                    {
                        if !self.schema.is_assignable(target, explicit.as_str()) {
                            // 非法引用保留记录与编辑入口，严格构建由引用诊断拒绝，不能静默放宽到字段父类型。
                            self.reference_diagnostics.push(CfdDiagnostic::error(CfdErrorCode::RefTargetTypeMismatch, format!("reference actual type `{target}` is not assignable to explicit `{explicit}`")).with_primary(Some(node.record), node.path.clone()));
                        }
                    }
                }
                match RecordId::new(key.clone()) {
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
                let fields =
                    self.resolve_fields(&record_draft.fields, node, cursor, record_draft)?;
                Some(CfdValue::Object(Box::new(CfdObject {
                    actual_type: record_draft.actual_type.clone(),
                    fields,
                })))
            }
            ValueDraft::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                let mut complete = true;
                for (index, item) in items.iter().enumerate() {
                    let Some(value) = self.resolve_node(
                        item,
                        node.index(index),
                        cursor,
                        false,
                        owner,
                        owner_node,
                    ) else {
                        complete = false;
                        continue;
                    };
                    out.push(value);
                }
                complete.then_some(CfdValue::Array(out))
            }
            ValueDraft::Dict(entries) => self
                .resolve_dict_entries(entries, node, cursor, owner, owner_node)
                .map(CfdValue::Dict),
        }
    }

    fn resolve_formatted_string(
        &mut self,
        value: &LoadedFormattedString,
        node: &ValueNode,
        cursor: TraversalCursor,
        owner: &RecordDraft,
        owner_node: &ValueNode,
    ) -> Option<CfdValue> {
        let mut rendered = String::new();
        for segment in &value.segments {
            match segment {
                LoadedFormatSegment::Text(text) => rendered.push_str(text),
                LoadedFormatSegment::Reference(reference) => {
                    let resolved =
                        self.resolve_format_reference(reference, node, cursor, owner, owner_node)?;
                    match stringify_value(&resolved) {
                        Ok(text) => rendered.push_str(&text),
                        Err(message) => {
                            self.diagnostics.push(
                                CfdDiagnostic::error(CfdErrorCode::TypeMismatch, message)
                                    .with_primary(Some(node.record), node.path.clone()),
                            );
                            return None;
                        }
                    }
                }
            }
        }
        Some(CfdValue::FormattedString(CfdFormattedString {
            source: value.source.clone(),
            rendered,
        }))
    }

    fn resolve_format_reference(
        &mut self,
        reference: &LoadedFieldReference,
        node: &ValueNode,
        cursor: TraversalCursor,
        owner: &RecordDraft,
        owner_node: &ValueNode,
    ) -> Option<CfdValue> {
        let first = reference.path.first()?;
        // self 绑定到实际包含模板的对象，集合和 Option 只传递所属对象，不重新绑定。
        let (mut current_value, mut current_type) = if reference.id.is_none() {
            self.resolve_owner_field(owner, owner_node, first, node, cursor)?
        } else {
            let expected = reference
                .type_name
                .as_deref()
                .unwrap_or(self.drafts.get(node.record.index())?.actual_type.as_str());
            let expected = TypeName::new(expected).ok()?;
            let (record, _) = self.resolve_ref_target(&expected, reference.id.as_deref()?, node)?;
            self.resolve_record_field(record, first, node, cursor)?
        };
        for field in reference.path.iter().skip(1) {
            let (value, value_type) =
                self.resolve_value_field(&current_value, &current_type, field, node, cursor)?;
            current_value = value;
            current_type = value_type;
        }
        Some(current_value)
    }

    fn resolve_owner_field(
        &mut self,
        owner: &RecordDraft,
        owner_node: &ValueNode,
        field_name: &str,
        blame: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<(CfdValue, CftValueType)> {
        // id 只属于顶层记录；data 即便位于记录内也没有虚拟身份。
        if field_name == "id" && owner_node.path.segments.is_empty() {
            return Some((CfdValue::String(owner.id.clone()), CftValueType::String));
        }
        let field = self.schema.field(owner.actual_type.as_str(), field_name);
        let Some(field) = field else {
            self.push_unknown_format_field(owner.actual_type.as_str(), field_name, blame);
            return None;
        };
        // 缺少显式覆盖时仍在当前上下文求值基础表达式，不能复用基础渲染结果。
        let value = self
            .dimension_override(owner_node, &field.name)
            .or_else(|| owner.fields.get(&field.name))?
            .clone();
        let resolved = self.resolve_node(
            &value,
            owner_node.field(&field.name),
            cursor,
            true,
            owner,
            owner_node,
        )?;
        Some((resolved, field.value_type.clone()))
    }

    fn resolve_record_field(
        &mut self,
        record_id: CfdRecordHandle,
        field_name: &str,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<(CfdValue, CftValueType)> {
        if record_id != self.root_record {
            self.reads
                .entry(self.root_record)
                .or_default()
                .insert(record_id);
        }
        let drafts = self.drafts;
        let owner = drafts.get(record_id.index())?;
        let owner_node = ValueNode {
            record: record_id,
            path: CfdPath::root(),
            context: node.context.clone(),
        };
        self.resolve_owner_field(owner, &owner_node, field_name, node, cursor)
    }

    fn resolve_value_field(
        &mut self,
        value: &CfdValue,
        ty: &CftValueType,
        field_name: &str,
        node: &ValueNode,
        cursor: TraversalCursor,
    ) -> Option<(CfdValue, CftValueType)> {
        match (value, ty) {
            (CfdValue::Object(object), CftValueType::Object(declared_type)) => {
                let actual_type = if object.actual_type().is_empty() {
                    declared_type.as_str()
                } else {
                    object.actual_type()
                };
                let field = self.schema.field(actual_type, field_name);
                let Some(field) = field else {
                    self.push_unknown_format_field(actual_type, field_name, node);
                    return None;
                };
                object
                    .field(field_name)
                    .cloned()
                    .map(|value| (value, field.value_type.clone()))
            }
            (CfdValue::Ref(key), CftValueType::RecordRef(target_type)) => {
                let (record_id, _) = self.resolve_ref_target(target_type, key.as_str(), node)?;
                self.resolve_record_field(record_id, field_name, node, cursor)
            }
            _ => {
                self.diagnostics.push(
                    CfdDiagnostic::error(
                        CfdErrorCode::TypeMismatch,
                        format!("cannot read field `{field_name}` from formatted string value"),
                    )
                    .with_primary(Some(node.record), node.path.clone()),
                );
                None
            }
        }
    }

    fn push_unknown_format_field(&mut self, type_name: &str, field_name: &str, node: &ValueNode) {
        self.diagnostics.push(
            CfdDiagnostic::error(
                CfdErrorCode::UnknownField,
                format!("unknown formatted string field `{field_name}` on type `{type_name}`"),
            )
            .with_primary(Some(node.record), node.path.clone()),
        );
    }

    fn resolve_ref_target(
        &mut self,
        expected_type: &TypeName,
        key: &str,
        node: &ValueNode,
    ) -> Option<(CfdRecordHandle, RecordId)> {
        let target = self
            .schema
            .inheritance_root(expected_type.as_str())
            .and_then(|inheritance_root| {
                self.record_by_domain_id
                    .get(inheritance_root)?
                    .get_key_value(key)
                    .map(|(key, id)| (*id, key.clone()))
            });

        let Some(target) = target else {
            self.diagnostics.push(
                CfdDiagnostic::error(
                    CfdErrorCode::RefTargetNotFound,
                    format!("ref target `{expected_type}` with id `{key}` was not found"),
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
        owner: &RecordDraft,
        owner_node: &ValueNode,
    ) -> Option<Vec<(CfdDictKey, CfdValue)>> {
        let diagnostic_start = self.diagnostics.len();
        let mut out = Vec::with_capacity(entries.len());
        let mut complete = true;
        for (key, value) in entries {
            let Some(value) =
                self.resolve_node(value, node.dict_key(key), cursor, false, owner, owner_node)
            else {
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
            .check_additional_depth(cursor, StructureKind::DataValue, additional_depth)
            .and_then(|()| {
                self.budget
                    .charge_nodes(StructureKind::DataValue, additional_nodes)
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
                left.cmp(right).then_with(|| left_index.cmp(right_index))
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
            format!("data value dependency cycle: {}", path.join(" -> ")),
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
            |draft| format!("{}.{}", draft.actual_type, draft.id),
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
        if let Some((dimension, variant)) = &node.context {
            let _ = write!(out, " @{dimension}:{variant}");
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
            CfdValue::OptionSome(value) => pending.push((value, child_depth)),
            CfdValue::Object(object) => {
                pending.extend(object.fields().values().map(|value| (value, child_depth)));
            }
            CfdValue::Array(items) => {
                pending.extend(items.iter().map(|value| (value, child_depth)));
            }
            CfdValue::Dict(entries) => {
                pending.extend(entries.iter().map(|(_, value)| (value, child_depth)));
            }
            CfdValue::OptionNone
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::FormattedString(_)
            | CfdValue::Enum(_)
            | CfdValue::Ref(_) => {}
        }
    }
    MaterializedShape { nodes, depth }
}
