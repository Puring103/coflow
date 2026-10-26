use super::RecordInput;
use crate::LoadedRecordDraft;
use std::ops::Index;
use std::ops::{Deref, Range};
use std::sync::Arc;

/// 不可变输入批次。身份与内容一同创建，不能在保留身份时替换内容。
/// 模型只持有轻量身份，不延长旧草稿值树的生命周期。
#[derive(Debug)]
pub struct CfdInputBatch<T> {
    values: Arc<[T]>,
    identity: Arc<()>,
}

impl<T> CfdInputBatch<T> {
    #[must_use]
    pub fn new(values: impl Into<Arc<[T]>>) -> Self {
        Self { values: values.into(), identity: Arc::new(()) }
    }

    #[must_use]
    pub fn same_generation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
    }

    pub(crate) fn identity(&self) -> Arc<()> { self.identity.clone() }
}

impl<T> Clone for CfdInputBatch<T> {
    fn clone(&self) -> Self { Self { values: self.values.clone(), identity: self.identity.clone() } }
}

impl<T> Default for CfdInputBatch<T> {
    fn default() -> Self { Self::new(Arc::<[T]>::from([])) }
}

impl<T> From<Vec<T>> for CfdInputBatch<T> {
    fn from(values: Vec<T>) -> Self { Self::new(values) }
}

impl<T> Deref for CfdInputBatch<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target { &self.values }
}

#[derive(Debug, Clone)]
pub(crate) struct SharedInputRange {
    pub range: Range<usize>,
    pub identity: Arc<()>,
}

/// 批次仅保存边界。单条添加集中在连续段中，避免每条记录单独分配容器。
#[derive(Debug)]
enum Chunk<'a> {
    Individual(Vec<RecordInput<'a>>),
    Batch(&'a [LoadedRecordDraft]),
    Shared(&'a CfdInputBatch<LoadedRecordDraft>),
}

impl Chunk<'_> {
    fn get(&self, index: usize) -> Option<&LoadedRecordDraft> {
        match self {
            Self::Individual(records) => records.get(index).map(|record| &**record),
            Self::Batch(records) => records.get(index),
            Self::Shared(records) => records.get(index),
        }
    }

}

enum ChunkIntoIter<'a> {
    Individual(std::vec::IntoIter<RecordInput<'a>>),
    Batch(std::slice::Iter<'a, LoadedRecordDraft>),
}

impl<'a> Iterator for ChunkIntoIter<'a> {
    type Item = RecordInput<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Individual(records) => records.next(),
            Self::Batch(records) => records.next().map(RecordInput::Borrowed),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct RecordInputs<'a> {
    chunks: Vec<Chunk<'a>>,
    ends: Vec<usize>,
}

impl<'a> RecordInputs<'a> {
    pub fn len(&self) -> usize {
        self.ends.last().copied().unwrap_or(0)
    }

    pub fn push(&mut self, record: RecordInput<'a>) {
        let end = self.len() + 1;
        if let Some(Chunk::Individual(records)) = self.chunks.last_mut() {
            records.push(record);
            let last = self.ends.len() - 1;
            self.ends[last] = end;
        } else {
            self.chunks.push(Chunk::Individual(vec![record]));
            self.ends.push(end);
        }
    }

    pub fn push_batch(&mut self, records: &'a [LoadedRecordDraft]) {
        if !records.is_empty() {
            self.ends.push(self.len() + records.len());
            self.chunks.push(Chunk::Batch(records));
        }
    }

    pub fn push_shared(&mut self, records: &'a CfdInputBatch<LoadedRecordDraft>) {
        if !records.is_empty() {
            self.ends.push(self.len() + records.len());
            self.chunks.push(Chunk::Shared(records));
        }
    }

    pub fn manifest(&self) -> Vec<SharedInputRange> {
        self.chunks.iter().enumerate().filter_map(|(index, chunk)| {
            let Chunk::Shared(batch) = chunk else { return None };
            Some(SharedInputRange { range: self.start(index)..self.ends[index], identity: batch.identity() })
        }).collect()
    }

    fn start(&self, index: usize) -> usize { index.checked_sub(1).map_or(0, |prior| self.ends[prior]) }

    /// 只扫描变化批次。旧代次保留身份 Arc，地址不会在缓存期被复用。
    pub fn changed_ranges(&self, previous: &[SharedInputRange]) -> Vec<Range<usize>> {
        let previous = previous.iter().map(|entry| (entry.range.start, entry)).collect::<std::collections::BTreeMap<_, _>>();
        self.chunks.iter().enumerate().filter_map(|(index, chunk)| {
            let range = self.start(index)..self.ends[index];
            if let Chunk::Shared(batch) = chunk {
                if previous.get(&range.start).is_some_and(|old| old.range == range && Arc::ptr_eq(&old.identity, &batch.identity)) {
                    return None;
                }
            }
            Some(range)
        }).collect()
    }

    pub fn get(&self, index: usize) -> Option<&LoadedRecordDraft> {
        let chunk = self.ends.partition_point(|end| *end <= index);
        let start = chunk
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous]);
        self.chunks.get(chunk)?.get(index - start)
    }

    pub fn into_iter(self) -> impl Iterator<Item = RecordInput<'a>> {
        // 完整构建逐条释放拥有态草稿，不能为批次访问而延长整批拥有值树的生命周期。
        self.chunks.into_iter().flat_map(|chunk| match chunk {
            Chunk::Individual(records) => ChunkIntoIter::Individual(records.into_iter()),
            Chunk::Batch(records) => ChunkIntoIter::Batch(records.iter()),
            Chunk::Shared(records) => ChunkIntoIter::Batch(records.iter()),
        })
    }
}

impl Index<usize> for RecordInputs<'_> {
    type Output = LoadedRecordDraft;

    fn index(&self, index: usize) -> &Self::Output {
        let chunk = self.ends.partition_point(|end| *end <= index);
        let start = chunk
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous]);
        match &self.chunks[chunk] {
            Chunk::Individual(records) => &records[index - start],
            Chunk::Batch(records) => &records[index - start],
            Chunk::Shared(records) => &records[index - start],
        }
    }
}
