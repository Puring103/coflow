use super::deps::{DependencyCollector, DependencyGraphBuilder};
use super::dimensions::{DimensionRoundView, DimensionVariantAbort};
use super::evaluator::CheckEvaluator;
use super::statements;
use super::value::{CheckRecordRef, CheckValue, ValueLocation};
use crate::{DependencyGraph, DimensionCheckContext};
use coflow_cft::CftSchema;
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdErrorCode, CfdRecordId, CfdValue,
};
use coflow_structure::{StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use std::collections::BTreeMap;

pub(crate) struct CheckRunner<'a> {
    schema: &'a CftSchema,
    model: &'a CfdDataModel,
    diagnostics: Vec<CfdDiagnostic>,
    diagnostic_roots: Vec<CfdRecordId>,
    /// When `Some`, the runner records read-from edges for each top-level
    /// record. The current root is the most recently pushed entry.
    deps: Option<DependencyGraphBuilder>,
    dimension_context: Option<DimensionCheckContext>,
    dimension_round: Option<DimensionRoundView>,
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
    fields: &'a BTreeMap<String, CfdValue>,
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
            structural_limits,
        }
    }

    pub(crate) fn with_dimension_context(
        schema: &'a CftSchema,
        model: &'a CfdDataModel,
        dimension_context: DimensionCheckContext,
        structural_limits: StructuralLimits,
    ) -> Self {
        let dimension_round = DimensionRoundView::compile(schema, model, &dimension_context);
        Self {
            schema,
            model,
            diagnostics: Vec::new(),
            diagnostic_roots: Vec::new(),
            deps: None,
            dimension_context: Some(dimension_context),
            dimension_round: Some(dimension_round),
            structural_limits,
        }
    }

    pub(crate) fn run(mut self) -> Result<(), CfdDiagnostics> {
        for (record_id, record) in self.model.records() {
            self.run_one_record(record_id, record);
        }
        self.into_result()
    }

    pub(crate) fn run_for(mut self, targets: &[CfdRecordId]) -> Result<(), CfdDiagnostics> {
        for id in targets {
            if let Some(record) = self.model.record(*id) {
                self.run_one_record(*id, record);
            }
        }
        self.into_result()
    }

    pub(crate) fn run_with_deps(mut self) -> (Result<(), CfdDiagnostics>, DependencyGraph) {
        self.deps = Some(DependencyGraphBuilder::new());
        for (record_id, record) in self.model.records() {
            self.run_one_record(record_id, record);
        }
        let graph = self
            .deps
            .take()
            .map_or_else(DependencyGraph::default, DependencyGraphBuilder::finish);
        (self.into_result(), graph)
    }

    pub(crate) fn run_for_with_deps_rooted(
        mut self,
        targets: &[CfdRecordId],
    ) -> (Vec<(CfdRecordId, CfdDiagnostic)>, DependencyGraph) {
        self.deps = Some(DependencyGraphBuilder::new());
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
        (rooted, graph)
    }

    fn run_one_record(&mut self, record_id: CfdRecordId, record: &coflow_data_model::CfdRecord) {
        let diagnostics_start = self.diagnostics.len();
        self.run_one_record_inner(record_id, record);
        self.diagnostic_roots.extend(std::iter::repeat_n(
            record_id,
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
            CheckRecordRef::Resolved(location.clone()),
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
                location,
                &mut traversal_budget,
                root_cursor,
            );
        } else {
            self.run_nested_field_checks(
                NestedFieldChecks {
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

    fn into_result(self) -> Result<(), CfdDiagnostics> {
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            Err(CfdDiagnostics::new(self.diagnostics))
        }
    }

    fn run_record_checks(
        &mut self,
        record: CheckRecordRef,
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
        let root = CheckValue::Record(record);
        let deps = self.deps.as_ref().map_or_else(
            || DependencyCollector::disabled(root_record),
            |deps| deps.collector_for(root_record),
        );
        let mut evaluator = CheckEvaluator::new(
            self.schema,
            self.model,
            root_location,
            root,
            deps,
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
        if let Some(deps) = self.deps.as_mut() {
            deps.extend_root(root_record, collector);
        }
    }

    fn run_nested_field_checks(
        &mut self,
        request: NestedFieldChecks<'_>,
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
                request.root_location.field(name),
                request.selection,
                traversal_budget,
                request.cursor,
            );
        }
    }

    fn run_dimension_nested_checks(
        &mut self,
        root_record: CfdRecordId,
        root_location: ValueLocation,
        traversal_budget: &mut Option<StructuralBudget>,
        cursor: TraversalCursor,
    ) {
        let Some(round) = self.dimension_round.clone() else {
            return;
        };
        for (field, message) in round.errors_for(root_record) {
            self.diagnostics.push(
                CfdDiagnostic::error(CfdErrorCode::CheckEvalTypeError, message).with_primary(
                    Some(root_record),
                    root_location.clone().field(field).blame.path,
                ),
            );
        }
        let fields = round
            .field_names(root_record)
            .map(str::to_string)
            .collect::<Vec<_>>();
        for field in fields {
            let logical_location = root_location.field(&field);
            match round.materialize(self.model, root_record, &field, &logical_location) {
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
                            .with_primary(Some(location.blame.record), location.blame.path),
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
                        .with_primary(Some(location.blame.record), location.blame.path.clone()),
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
                    CheckRecordRef::Resolved(location.clone()),
                    root_record,
                    location.clone(),
                    selection,
                );
                self.run_nested_field_checks(
                    NestedFieldChecks {
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
