use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

use serde::{Deserialize, Serialize};

use crate::data_model::{CfdValue, RecordCoordinate};
use crate::session::ProjectSession;
use crate::{Diagnostic, DiagnosticSet, Project, Runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
#[serde(rename_all = "snake_case")]
pub enum ProjectDiffChange {
    Added,
    Deleted,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectDiffValue {
    pub path: String,
    pub value: CfdValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectRecordSnapshot {
    pub file_path: String,
    pub values: Vec<ProjectDiffValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
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
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
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
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
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
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectDiffDiagnostic {
    pub endpoint: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
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
    snapshot: ProjectRecordSnapshot,
    values: BTreeMap<String, CfdValue>,
}

pub(crate) fn diff_against_head(
    session: &ProjectSession,
    target_revision: u64,
) -> Result<ProjectDiff, DiagnosticSet> {
    let git = GitProject::open(&session.project)?;
    let head = git.materialize_head()?;
    let current_sources = session_sources(session);

    // HEAD 必须使用提交内自己的 coflow.yaml 与 CFT/CFD，不能套用当前 schema。
    let head_session = Project::open_schema_only(Some(&head.config_path))
        .and_then(|project| Runtime::new().open_read_only_session(project));

    let mut diagnostics = Vec::new();
    append_diagnostics(&mut diagnostics, "current", session.diagnostics.as_set());
    let (head_sources, head_records, head_valid) = match head_session {
        Ok(head_session) => {
            append_diagnostics(
                &mut diagnostics,
                "head",
                head_session.queries().diagnostics().as_set(),
            );
            let valid = head_session.queries().diagnostics().as_set().is_empty();
            (
                session_sources(&head_session.session),
                record_states(&head_session.session),
                valid,
            )
        }
        Err(error) => {
            append_diagnostics(&mut diagnostics, "head", &error);
            (head.sources, BTreeMap::new(), false)
        }
    };

    let current_valid = session.diagnostics.as_set().is_empty();
    let files = source_diffs(&git, &head_sources, &current_sources)?;
    let records = if head_valid && current_valid {
        let mut current_records = record_states(session);
        filter_ignored_additions(&git, &head_sources, &mut current_records)?;
        record_diffs(&head_records, &current_records)
    } else {
        Vec::new()
    };

    Ok(ProjectDiff {
        head_oid: git.head_oid,
        target_revision,
        semantic_available: head_valid && current_valid,
        files,
        records,
        diagnostics,
    })
}

fn filter_ignored_additions(
    git: &GitProject,
    head_sources: &BTreeMap<String, String>,
    records: &mut BTreeMap<RecordCoordinate, RecordState>,
) -> Result<(), DiagnosticSet> {
    let paths = records
        .values()
        .map(|record| record.snapshot.file_path.clone())
        .collect::<BTreeSet<_>>();
    let mut ignored = BTreeSet::new();
    for path in paths {
        if !head_sources.contains_key(&path) && git.is_ignored(&path)? {
            ignored.insert(path);
        }
    }
    records.retain(|_, record| !ignored.contains(&record.snapshot.file_path));
    Ok(())
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

fn session_sources(session: &ProjectSession) -> BTreeMap<String, String> {
    let root = session.project.root_dir();
    let mut sources = BTreeMap::new();
    if let Ok(relative) = session.project.config_path().strip_prefix(root) {
        sources.insert(
            crate::path_to_slash(relative),
            session.project.config_source.to_string(),
        );
    }
    for (_, module) in session.modules.modules() {
        if let Ok(relative) = module.path().strip_prefix(root) {
            sources.insert(crate::path_to_slash(relative), module.source().to_string());
        }
    }
    for (path, source) in session.source_data.sources() {
        sources.insert(path.to_string(), source.to_string());
    }
    sources
}

fn record_states(session: &ProjectSession) -> BTreeMap<RecordCoordinate, RecordState> {
    let mut records = BTreeMap::new();
    for (_, record) in session.model.records() {
        let coordinate = record.coordinate();
        let Some(file_path) = session.file_for_record(coordinate.actual_type(), coordinate.key())
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
        let snapshot = ProjectRecordSnapshot {
            file_path: file_path.to_string(),
            values: values
                .iter()
                .map(|(path, value)| ProjectDiffValue {
                    path: path.clone(),
                    value: value.clone(),
                })
                .collect(),
        };
        records.insert(coordinate, RecordState { snapshot, values });
    }
    records
}

fn normalized_semantic_value(value: &CfdValue) -> CfdValue {
    match value {
        CfdValue::Function(function) => CfdValue::Function(crate::CfdFunction {
            source: normalized_line_endings(&function.source),
        }),
        CfdValue::FormattedString(value) => CfdValue::FormattedString(crate::CfdFormattedString {
            source: normalized_line_endings(&value.source),
            rendered: normalized_line_endings(&value.rendered),
        }),
        CfdValue::OptionSome(value) => {
            CfdValue::OptionSome(Box::new(normalized_semantic_value(value)))
        }
        CfdValue::ResultOk(value) => CfdValue::ResultOk(Box::new(normalized_semantic_value(value))),
        CfdValue::ResultErr(value) => {
            CfdValue::ResultErr(Box::new(normalized_semantic_value(value)))
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
    coordinates
        .into_iter()
        .filter_map(
            |coordinate| match (before.get(&coordinate), after.get(&coordinate)) {
                (None, Some(after)) => Some(ProjectRecordDiff {
                    coordinate,
                    change: ProjectDiffChange::Added,
                    before: None,
                    after: Some(after.snapshot.clone()),
                    fields: field_diffs(&BTreeMap::new(), &after.values),
                }),
                (Some(before), None) => Some(ProjectRecordDiff {
                    coordinate,
                    change: ProjectDiffChange::Deleted,
                    before: Some(before.snapshot.clone()),
                    after: None,
                    fields: field_diffs(&before.values, &BTreeMap::new()),
                }),
                (Some(before), Some(after))
                    if before.values != after.values
                        || before.snapshot.file_path != after.snapshot.file_path =>
                {
                    Some(ProjectRecordDiff {
                        coordinate,
                        change: ProjectDiffChange::Modified,
                        before: Some(before.snapshot.clone()),
                        after: Some(after.snapshot.clone()),
                        fields: field_diffs(&before.values, &after.values),
                    })
                }
                _ => None,
            },
        )
        .collect()
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
    git: &GitProject,
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
        if old.is_none() && git.is_ignored(&path)? {
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
    let temp = tempfile::tempdir()
        .map_err(|error| git_error(format!("创建 Diff 临时目录失败：{error}")))?;
    let before_path = temp.path().join("before");
    let after_path = temp.path().join("after");
    fs::write(&before_path, before)
        .and_then(|()| fs::write(&after_path, after))
        .map_err(|error| git_error(format!("写入 Diff 临时文件失败：{error}")))?;
    let output = Command::new("git")
        .args(["diff", "--no-index", "--no-color", "--unified=3", "--"])
        .arg(&before_path)
        .arg(&after_path)
        .output()
        .map_err(|error| git_error(format!("无法执行 git diff：{error}")))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(git_command_error("git diff --no-index", &output));
    }
    let patch = String::from_utf8(output.stdout)
        .map_err(|error| git_error(format!("git diff 输出不是 UTF-8：{error}")))?;
    Ok(patch
        .lines()
        .skip_while(|line| !line.starts_with("@@"))
        .collect::<Vec<_>>()
        .join("\n"))
}

struct GitProject {
    repo_root: PathBuf,
    project_relative: PathBuf,
    config_relative: PathBuf,
    head_oid: String,
}

struct HeadMaterialization {
    _temp: tempfile::TempDir,
    config_path: PathBuf,
    sources: BTreeMap<String, String>,
}

impl GitProject {
    fn open(project: &Project) -> Result<Self, DiagnosticSet> {
        let root_output = git_output(project.root_dir(), ["rev-parse", "--show-toplevel"])?;
        let repo_root_text = utf8_stdout("git rev-parse --show-toplevel", root_output)?;
        let repo_root = fs::canonicalize(repo_root_text.trim())
            .map_err(|error| git_error(format!("无法解析 Git 仓库根目录：{error}")))?;
        let project_root = fs::canonicalize(project.root_dir())
            .map_err(|error| git_error(format!("无法解析项目目录：{error}")))?;
        let config_path = fs::canonicalize(project.config_path())
            .map_err(|error| git_error(format!("无法解析项目配置：{error}")))?;
        let project_relative = project_root
            .strip_prefix(&repo_root)
            .map_err(|_| git_error("Coflow 项目不在当前 Git 工作区中"))?
            .to_path_buf();
        let config_relative = config_path
            .strip_prefix(&repo_root)
            .map_err(|_| git_error("项目配置不在当前 Git 工作区中"))?
            .to_path_buf();
        let head_oid = utf8_stdout(
            "git rev-parse HEAD",
            git_output(&repo_root, ["rev-parse", "HEAD^{commit}"])?,
        )?
        .trim()
        .to_string();
        Ok(Self {
            repo_root,
            project_relative,
            config_relative,
            head_oid,
        })
    }

    fn materialize_head(&self) -> Result<HeadMaterialization, DiagnosticSet> {
        let pathspec = if self.project_relative.as_os_str().is_empty() {
            ".".to_string()
        } else {
            crate::path_to_slash(&self.project_relative)
        };
        let output = git_output(
            &self.repo_root,
            [
                "ls-tree",
                "-r",
                "-z",
                "--name-only",
                &self.head_oid,
                "--",
                &pathspec,
            ],
        )?;
        let temp = tempfile::tempdir()
            .map_err(|error| git_error(format!("创建 HEAD 快照目录失败：{error}")))?;
        let mut sources = BTreeMap::new();
        for raw_path in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let git_path = std::str::from_utf8(raw_path)
                .map_err(|error| git_error(format!("Git 路径不是 UTF-8：{error}")))?;
            let relative = safe_git_path(git_path)?;
            let blob = git_output(
                &self.repo_root,
                ["show", &format!("{}:{git_path}", self.head_oid)],
            )?;
            if is_project_text_path(&relative) {
                if let (Ok(project_path), Ok(source)) = (
                    relative.strip_prefix(&self.project_relative),
                    std::str::from_utf8(&blob.stdout),
                ) {
                    sources.insert(crate::path_to_slash(project_path), source.to_string());
                }
            }
            let target = temp.path().join(&relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| git_error(format!("创建 HEAD 快照目录失败：{error}")))?;
            }
            fs::write(&target, blob.stdout)
                .map_err(|error| git_error(format!("写入 HEAD 快照 `{git_path}` 失败：{error}")))?;
        }
        Ok(HeadMaterialization {
            config_path: temp.path().join(&self.config_relative),
            _temp: temp,
            sources,
        })
    }

    fn is_ignored(&self, project_path: &str) -> Result<bool, DiagnosticSet> {
        let repo_path = self.project_relative.join(project_path);
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.repo_root)
            .args(["check-ignore", "--quiet", "--"])
            .arg(repo_path)
            .output()
            .map_err(|error| git_error(format!("无法执行 git check-ignore：{error}")))?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(git_command_error("git check-ignore", &output)),
        }
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

fn git_output<I, S>(cwd: &Path, args: I) -> Result<Output, DiagnosticSet>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| git_error(format!("无法执行 Git：{error}")))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(git_command_error("git", &output))
    }
}

fn utf8_stdout(command: &str, output: Output) -> Result<String, DiagnosticSet> {
    String::from_utf8(output.stdout)
        .map_err(|error| git_error(format!("{command} 输出不是 UTF-8：{error}")))
}

fn git_command_error(command: &str, output: &Output) -> DiagnosticSet {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
    git_error(if message.is_empty() {
        format!("{command} 执行失败")
    } else {
        format!("{command} 执行失败：{message}")
    })
}

fn git_error(message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic::error("GIT-DIFF", "GIT", message))
}
