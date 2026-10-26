use super::{BuildSchema, CfdInputBatch, CfdModelBuilder};
use crate::{CfdDataModel, CfdRecordHandle, DimensionValueDraft};
use coflow_language::cft::{DimensionName, FieldName, VariantName};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::Arc;

pub(crate) type DimensionKey = (CfdRecordHandle, FieldName, DimensionName, VariantName);
type HostKey = (FieldName, DimensionName, VariantName);

/// 覆盖草稿与发现的变体一起冻结；未变化批次不重复枚举覆盖内容。
#[derive(Debug, Clone, Default)]
pub struct CfdDimensionInputBatch {
    values: CfdInputBatch<DimensionValueDraft>,
    pub(crate) variants: BTreeSet<(DimensionName, VariantName)>,
}

impl From<Vec<DimensionValueDraft>> for CfdDimensionInputBatch {
    fn from(values: Vec<DimensionValueDraft>) -> Self {
        let variants = values
            .iter()
            .map(|value| (value.dimension.clone(), value.variant.clone()))
            .collect();
        Self {
            values: values.into(),
            variants,
        }
    }
}

impl CfdDimensionInputBatch {
    #[must_use]
    pub fn same_generation(&self, other: &Self) -> bool {
        self.values.same_generation(&other.values)
    }
    fn key(&self) -> usize {
        Arc::as_ptr(&self.values.identity()) as usize
    }
}

impl Deref for CfdDimensionInputBatch {
    type Target = [DimensionValueDraft];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

#[derive(Debug, Clone)]
struct BatchKeys {
    // 保留身份以防地址复用；不持有旧批次草稿。
    _identity: Arc<()>,
    keys: Arc<[DimensionKey]>,
}

#[derive(Debug, Clone, Copy)]
struct InputLocation {
    batch: Option<usize>,
    index: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct DimensionInputIndex {
    hosts: coflow_storage::PagedVec<Arc<BTreeMap<HostKey, InputLocation>>>,
    batches: BTreeMap<usize, Arc<BatchKeys>>,
    reusable: bool,
}

pub(crate) struct PreparedDimensions<'a> {
    pub index: DimensionInputIndex,
    builder: &'a CfdModelBuilder<'a>,
    batches: BTreeMap<usize, &'a CfdDimensionInputBatch>,
    pub changed: Vec<DimensionKey>,
    pub checked: usize,
}

impl DimensionInputIndex {
    /// 持久宿主分片只复制命中页及宿主表；批次目录只保存轻量来源位置。
    pub fn prepare<'a>(
        builder: &'a CfdModelBuilder<'a>,
        model: &CfdDataModel,
        schema: BuildSchema<'_>,
    ) -> Option<PreparedDimensions<'a>> {
        let reusable = builder.dimension_values.is_empty();
        let prior = model
            .dimension_input_index
            .as_deref()
            .filter(|index| index.reusable && reusable);
        let mut index = prior.cloned().unwrap_or_else(|| {
            let empty = Arc::new(BTreeMap::new());
            Self {
                hosts: (0..model.record_count()).map(|_| empty.clone()).collect(),
                batches: BTreeMap::new(),
                reusable,
            }
        });
        let batches = builder
            .dimension_batches
            .iter()
            .map(|batch| (batch.key(), *batch))
            .collect::<BTreeMap<_, _>>();
        if batches.len() != builder.dimension_batches.len() {
            return None;
        }
        let mut changed = Vec::new();
        let mut checked = 0;
        let removed = index
            .batches
            .keys()
            .filter(|key| !batches.contains_key(key))
            .copied()
            .collect::<Vec<_>>();
        for key in removed {
            let old = index.batches.remove(&key)?;
            for entry in old.keys.iter() {
                let (host, field, dimension, variant) = entry;
                Arc::make_mut(&mut index.hosts[host.index()]).remove(&(
                    field.clone(),
                    dimension.clone(),
                    variant.clone(),
                ));
                changed.push(entry.clone());
            }
        }
        for (key, batch) in &batches {
            if index.batches.contains_key(key) {
                continue;
            }
            let mut keys = Vec::with_capacity(batch.len());
            for (position, input) in batch.iter().enumerate() {
                let entry = index.insert(
                    input,
                    InputLocation {
                        batch: Some(*key),
                        index: position,
                    },
                    model,
                    builder,
                    schema,
                )?;
                keys.push(entry.clone());
                changed.push(entry);
                checked += 1;
            }
            index.batches.insert(
                *key,
                Arc::new(BatchKeys {
                    _identity: batch.values.identity(),
                    keys: keys.into(),
                }),
            );
        }
        for (position, input) in builder.dimension_values.iter().enumerate() {
            changed.push(index.insert(
                input,
                InputLocation {
                    batch: None,
                    index: position,
                },
                model,
                builder,
                schema,
            )?);
            checked += 1;
        }
        Some(PreparedDimensions {
            index,
            builder,
            batches,
            changed,
            checked,
        })
    }

    fn insert(
        &mut self,
        input: &DimensionValueDraft,
        location: InputLocation,
        model: &CfdDataModel,
        builder: &CfdModelBuilder<'_>,
        schema: BuildSchema<'_>,
    ) -> Option<DimensionKey> {
        let host = model.lookup_assignable(builder.schema, &input.source_type, &input.source_id)?;
        let owner = model.record(host)?;
        let field = schema.field(owner.actual_type(), &input.field)?;
        if field
            .dimension
            .as_ref()
            .is_none_or(|binding| binding.dimension != input.dimension)
        {
            return None;
        }
        let key = (
            input.field.clone(),
            input.dimension.clone(),
            input.variant.clone(),
        );
        if Arc::make_mut(&mut self.hosts[host.index()])
            .insert(key, location)
            .is_some()
        {
            return None;
        }
        Some((
            host,
            input.field.clone(),
            input.dimension.clone(),
            input.variant.clone(),
        ))
    }
}

impl<'a> PreparedDimensions<'a> {
    pub fn get(&self, key: &DimensionKey) -> Option<&'a DimensionValueDraft> {
        let location = self.index.hosts.get(key.0.index())?.get(&(
            key.1.clone(),
            key.2.clone(),
            key.3.clone(),
        ))?;
        match location.batch {
            Some(batch) => self.batches.get(&batch)?.get(location.index),
            None => self
                .builder
                .dimension_values
                .get(location.index)
                .map(|input| &**input),
        }
    }

    pub fn entries(
        &self,
        host: CfdRecordHandle,
    ) -> impl Iterator<Item = (DimensionKey, &'a DimensionValueDraft)> + '_ {
        self.index
            .hosts
            .get(host.index())
            .into_iter()
            .flat_map(|entries| entries.keys())
            .filter_map(move |(field, dimension, variant)| {
                let key = (host, field.clone(), dimension.clone(), variant.clone());
                self.get(&key).map(|input| (key, input))
            })
    }
}
