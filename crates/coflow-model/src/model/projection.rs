use crate::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdErrorCode, CfdModelBuilder,
    CfdValue, LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cft::CftSchema;
use std::collections::BTreeMap;
use std::sync::Arc;

use super::DimensionProjection;

impl CfdDataModel {
    /// 合并所选维度后重新解析整个模型，保证引用及嵌套模板使用同一视图。
    /// 连续调用保留其他维度选择，并从原始基础来源重新求值本次选择。
    pub fn project_dimensions(
        &self,
        schema: &CftSchema,
        dimensions: &BTreeMap<String, String>,
    ) -> Result<Self, CfdDiagnostics> {
        if dimensions.is_empty() {
            return Ok(self.clone());
        }
        for dimension in dimensions.keys() {
            if schema.resolve_dimension(dimension).is_none() {
                return Err(CfdDiagnostic::error(
                    CfdErrorCode::TypeMismatch,
                    format!("unknown dimension `{dimension}`"),
                )
                .into());
            }
        }
        let (source, mut selections) = self.projection.as_ref().map_or_else(
            || (Arc::new(self.clone()), BTreeMap::new()),
            |projection| {
                (
                    Arc::clone(&projection.source),
                    projection.selections.clone(),
                )
            },
        );
        selections.extend(dimensions.clone());
        // 相同选择共享已有不可变结果，不重复解析模板与重建引用索引。
        if self
            .projection
            .as_ref()
            .is_some_and(|view| view.selections == selections)
        {
            return Ok(self.clone());
        }
        let mut builder = CfdModelBuilder::new(schema);
        for (_, record) in source.records() {
            let fields = record
                .fields()
                .iter()
                .map(|(name, base)| {
                    let value = record
                        .dimension_fields
                        .get(name)
                        .and_then(|field| {
                            selections
                                .get(field.dimension.as_str())
                                .and_then(|variant| field.variants.get(variant.as_str()))
                        })
                        .map_or(base, |value| &value.value);
                    Ok((name.to_string(), draft(value)?))
                })
                .collect::<Result<BTreeMap<_, _>, CfdDiagnostics>>()?;
            builder.add_loaded_record(
                LoadedRecordDraft::new(record.id(), record.actual_type(), fields)
                    .with_origin(record.origin.clone()),
            );
        }
        let mut projected = builder.build()?;
        // 始终重新求值所选来源表达式，同时保留原始单维查询及显式覆盖元数据。
        for record in Arc::make_mut(&mut projected.records) {
            if let Some(original) = source
                .record_by_type_id(record.actual_type(), record.id())
                .and_then(|id| source.record(id))
            {
                record.dimension_fields = original.dimension_fields.clone();
            }
        }
        projected.dimension_variants = source.dimension_variants.clone();
        projected.projection = Some(DimensionProjection { source, selections });
        Ok(projected)
    }
}

fn draft(value: &CfdValue) -> Result<LoadedValueDraft, CfdDiagnostics> {
    use LoadedValueDraft as D;
    Ok(match value {
        CfdValue::OptionNone => D::OptionNone,
        CfdValue::OptionSome(value) => D::OptionSome(Box::new(draft(value)?)),
        CfdValue::Bool(value) => D::Bool(*value),
        CfdValue::Int(value) => D::Int(*value),
        CfdValue::Float(value) => D::Float(*value),
        CfdValue::String(value) => D::String(value.clone()),
        CfdValue::FormattedString(value) => D::FormattedString(
            crate::cell_value::parse_formatted_string(&value.source).map_err(|error| {
                CfdDiagnostics::new(
                    error
                        .diagnostics
                        .into_iter()
                        .map(|error| {
                            CfdDiagnostic::error(CfdErrorCode::TypeMismatch, error.message)
                        })
                        .collect(),
                )
            })?,
        ),
        CfdValue::Enum(value) => D::EnumValue {
            enum_name: value.enum_name.to_string(),
            value: value.value,
        },
        CfdValue::Ref(key) => D::RecordRef(key.to_string()),
        CfdValue::Object(value) => D::Object {
            actual_type: Some(value.actual_type().into()),
            fields: value
                .fields
                .iter()
                .map(|(name, value)| Ok((name.to_string(), draft(value)?)))
                .collect::<Result<_, CfdDiagnostics>>()?,
        },
        CfdValue::Array(values) => D::Array(values.iter().map(draft).collect::<Result<_, _>>()?),
        CfdValue::Dict(values) => D::Dict(
            values
                .iter()
                .map(|(key, value)| {
                    Ok((
                        match key {
                            CfdDictKey::String(value) => LoadedDictKeyDraft::String(value.clone()),
                            CfdDictKey::Int(value) => LoadedDictKeyDraft::Int(*value),
                            CfdDictKey::Enum(value) => LoadedDictKeyDraft::EnumValue {
                                enum_name: value.enum_name.to_string(),
                                value: value.value,
                            },
                        },
                        draft(value)?,
                    ))
                })
                .collect::<Result<_, CfdDiagnostics>>()?,
        ),
    })
}
