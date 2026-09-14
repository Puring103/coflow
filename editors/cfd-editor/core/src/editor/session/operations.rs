//! Record queries and mutation commands for loaded editor sessions.
//!
//! 按职责拆分为三个子模块（仅结构拆分，不改行为）：
//! - `language`：LSP 文档同步、补全/格式化、源码校验与落盘；
//! - `data_queries`：文件行快照、搜索、插件投影、图查询等读操作；
//! - `mutations`：字段写回、集合编辑、记录增删改与排序。
//! 本文件只做模块声明，不再经 `super::*` 通配导入。

mod data_queries;
mod language;
mod mutations;
