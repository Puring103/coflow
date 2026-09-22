//! 插入顺序与查询索引分离：稠密整数和 bool 直接寻址，其他键使用借用哈希查询。
use super::{ScalarKey, ValueId};
use hashbrown::HashTable;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

type Pair = (ValueId, ValueId);
type Entry = (ScalarKey, Pair);
const MISSING: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyRef<'a> {
    Bool(bool),
    Int(i32),
    String(&'a str),
    Enum { type_name: &'a str, value: u32 },
}
impl<'a> From<&'a ScalarKey> for KeyRef<'a> {
    fn from(key: &'a ScalarKey) -> Self {
        match key {
            ScalarKey::Bool(v) => Self::Bool(*v),
            ScalarKey::Int(v) => Self::Int(*v),
            ScalarKey::String(v) => Self::String(v),
            ScalarKey::Enum { type_name, value } => Self::Enum {
                type_name,
                value: *value,
            },
        }
    }
}
impl KeyRef<'_> {
    // 专用键只散列有效载荷，碰撞时仍完整比较类型和值；保留随机种子防止恶意碰撞。
    fn hash(self, state: &RandomState) -> u64 {
        match self {
            Self::Bool(v) => state.hash_one(v),
            Self::Int(v) => state.hash_one(v),
            Self::String(v) => state.hash_one(v),
            Self::Enum { type_name, value } => state.hash_one((type_name, value)),
        }
    }
}

#[derive(Debug, Clone)]
enum Index {
    Hash(HashTable<usize>),
    Enum(HashTable<usize>),
    Bool([u32; 2]),
    DenseInt { base: i32, positions: Vec<u32> },
}

/// 顺序条目是唯一的键所有者；索引仅保存条目位置，不复制字符串或公开身份。
#[derive(Debug)]
pub struct DictionaryValue {
    entries: Vec<Entry>,
    index: Index,
    hasher: RandomState,
    key_bytes: usize,
}
impl Clone for DictionaryValue {
    fn clone(&self) -> Self {
        let entries = self.entries.clone();
        let key_bytes = entries.iter().map(|(key, _)| Self::key_bytes(key)).sum();
        Self { entries, index: self.index.clone(), hasher: self.hasher.clone(), key_bytes }
    }
}
impl Default for DictionaryValue {
    fn default() -> Self {
        Self::new()
    }
}
impl DictionaryValue {
    fn key_bytes(key: &ScalarKey) -> usize {
        match key { ScalarKey::String(text) => text.capacity(), ScalarKey::Enum { type_name, .. } => type_name.capacity(), _ => 0 }
    }
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: Index::Hash(HashTable::new()),
            hasher: RandomState::new(),
            key_bytes: 0,
        }
    }
    pub fn with_capacity(capacity: usize) -> Self {
        let mut result = Self::new();
        result.try_reserve(capacity).expect("字典容量分配失败");
        result
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub(super) fn capacity(&self) -> usize {
        match &self.index {
            Index::Hash(table) => self.entries.capacity().min(table.capacity()),
            _ => self.entries.len(),
        }
    }
    pub(super) fn heap_bytes(&self) -> usize {
        self.entries.capacity() * size_of::<Entry>()
            + self.key_bytes
            + match &self.index {
                Index::Hash(table) | Index::Enum(table) => table.allocation_size(),
                Index::DenseInt { positions, .. } => positions.capacity() * size_of::<u32>(),
                Index::Bool(_) => 0,
            }
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), String> {
        let required = self.len().checked_add(additional).ok_or("字典容量溢出")?;
        if required > self.entries.capacity() {
            // 初次按请求大小预留，后续按倍数增长；逐项构造保持摊销线性搬移。
            let capacity = required.max(self.entries.capacity().saturating_mul(2));
            self.entries
                .try_reserve_exact(capacity - self.len())
                .map_err(|_| "字典条目分配失败")?;
        }
        if !matches!(self.index, Index::Hash(_)) {
            let mut table = HashTable::new();
            table
                .try_reserve(
                    self.len().saturating_add(additional),
                    |_: &usize| unreachable!(),
                )
                .map_err(|_| "字典索引分配失败")?;
            for (index, (key, _)) in self.entries.iter().enumerate() {
                table.insert_unique(KeyRef::from(key).hash(&self.hasher), index, |index| {
                    KeyRef::from(&self.entries[*index].0).hash(&self.hasher)
                });
            }
            self.index = Index::Hash(table);
        }
        let Index::Hash(table) = &mut self.index else {
            unreachable!()
        };
        table
            .try_reserve(additional, |index| {
                KeyRef::from(&self.entries[*index].0).hash(&self.hasher)
            })
            .map_err(|_| "字典索引分配失败".into())
    }
    fn position(&self, key: KeyRef<'_>) -> Option<usize> {
        let position = match (&self.index, key) {
            (Index::Bool(positions), KeyRef::Bool(key)) => *positions.get(usize::from(key))?,
            (Index::DenseInt { base, positions }, KeyRef::Int(key)) => {
                let offset = usize::try_from(i64::from(key) - i64::from(*base)).ok()?;
                *positions.get(offset)?
            }
            (Index::Enum(table), KeyRef::Enum { type_name, value }) => {
                let ScalarKey::Enum {
                    type_name: expected,
                    ..
                } = &self.entries.first()?.0
                else {
                    return None;
                };
                if type_name != expected {
                    return None;
                }
                return table.find(self.hasher.hash_one(value), |index|
                    matches!(&self.entries[*index].0, ScalarKey::Enum { value: stored, .. } if *stored == value)).copied();
            }
            (Index::Hash(table), key) => {
                return table
                    .find(key.hash(&self.hasher), |index| {
                        KeyRef::from(&self.entries[*index].0) == key
                    })
                    .copied();
            }
            _ => return None,
        };
        (position != MISSING).then_some(position as usize)
    }
    pub(super) fn get_ref(&self, key: KeyRef<'_>) -> Option<&Pair> {
        self.entries
            .get(self.position(key)?)
            .map(|(_, value)| value)
    }
    pub fn get(&self, key: &ScalarKey) -> Option<&Pair> {
        self.get_ref(key.into())
    }
    pub fn contains_key(&self, key: &ScalarKey) -> bool {
        self.get(key).is_some()
    }
    pub fn get_index(&self, index: usize) -> Option<(&ScalarKey, &Pair)> {
        self.entries.get(index).map(|(key, value)| (key, value))
    }
    pub fn insert(&mut self, key: ScalarKey, value: Pair) -> Option<Pair> {
        if let Some(index) = self.position((&key).into()) {
            return Some(std::mem::replace(&mut self.entries[index].1, value));
        }
        if self.len() == self.capacity() {
            self.try_reserve(1).expect("字典预留失败");
        }
        let hash = KeyRef::from(&key).hash(&self.hasher);
        let index = self.entries.len();
        self.key_bytes += Self::key_bytes(&key);
        self.entries.push((key, value));
        let Index::Hash(table) = &mut self.index else {
            unreachable!()
        };
        table.insert_unique(hash, index, |index| {
            KeyRef::from(&self.entries[*index].0).hash(&self.hasher)
        });
        None
    }
    pub fn shift_remove(&mut self, key: &ScalarKey) -> Option<Pair> {
        let index = self.position(key.into())?;
        let (removed, value) = self.entries.remove(index);
        self.key_bytes -= Self::key_bytes(&removed);
        // 删除保持插入顺序；重建位置只复用已有索引容量，不分配内存。
        match &mut self.index {
            Index::Hash(table) => {
                table.clear();
                for (index, (key, _)) in self.entries.iter().enumerate() {
                    table.insert_unique(KeyRef::from(key).hash(&self.hasher), index, |index| {
                        KeyRef::from(&self.entries[*index].0).hash(&self.hasher)
                    });
                }
            }
            Index::Enum(table) => {
                table.clear();
                for (index, (key, _)) in self.entries.iter().enumerate() {
                    let ScalarKey::Enum { value, .. } = key else {
                        unreachable!()
                    };
                    table.insert_unique(self.hasher.hash_one(value), index, |index| {
                        let ScalarKey::Enum { value, .. } = &self.entries[*index].0 else {
                            unreachable!()
                        };
                        self.hasher.hash_one(value)
                    });
                }
            }
            Index::Bool(positions) => {
                positions.fill(MISSING);
                for (index, (key, _)) in self.entries.iter().enumerate() {
                    let ScalarKey::Bool(key) = key else {
                        unreachable!()
                    };
                    positions[usize::from(*key)] = index as u32;
                }
            }
            Index::DenseInt { base, positions } => {
                positions.fill(MISSING);
                for (index, (key, _)) in self.entries.iter().enumerate() {
                    let ScalarKey::Int(key) = key else {
                        unreachable!()
                    };
                    positions[(i64::from(*key) - i64::from(*base)) as usize] = index as u32;
                }
            }
        }
        Some(value)
    }
    /// 冻结时按实际分布选索引；稠密索引峰值分配受调用方剩余预算约束。
    pub(super) fn optimize_index(&mut self, spare_bytes: usize) -> Result<(), String> {
        if self.is_empty() || !matches!(self.index, Index::Hash(_)) {
            return Ok(());
        }
        if self
            .entries
            .iter()
            .all(|(key, _)| matches!(key, ScalarKey::Bool(_)))
        {
            let mut positions = [MISSING; 2];
            for (i, (key, _)) in self.entries.iter().enumerate() {
                let ScalarKey::Bool(key) = key else {
                    unreachable!()
                };
                positions[usize::from(*key)] = i as u32;
            }
            self.index = Index::Bool(positions);
            return Ok(());
        }
        if let ScalarKey::Enum { type_name, .. } = &self.entries[0].0 {
            if self.entries.iter().all(|(key, _)| matches!(key, ScalarKey::Enum { type_name: other, .. } if other == type_name)) {
                // 同一枚举域只散列数值；域名借用首条键并在查询入口校验。
                let Index::Hash(mut table) = std::mem::replace(&mut self.index, Index::Hash(HashTable::new())) else { unreachable!() };
                table.clear();
                for (index, (key, _)) in self.entries.iter().enumerate() {
                    let ScalarKey::Enum { value, .. } = key else { unreachable!() };
                    table.insert_unique(self.hasher.hash_one(value), index, |index| {
                        let ScalarKey::Enum { value, .. } = &self.entries[*index].0 else { unreachable!() };
                        self.hasher.hash_one(value)
                    });
                }
                self.index = Index::Enum(table);
            }
            return Ok(());
        }
        let (mut min, mut max) = (i32::MAX, i32::MIN);
        for (key, _) in &self.entries {
            let ScalarKey::Int(key) = key else {
                return Ok(());
            };
            min = min.min(*key);
            max = max.max(*key);
        }
        let range = (i64::from(max) - i64::from(min) + 1) as u64;
        if range > (self.len() as u64).saturating_mul(2)
            || range > 1_000_000
            || range.saturating_mul(4) > spare_bytes as u64
        {
            return Ok(());
        }
        let mut positions = Vec::new();
        positions
            .try_reserve_exact(range as usize)
            .map_err(|_| "稠密字典索引分配失败")?;
        positions.resize(range as usize, MISSING);
        for (i, (key, _)) in self.entries.iter().enumerate() {
            let ScalarKey::Int(key) = key else {
                unreachable!()
            };
            positions[(i64::from(*key) - i64::from(min)) as usize] = i as u32;
        }
        self.index = Index::DenseInt {
            base: min,
            positions,
        };
        Ok(())
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&ScalarKey, &Pair)> + DoubleEndedIterator {
        self.entries.iter().map(|(key, value)| (key, value))
    }
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &ScalarKey> {
        self.entries.iter().map(|(key, _)| key)
    }
    pub fn values(&self) -> impl ExactSizeIterator<Item = &Pair> + DoubleEndedIterator {
        self.entries.iter().map(|(_, value)| value)
    }
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Pair> {
        self.entries.iter_mut().map(|(_, value)| value)
    }
}
impl<'a> IntoIterator for &'a DictionaryValue {
    type Item = (&'a ScalarKey, &'a Pair);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, Entry>, fn(&Entry) -> (&ScalarKey, &Pair)>;
    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(key, value)| (key, value))
    }
}
impl<const N: usize> From<[Entry; N]> for DictionaryValue {
    fn from(entries: [Entry; N]) -> Self {
        entries.into_iter().collect()
    }
}
impl FromIterator<Entry> for DictionaryValue {
    fn from_iter<T: IntoIterator<Item = Entry>>(entries: T) -> Self {
        let entries = entries.into_iter();
        let mut result = Self::with_capacity(entries.size_hint().0);
        for (key, value) in entries {
            result.insert(key, value);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_negative_keys_preserve_holes_order_updates_and_removal() {
        let mut values = DictionaryValue::from([
            (ScalarKey::Int(-1), (1, 10)),
            (ScalarKey::Int(-3), (3, 30)),
            (ScalarKey::Int(0), (0, 0)),
        ]);
        values.optimize_index(0).unwrap();
        assert!(matches!(values.index, Index::Hash(_)));
        values.optimize_index(usize::MAX).unwrap();
        assert!(matches!(values.index, Index::DenseInt { .. }));
        assert_eq!(values.get(&ScalarKey::Int(-2)), None);
        assert_eq!(values.get(&ScalarKey::Int(i32::MIN)), None);
        assert_eq!(values.insert(ScalarKey::Int(-1), (1, 11)), Some((1, 10)));
        assert_eq!(values.shift_remove(&ScalarKey::Int(-3)), Some((3, 30)));
        values.try_reserve(1).unwrap();
        values.insert(ScalarKey::Int(i32::MAX), (4, 40));
        values.optimize_index(usize::MAX).unwrap();
        assert!(matches!(values.index, Index::Hash(_)));
        assert_eq!(
            values.values().copied().collect::<Vec<_>>(),
            [(1, 11), (0, 0), (4, 40)]
        );
    }

    #[test]
    fn borrowed_string_enum_and_bool_keys_keep_type_identity() {
        let mut values = DictionaryValue::from([
            (ScalarKey::String("中文".into()), (1, 2)),
            (
                ScalarKey::Enum {
                    type_name: "Kind".into(),
                    value: 3,
                },
                (3, 4),
            ),
            (ScalarKey::Int(3), (5, 6)),
        ]);
        values.optimize_index(usize::MAX).unwrap();
        assert_eq!(values.get_ref(KeyRef::String("中文")), Some(&(1, 2)));
        assert_eq!(
            values.get_ref(KeyRef::Enum {
                type_name: "Kind",
                value: 3
            }),
            Some(&(3, 4))
        );
        assert_eq!(
            values.get_ref(KeyRef::Enum {
                type_name: "Other",
                value: 3
            }),
            None
        );
        assert_eq!(values.get_ref(KeyRef::Int(3)), Some(&(5, 6)));
        let mut booleans = DictionaryValue::from([
            (ScalarKey::Bool(true), (1, 2)),
            (ScalarKey::Bool(false), (3, 4)),
        ]);
        booleans.optimize_index(usize::MAX).unwrap();
        assert!(matches!(booleans.index, Index::Bool(_)));
        assert_eq!(booleans.get_ref(KeyRef::Int(1)), None);
        booleans.shift_remove(&ScalarKey::Bool(true));
        assert_eq!(booleans.get_ref(KeyRef::Bool(false)), Some(&(3, 4)));
    }

    #[test]
    fn enum_index_checks_domain_after_deletion_and_copy() {
        let key = |name: &str, value| ScalarKey::Enum {
            type_name: name.into(),
            value,
        };
        let mut values =
            DictionaryValue::from([(key("Kind", 2), (2, 20)), (key("Kind", 9), (9, 90))]);
        values.optimize_index(0).unwrap();
        assert!(matches!(values.index, Index::Enum(_)));
        assert_eq!(
            values.get_ref(KeyRef::Enum {
                type_name: "Kind",
                value: 9
            }),
            Some(&(9, 90))
        );
        assert_eq!(
            values.get_ref(KeyRef::Enum {
                type_name: "Other",
                value: 9
            }),
            None
        );
        values.shift_remove(&key("Kind", 2));
        let mut copy = values.clone();
        copy.try_reserve(1).unwrap();
        copy.insert(key("Other", 9), (99, 99));
        assert_eq!(copy.get(&key("Kind", 9)), Some(&(9, 90)));
        assert_eq!(copy.get(&key("Other", 9)), Some(&(99, 99)));
        assert_eq!(values.len(), 1);
    }

    #[test]
    fn index_transitions_match_ordered_dictionary_under_mutation() {
        let mut actual = DictionaryValue::new();
        let mut expected = indexmap::IndexMap::new();
        let mut random = 42_u32;
        for step in 0..2000 {
            random = random.wrapping_mul(1664525).wrapping_add(1013904223);
            let key = ScalarKey::Int((random % 31) as i32 - 15);
            if random & 3 == 0 {
                assert_eq!(actual.shift_remove(&key), expected.shift_remove(&key));
            } else {
                actual.try_reserve(1).unwrap();
                assert_eq!(
                    actual.insert(key.clone(), (step, step + 1)),
                    expected.insert(key, (step, step + 1))
                );
            }
            if step % 11 == 0 {
                actual.optimize_index(usize::MAX).unwrap();
            }
            assert!(actual.iter().eq(expected.iter()));
            for key in -16..=16 {
                assert_eq!(
                    actual.get(&ScalarKey::Int(key)),
                    expected.get(&ScalarKey::Int(key))
                );
            }
        }
    }
}
