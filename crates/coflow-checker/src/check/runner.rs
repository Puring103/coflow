use super::evaluator::CheckEvaluator;
use super::value::{CheckRecordRef, CheckValue};
use crate::schema_view::SchemaView;
use coflow_cft::CftContainer;
use coflow_data_model::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdPath, CfdRecordId, CfdValue,
};
use std::collections::BTreeMap;

pub(crate) struct CheckRunner<'a> {
    schema: SchemaView,
    model: &'a CfdDataModel,
    diagnostics: Vec<CfdDiagnostic>,
}

impl<'a> CheckRunner<'a> {
    pub(crate) fn new(schema: &'a CftContainer, model: &'a CfdDataModel) -> Self {
        Self {
            schema: SchemaView::new(schema),
            model,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn run(mut self) -> Result<(), CfdDiagnostics> {
        for (record_id, record) in self.model.records() {
            let path = CfdPath::root();
            self.run_record_checks(
                CheckRecordRef::Top(record_id),
                Some(record_id),
                path.clone(),
            );
            self.run_nested_field_checks(Some(record_id), &record.fields, path);
        }

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
        let checks = self.schema.checks_for_actual(&actual_type);
        let root = CheckValue::Record(record);
        let mut evaluator =
            CheckEvaluator::new(&self.schema, self.model, root_record, root_path, root);
        for check in checks {
            evaluator.eval_check_block(&check);
        }
        self.diagnostics.extend(evaluator.diagnostics);
    }

    fn run_nested_field_checks(
        &mut self,
        root_record: Option<CfdRecordId>,
        fields: &BTreeMap<String, CfdValue>,
        root_path: CfdPath,
    ) {
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
                        record: record.as_ref().clone(),
                        path: Some(path.clone()),
                    },
                    root_record,
                    path.clone(),
                );
                self.run_nested_field_checks(root_record, &record.fields, path);
            }
            CfdValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.run_nested_value_checks(root_record, item, path.clone().index(index));
                }
            }
            CfdValue::Dict(entries) => {
                for (index, (_, item)) in entries.iter().enumerate() {
                    self.run_nested_value_checks(
                        root_record,
                        item,
                        path.clone().dict_key(index.to_string()),
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
