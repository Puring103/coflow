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
        self.with_model_changes_from(
            schema,
            before,
            after,
            after.records().map(|(handle, _)| handle),
            true,
        )
    }

    pub(super) fn with_changed_records(
        &self,
        schema: &coflow_language::cft::CftSchema,
        before: &crate::data_model::CfdDataModel,
        after: &crate::data_model::CfdDataModel,
        changed: &[crate::data_model::CfdRecordHandle],
    ) -> Self {
        // 调用方保证成员与句柄保持稳定；列表包含模板和维度传播后的全部有效值变化。
        self.with_model_changes_from(schema, before, after, changed.iter().copied(), false)
    }

    fn with_model_changes_from(
        &self,
        schema: &coflow_language::cft::CftSchema,
        before: &crate::data_model::CfdDataModel,
        after: &crate::data_model::CfdDataModel,
        records: impl IntoIterator<Item = crate::data_model::CfdRecordHandle>,
        membership_may_change: bool,
    ) -> Self {
        let mut impact = self.clone();
        for handle in records {
            let Some(current) = after.record(handle) else {
                continue;
            };
            let coordinate = current.coordinate();
            let previous = before
                .record_by_type_id(current.actual_type(), current.id())
                .and_then(|handle| before.record(handle));
            let Some(previous) = previous else {
                impact.add_membership(schema, coordinate);
                if membership_may_change {
                    impact.add_reference_membership(after, handle);
                }
                continue;
            };
            if !std::sync::Arc::ptr_eq(&previous.object, &current.object) {
                for (field, value) in current.fields() {
                    if !previous
                        .field(field.as_str())
                        .is_some_and(|previous| previous.same_stored_value(value))
                    {
                        impact.add_field(
                            coordinate.clone(),
                            ChangedField {
                                field: field.clone(),
                                projection: ChangedProjection::Base,
                            },
                        );
                    }
                }
            }
            if std::sync::Arc::ptr_eq(&previous.dimension_fields, &current.dimension_fields) {
                continue;
            }
            for (field, values) in current.dimension_fields.iter() {
                let previous = previous.dimension_field(field.as_str());
                for (variant, value) in &values.effective_variants {
                    let previous_value =
                        previous.and_then(|values| values.effective_variants.get(variant));
                    let previous_source = previous
                        .and_then(|values| values.variants.get(variant))
                        .map(|value| &value.value);
                    let current_source = values.variants.get(variant).map(|value| &value.value);
                    let same_value =
                        previous_value.is_some_and(|previous| previous.same_stored_value(value));
                    let same_source = match (previous_source, current_source) {
                        (None, None) => true,
                        (Some(previous), Some(current)) => previous.same_stored_value(current),
                        _ => false,
                    };
                    if !same_value || !same_source {
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
        if membership_may_change {
            for (handle, previous) in before.records() {
                if after
                    .record_by_type_id(previous.actual_type(), previous.id())
                    .is_none()
                {
                    impact.add_membership(schema, previous.coordinate());
                    impact.add_reference_membership(before, handle);
                }
            }
        }
        impact
    }

    fn add_reference_membership(
        &mut self,
        model: &crate::data_model::CfdDataModel,
        target: crate::data_model::CfdRecordHandle,
    ) {
        // 引用字面量可能不变而目标被删除或替换为另一个实际类型；类型判断和虚拟 id
        // 依赖引用解析结果。只使真实入边的拥有字段失效，避免扩大到整个声明类型。
        for edge in model.ref_edges_to_target(target) {
            let Some(host) = model.record(edge.site.host) else {
                continue;
            };
            let changed = if let Some(dimension) = &edge.site.dimension {
                ChangedField {
                    field: dimension.field.clone(),
                    projection: ChangedProjection::Dimension {
                        dimension: dimension.dimension.clone(),
                        variant: dimension.variant.clone(),
                    },
                }
            } else if let Some(crate::data_model::CfdPathSegment::Field(field)) =
                edge.site.path.segments.first()
            {
                let Ok(field) = FieldName::new(field) else {
                    continue;
                };
                ChangedField {
                    field,
                    projection: ChangedProjection::Base,
                }
            } else {
                continue;
            };
            self.add_field(host.coordinate(), changed);
        }
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
