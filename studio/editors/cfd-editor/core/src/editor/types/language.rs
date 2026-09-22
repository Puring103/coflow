//! 语言结果由共享服务定义，编辑器直接使用同一份类型。
pub use coflow_lsp::service::{
    FunctionDocumentState, LanguageCompletion, LanguageDiagnostic, LanguageDocumentState,
    LanguageFormattingResult, LanguagePosition, LanguageRange, LanguageTextEdit,
};
