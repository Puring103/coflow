//! 文件快照传输：注解按内容去重为拓扑有序的描述表，记录只携带描述索引。
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use super::records::{FieldAnnotation, FileRecords};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FieldAnnotationDescriptor {
    /// 仅包含本节点元数据；嵌套形状统一通过描述索引共享。
    pub metadata: FieldAnnotation,
    pub item: Option<usize>,
    pub key: Option<usize>,
    pub children: BTreeMap<String, usize>,
}

#[derive(Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct FileRecordsTransfer {
    /// snapshot 内字段的 annotation 已提取；恢复过程由宿主的接收边界负责。
    pub snapshot: FileRecords,
    pub annotations: Vec<FieldAnnotationDescriptor>,
    /// 行和字段索引与 snapshot 完全一致，不依赖字段名称或记录排序猜测。
    pub annotation_ids: Vec<Vec<Option<usize>>>,
}

#[derive(Default)]
struct AnnotationTable {
    descriptors: Vec<FieldAnnotationDescriptor>,
    index: HashMap<FieldAnnotationDescriptor, usize>,
    shared_index: HashMap<usize, usize>,
    retained: Vec<Arc<FieldAnnotation>>,
}
impl AnnotationTable {
    fn intern_shared(&mut self, annotation: Arc<FieldAnnotation>) -> usize {
        let key = Arc::as_ptr(&annotation) as usize;
        if let Some(index) = self.shared_index.get(&key) {
            return *index;
        }
        let index = self.intern((*annotation).clone());
        self.shared_index.insert(key, index);
        // 保留句柄至编码结束，防止分配器复用地址而将不同描述误判为同一项。
        self.retained.push(annotation);
        index
    }
    fn intern(&mut self, mut annotation: FieldAnnotation) -> usize {
        let item = annotation
            .item_annotation
            .take()
            .map(|item| self.intern(*item));
        let key = annotation
            .key_annotation
            .take()
            .map(|key| self.intern(*key));
        let children = std::mem::take(&mut annotation.children)
            .into_iter()
            .map(|(name, child)| (name, self.intern(child)))
            .collect();
        let descriptor = FieldAnnotationDescriptor {
            metadata: annotation,
            item,
            key,
            children,
        };
        // 内容索引只保存当前节点与子描述 ID，深层结构也保持线性存储。
        if let Some(index) = self.index.get(&descriptor) {
            return *index;
        }
        let index = self.descriptors.len();
        self.index.insert(descriptor.clone(), index);
        self.descriptors.push(descriptor);
        index
    }
}
impl From<FileRecords> for FileRecordsTransfer {
    fn from(mut snapshot: FileRecords) -> Self {
        let mut table = AnnotationTable::default();
        let annotation_ids = snapshot
            .records
            .iter_mut()
            .map(|row| {
                row.fields
                    .iter_mut()
                    .map(|field| {
                        field
                            .annotation
                            .take()
                            .map(|annotation| table.intern_shared(annotation))
                    })
                    .collect()
            })
            .collect();
        Self {
            snapshot,
            annotations: table.descriptors,
            annotation_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_share_subtrees_and_keep_enum_instance_values() {
        let first = FieldAnnotation {
            enum_int_value: Some(i64::MAX),
            ..FieldAnnotation::default()
        };
        let second = FieldAnnotation {
            enum_int_value: Some(i64::MIN),
            ..FieldAnnotation::default()
        };
        let parent = Arc::new(FieldAnnotation {
            item_annotation: Some(Box::new(first.clone())),
            children: BTreeMap::from([("0".into(), first), ("1".into(), second)]),
            ..FieldAnnotation::default()
        });
        let mut table = AnnotationTable::default();
        let root = table.intern_shared(parent.clone());
        assert_eq!(table.intern_shared(parent), root);
        assert_eq!(table.descriptors.len(), 3);
        let descriptor = &table.descriptors[root];
        assert_eq!(descriptor.item, descriptor.children.get("0").copied());
        assert!(descriptor.children.values().all(|child| *child < root));
        assert_eq!(
            table.descriptors[descriptor.children["0"]]
                .metadata
                .enum_int_value,
            Some(i64::MAX)
        );
        assert_eq!(
            table.descriptors[descriptor.children["1"]]
                .metadata
                .enum_int_value,
            Some(i64::MIN)
        );
    }
}
