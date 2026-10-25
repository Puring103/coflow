//! 不可变代次之间共享页，写入只复制命中的页和轻量页目录。
use std::ops::{Index, IndexMut};
use std::sync::Arc;
const PAGE_SIZE: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PagedVec<T> {
    pages: Vec<Arc<Vec<T>>>,
    len: usize,
}
impl<T> Default for PagedVec<T> {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            len: 0,
        }
    }
}
impl<T> PagedVec<T> {
    pub const fn len(&self) -> usize {
        self.len
    }
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        self.pages.get(index / PAGE_SIZE)?.get(index % PAGE_SIZE)
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.pages.iter().flat_map(|page| page.iter())
    }
}
impl<T: Clone> PagedVec<T> {
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index >= self.len {
            return None;
        }
        Arc::make_mut(&mut self.pages[index / PAGE_SIZE]).get_mut(index % PAGE_SIZE)
    }
    /// 仅对确实需要修改全部元素的操作使用；只读扫描应调用 iter。
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.pages
            .iter_mut()
            .flat_map(|page| Arc::make_mut(page).iter_mut())
    }
    pub fn push(&mut self, value: T) {
        if self.len % PAGE_SIZE == 0 {
            self.pages.push(Arc::new(Vec::with_capacity(PAGE_SIZE)));
        }
        Arc::make_mut(&mut self.pages[self.len / PAGE_SIZE]).push(value);
        self.len += 1;
    }
    pub fn resize_with(&mut self, new_len: usize, mut create: impl FnMut() -> T) {
        if new_len < self.len {
            self.pages.truncate(new_len.div_ceil(PAGE_SIZE));
            if new_len % PAGE_SIZE != 0 {
                if let Some(last) = self.pages.last_mut() {
                    Arc::make_mut(last).truncate(new_len % PAGE_SIZE);
                }
            }
            self.len = new_len;
        }
        while self.len < new_len {
            self.push(create());
        }
    }
}
impl<T> Index<usize> for PagedVec<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        &self.pages[index / PAGE_SIZE][index % PAGE_SIZE]
    }
}
impl<T: Clone> IndexMut<usize> for PagedVec<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut Arc::make_mut(&mut self.pages[index / PAGE_SIZE])[index % PAGE_SIZE]
    }
}
impl<T: Clone> FromIterator<T> for PagedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        let mut result = Self::default();
        for item in items {
            result.push(item);
        }
        result
    }
}
impl<T: Clone> From<Vec<T>> for PagedVec<T> {
    fn from(items: Vec<T>) -> Self {
        items.into_iter().collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_updates_copy_only_touched_pages_and_preserve_snapshot() {
        let old: PagedVec<_> = (0..1600).collect();
        let mut next = old.clone();
        next[512] = 99;
        assert_eq!(old[512], 512);
        assert_eq!(next[512], 99);
        assert!(Arc::ptr_eq(&old.pages[0], &next.pages[0]));
        assert!(!Arc::ptr_eq(&old.pages[1], &next.pages[1]));
        assert!(Arc::ptr_eq(&old.pages[2], &next.pages[2]));
        next.resize_with(511, || 0);
        next.resize_with(1025, || 7);
        assert_eq!(
            next.iter().copied().collect::<Vec<_>>(),
            (0..511)
                .chain(std::iter::repeat_n(7, 514))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            old.iter().copied().collect::<Vec<_>>(),
            (0..1600).collect::<Vec<_>>()
        );
    }
}
