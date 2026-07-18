use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use coflow_runtime::{
    DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, DimensionValueView,
    MutationOp, MutationRequest, MutationValue,
};

use crate::editor::types::{
    DimensionFileRecords, DimensionFileRow, EditorError, WriteDimensionValueOutcome,
};

use super::{finalize_mutation, SessionStore};

impl SessionStore {
    pub fn get_dimension_file_records(
        &self,
        id: u32,
        file_path: &str,
    ) -> Result<DimensionFileRecords, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let queries = session.queries();
        let normalized_path = file_path.replace('\\', "/");
        let (dimension, source_type, source_field) = queries
            .dimension_field_for_file(&normalized_path)
            .ok_or_else(|| EditorError::not_found("managed dimension file not found"))?;
        let field_name = FieldName::new(source_field.clone())
            .map_err(|error| EditorError::other(error.to_string()))?;
        let dimension_name = DimensionName::new(dimension.name.clone())
            .map_err(|error| EditorError::other(error.to_string()))?;
        let mut rows = Vec::new();
        for target in queries.ref_targets(&source_type) {
            let Some(view) =
                queries.record_view(&target.coordinate.actual_type, &target.coordinate.key)
            else {
                continue;
            };
            let Some(default_value) = view.record.field(&source_field).cloned() else {
                continue;
            };
            let actual_type = TypeName::new(target.coordinate.actual_type.clone())
                .map_err(|error| EditorError::other(error.to_string()))?;
            let record_key = RecordKey::new(target.coordinate.key.clone())
                .map_err(|error| EditorError::other(error.to_string()))?;
            let mut values = std::collections::BTreeMap::new();
            for variant in &dimension.variants {
                let coordinate = DimensionValueCoordinate {
                    actual_type: actual_type.clone(),
                    record_key: record_key.clone(),
                    field: field_name.clone(),
                    dimension: dimension_name.clone(),
                    variant: VariantName::new(variant.clone())
                        .map_err(|error| EditorError::other(error.to_string()))?,
                    path: Vec::new(),
                };
                let state = queries
                    .dimension_value(&coordinate)
                    .map_or(DimensionValueState::Missing, |value| value.state);
                values.insert(variant.clone(), state);
            }
            rows.push(DimensionFileRow {
                owner_file_path: queries
                    .file_for_record(&target.coordinate.actual_type, &target.coordinate.key)
                    .unwrap_or_default()
                    .to_string(),
                coordinate: target.coordinate,
                default_value,
                values,
            });
        }
        Ok(DimensionFileRecords {
            revision: session.revisions.current(),
            file_path: normalized_path,
            dimension: dimension.name,
            display_name: dimension.display_name,
            field: source_field,
            variants: dimension.variants,
            rows,
        })
    }

    pub fn get_dimension_value(
        &self,
        id: u32,
        coordinate: &DimensionValueCoordinate,
    ) -> Result<DimensionValueView, EditorError> {
        let entry = self.session(id)?;
        let session = entry
            .state
            .read()
            .map_err(|_| EditorError::session("session poisoned"))?;
        session
            .queries()
            .dimension_value(coordinate)
            .ok_or_else(|| EditorError::not_found("dimension value not found"))
    }

    pub fn write_dimension_value(
        &self,
        id: u32,
        coordinate: &DimensionValueCoordinate,
        expected_value: &DimensionValueState,
        new_value: &DimensionValueState,
    ) -> Result<WriteDimensionValueOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry
            .state
            .write()
            .map_err(|_| EditorError::session("session poisoned"))?;
        let expected = match expected_value {
            DimensionValueState::Missing => DimensionValueExpectation::Missing,
            DimensionValueState::Value(value) => {
                DimensionValueExpectation::Value(MutationValue::Cfd(value.clone()))
            }
        };
        let op = match new_value {
            DimensionValueState::Missing => MutationOp::ClearDimensionValue {
                coordinate: coordinate.clone(),
                expected,
            },
            DimensionValueState::Value(value) => MutationOp::SetDimensionValue {
                coordinate: coordinate.clone(),
                expected,
                value: MutationValue::Cfd(value.clone()),
            },
        };
        let report = session.engine.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops: vec![op],
        });
        let report = finalize_mutation(&mut session, report, "write dimension value failed")?;
        let new_value = session
            .queries()
            .dimension_value(coordinate)
            .ok_or_else(|| EditorError::not_found("dimension value not found after write"))?
            .state;
        Ok(WriteDimensionValueOutcome {
            revision: session.revisions.current(),
            coordinate: coordinate.clone(),
            old_value: expected_value.clone(),
            new_value,
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }
}
