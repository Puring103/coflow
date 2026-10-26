//! 发布结果描述实际写入内容，宿主无需再次读盘来猜测内部版本。
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedFile {
    pub path: PathBuf,
    /// None 表示已删除；摘要始终来自发布的字节，而不是发布后的磁盘。
    pub digest: Option<[u8; 32]>,
}

impl PublishedFile {
    #[must_use]
    pub fn from_content(path: PathBuf, content: Option<&[u8]>) -> Self {
        Self {
            path,
            digest: content.map(|bytes| Sha256::digest(bytes).into()),
        }
    }
}

// 发布边界集中维护数据写入与代码生成的持久化结果。
pub(crate) mod artifacts;
pub(crate) mod operation;
