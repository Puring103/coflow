use super::WriteProjectSession;
use crate::api::{Diagnostic, DiagnosticSet, FlatDiagnostic};
use crate::indexes::SessionIndexBuilder;
use crate::load::{
    reload_project_data_from_cache, LoadProjectDataOptions, ReloadProjectDataOptions,
};
use crate::project_schema::SchemaTextOverride;
use crate::session::ProjectSession;
use crate::session_build::open_project_session_from_schema;
use crate::{DataSourceTextOverride, ProjectFileUpdate, ProjectQueries, SchemaCache};
use coflow_core::schema::CftSchema;
use std::{collections::BTreeSet, sync::Arc};

/// 编辑器对未保存草稿做项目级校验所需的只读快照。
#[derive(Debug, Clone)]
pub struct SourceValidationContext {
    pub(super) schema_cache: Arc<std::sync::Mutex<SchemaCache>>,
    pub(super) schema: Arc<CftSchema>,
    pub(super) source_data: crate::load::SourceDataCache,
}

impl SourceValidationContext {
    /// 隔离校验单个 CFT 草稿；共享尝试缓存，但不改变语言服务的打开文档。
    pub fn validate_schema(
        &self,
        overrides: &[SchemaTextOverride],
    ) -> Result<Vec<FlatDiagnostic>, DiagnosticSet> {
        let mut cache = self.schema_cache.lock().map_err(|_| {
            DiagnosticSet::one(Diagnostic::error(
                "SOURCE-VALIDATION",
                "PROJECT",
                "schema validation cache poisoned",
            ))
        })?;
        Ok(match cache.refresh_with_overrides(overrides) {
            Ok(_) => Vec::new(),
            Err(diagnostics) => diagnostics.flat_diagnostics(),
        })
    }

    /// 在当前会话的源缓存上应用文本覆盖并返回项目诊断，不修改任何会话。
    ///
    /// 只有被覆盖的文件会被重新解析，其余文件复用缓存；这避免了宿主每次编辑
    /// 都从磁盘重新加载整个项目。
    #[must_use]
    pub fn validate(&self, overrides: &[DataSourceTextOverride]) -> Vec<FlatDiagnostic> {
        let override_paths = overrides
            .iter()
            .map(|source_override| source_override.normalized_path.clone())
            .collect::<BTreeSet<_>>();
        let reload_paths = self.source_data.display_paths_for_paths(&override_paths);
        let mut indexes = SessionIndexBuilder::default();
        let result = reload_project_data_from_cache(
            &self.schema,
            &mut indexes,
            &self.source_data,
            &reload_paths,
            ReloadProjectDataOptions {
                load: LoadProjectDataOptions { run_checks: true },
                source_overrides: overrides,
            },
        );
        match result {
            Ok(output) => output.diagnostics.flat_diagnostics(),
            Err(failure) => failure.diagnostics.flat_diagnostics(),
        }
    }
}

/// 已校验的源码事务；提交同时验证会话版本和原始磁盘内容。
#[derive(Debug)]
pub struct PreparedSourceUpdate {
    identity: Arc<()>,
    revision: u64,
    candidate: ProjectSession,
    file: ProjectFileUpdate,
}

impl PreparedSourceUpdate {
    /// 对照编辑开始时的原文，而不是准备事务时读到的新内容。
    pub fn verify_base(&self, expected: &str) -> Result<(), DiagnosticSet> {
        if self.file.expected.as_deref() != Some(expected.as_bytes()) {
            return Err(DiagnosticSet::one(Diagnostic::error("SOURCE-CONFLICT", "PROJECT", "文件已被其他操作修改，请选择使用磁盘内容或保留本地内容")));
        }
        Ok(())
    }
}

impl WriteProjectSession {
    pub fn source_update_context(&self) -> SourceUpdateContext {
        SourceUpdateContext {
            identity: Arc::clone(&self.identity),
            revision: self.revision,
            session: Arc::clone(&self.session),
        }
    }
    pub fn prepare_source_update(
        &self,
        path: &std::path::Path,
        source: &str,
    ) -> Result<PreparedSourceUpdate, DiagnosticSet> {
        self.source_update_context().prepare(path, source)
    }

    /// 文件冲突或代际冲突均不发布候选，也不覆盖外部编辑。
    pub fn commit_source_update(
        &mut self,
        update: PreparedSourceUpdate,
    ) -> Result<crate::ProjectCommit, DiagnosticSet> {
        if !Arc::ptr_eq(&update.identity, &self.identity) || update.revision != self.revision {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "SOURCE-CONFLICT",
                "PROJECT",
                "project changed while source update was prepared",
            )));
        }
        let unchanged = update.file.expected.as_deref() == Some(update.file.contents.as_slice());
        if unchanged {
            if std::fs::read(&update.file.path).ok().as_deref() != update.file.expected.as_deref() {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "SOURCE-CONFLICT",
                    "PROJECT",
                    "source changed while update was prepared",
                )));
            }
            return Ok(crate::ProjectCommit::default());
        }
        let next_revision = self.revision.checked_add(1).ok_or_else(|| {
            DiagnosticSet::one(Diagnostic::error(
                "SOURCE-CONFLICT",
                "PROJECT",
                "project revision exhausted",
            ))
        })?;
        let writer = crate::cfd_loader::CfdWriter::new();
        let path = update.file.path.clone();
        writer.add_project_file_updates(vec![update.file])?;
        writer.publish()?;
        self.project().source_store().invalidate(&path);
        self.session = Arc::new(update.candidate);
        self.revision = next_revision;
        Ok(crate::ProjectCommit {
            generation_changed: true,
            schema_changed: path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cft")),
            written_files: vec![crate::project_path(self.project().root_dir(), &path)],
            affected_files: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SourceUpdateContext {
    identity: Arc<()>,
    revision: u64,
    session: Arc<ProjectSession>,
}
impl SourceUpdateContext {
    /// 在当前数据代际上准备源码替换；候选只构建一次，提交时直接发布。
    pub fn prepare(
        &self,
        path: &std::path::Path,
        source: &str,
    ) -> Result<PreparedSourceUpdate, DiagnosticSet> {
        let path = crate::normalize_path(path);
        let expected = std::fs::read(&path).map_err(|error| {
            DiagnosticSet::one(Diagnostic::error(
                "SOURCE-READ",
                "PROJECT",
                error.to_string(),
            ))
        })?;
        let candidate = if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cft"))
        {
            if !self
                .session
                .project
                .schema_files()?
                .iter()
                .any(|file| crate::normalize_path(&file.canonical_path) == path)
            {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "SOURCE-PATH",
                    "PROJECT",
                    "source is not part of the project schema",
                )));
            }
            let mut runtime = SchemaCache::new(self.session.project.clone());
            let attempt = runtime.refresh_with_overrides(&[SchemaTextOverride {
                requested_module: None,
                normalized_path: path.clone(),
                source: source.to_string(),
            }]);
            if runtime.latest_attempt().is_none() { attempt?; }
            let schema = runtime.into_latest_attempt().ok_or_else(|| {
                DiagnosticSet::one(Diagnostic::error(
                    "SOURCE-SCHEMA",
                    "PROJECT",
                    "candidate schema unavailable",
                ))
            })?;
            open_project_session_from_schema(schema)?
        } else {
            let display_path = crate::project_path(self.session.project.root_dir(), &path);
            if !ProjectQueries::new(&self.session, self.revision).has_source_file(&display_path) {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "SOURCE-PATH",
                    "PROJECT",
                    "source is not part of the project data",
                )));
            }
            let mut impact = crate::writes::MutationImpact::default();
            impact.affected_files.insert(display_path);
            crate::session_build::rebuild_project_session_from_generation(
                &self.session,
                &impact,
                &[DataSourceTextOverride {
                    normalized_path: path.clone(),
                    source: source.to_string(),
                    deleted: false,
                }],
            )?
            .session
        };
        Ok(PreparedSourceUpdate {
            identity: Arc::clone(&self.identity),
            revision: self.revision,
            candidate,
            file: ProjectFileUpdate::new(path, Some(expected), source.as_bytes().to_vec()),
        })
    }
}
