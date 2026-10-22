use std::collections::{BTreeMap, BTreeSet};

use coflow_language::cft::{DimensionName, FieldName, TypeName, VariantName};

use crate::RecordCoordinate;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CheckImpact {
    pub(crate) records: BTreeMap<RecordCoordinate, ChangedRecordFields>,
    pub(crate) record_sets: BTreeSet<TypeName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChangedRecordFields {
    All,
    Fields(BTreeSet<ChangedField>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChangedField {
    pub(crate) field: FieldName,
    pub(crate) projection: ChangedProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChangedProjection {
    Base,
    Dimension {
        dimension: DimensionName,
        variant: VariantName,
    },
}

impl ChangedRecordFields {
    /// 顶层拥有字段是内联任务失效边界；数组重排和对象替换要同时移除该树旧路径。
    pub(super) fn affects_owned_field(&self, field: &str, variant: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::Fields(fields) => fields.iter().any(|changed| {
                changed.field.as_str() == field
                    && match &changed.projection {
                        ChangedProjection::Base => true,
                        ChangedProjection::Dimension {
                            variant: changed, ..
                        } => variant == Some(changed.as_str()),
                    }
            }),
        }
    }
}

impl CheckImpact {
    /// 来源变更还会改变跨记录模板的有效值。对比已共享的代次快照，补齐间接失效。
    /// 只比较值及显式覆盖存在性，不把来源行号变化算作业务依赖变化。
    pub(super) fn with_model_changes(
        &self,
        schema: &coflow_language::cft::CftSchema,
        before: &crate::data_model::CfdDataModel,
        after: &crate::data_model::CfdDataModel,
    ) -> Self {
        let mut impact = self.clone();
        for (_, current) in after.records() {
            let coordinate = current.coordinate();
            let previous = before
                .record_by_type_id(current.actual_type(), current.id())
                .and_then(|handle| before.record(handle));
            let Some(previous) = previous else {
                impact.add_membership(schema, coordinate);
                continue;
            };
            for (field, value) in current.fields() {
                if previous.field(field.as_str()) != Some(value) {
                    impact.add_field(
                        coordinate.clone(),
                        ChangedField {
                            field: field.clone(),
                            projection: ChangedProjection::Base,
                        },
                    );
                }
            }
            for (field, values) in &current.dimension_fields {
                let previous = previous.dimension_field(field.as_str());
                for (variant, value) in &values.effective_variants {
                    let previous_value =
                        previous.and_then(|values| values.effective_variants.get(variant));
                    let previous_source = previous
                        .and_then(|values| values.variants.get(variant))
                        .map(|value| &value.value);
                    let current_source = values.variants.get(variant).map(|value| &value.value);
                    if previous_value != Some(value) || previous_source != current_source {
                        impact.add_field(
                            coordinate.clone(),
                            ChangedField {
                                field: field.clone(),
                                projection: ChangedProjection::Dimension {
                                    dimension: values.dimension.clone(),
                                    variant: variant.clone(),
                                },
                            },
                        );
                    }
                }
            }
        }
        for (_, previous) in before.records() {
            if after
                .record_by_type_id(previous.actual_type(), previous.id())
                .is_none()
            {
                impact.add_membership(schema, previous.coordinate());
            }
        }
        impact
    }

    fn add_field(&mut self, coordinate: RecordCoordinate, field: ChangedField) {
        if let ChangedRecordFields::Fields(fields) = self
            .records
            .entry(coordinate)
            .or_insert_with(|| ChangedRecordFields::Fields(BTreeSet::new()))
        {
            fields.insert(field);
        }
    }

    fn add_membership(
        &mut self,
        schema: &coflow_language::cft::CftSchema,
        coordinate: RecordCoordinate,
    ) {
        self.record_sets.insert(coordinate.actual_type.clone());
        self.record_sets.extend(
            schema
                .ancestor_type_names(&coordinate.actual_type)
                .into_iter()
                .flatten()
                .cloned(),
        );
        self.records.insert(coordinate, ChangedRecordFields::All);
    }
}
