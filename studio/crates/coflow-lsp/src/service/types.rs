//! 编辑器与语言服务共用的类型化结果；协议适配负责 LSP 字段编码。
use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguagePosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageRange {
    pub start: LanguagePosition,
    pub end: LanguagePosition,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageTextEdit {
    pub range: LanguageRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageFormattingResult {
    pub text: String,
    pub edits: Vec<LanguageTextEdit>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageDiagnostic {
    pub range: LanguageRange,
    pub severity: u8,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub related_information: Option<Vec<RelatedInformation>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageDocumentState {
    pub diagnostics: Vec<LanguageDiagnostic>,
    pub semantic_token_data: Vec<u32>,
    pub semantic_token_types: Vec<String>,
    pub syntax_valid: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct LanguageCompletion {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub kind: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub insert_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub insert_text_format: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub documentation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub sort_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub filter_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub text_edit: Option<LanguageTextEdit>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct FunctionDocumentState {
    pub source: String,
    pub signature: String,
    pub body: String,
    pub body_range: LanguageRange,
    pub diagnostics: Vec<LanguageDiagnostic>,
    pub semantic_token_data: Vec<u32>,
    pub semantic_token_types: Vec<String>,
    pub completions: Vec<LanguageCompletion>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct Location {
    pub uri: String,
    pub range: LanguageRange,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS))]
pub struct RelatedInformation {
    pub location: Location,
    pub message: String,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Markup {
    pub kind: String,
    pub value: String,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hover {
    pub contents: Markup,
    pub range: LanguageRange,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub kind: u8,
    pub range: LanguageRange,
    pub selection_range: LanguageRange,
    pub children: Vec<DocumentSymbol>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticTokens {
    pub data: Vec<u32>,
    #[serde(rename = "x-coflow-syntax-valid")]
    pub syntax_valid: bool,
}
