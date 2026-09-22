use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix::diff::blob::{diff_with_slider_heuristics, Algorithm, InternedInput, UnifiedDiff};

use serde::{Deserialize, Serialize};

use crate::data_model::{CfdValue, RecordCoordinate};
use crate::session::ProjectSession;
use crate::{Diagnostic, DiagnosticSet, Project, ProjectSessionFactory};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ProjectDiffChange {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectDiffValue {
    pub path: String,
    pub value: CfdValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectRecordSnapshot {
    pub file_path: String,
    pub values: Vec<ProjectDiffValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectFieldDiff {
    pub path: String,
    pub change: ProjectDiffChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub before: Option<CfdValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub after: Option<CfdValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectRecordDiff {
    pub coordinate: RecordCoordinate,
    pub change: ProjectDiffChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub before: Option<ProjectRecordSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub after: Option<ProjectRecordSnapshot>,
    pub fields: Vec<ProjectFieldDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectFileDiff {
    pub path: String,
    pub change: ProjectDiffChange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub after: Option<String>,
    /// 仅包含统一 Diff 的 hunk；文件身份由 `path` 和 `change` 提供。
    pub patch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectDiffDiagnostic {
    pub endpoint: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct ProjectDiff {
    pub head_oid: String,
    #[cfg_attr(feature = "ts-export", ts(type = "number"))]
    pub target_revision: u64,
    pub semantic_available: bool,
    pub files: Vec<ProjectFileDiff>,
    pub records: Vec<ProjectRecordDiff>,
    pub diagnostics: Vec<ProjectDiffDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
struct RecordState {
    // 文件路径与语义值分开存放，快照仅在确认变化后构造，
    // 避免为海量未变化记录重复克隆 CfdValue。
    file_path: String,
    values: BTreeMap<String, CfdValue>,
}

// HEAD 分支在作用域线程内独立计算的结果；Loaded 携带的诊断已按
// “会话诊断在前、来源附注在后”排好序，合并点直接追加以保持原有诊断顺序。
enum HeadOutcome {
    Loaded {
        sources: BTreeMap<String, String>,
        records: BTreeMap<RecordCoordinate, RecordState>,
        valid: bool,
        head_diagnostics: Vec<ProjectDiffDiagnostic>,
        session_diagnostics: Vec<ProjectDiffDiagnostic>,
    },
    Failed {
        error: DiagnosticSet,
    },
}

pub(crate) fn diff_against_head(
    session: &ProjectSession,
    target_revision: u64,
) -> Result<ProjectDiff, DiagnosticSet> {
    let git = GitProject::open(&session.project)?;
    let head = git.materialize_head()?;
    let mut diagnostics = Vec::new();
    let current_sources = session_sources(session, &git.diff_paths(), None, &mut diagnostics);
    let ignored = git.ignored_paths(current_sources.keys())?;

    // HEAD 分支（快照重载 + HEAD 记录）与当前记录计算相互独立，
    // 用作用域线程并行执行；gix Repository 非 Sync，线程内仅使用纯路径
    // 快照 GitDiffPaths，合并点后保持原有诊断顺序。
    let head_paths = git.diff_paths();
    let (head_outcome, mut current_records) = std::thread::scope(|scope| {
        let head_handle = scope.spawn(|| {
            // HEAD 必须使用提交内自己的 coflow.yaml 与 CFT/CFD，不能套用当前 schema。
            let head_session = Project::open_schema_only(Some(&head.config_path))
                .and_then(|mut project| {
                    head_paths.rebase_head_inputs(&mut project, head.temp.path())?;
                    Ok(project)
                })
                .and_then(|project| ProjectSessionFactory::new().open_read_only_session(project));
            match head_session {
                Ok(head_session) => {
                    let session_set = head_session.queries().diagnostics().as_set();
                    let valid = session_set.is_empty();
                    let session_diagnostics = session_set
                        .iter()
                        .map(|diagnostic| ProjectDiffDiagnostic {
                            endpoint: "head".to_string(),
                            code: diagnostic.code.clone(),
                            message: diagnostic.message.clone(),
                        })
                        .collect::<Vec<_>>();
                    let mut head_diagnostics = Vec::new();
                    let sources = session_sources(
                        &head_session.session,
                        &head_paths,
                        Some(head.temp.path()),
                        &mut head_diagnostics,
                    );
                    let records =
                        record_states(&head_session.session, &head_paths, Some(head.temp.path()));
                    HeadOutcome::Loaded {
                        sources,
                        records,
                        valid,
                        head_diagnostics,
                        session_diagnostics,
                    }
                }
                Err(error) => HeadOutcome::Failed { error },
            }
        });
        let current_records = record_states(session, &git.diff_paths(), None);
        // HEAD 会话打开失败属于可恢复路径，直接展开；线程本身无 fallible 操作。
        let head_outcome = head_handle.join().unwrap_or_else(|_| HeadOutcome::Failed {
            error: git_error("HEAD 比较线程异常结束"),
        });
        (head_outcome, current_records)
    });

    append_diagnostics(&mut diagnostics, "current", session.diagnostics.as_set());
    let (head_sources, head_records, head_valid) = match head_outcome {
        HeadOutcome::Loaded {
            sources,
            records,
            valid,
            head_diagnostics,
            session_diagnostics,
        } => {
            diagnostics.extend(session_diagnostics);
            diagnostics.extend(head_diagnostics);
            (sources, records, valid)
        }
        HeadOutcome::Failed { error } => {
            append_diagnostics(&mut diagnostics, "head", &error);
            (head.sources, BTreeMap::new(), false)
        }
    };

    let current_valid = session.diagnostics.as_set().is_empty();
    let files = source_diffs(&ignored, &head_sources, &current_sources)?;
    let records = if head_valid && current_valid {
        filter_ignored_additions(&ignored, &head_sources, &mut current_records);
        record_diffs(&head_records, &current_records)
    } else {
        Vec::new()
    };

    Ok(ProjectDiff {
        head_oid: git.head_oid.to_string(),
        target_revision,
        semantic_available: head_valid && current_valid,
        files,
        records,
        diagnostics,
    })
}

fn filter_ignored_additions(
    ignored: &BTreeSet<String>,
    head_sources: &BTreeMap<String, String>,
    records: &mut BTreeMap<RecordCoordinate, RecordState>,
) {
    // 仅保留 HEAD 已跟踪或未被忽略的新增记录，避免忽略文件放大语义差异。
    records.retain(|_, record| {
        head_sources.contains_key(&record.file_path) || !ignored.contains(&record.file_path)
    });
}

fn append_diagnostics(
    target: &mut Vec<ProjectDiffDiagnostic>,
    endpoint: &str,
    diagnostics: &DiagnosticSet,
) {
    target.extend(diagnostics.iter().map(|diagnostic| ProjectDiffDiagnostic {
        endpoint: endpoint.to_string(),
        code: diagnostic.code.clone(),
        message: diagnostic.message.clone(),
    }));
}

fn session_sources(
    session: &ProjectSession,
    git: &GitDiffPaths,
    snapshot: Option<&Path>,
    diagnostics: &mut Vec<ProjectDiffDiagnostic>,
) -> BTreeMap<String, String> {
    let root = session.project.root_dir();
    let mut sources = BTreeMap::new();
    if let Ok(relative) = session.project.config_path().strip_prefix(root) {
        sources.insert(
            crate::path_to_slash(relative),
            session.project.config_source.to_string(),
        );
    }
    for (_, module) in session.modules.modules() {
        sources.insert(
            crate::project_path(root, module.path()),
            module.source().to_string(),
        );
    }
    for (path, source) in session.source_data.sources() {
        sources.insert(path.to_string(), source.to_string());
    }
    sources
        .into_iter()
        .filter_map(|(path, source)| {
            git.diff_path(root, &path, snapshot).map_or_else(
                || {
                    diagnostics.push(ProjectDiffDiagnostic {
                        endpoint: if snapshot.is_some() {
                            "head"
                        } else {
                            "current"
                        }
                        .to_string(),
                        code: "GIT-EXTERNAL-SOURCE".to_string(),
                        message: format!("已排除 Git 仓库外的项目文件 `{path}`"),
                    });
                    None
                },
                |path| Some((path, source)),
            )
        })
        .collect()
}

fn record_states(
    session: &ProjectSession,
    git: &GitDiffPaths,
    snapshot: Option<&Path>,
) -> BTreeMap<RecordCoordinate, RecordState> {
    let mut records = BTreeMap::new();
    // diff_path 内部多次访问文件系统（symlink_metadata + canonicalize），
    // 同一文件的每条记录结果完全相同，按文件路径记忆化，将万次系统调用降为文件数级别。
    let mut file_path_cache: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    let root = session.project.root_dir();
    for (_, record) in session.model.records() {
        let coordinate = record.coordinate();
        let Some(source_path) = session.file_for_record(coordinate.actual_type(), coordinate.key())
        else {
            continue;
        };
        let Some(file_path) = file_path_cache
            .entry(source_path.to_string())
            .or_insert_with(|| git.diff_path(root, source_path, snapshot))
            .clone()
        else {
            continue;
        };
        let mut values = record
            .fields()
            .iter()
            .map(|(name, value)| (name.to_string(), normalized_semantic_value(value)))
            .collect::<BTreeMap<_, _>>();
        for (field_name, dimensions) in &record.dimension_fields {
            for (variant, value) in &dimensions.variants {
                values.insert(
                    format!("{}[{}={}]", field_name, dimensions.dimension, variant),
                    normalized_semantic_value(&value.value),
                );
            }
        }
        // 此处只保存归一化后的值映射，快照在 record_diffs 确认变化后按需构造。
        records.insert(coordinate, RecordState { file_path, values });
    }
    records
}

fn normalized_semantic_value(value: &CfdValue) -> CfdValue {
    match value {
        CfdValue::Function(function) => CfdValue::Function(crate::CallableSource {
            from_default: false,
            location: None,
            imports: Default::default(),
            constant_origin: None,
            source: normalized_line_endings(&function.source),
        }),
        CfdValue::FormattedString(value) => CfdValue::FormattedString(crate::CallableSource {
            from_default: false,
            location: None,
            imports: Default::default(),
            constant_origin: None,
            source: normalized_line_endings(&value.source),
        }),
        CfdValue::OptionSome(value) => {
            CfdValue::OptionSome(Box::new(normalized_semantic_value(value)))
        }
        CfdValue::Object(object) => {
            let mut normalized = object.as_ref().clone();
            for value in normalized.fields_mut().values_mut() {
                *value = normalized_semantic_value(value);
            }
            CfdValue::Object(Box::new(normalized))
        }
        CfdValue::Array(values) => {
            CfdValue::Array(values.iter().map(normalized_semantic_value).collect())
        }
        CfdValue::Dict(values) => CfdValue::Dict(
            values
                .iter()
                .map(|(key, value)| (key.clone(), normalized_semantic_value(value)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn normalized_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

fn record_diffs(
    before: &BTreeMap<RecordCoordinate, RecordState>,
    after: &BTreeMap<RecordCoordinate, RecordState>,
) -> Vec<ProjectRecordDiff> {
    let coordinates = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    // 先比较值映射与文件路径，仅对新增/删除/修改的记录构造快照，
    // 未变化记录零克隆，直接跳过。
    coordinates
        .into_iter()
        .filter_map(
            |coordinate| match (before.get(&coordinate), after.get(&coordinate)) {
                (None, Some(after)) => Some(ProjectRecordDiff {
                    coordinate,
                    change: ProjectDiffChange::Added,
                    before: None,
                    after: Some(snapshot_of(after)),
                    fields: field_diffs(&BTreeMap::new(), &after.values),
                }),
                (Some(before), None) => Some(ProjectRecordDiff {
                    coordinate,
                    change: ProjectDiffChange::Deleted,
                    before: Some(snapshot_of(before)),
                    after: None,
                    fields: field_diffs(&before.values, &BTreeMap::new()),
                }),
                (Some(before), Some(after))
                    if before.values != after.values || before.file_path != after.file_path =>
                {
                    Some(ProjectRecordDiff {
                        coordinate,
                        change: ProjectDiffChange::Modified,
                        before: Some(snapshot_of(before)),
                        after: Some(snapshot_of(after)),
                        fields: field_diffs(&before.values, &after.values),
                    })
                }
                _ => None,
            },
        )
        .collect()
}

fn snapshot_of(state: &RecordState) -> ProjectRecordSnapshot {
    // 变化记录的唯一克隆点，由值映射生成有序快照。
    ProjectRecordSnapshot {
        file_path: state.file_path.clone(),
        values: state
            .values
            .iter()
            .map(|(path, value)| ProjectDiffValue {
                path: path.clone(),
                value: value.clone(),
            })
            .collect(),
    }
}

fn field_diffs(
    before: &BTreeMap<String, CfdValue>,
    after: &BTreeMap<String, CfdValue>,
) -> Vec<ProjectFieldDiff> {
    before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| match (before.get(&path), after.get(&path)) {
            (None, Some(after)) => Some(ProjectFieldDiff {
                path,
                change: ProjectDiffChange::Added,
                before: None,
                after: Some(after.clone()),
            }),
            (Some(before), None) => Some(ProjectFieldDiff {
                path,
                change: ProjectDiffChange::Deleted,
                before: Some(before.clone()),
                after: None,
            }),
            (Some(before), Some(after)) if before != after => Some(ProjectFieldDiff {
                path,
                change: ProjectDiffChange::Modified,
                before: Some(before.clone()),
                after: Some(after.clone()),
            }),
            _ => None,
        })
        .collect()
}

fn source_diffs(
    ignored: &BTreeSet<String>,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<Vec<ProjectFileDiff>, DiagnosticSet> {
    let paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut files = Vec::new();
    for path in paths {
        let old = before
            .get(&path)
            .map(|source| normalized_line_endings(source));
        let new = after
            .get(&path)
            .map(|source| normalized_line_endings(source));
        if old == new {
            continue;
        }
        if old.is_none() && ignored.contains(&path) {
            continue;
        }
        let change = match (old.as_ref(), new.as_ref()) {
            (None, Some(_)) => ProjectDiffChange::Added,
            (Some(_), None) => ProjectDiffChange::Deleted,
            _ => ProjectDiffChange::Modified,
        };
        let patch = unified_hunks(old.as_deref().unwrap_or(""), new.as_deref().unwrap_or(""))?;
        files.push(ProjectFileDiff {
            path,
            change,
            before: old,
            after: new,
            patch,
        });
    }
    Ok(files)
}

fn unified_hunks(before: &str, after: &str) -> Result<String, DiagnosticSet> {
    // 直接比较 session 文本，不经过工作区过滤器或外部 diff 驱动。
    let input = InternedInput::new(before, after);
    let diff = diff_with_slider_heuristics(Algorithm::Myers, &input);
    UnifiedDiff::new(
        &diff,
        &input,
        PatchHunks::default(),
        ContextSize::symmetrical(3),
    )
    .consume()
    .map_err(|error| git_error(format!("生成文本差异失败：{error}")))
}

#[derive(Default)]
struct PatchHunks(String);

impl ConsumeHunk for PatchHunks {
    type Out = String;

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        // Unified Diff 的空区间从前一行起算，末行缺少换行时保留显式标记。
        let before_start = header.before_hunk_start - u32::from(header.before_hunk_len == 0);
        let after_start = header.after_hunk_start - u32::from(header.after_hunk_len == 0);
        writeln!(
            self.0,
            "@@ -{before_start},{} +{after_start},{} @@",
            header.before_hunk_len, header.after_hunk_len,
        )
        .map_err(std::io::Error::other)?;
        for (kind, bytes) in lines {
            self.0.push(kind.to_prefix());
            self.0
                .push_str(std::str::from_utf8(bytes).map_err(std::io::Error::other)?);
            if !bytes.ends_with(b"\n") {
                self.0.push_str("\n\\ No newline at end of file\n");
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Self::Out {
        if self.0.ends_with('\n') {
            self.0.pop();
        }
        self.0
    }
}

struct GitProject {
    repo: gix::Repository,
    repo_root: PathBuf,
    project_relative: PathBuf,
    config_relative: PathBuf,
    head_oid: gix::ObjectId,
}

struct HeadMaterialization {
    temp: tempfile::TempDir,
    config_path: PathBuf,
    sources: BTreeMap<String, String>,
}

impl GitProject {
    fn open(project: &Project) -> Result<Self, DiagnosticSet> {
        let repo = gix::discover(project.root_dir())
            .map_err(|error| git_error(format!("无法打开 Git 仓库：{error}")))?;
        let workdir = repo
            .workdir()
            .ok_or_else(|| git_error("Coflow 项目需要 Git 工作区"))?;
        let repo_root = crate::canonicalize_path(workdir)
            .map_err(|error| git_error(format!("无法解析 Git 仓库根目录：{error}")))?;
        let project_root = crate::canonicalize_path(project.root_dir())
            .map_err(|error| git_error(format!("无法解析项目目录：{error}")))?;
        let config_path = crate::canonicalize_path(project.config_path())
            .map_err(|error| git_error(format!("无法解析项目配置：{error}")))?;
        let project_relative = project_root
            .strip_prefix(&repo_root)
            .map_err(|_| git_error("Coflow 项目不在当前 Git 工作区中"))?
            .to_path_buf();
        let config_relative = config_path
            .strip_prefix(&repo_root)
            .map_err(|_| git_error("项目配置不在当前 Git 工作区中"))?
            .to_path_buf();
        let head_oid = repo
            .head_commit()
            .map_err(|error| git_error(format!("无法读取 HEAD 提交：{error}")))?
            .id;
        Ok(Self {
            repo,
            repo_root,
            project_relative,
            config_relative,
            head_oid,
        })
    }

    fn materialize_head(&self) -> Result<HeadMaterialization, DiagnosticSet> {
        // 固定同一个提交对象，避免查询过程中 HEAD 移动导致快照混用。
        let tree = self
            .repo
            .find_object(self.head_oid)
            .map_err(|error| git_error(format!("无法读取 HEAD 对象：{error}")))?
            .peel_to_tree()
            .map_err(|error| git_error(format!("无法读取 HEAD 文件树：{error}")))?;
        let entries = tree
            .traverse()
            .breadthfirst
            .files()
            .map_err(|error| git_error(format!("无法遍历 HEAD 文件树：{error}")))?;
        let mut roots = vec![self.project_relative.clone()];
        if let Some(entry) = tree
            .lookup_entry_by_path(&self.config_relative)
            .map_err(|error| git_error(format!("无法读取 HEAD 配置入口：{error}")))?
        {
            let object = entry
                .object()
                .map_err(|error| git_error(format!("无法读取 HEAD 配置：{error}")))?;
            if let Ok(config) = serde_yaml::from_slice::<crate::ProjectConfig>(&object.data) {
                let inputs = config
                    .schema
                    .paths()
                    .iter()
                    .chain(config.data.iter().map(crate::SourceConfig::path));
                for input in inputs {
                    let absolute = crate::normalize_path(
                        &self.repo_root.join(&self.project_relative).join(input),
                    );
                    if let Ok(relative) = absolute.strip_prefix(&self.repo_root) {
                        roots.push(relative.to_path_buf());
                    }
                }
            }
        }
        let temp = tempfile::tempdir()
            .map_err(|error| git_error(format!("创建 HEAD 快照目录失败：{error}")))?;
        let mut sources = BTreeMap::new();
        for entry in entries {
            if entry.mode.is_tree() || entry.mode.is_commit() {
                continue;
            }
            let git_path = std::str::from_utf8(entry.filepath.as_ref())
                .map_err(|error| git_error(format!("Git 路径不是 UTF-8：{error}")))?;
            let relative = safe_git_path(git_path)?;
            if !is_project_text_path(&relative)
                || !roots.iter().any(|root| relative.starts_with(root))
            {
                continue;
            }
            let blob = self
                .repo
                .find_blob(entry.oid)
                .map_err(|error| git_error(format!("无法读取 HEAD 文件 `{git_path}`：{error}")))?;
            if is_project_text_path(&relative) {
                if let (Ok(project_path), Ok(source)) = (
                    relative.strip_prefix(&self.project_relative),
                    std::str::from_utf8(&blob.data),
                ) {
                    sources.insert(crate::path_to_slash(project_path), source.to_string());
                }
            }
            let target = temp.path().join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| git_error(format!("创建 HEAD 快照目录失败：{error}")))?;
            }
            fs::write(&target, &blob.data)
                .map_err(|error| git_error(format!("写入 HEAD 快照 `{git_path}` 失败：{error}")))?;
        }
        Ok(HeadMaterialization {
            config_path: temp.path().join(&self.config_relative),
            temp,
            sources,
        })
    }

    fn ignored_paths<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a String>,
    ) -> Result<BTreeSet<String>, DiagnosticSet> {
        let index = self
            .repo
            .index_or_empty()
            .map_err(|error| git_error(format!("无法读取 Git 索引：{error}")))?;
        let mut excludes = self
            .repo
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::default(),
            )
            .map_err(|error| git_error(format!("无法读取 Git 忽略规则：{error}")))?;
        let mut ignored = BTreeSet::new();
        for path in paths {
            let absolute = self.repo_root.join(&self.project_relative).join(path);
            let repo_path = absolute.strip_prefix(&self.repo_root).map_err(|error| {
                git_error(format!("无法解析 Git 仓库相对路径 `{path}`：{error}"))
            })?;
            let git_path = crate::path_to_slash(repo_path);
            // 与 check-ignore 一致，索引中已跟踪的文件不受忽略规则影响。
            if index.entry_by_path(git_path.as_bytes().as_bstr()).is_some() {
                continue;
            }
            if excludes
                .at_path(repo_path, None)
                .map_err(|error| git_error(format!("无法判断 Git 忽略路径 `{path}`：{error}")))?
                .is_excluded()
            {
                ignored.insert(path.clone());
            }
        }
        Ok(ignored)
    }

    fn diff_paths(&self) -> GitDiffPaths {
        // 仅克隆两个路径，可跨线程传递；gix Repository 本身非 Sync。
        GitDiffPaths {
            repo_root: self.repo_root.clone(),
            project_relative: self.project_relative.clone(),
        }
    }
}

#[derive(Debug, Clone)]
struct GitDiffPaths {
    repo_root: PathBuf,
    project_relative: PathBuf,
}

impl GitDiffPaths {
    fn diff_path(&self, root: &Path, path: &str, snapshot: Option<&Path>) -> Option<String> {
        let absolute = crate::normalize_path(&root.join(path));
        let absolute = match snapshot {
            Some(snapshot) => self.repo_root.join(
                absolute
                    .strip_prefix(crate::normalize_path(snapshot))
                    .ok()?,
            ),
            None => absolute,
        };
        absolute.strip_prefix(&self.repo_root).ok()?;
        Some(crate::project_path(
            &self.repo_root.join(&self.project_relative),
            &absolute,
        ))
    }

    fn rebase_head_inputs(
        &self,
        project: &mut Project,
        snapshot: &Path,
    ) -> Result<(), DiagnosticSet> {
        // HEAD 的绝对配置路径必须映射到快照，禁止读取当前工作区或仓库外文件。
        let rebase = |path: &Path| -> Result<PathBuf, DiagnosticSet> {
            let absolute =
                crate::normalize_path(&self.repo_root.join(&self.project_relative).join(path));
            let relative = absolute.strip_prefix(&self.repo_root).map_err(|_| {
                git_error(format!(
                    "HEAD 引用仓库外路径 `{}`，仅提供仓库内源码比较",
                    path.display()
                ))
            })?;
            Ok(snapshot.join(relative))
        };
        for path in &mut project.config.schema.paths {
            *path = rebase(path)?;
        }
        for source in &mut project.config.data {
            *source = crate::SourceConfig::from_path(rebase(source.path())?);
        }
        Ok(())
    }
}

fn is_project_text_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("yaml" | "yml" | "cft" | "cfd")
    )
}

fn safe_git_path(path: &str) -> Result<PathBuf, DiagnosticSet> {
    let path = Path::new(path);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(git_error(format!(
            "Git 快照包含不安全路径 `{}`",
            path.display()
        )));
    }
    Ok(path.to_path_buf())
}

fn git_error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("GIT-DIFF", "GIT", message))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::unified_hunks;

    #[test]
    fn unified_hunks_handles_empty_sides_and_missing_newlines() {
        for (before, after, expected) in [
            ("", "", ""),
            ("same\n", "same\n", ""),
            ("", "added\n", "@@ -0,0 +1,1 @@\n+added"),
            ("deleted\n", "", "@@ -1,1 +0,0 @@\n-deleted"),
            (
                "before",
                "after",
                "@@ -1,1 +1,1 @@\n-before\n\\ No newline at end of file\n+after\n\\ No newline at end of file",
            ),
            (
                "same",
                "same\n",
                "@@ -1,1 +1,1 @@\n-same\n\\ No newline at end of file\n+same",
            ),
        ] {
            assert_eq!(unified_hunks(before, after).unwrap(), expected);
        }
    }

    #[test]
    fn unified_hunks_keeps_three_context_lines_and_separates_distant_changes() {
        let before = "old\n1\n2\n3\n4\n5\n6\n7\n8\n9\nold\n";
        let after = before.replace("old", "new");
        assert_eq!(
            unified_hunks(before, &after).unwrap(),
            "@@ -1,4 +1,4 @@\n-old\n+new\n 1\n 2\n 3\n@@ -8,4 +8,4 @@\n 7\n 8\n 9\n-old\n+new",
        );
    }
}
