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

#[derive(Debug, Clone, Default)]
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
        self.remove_rule(&rule);
        let rule = Arc::new(rule);
        for field in reads
            .fields
            .iter()
            .map(|read| &read.key)
            .collect::<BTreeSet<_>>()
        {
            self.readers.update(Arc::new(field.clone()), |readers| {
                readers.insert(Arc::clone(&rule));
            });
        }
        if let Some(record) = host(&rule.target) {
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
            let empty = if let Some(readers) = self.readers.get(field) {
                let mut readers = (**readers).clone();
                readers.remove(rule);
                if readers.is_empty() {
                    true
                } else {
                    self.readers.insert(Arc::new(field.clone()), readers);
                    false
                }
            } else {
                false
            };
            if empty {
                self.readers.remove(field);
            }
        }
        if let Some(record) = host(&rule.target) {
            if let Some(rules) = self.hosts.get(record) {
                let mut rules = (**rules).clone();
                rules.remove(rule);
                if rules.is_empty() {
                    self.hosts.remove(record);
                } else {
                    self.hosts.insert(Arc::new(record.clone()), rules);
                }
            }
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
                        reads.fields.iter().any(|read| {
                            read.key == key
                                && projection_changed(&read.projection, &changed.projection)
                        })
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
