use crate::{
    CfdDataModel, CfdDiagnostic, CfdDiagnostics, CfdDictKey, CfdErrorCode, CfdModelBuilder,
    CfdValue, LoadedDictKeyDraft, LoadedRecordDraft, LoadedValueDraft,
};
use coflow_language::cft::CftSchema;
use std::collections::BTreeMap;

impl CfdDataModel {
    /// 合并所选维度后重新解析整个模型，保证跨记录引用及嵌套 fstring 使用同一投影视角。
    pub fn project_dimensions(
        &self,
        schema: &CftSchema,
        dimensions: &BTreeMap<String, String>,
    ) -> Result<Self, CfdDiagnostics> {
        if dimensions.is_empty() {
            return Ok(self.clone());
        }
        let mut builder = CfdModelBuilder::new(schema);
        for (_, record) in self.records() {
            let fields = record
                .fields()
                .iter()
                .map(|(name, base)| {
                    let value = record
                        .dimension_fields
                        .get(name)
                        .and_then(|field| {
                            dimensions
                                .get(field.dimension.as_str())
                                .and_then(|variant| field.variants.get(variant.as_str()))
                        })
                        .map_or(base, |value| &value.value);
                    Ok((name.to_string(), draft(value)?))
                })
                .collect::<Result<BTreeMap<_, _>, CfdDiagnostics>>()?;
            builder.add_loaded_record(
                LoadedRecordDraft::new(record.key(), record.actual_type(), fields)
                    .with_origin(record.origin.clone()),
            );
        }
        let mut projected = builder.build()?;
        // 投影改变基础有效值，保留覆盖元数据供来源查询与覆盖状态比较使用。
        for record in &mut projected.records {
            if let Some(original) = self
                .record_by_type_key(record.actual_type(), record.key())
                .and_then(|id| self.record(id))
            {
                record.dimension_fields = original.dimension_fields.clone();
            }
        }
        projected.dimension_variants = self.dimension_variants.clone();
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
