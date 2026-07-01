use super::evaluator::CheckEvaluator;
use super::value::{CheckRecordRef, CheckValue};
use crate::schema_view::SchemaView;
use crate::{DependencyGraph, DimensionCheckContext};
use coflow_cft::CftContainer;
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdPath, CfdRecordId, CfdValue,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct CheckRunner<'a> {
    schema: SchemaView,
    model: &'a CfdDataModel,
    diagnostics: Vec<CfdDiagnostic>,
    /// When `Some`, the runner records read-from edges for each top-level
    /// record. The current root is the most recently pushed entry.
    deps: Option<BTreeMap<CfdRecordId, BTreeSet<CfdRecordId>>>,
    dimension_context: Option<DimensionCheckContext>,
}

impl<'a> CheckRunner<'a> {
    pub(crate) fn new(schema: &'a CftContainer, model: &'a CfdDataModel) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
            diagnostics: Vec::new(),
            deps: None,
            dimension_context: None,
        }
    }

    pub(crate) fn with_dimension_context(
        schema: &'a CftContainer,
        model: &'a CfdDataModel,
        dimension_context: DimensionCheckContext,
    ) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
            diagnostics: Vec::new(),
            deps: None,
            dimension_context: Some(dimension_context),
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
        self.deps = Some(BTreeMap::new());
        for (record_id, record) in self.model.records() {
            self.run_one_record(record_id, record);
        }
        let graph = DependencyGraph {
            reads_from: self.deps.take().unwrap_or_default(),
        };
        (self.into_result(), graph)
    }

    fn run_one_record(&mut self, record_id: CfdRecordId, record: &coflow_data_model::CfdRecord) {
        let path = CfdPath::root();
        self.run_record_checks(
            CheckRecordRef::Top(record_id),
            Some(record_id),
            path.clone(),
        );
        self.run_nested_field_checks(Some(record_id), record.fields(), path);
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
        root_path: CfdPath,
    ) {
        let Some(actual_type) = record.actual_type(self.model).map(ToOwned::to_owned) else {
            return;
        };
        let checks = self.schema.checks_for_actual(
            &actual_type,
            self.dimension_context
                .as_ref()
                .map(|context| context.dimension.as_str()),
        );
        let root = CheckValue::Record(record);
        let mut evaluator =
            CheckEvaluator::new(&self.schema, self.model, root_record, root_path, root);
        evaluator.dep_collector_enabled = self.deps.is_some();
        evaluator
            .dimension_context
            .clone_from(&self.dimension_context);
        for check in checks {
            let _ = evaluator.eval_check_block(&check);
        }
        self.diagnostics.extend(evaluator.diagnostics);
        if let (Some(deps), Some(root_id)) = (self.deps.as_mut(), root_record) {
            if !evaluator.reads_from.is_empty() {
                deps.entry(root_id)
                    .or_default()
                    .extend(evaluator.reads_from);
            }
        }
    }

    fn run_nested_field_checks(
        &mut self,
        root_record: Option<CfdRecordId>,
        fields: &BTreeMap<String, CfdValue>,
        root_path: CfdPath,
    ) {
        if self.dimension_context.is_some() {
            return;
        }
        for (name, value) in fields {
            self.run_nested_value_checks(root_record, value, root_path.clone().field(name));
        }
    }

    fn run_nested_value_checks(
        &mut self,
        root_record: Option<CfdRecordId>,
        value: &CfdValue,
        path: CfdPath,
    ) {
        match value {
            CfdValue::Object(record) => {
                self.run_record_checks(
                    CheckRecordRef::Inline {
                        object: Box::new(record.as_ref().clone()),
                        path: Some(path.clone()),
                        host: root_record,
                    },
                    root_record,
                    path.clone(),
                );
                self.run_nested_field_checks(root_record, record.fields(), path);
            }
            CfdValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.run_nested_value_checks(root_record, item, path.clone().index(index));
                }
            }
            CfdValue::Dict(entries) => {
                for (key, item) in entries {
                    self.run_nested_value_checks(
                        root_record,
                        item,
                        path.clone().dict_key_value(key),
                    );
                }
            }
            CfdValue::Ref { .. }
            | CfdValue::Null
            | CfdValue::Bool(_)
            | CfdValue::Int(_)
            | CfdValue::Float(_)
            | CfdValue::String(_)
            | CfdValue::Enum(_) => {}
        }
    }
}
