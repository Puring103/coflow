use coflow_runtime::{
    DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState, DimensionValueView,
    MutationOp, MutationRequest, MutationValue,
};

use crate::editor::types::{EditorError, WriteDimensionValueOutcome};

use super::{finalize_mutation, SessionStore};

impl SessionStore {
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
