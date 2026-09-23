//! Wire types serialized to the editor frontend.
//!
//! 按职责拆分子模块（仅结构拆分，不改行为）：
//! - `error`：`EditorError` 系列；
//! - `project`：引导快照/文件类型/插件投影/搜索；
//! - `language`：语言服务位置/诊断/补全/格式化；
//! - `settings`：图坐标/视图/分组/workspace；
//! - `records`：行/列/单元格/注解/mutation 出入参；
//! - `graph`：图谱节点/边/引用目标。

pub mod error;
pub mod graph;
pub mod language;
pub mod project;
pub mod records;
pub mod settings;

pub use error::{EditorError, EditorErrorKind};
pub use graph::{GraphData, GraphEdge, GraphNode, GraphQuery, RefTarget};
pub use language::{
    FunctionDocumentState, LanguageCompletion, LanguageDiagnostic, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, LanguageRange, LanguageTextEdit,
};
pub use project::{
    FileTypeOption, PluginSchemaField, PluginSchemaType, ProjectBootstrap, ProjectSearchHit,
    ProjectSearchMode, ProjectSearchResults,
};
pub use records::{
    BatchWriteFieldEditOutcome, BatchWriteFieldInput, BatchWriteFieldOutcome, CollectionEdit,
    CreateRecordDraft, CreateRecordFieldDraft, DeleteRecordOutcome, DeletedRecordSnapshot,
    DimensionFileRecords, DimensionFileRow, EnumVariantOption, FieldAnnotation, FieldCell,
    FieldDiagnostic, FileRecords, InsertRecordOutcome, RecordColumn, RecordRow,
    RenameRecordOutcome, ReorderRecordsOutcome, WriteDimensionValueOutcome, WriteFieldOutcome,
};
pub use records::{CreateFieldSource, CreateRequiredInput};
pub use settings::{
    EditorDimensionTarget, EditorProjectSettings, EditorRecordGroup, EditorWorkspaceState, EditorWorkspaceTab, ViewConfig,
    ViewKind, WorkspaceViewKind,
};
