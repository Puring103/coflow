use super::RecordInput;
use crate::LoadedRecordDraft;
use std::ops::Index;

/// 批次仅保存边界。单条添加集中在连续段中，避免每条记录单独分配容器。
#[derive(Debug)]
enum Chunk<'a> {
    Individual(Vec<RecordInput<'a>>),
    Batch(&'a [LoadedRecordDraft]),
}

impl Chunk<'_> {
    fn get(&self, index: usize) -> Option<&LoadedRecordDraft> {
        match self {
            Self::Individual(records) => records.get(index).map(|record| &**record),
            Self::Batch(records) => records.get(index),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Individual(records) => records.len(),
            Self::Batch(records) => records.len(),
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
            *self.ends.last_mut().expect("chunk boundary") = end;
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

    pub fn get(&self, index: usize) -> Option<&LoadedRecordDraft> {
        let chunk = self.ends.partition_point(|end| *end <= index);
        let start = chunk.checked_sub(1).map_or(0, |previous| self.ends[previous]);
        self.chunks.get(chunk)?.get(index - start)
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoadedRecordDraft> {
        self.chunks.iter().flat_map(|chunk| (0..chunk.len()).map(|index| {
            // 顺序迭代直接访问当前批次，不能逐记录二分查找批次边界。
            chunk.get(index).expect("index within chunk")
        }))
    }

    pub fn into_iter(self) -> impl Iterator<Item = RecordInput<'a>> {
        // 完整构建逐条释放拥有态草稿，不能为批次访问而延长整批拥有值树的生命周期。
        self.chunks.into_iter().flat_map(|chunk| match chunk {
            Chunk::Individual(records) => ChunkIntoIter::Individual(records.into_iter()),
            Chunk::Batch(records) => ChunkIntoIter::Batch(records.iter()),
        })
    }
}

impl Index<usize> for RecordInputs<'_> {
    type Output = LoadedRecordDraft;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index).expect("record input index out of bounds")
    }
}
