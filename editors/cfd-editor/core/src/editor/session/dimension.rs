use coflow_runtime::{
    DimensionName, DimensionValueCoordinate, DimensionValueExpectation, DimensionValueState,
    DimensionValueView, FieldName, MutationOp, MutationRequest, MutationValue, VariantName,
};

use crate::editor::types::{
    DimensionFileRecords, DimensionFileRow, EditorError, WriteDimensionValueOutcome,
};

use super::errors::api_diagnostics_to_editor_error;
use super::mutation_apply::finalize_mutation;
use super::SessionStore;

impl SessionStore {
    pub fn get_dimension_file_records(
        &self,
        id: u32,
        file_path: &str,
    ) -> Result<DimensionFileRecords, EditorError> {
        let entry = self.session(id)?;
        let session = entry.state.read();
        let queries = session.queries();
        let normalized_path = file_path.replace('\\', "/");
        let (name, source_file) = normalized_path
            .strip_prefix("@dimension/")
            .and_then(|path| path.split_once('/'))
            .ok_or_else(|| EditorError::not_found("dimension file view not found"))?;
        let dimension = queries
            .dimensions()
            .into_iter()
            .find(|item| item.name == name)
            .ok_or_else(|| EditorError::not_found("dimension not found"))?;
        let fields = dimension.fields.clone();
        let dimension_name = DimensionName::new(dimension.name.clone())
            .map_err(|error| EditorError::other(error.to_string()))?;
        let mut rows = Vec::new();
        for field in fields {
            let field_name = FieldName::new(field.source_field.clone())
                .map_err(|error| EditorError::other(error.to_string()))?;
            for target in queries.ref_targets(&field.source_type) {
                let Some(view) =
                    queries.record_view(&target.coordinate.actual_type, &target.coordinate.key)
                else {
                    continue;
                };
                let Some(default_value) = view.record.field(&field.source_field).cloned() else {
                    continue;
                };
                let actual_type = target.coordinate.actual_type.clone();
                let record_key = target.coordinate.key.clone();
                let mut values = std::collections::BTreeMap::new();
                let mut sources = std::collections::BTreeMap::new();
                let mut contains_override = false;
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
                    let source = queries
                        .dimension_source_file(&coordinate)
                        .map(str::to_string);
                    contains_override |= source.as_deref() == Some(source_file);
                    sources.insert(variant.clone(), source);
                    values.insert(variant.clone(), state);
                }
                let recommended = queries
                    .dimension_destination(actual_type.as_str(), record_key.as_str(), name)
                    .ok();
                // 推荐文件可编辑主体的全部字段；分散来源只展示实际包含的覆盖字段。
                if !contains_override && recommended.as_deref() != Some(source_file) {
                    continue;
                }
                rows.push(DimensionFileRow {
                    owner_file_path: queries
                        .file_for_record(&target.coordinate.actual_type, &target.coordinate.key)
                        .unwrap_or_default()
                        .to_string(),
                    coordinate: target.coordinate,
                    field: field.source_field.clone(),
                    default_value,
                    values,
                    sources,
                });
            }
        }
        Ok(DimensionFileRecords {
            revision: session.revisions.current(),
            file_path: normalized_path,
            dimension: dimension.name,
            display_name: dimension.display_name,
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
        let session = entry.state.read();
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
        destination_file: Option<&str>,
    ) -> Result<WriteDimensionValueOutcome, EditorError> {
        let entry = self.session(id)?;
        let mut session = entry.state.write();
        let expected = match expected_value {
            DimensionValueState::Missing => DimensionValueExpectation::Missing,
            DimensionValueState::Value(value) => {
                DimensionValueExpectation::Value(MutationValue::Cfd(value.clone()))
            }
        };
        // 推荐路径由配置目录与主体项目相对路径组成；已有覆盖优先写回真实来源。
        let suggested_file = if destination_file.is_none()
            && matches!(new_value, DimensionValueState::Value(_))
            && session
                .queries()
                .dimension_source_file(coordinate)
                .is_none()
        {
            Some(
                session
                    .queries()
                    .dimension_destination(
                        coordinate.actual_type.as_str(),
                        coordinate.record_key.as_str(),
                        coordinate.dimension.as_str(),
                    )
                    .map_err(api_diagnostics_to_editor_error)?,
            )
        } else {
            None
        };
        let op = match new_value {
            DimensionValueState::Missing => MutationOp::ClearDimensionValue {
                coordinate: coordinate.clone(),
                expected,
            },
            DimensionValueState::Value(value) => MutationOp::SetDimensionValue {
                file: destination_file.map(str::to_string).or(suggested_file),
                coordinate: coordinate.clone(),
                expected,
                value: MutationValue::Cfd(value.clone()),
            },
        };
        let report = coflow_runtime::commands::apply_project_mutation(
            &mut session.engine,
            MutationRequest {
                stop_on_write_error: true,
                ops: vec![op],
            },
        )
        .map_err(api_diagnostics_to_editor_error)?;
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
            file_tree: session.queries().file_tree(),
            dimensions: session.queries().dimensions(),
            diagnostics: report.diagnostics,
            affected_files: report.affected_files,
        })
    }
}
