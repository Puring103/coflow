use std::collections::{BTreeMap, BTreeSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use coflow_checker::CheckReadProjection;

use super::impact::{ChangedProjection, ChangedRecordFields, CheckImpact};
use super::store::StableCheckTarget;
use crate::RecordCoordinate;

// 小更新只复制命中的索引桶；键和值共享，旧代次仍可独立读取。
const INDEX_BUCKETS: usize = 256;

#[derive(Debug, Clone)]
struct SharedIndex<K, V> {
    buckets: Vec<Arc<BTreeMap<Arc<K>, Arc<V>>>>,
}

impl<K, V> Default for SharedIndex<K, V> {
    fn default() -> Self {
        Self {
            buckets: (0..INDEX_BUCKETS)
                .map(|_| Arc::new(BTreeMap::new()))
                .collect(),
        }
    }
}

impl<K: Ord + Hash, V: Clone> SharedIndex<K, V> {
    fn bucket(key: &K) -> usize {
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        (hash.finish() as usize) % INDEX_BUCKETS
    }

    fn get(&self, key: &K) -> Option<&Arc<V>> {
        self.buckets[Self::bucket(key)].get(key)
    }

    fn insert(&mut self, key: Arc<K>, value: V) {
        Arc::make_mut(&mut self.buckets[Self::bucket(&key)]).insert(key, Arc::new(value));
    }

    fn remove(&mut self, key: &K) -> Option<Arc<V>> {
        Arc::make_mut(&mut self.buckets[Self::bucket(key)]).remove(key)
    }

    fn update(&mut self, key: Arc<K>, operation: impl FnOnce(&mut V))
    where
        V: Default,
    {
        let map = Arc::make_mut(&mut self.buckets[Self::bucket(&key)]);
        let value = map.entry(key).or_insert_with(|| Arc::new(V::default()));
        operation(Arc::make_mut(value));
    }

    fn keys(&self) -> impl Iterator<Item = &Arc<K>> {
        self.buckets.iter().flat_map(|bucket| bucket.keys())
    }

    fn retain_value(&mut self, key: &K, operation: impl FnOnce(&mut V) -> bool) {
        let map = Arc::make_mut(&mut self.buckets[Self::bucket(key)]);
        if let Some(value) = map.get_mut(key) {
            // 同一候选代次只在第一次修改共享集合时复制，后续直接维护独占集合。
            if !operation(Arc::make_mut(value)) {
                map.remove(key);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct StableCheckRule {
    pub(super) target: StableCheckTarget,
    pub(super) rule_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct StableFieldKey {
    pub(super) record: RecordCoordinate,
    pub(super) field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct StableFieldRead {
    pub(super) key: StableFieldKey,
    pub(super) projection: CheckReadProjection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct StoredRuleReads {
    pub(super) fields: BTreeSet<StableFieldRead>,
}

#[derive(Debug, Clone)]
pub(super) struct CheckReadGraph {
    rules: SharedIndex<StableCheckRule, StoredRuleReads>,
    readers: SharedIndex<StableFieldKey, BTreeSet<Arc<StableCheckRule>>>,
    hosts: SharedIndex<RecordCoordinate, BTreeSet<Arc<StableCheckRule>>>,
    pub(super) tracked: bool,
}

impl Default for CheckReadGraph {
    fn default() -> Self {
        Self {
            rules: SharedIndex::default(),
            readers: SharedIndex::default(),
            hosts: SharedIndex::default(),
            tracked: true,
        }
    }
}

impl CheckReadGraph {
    pub(super) fn replace_rule(&mut self, rule: StableCheckRule, reads: StoredRuleReads) {
        let previous = self.rules.get(&rule).cloned();
        if previous.as_deref() == Some(&reads) {
            return;
        }
        // 求值结果的读集通常不变；只更新新增或消失的字段关联，投影变化留在规则读集中。
        let old_fields: BTreeSet<_> = previous.iter()
            .flat_map(|reads| reads.fields.iter().map(|read| &read.key)).collect();
        let new_fields: BTreeSet<_> = reads.fields.iter().map(|read| &read.key).collect();
        for field in old_fields.difference(&new_fields) {
            self.readers.retain_value(field, |readers| {
                readers.remove(&rule);
                !readers.is_empty()
            });
        }
        let rule = Arc::new(rule);
        for field in new_fields.difference(&old_fields) {
            self.readers.update(Arc::new((*field).clone()), |readers| {
                readers.insert(Arc::clone(&rule));
            });
        }
        if let Some(record) = host(&rule.target).filter(|_| previous.is_none()) {
            self.hosts.update(Arc::new(record.clone()), |rules| {
                rules.insert(Arc::clone(&rule));
            });
        }
        self.rules.insert(rule, reads);
    }

    fn remove_rule(&mut self, rule: &StableCheckRule) {
        let Some(previous) = self.rules.remove(rule) else {
            return;
        };
        for field in previous
            .fields
            .iter()
            .map(|read| &read.key)
            .collect::<BTreeSet<_>>()
        {
            self.readers.retain_value(field, |readers| {
                readers.remove(rule);
                !readers.is_empty()
            });
        }
        if let Some(record) = host(&rule.target) {
            self.hosts.retain_value(record, |rules| {
                rules.remove(rule);
                !rules.is_empty()
            });
        }
    }

    pub(super) fn affected(&self, impact: &CheckImpact) -> BTreeSet<Arc<StableCheckRule>> {
        let mut affected = BTreeSet::new();
        for (record, changes) in &impact.records {
            let ChangedRecordFields::Fields(fields) = changes else {
                continue;
            };
            for changed in fields {
                let key = StableFieldKey {
                    record: record.clone(),
                    field: changed.field.to_string(),
                };
                let Some(readers) = self.readers.get(&key) else {
                    continue;
                };
                for rule in readers.iter() {
                    if self.rules.get(rule).is_some_and(|reads| {
                        // 只查当前字段的有序投影区间，避免全局规则对每个变化字段重扫完整读集。
                        reads.fields.range(StableFieldRead { key: key.clone(), projection: CheckReadProjection::Base }
                            ..=StableFieldRead { key: key.clone(), projection: CheckReadProjection::AllVariants })
                            .any(|read| projection_changed(&read.projection, &changed.projection))
                    }) {
                        affected.insert(Arc::clone(rule));
                    }
                }
            }
        }
        affected
    }

    pub(super) fn remove_changed_data(&mut self, impact: &CheckImpact) {
        let mut removed = BTreeSet::new();
        for (record, changes) in &impact.records {
            let Some(rules) = self.hosts.get(record) else {
                continue;
            };
            for rule in rules.iter() {
                let StableCheckTarget::Data {
                    path, dimension, ..
                } = &rule.target
                else {
                    continue;
                };
                let owned = dimension
                    .as_ref()
                    .map(|(field, variant)| (field.as_str(), Some(variant.as_str())))
                    .or_else(|| match path.segments.first() {
                        Some(coflow_model::CfdPathSegment::Field(field)) => {
                            Some((field.as_str(), None))
                        }
                        _ => None,
                    });
                if owned.is_some_and(|(field, variant)| changes.affects_owned_field(field, variant))
                {
                    removed.insert(Arc::clone(rule));
                }
            }
        }
        for rule in removed {
            self.remove_rule(&rule);
        }
    }

    pub(super) fn remove_missing_records(&mut self, model: &coflow_model::CfdDataModel) {
        let missing: Vec<_> = self
            .hosts
            .keys()
            .filter(|record| {
                model
                    .record_by_type_id(&record.actual_type, &record.id)
                    .is_none()
            })
            .cloned()
            .collect();
        for record in missing {
            let rules = self.hosts.get(&record).cloned();
            if let Some(rules) = rules {
                for rule in rules.iter() {
                    self.remove_rule(rule);
                }
            }
        }
    }
}

fn host(target: &StableCheckTarget) -> Option<&RecordCoordinate> {
    match target {
        StableCheckTarget::Record(record) | StableCheckTarget::Data { record, .. } => Some(record),
        StableCheckTarget::Project => None,
    }
}

fn projection_changed(read: &CheckReadProjection, changed: &ChangedProjection) -> bool {
    match changed {
        ChangedProjection::Base => true,
        ChangedProjection::Dimension { variant, .. } => match read {
            CheckReadProjection::Base => false,
            CheckReadProjection::Variant(read) => read == variant.as_str(),
            CheckReadProjection::AllVariants => true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::impact::ChangedField;

    #[test]
    fn read_replacement_preserves_snapshots_and_projection_invalidation() {
        let record = RecordCoordinate::new("Item".try_into().unwrap(), "a".try_into().unwrap());
        let rule = StableCheckRule { target: StableCheckTarget::Record(record.clone()), rule_id: "rule".into() };
        let key = StableFieldKey { record: record.clone(), field: "value".into() };
        let reads = |projection| StoredRuleReads { fields: BTreeSet::from([StableFieldRead { key: key.clone(), projection }]) };
        let impact = CheckImpact {
            records: BTreeMap::from([(record.clone(), ChangedRecordFields::Fields(BTreeSet::from([ChangedField {
                field: "value".try_into().unwrap(), projection: ChangedProjection::Dimension {
                    dimension: "locale".try_into().unwrap(), variant: "en".try_into().unwrap(),
                },
            }])))]), ..Default::default()
        };
        let mut graph = CheckReadGraph::default();
        graph.replace_rule(rule.clone(), reads(CheckReadProjection::Base));
        let previous = graph.clone();
        graph.replace_rule(rule.clone(), reads(CheckReadProjection::Base));
        assert!(Arc::ptr_eq(graph.rules.get(&rule).unwrap(), previous.rules.get(&rule).unwrap()));
        graph.replace_rule(rule.clone(), reads(CheckReadProjection::Variant("en".into())));
        assert!(previous.affected(&impact).is_empty());
        assert_eq!(graph.affected(&impact).len(), 1);
        assert!(Arc::ptr_eq(graph.readers.get(&key).unwrap(), previous.readers.get(&key).unwrap()));
        let projected = graph.clone();
        graph.replace_rule(rule.clone(), StoredRuleReads::default());
        assert!(graph.affected(&impact).is_empty());
        assert_eq!(projected.affected(&impact).len(), 1);
        graph.replace_rule(rule.clone(), reads(CheckReadProjection::AllVariants));
        assert_eq!(graph.affected(&impact).len(), 1);
        graph.remove_rule(&rule);
        assert!(graph.hosts.get(&record).is_none());
        assert!(graph.readers.get(&key).is_none());
        assert_eq!(projected.affected(&impact).len(), 1);
    }
}
