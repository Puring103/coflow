use super::deps::{DependencyCollector, DependencyGraphBuilder};
use super::dimensions::{DimensionRoundView, DimensionVariantAbort};
use super::evaluator::CheckEvaluator;
use super::statements;
use super::value::{EvalRecordRef, EvalValue, ValueLocation};
use crate::{CheckDiagnostic, CheckDiagnosticContext, CheckExecutionId, DependencyGraph, DimensionCheckContext};
use coflow_cft::{CftSchema, FieldName};
use coflow_data_model::{CfdDataModel, CfdDiagnostic, CfdErrorCode, CfdRecordId, CfdValue};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

pub(crate) struct CheckRunner<'a> {
    schema: &'a CftSchema,
    model: &'a CfdDataModel,
    diagnostics: Vec<CheckDiagnostic>,
    diagnostic_roots: Vec<CheckExecutionId>,
    /// When `Some`, the runner records read-from edges for each top-level
    /// record. The current root is the most recently pushed entry.
    deps: Option<DependencyGraphBuilder>,
    dimension_context: Option<DimensionCheckContext>,
    dimension_round: Option<DimensionRoundView>,
    regex_cache: Rc<RefCell<super::builtins::RegexCache>>,
    structural_limits: StructuralLimits,
}

#[derive(Debug, Clone, Copy)]
enum CheckSelection {
    Default,
    DimensionRelevant,
    FullVariantSubtree,
}

struct NestedFieldChecks<'a> {
    root_record: Option<CfdRecordId>,
    actual_type: &'a str,
    fields: &'a BTreeMap<FieldName, CfdValue>,
    root_location: ValueLocation,
    selection: CheckSelection,
    cursor: TraversalCursor,
}

impl<'a> CheckRunner<'a> {
    pub(crate) fn new(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        structural_limits: StructuralLimits,
    ) -> Self {
        Self {
            schema,
            model,
            diagnostics: Vec::new(),
            diagnostic_roots: Vec::new(),
            deps: None,
            dimension_context: None,
            dimension_round: None,
            regex_cache: Rc::new(RefCell::new(super::builtins::RegexCache::new())),
            structural_limits,
        }
    }

    pub(crate) fn with_dimension_context(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        dimension_context: DimensionCheckContext,
        structural_limits: StructuralLimits,
    ) -> Self {
        let dimension_round = DimensionRoundView::new(&dimension_context);
        Self {
            schema,
            model,
            diagnostics: Vec::new(),
            diagnostic_roots: Vec::new(),
            deps: None,
            dimension_context: Some(dimension_context),
            dimension_round: Some(dimension_round),
            regex_cache: Rc::new(RefCell::new(super::builtins::RegexCache::new())),
            structural_limits,
        }
    }

    pub(crate) fn run_rooted(
        mut self,
        targets: &[CfdRecordId],
        collect_dependencies: bool,
    ) -> (Vec<(CheckExecutionId, CheckDiagnostic)>, DependencyGraph, usize) {
        if collect_dependencies {
            self.deps = Some(DependencyGraphBuilder::new());
        }
        for id in targets {
            if let Some(record) = self.model.record(*id) {
                self.run_one_record(*id, record);
            }
        }
        let graph = self
            .deps
            .take()
            .map_or_else(DependencyGraph::default, DependencyGraphBuilder::finish);
        let rooted = self
            .diagnostic_roots
            .into_iter()
            .zip(self.diagnostics)
            .collect();
        let projected_records = self
            .dimension_round
            .as_ref()
            .map_or(0, DimensionRoundView::projected_record_count);
        (rooted, graph, projected_records)
    }

    pub(crate) fn run_top_level(
        mut self,
        targets: &[coflow_cft::CheckName],
        collect_dependencies: bool,
    ) -> (Vec<(CheckExecutionId, CheckDiagnostic)>, DependencyGraph) {
        if collect_dependencies {
            self.deps = Some(DependencyGraphBuilder::new());
        }
        for name in targets {
            let Some(check) = self.schema.resolve_check(name) else {
                continue;
            };
            let execution = CheckExecutionId::TopLevel(name.clone());
            let collector = if self.deps.is_some() {
                DependencyGraphBuilder::collector_for(execution.clone())
            } else {
                DependencyCollector::disabled(Some(execution.clone()))
            };
            let mut evaluator = CheckEvaluator::new(
                self.schema,
                self.model,
                None,
                EvalValue::null(),
                collector,
                Rc::clone(&self.regex_cache),
                self.structural_limits,
            );
            evaluator.schema_location = Some(crate::CheckSchemaLocation {
                module: check.module.clone(),
                span: check.block.span,
            });
            evaluator.contexts.push(CheckDiagnosticContext::Check {
                name: name.to_string(),
            });
            if self.dimension_round.is_some() {
                evaluator.dimension_round.clone_from(&self.dimension_round);
            }
            let statement_indices = self
                .dimension_context
                .as_ref()
                .and_then(|context| check.statement_indices(&context.dimension));
            let _ = match statement_indices {
                Some(statement_indices) => statements::eval_scheduled_check_block(
                    &mut evaluator,
                    coflow_cft::ScheduledCheckBlock::new(&check.block, statement_indices),
                ),
                None if self.dimension_context.is_none() => {
                    statements::eval_check_block(&mut evaluator, &check.block)
                }
                None => continue,
            };
            let (diagnostics, collector) = evaluator.into_outputs();
            self.diagnostic_roots.extend(std::iter::repeat_n(
                execution.clone(),
                diagnostics.len(),
            ));
            self.diagnostics.extend(diagnostics);
            if let Some(deps) = self.deps.as_mut() {
                deps.extend_root(execution, collector);
            }
        }
        let mut graph = self
            .deps
            .take()
            .map_or_else(DependencyGraph::default, DependencyGraphBuilder::finish);
        if collect_dependencies {
            for name in targets {
                if let Some(check) = self.schema.resolve_check(name) {
                    graph.record_sets.insert(
                        CheckExecutionId::TopLevel(name.clone()),
                        check.record_sets.clone(),
                    );
                }
            }
        }
        let rooted = self
            .diagnostic_roots
            .into_iter()
            .zip(self.diagnostics)
            .collect();
        (rooted, graph)
    }

    fn run_one_record(&mut self, record_id: CfdRecordId, record: &coflow_data_model::CfdRecord) {
        let diagnostics_start = self.diagnostics.len();
        self.run_one_record_inner(record_id, record);
        self.diagnostic_roots.extend(std::iter::repeat_n(
            CheckExecutionId::Record(record_id),
            self.diagnostics.len() - diagnostics_start,
        ));
    }

    fn run_one_record_inner(
        &mut self,
        record_id: CfdRecordId,
        record: &coflow_data_model::CfdRecord,
    ) {
        let location = ValueLocation::root(record_id);
        let mut traversal_budget = Some(StructuralBudget::new(self.structural_limits));
        let Some(root_cursor) =
            self.enter_data_value(&mut traversal_budget, TraversalCursor::root(), &location)
        else {
            return;
        };
        self.run_record_checks(
            EvalRecordRef::Resolved(location.clone()),
            Some(record_id),
            location.clone(),
            if self.dimension_round.is_some() {
                CheckSelection::DimensionRelevant
            } else {
                CheckSelection::Default
            },
        );
        if self.dimension_round.is_some() {
            self.run_dimension_nested_checks(
                record_id,
                &location,
                &mut traversal_budget,
                root_cursor,
            );
        } else {
            self.run_nested_field_checks(
                &NestedFieldChecks {
                    root_record: Some(record_id),
                    actual_type: record.actual_type(),
                    fields: record.fields(),
                    root_location: location,
                    selection: CheckSelection::Default,
                    cursor: root_cursor,
                },
                &mut traversal_budget,
            );
        }
    }

    fn run_record_checks(
        &mut self,
        record: EvalRecordRef,
        root_record: Option<CfdRecordId>,
        root_location: ValueLocation,
        selection: CheckSelection,
    ) {
        let Some(actual_type) = record.actual_type(self.model).map(ToOwned::to_owned) else {
            return;
        };
        let dimension = match selection {
            CheckSelection::DimensionRelevant => self
                .dimension_context
                .as_ref()
                .map(|context| context.dimension.as_str()),
            CheckSelection::Default | CheckSelection::FullVariantSubtree => None,
        };
        let checks = self.schema.check_schedule(&actual_type, dimension);
        let root = EvalValue::Record(record);
        let execution = root_record.map(CheckExecutionId::Record);
        let deps = match (&self.deps, &execution) {
            (Some(_), Some(execution)) => {
                DependencyGraphBuilder::collector_for(execution.clone())
            }
            _ => DependencyCollector::disabled(execution.clone()),
        };
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            Some(root_location),
            root,
            deps,
            Rc::clone(&self.regex_cache),
            self.structural_limits,
        );
        if matches!(selection, CheckSelection::DimensionRelevant) {
            evaluator.dimension_round.clone_from(&self.dimension_round);
        }
        for check in checks {
            let _ = statements::eval_scheduled_check_block(&mut evaluator, check);
        }
        let (diagnostics, collector) = evaluator.into_outputs();
        self.diagnostics.extend(diagnostics);
        if let (Some(deps), Some(execution)) = (self.deps.as_mut(), execution) {
            deps.extend_root(execution, collector);
        }
    }

    fn run_nested_field_checks(
        &mut self,
        request: &NestedFieldChecks<'_>,
        traversal_budget: &mut Option<StructuralBudget>,
    ) {
        for (name, value) in request.fields {
            if !self
                .schema
                .field_has_nested_checks(request.actual_type, name)
            {
                continue;
            }
            self.run_nested_value_checks(
                request.root_record,
                value,
                request.root_location.field(name.as_str()),
                request.selection,
                traversal_budget,
                request.cursor,
            );
        }
    }

    fn run_dimension_nested_checks(
        &mut self,
        root_record: CfdRecordId,
        root_location: &ValueLocation,
        traversal_budget: &mut Option<StructuralBudget>,
        cursor: TraversalCursor,
    ) {
        let Some(round) = self.dimension_round.clone() else {
            return;
        };
        for (field, error) in round.nested_fields(self.schema, self.model, root_record) {
            if let Some(message) = error {
                self.diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, message)
                        .with_primary(Some(root_record), root_location.field(&field).blame.path)
                        .into(),
                );
                continue;
            }
            let logical_location = root_location.field(&field);
            match round.materialize(
                self.schema,
                self.model,
                root_record,
                &field,
                &logical_location,
            ) {
                Ok(Some(materialized)) => {
                    self.run_nested_value_checks(
                        Some(root_record),
                        materialized.value,
                        materialized.location,
                        CheckSelection::FullVariantSubtree,
                        traversal_budget,
                        cursor,
                    );
                }
                Ok(None) | Err(DimensionVariantAbort::Skipped) => {}
                Err(DimensionVariantAbort::Error {
                    code,
                    location,
                    message,
                }) => {
                    let location = (*location).unwrap_or_else(|| logical_location.clone());
                    self.diagnostics.push(
                        CfdDiagnostic::error(code, message)
                            .with_primary(Some(location.blame.record), location.blame.path)
                            .into(),
                    );
                }
            }
        }
    }

    fn enter_data_value(
        &mut self,
        budget: &mut Option<StructuralBudget>,
        cursor: TraversalCursor,
        location: &ValueLocation,
    ) -> Option<TraversalCursor> {
        let result = budget.as_mut()?.enter(cursor, StructureKind::DataValue, 1);
        match result {
            Ok(cursor) => Some(cursor),
            Err(error) => {
                *budget = None;
                self.diagnostics.push(
                    CfdDiagnostic::error(CfdErrorCode::CheckBudgetExceeded, error.to_string())
                        .with_primary(Some(location.blame.record), location.blame.path.clone())
                        .into(),
                );
                None
            }
        }
    }

    fn run_nested_value_checks(
        &mut self,
        root_record: Option<CfdRecordId>,
        value: &CfdValue,
        location: ValueLocation,
        selection: CheckSelection,
        traversal_budget: &mut Option<StructuralBudget>,
        cursor: TraversalCursor,
    ) {
        if matches!(
            value,
            CfdValue::Ref(_)
                | CfdValue::Null
                | CfdValue::Bool(_)
                | CfdValue::Int(_)
                | CfdValue::Float(_)
                | CfdValue::String(_)
                | CfdValue::Enum(_)
        ) {
            return;
        }
        let Some(cursor) = self.enter_data_value(traversal_budget, cursor, &location) else {
            return;
        };
        match value {
            CfdValue::Object(record) => {
                if root_record.is_none() {
                    return;
                }
                self.run_record_checks(
                    EvalRecordRef::Resolved(location.clone()),
                    root_record,
                    location.clone(),
                    selection,
                );
                self.run_nested_field_checks(
                    &NestedFieldChecks {
                        root_record,
                        actual_type: record.actual_type(),
                        fields: record.fields(),
                        root_location: location,
                        selection,
                        cursor,
                    },
                    traversal_budget,
                );
            }
            CfdValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.run_nested_value_checks(
                        root_record,
                        item,
                        location.index(index),
                        selection,
                        traversal_budget,
                        cursor,
                    );
                }
            }
            CfdValue::Dict(entries) => {
                for (key, item) in entries {
                    self.run_nested_value_checks(
                        root_record,
                        item,
                        location.dict_key_value(key),
                        selection,
                        traversal_budget,
                        cursor,
                    );
                }
            }
            CfdValue::Ref(_)
            | CfdValue::Null
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::Enum(_) => {}
        }
    }
}
