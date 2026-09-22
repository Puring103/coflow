mod queries;
// 无传输依赖的进程内语言服务。文档覆盖和诊断快照只在此处持有。
mod types;
use crate::{LspPosition, LspValidationCore};
use coflow_project::{Project, SchemaCache};
use std::path::{Path, PathBuf};
pub use types::*;

pub struct LanguageService {
    core: LspValidationCore,
}
impl std::fmt::Debug for LanguageService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LanguageService").finish_non_exhaustive()
    }
}
impl LanguageService {
    pub fn new(project: Project) -> Self {
        Self {
            core: LspValidationCore::new(project),
        }
    }
    pub fn with_schema_cache(project: Project, cache: SchemaCache) -> Self {
        let mut service = Self::new(project);
        service.core.use_schema_runtime(cache);
        service
    }
    pub fn rebase(&mut self, project: Project) -> Result<(), String> {
        self.core.rebase(project)
    }
    pub fn invalidate_files(&mut self, paths: &[PathBuf]) -> Result<(), String> {
        self.core.apply_watched_files(
            &paths
                .iter()
                .map(|p| crate::path_to_file_uri(p))
                .collect::<Vec<_>>(),
        )?;
        Ok(())
    }
    /// 同版本同内容可重复查询；过期文本不能得到新版本的结果来冒充自身结果。
    pub fn synchronize(&mut self, path: &Path, source: &str, version: i64) -> Result<(), String> {
        if let Some(current) = self
            .core
            .open_documents()
            .get(&coflow_project::normalize_path(path))
        {
            if current
                .version
                .is_some_and(|v| v > version || (v == version && current.text.as_ref() != source))
            {
                return Err("stale language document version".into());
            }
        }
        self.core.apply_change_document(
            crate::path_to_file_uri(path),
            source.to_owned(),
            Some(version),
        )?;
        Ok(())
    }
    pub fn close_document(&mut self, path: &Path) -> Result<(), String> {
        self.core
            .apply_close_document(&crate::path_to_file_uri(path))?;
        Ok(())
    }
    pub fn document(&mut self, path: &Path) -> Result<LanguageDocumentState, String> {
        self.core.ensure_build_publications();
        let uri = crate::path_to_file_uri(path);
        let tokens = self.core.semantic_tokens(&uri);
        Ok(LanguageDocumentState {
            diagnostics: self.core.document_diagnostics(&uri).to_vec(),
            semantic_token_data: tokens.data,
            semantic_token_types: token_types(),
            syntax_valid: tokens.syntax_valid,
        })
    }
    pub fn completion(
        &mut self,
        path: &Path,
        position: &LanguagePosition,
    ) -> Result<Vec<LanguageCompletion>, String> {
        self.core.ensure_build_publications();
        Ok(self.core.completion(
            &crate::path_to_file_uri(path),
            LspPosition {
                line: position.line,
                character: position.character,
            },
        ))
    }
    pub fn formatting(&mut self, path: &Path) -> Result<LanguageFormattingResult, String> {
        self.core.ensure_build_publications();
        let uri = crate::path_to_file_uri(path);
        let edits = self.core.formatting(&uri);
        let source = match self.core.request_document(&uri) {
            crate::LspRequestDocument::Cfd(d) => d.source,
            crate::LspRequestDocument::Cft { document, .. } => &document.source,
            crate::LspRequestDocument::Missing => {
                return Err("language document unavailable".into())
            }
        };
        let mut text = source.to_owned();
        for edit in edits.iter().rev() {
            let offset = |p: &LanguagePosition| {
                crate::byte_offset_from_position(
                    source,
                    LspPosition {
                        line: p.line,
                        character: p.character,
                    },
                )
            };
            text.replace_range(
                offset(&edit.range.start)..offset(&edit.range.end),
                &edit.new_text,
            );
        }
        Ok(LanguageFormattingResult { text, edits })
    }
    pub fn function_document(source: &str, body: Option<&str>) -> FunctionDocumentState {
        crate::cfd::function_document(source, body)
    }
    pub fn hover(&mut self, path: &Path, position: LanguagePosition) -> Option<Hover> {
        self.core.ensure_build_publications();
        self.core.hover(&crate::path_to_file_uri(path), position)
    }
    pub fn definitions(&mut self, path: &Path, position: LanguagePosition) -> Vec<Location> {
        self.core.ensure_build_publications();
        self.core
            .definition(&crate::path_to_file_uri(path), position)
    }
    pub fn symbols(&mut self, path: &Path) -> Vec<DocumentSymbol> {
        self.core.ensure_build_publications();
        self.core.document_symbol(&crate::path_to_file_uri(path))
    }
    pub fn highlight(
        &mut self,
        path: &Path,
        source: &str,
    ) -> Result<LanguageDocumentState, String> {
        self.core.ensure_build_publications();
        let (data, syntax_valid) = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cfd"))
        {
            let (ast, errors) = coflow_language::cfd::parse_cfd(source);
            (
                crate::cfd::semantic_tokens(source, &ast, self.core.schema()).data,
                errors.is_empty(),
            )
        } else {
            let module_id = self
                .core
                .build()
                .and_then(|b| b.document_by_uri(&crate::path_to_file_uri(path)))
                .map_or_else(|| "__snapshot__".to_owned(), |d| d.module_id.clone());
            let ast = coflow_language::cft::syntax::parser::parse_module(
                &module_id.clone().into(),
                source,
            )
            .ok()
            .map(std::sync::Arc::new);
            let valid = ast.is_some();
            let document = crate::LspDocument {
                module_id,
                uri: crate::path_to_file_uri(path),
                source: source.into(),
                ast,
            };
            (
                crate::semantic_tokens::snapshot_token_data(self.core.build(), &document),
                valid,
            )
        };
        Ok(LanguageDocumentState {
            diagnostics: Vec::new(),
            semantic_token_data: data,
            semantic_token_types: token_types(),
            syntax_valid,
        })
    }
}
pub(crate) fn token_types() -> Vec<String> {
    crate::SEMANTIC_TOKEN_TYPES
        .iter()
        .map(|v| (*v).to_owned())
        .collect()
}
