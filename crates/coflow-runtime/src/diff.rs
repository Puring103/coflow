use coflow_language::cft::CftSchema;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use gix::bstr::ByteSlice;
use gix::diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix::diff::blob::{diff_with_slider_heuristics, Algorithm, InternedInput, UnifiedDiff};

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
pub struct ProjectDiffDisplay {
    pub label: Option<String>,
    pub description: Option<String>,
    pub children: BTreeMap<String, ProjectDiffDisplay>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub display: Option<ProjectDiffDisplay>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts-export",
    ts(export, export_to = "../../frontend/src/bindings/")
)]
pub struct ProjectRecordSnapshot {
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub dimension: Option<String>,
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

type RecordStateKey = (RecordCoordinate, Option<String>, String);

#[derive(Debug, Clone)]
struct RecordState {
    // 文件路径与语义值分开存放，快照仅在确认变化后构造，
    // 避免为海量未变化记录重复克隆 CfdValue。
    file_path: String,
    values: BTreeMap<String, CfdValue>,
    dimension: Option<String>,
    actual_type: String,
    schema: Arc<CftSchema>,
}

// HEAD 分支在作用域线程内独立计算的结果；Loaded 携带的诊断已按
// “会话诊断在前、来源附注在后”排好序，合并点直接追加以保持原有诊断顺序。
enum HeadOutcome {
    Loaded {
        sources: BTreeMap<String, String>,
        records: BTreeMap<RecordStateKey, RecordState>,
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
    diff_against_revision(session, target_revision, "HEAD")
}

pub(crate) fn diff_against_revision(
    session: &ProjectSession,
    target_revision: u64,
    revision: &str,
) -> Result<ProjectDiff, DiagnosticSet> {
    let mut git = GitProject::open(&session.project)?;
    if revision != "HEAD" {
        git.head_oid = git
            .repo
            .rev_parse_single(revision)
            .map_err(|error| git_error(format!("无法解析比较基线 `{revision}`：{error}")))?
            .object()
            .map_err(|error| git_error(error.to_string()))?
            .peel_to_commit()
            .map_err(|error| git_error(error.to_string()))?
            .id;
    }
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
                .and_then(|project| Runtime::new().open_data_session(project));
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
    records: &mut BTreeMap<RecordStateKey, RecordState>,
) {
    // 仅保留 HEAD 已跟踪或未被忽略的新增记录，避免忽略文件放大语义差异。
    records.retain(|_, record| {
        head_sources.contains_key(&record.file_path) || !ignored.contains(&record.file_path)
    });
}

/// CLI 差异独立于桌面 DTO，以公共 ModelValue 表达有效值与显式赋值。
/// 即使任一端模型无法构建，源码快照仍可比较。
pub(crate) fn automation_diff(
    project: &Project,
    current: &crate::automation::InputSnapshot,
    args: &crate::automation::Arguments,
) -> crate::automation::Result<serde_json::Value> {
    use crate::automation::{
        semantic_diff::{compare, select, SemanticSnapshot},
        InputSnapshot,
    };
    use serde_json::json;
    let mut git = GitProject::open(project)?;
    let revision = args.get("base").unwrap_or("HEAD");
    git.head_oid = git
        .repo
        .rev_parse_single(revision)
        .map_err(|error| git_error(error.to_string()))?
        .object()
        .map_err(|error| git_error(error.to_string()))?
        .peel_to_commit()
        .map_err(|error| git_error(error.to_string()))?
        .id;
    let head = git.materialize_head()?;
    let paths = git.diff_paths();
    let current_sources = current
        .files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                String::from_utf8_lossy(&file.contents).into_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut head_sources = head.sources.clone();
    let mut endpoint_errors = Vec::new();
    let mut missing_inputs = Vec::new();
    let comparison_root = git.repo_root.join(&git.project_relative);
    let historical = Project::open_schema_only(Some(&head.config_path)).and_then(|mut project| {
        // 未还原输入映射为快照内的缺失路径，既保留其他输入，也不读取工作区补全历史。
        let mut rebase = |path: &Path| {
            let original = crate::normalize_path(&git.repo_root.join(&git.project_relative).join(path));
            match original.strip_prefix(&git.repo_root) {
                Ok(relative) => {
                    let restored = head.temp.path().join(relative);
                    if !restored.exists() { missing_inputs.push(json!({"path":crate::project_path(&comparison_root,&original),"reason":"input is not present in the selected revision"})); }
                    restored
                }
                Err(_) => {
                    missing_inputs.push(json!({"path":crate::project_path(&comparison_root,&original),"reason":"input is outside the selected repository"}));
                    head.temp.path().join("unavailable-inputs").join(format!("input-{}",missing_inputs.len()))
                }
            }
        };
        for path in &mut project.config.schema.paths { *path = rebase(path); }
        for source in &mut project.config.data { *source = crate::SourceConfig::from_path(rebase(source.path())); }
        for dimension in project.config.dimensions.values_mut() { if let Some(dir) = &mut dimension.dir { *dir = rebase(dir); } }
        Ok(project)
    });
    let before = match historical {
        Ok(historical) => {
            let capture = InputSnapshot::capture(&historical)?;
            head_sources = capture
                .files
                .iter()
                .filter_map(|file| {
                    paths
                        .diff_path(historical.root_dir(), &file.path, Some(head.temp.path()))
                        .map(|path| (path, String::from_utf8_lossy(&file.contents).into_owned()))
                })
                .collect();
            SemanticSnapshot::capture(&historical, |path| {
                paths
                    .diff_path(
                        historical.root_dir(),
                        &crate::project_path(historical.root_dir(), path),
                        Some(head.temp.path()),
                    )
                    .ok_or_else(|| {
                        crate::automation::Failure::request(
                            "historical source has no repository identity",
                        )
                    })
            })?
        }
        Err(error) => {
            endpoint_errors.extend(
                error.diagnostics.into_iter().map(
                    |error| json!({"endpoint":"base","code":error.code,"message":error.message}),
                ),
            );
            SemanticSnapshot::default()
        }
    };
    let after = SemanticSnapshot::capture(project, |path| {
        Ok(crate::project_path(project.root_dir(), path))
    })?;
    for (endpoint, snapshot) in [("base", &before), ("current", &after)] {
        endpoint_errors.extend(snapshot.diagnostics.diagnostics.iter().map(|error| {
            let message = if endpoint == "base" {
                error.message.replace(
                    &head.temp.path().to_string_lossy().to_string(),
                    &git.repo_root.to_string_lossy(),
                )
            } else {
                error.message.clone()
            };
            json!({"endpoint":endpoint,"code":error.code,"message":message})
        }));
    }
    let mut difference = compare(&before, &after);
    if !missing_inputs.is_empty() {
        difference.semantic_available = false;
    }
    let mut result = select(difference, args)?;
    let mut files = source_diffs(&BTreeSet::new(), &head_sources, &current_sources)?;
    if !args.flag("include-source") || args.flag("summary") {
        for file in &mut files {
            file.before = None;
            file.after = None;
        }
    }
    if args.flag("summary") {
        result["files"] = json!(files
            .iter()
            .map(|file| json!({"path":file.path,"change":file.change}))
            .collect::<Vec<_>>());
    } else if args.get("view").is_none_or(|view| view == "source") {
        result["files"] = json!(files);
    }
    result["base"] = json!(git.head_oid.to_string());
    result["endpoints"] = json!({"base":{"config":crate::project_path(project.root_dir(), project.config_path()),"complete":before.available&&missing_inputs.is_empty(),"missing_inputs":missing_inputs},
        "current":{"config":crate::project_path(project.root_dir(),project.config_path()),"complete":after.available}});
    result["diagnostics"] = json!(endpoint_errors);
    Ok(result)
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
) -> BTreeMap<RecordStateKey, RecordState> {
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
        let values = record
            .fields()
            .iter()
            .map(|(name, value)| (name.to_string(), normalized_semantic_value(value)))
            .collect::<BTreeMap<_, _>>();
        records.insert(
            (coordinate.clone(), None, String::new()),
            RecordState {
                file_path,
                values,
                dimension: None,
                actual_type: record.actual_type().to_string(),
                schema: Arc::clone(&session.schema),
            },
        );
        // 覆盖值按真实来源文件和维度独立比较，不混入基础记录。
        for (field_name, dimensions) in &record.dimension_fields {
            for (variant, value) in &dimensions.variants {
                let Some(source) = session.source_data.dimension_source(
                    session.schema(),
                    coordinate.actual_type(),
                    coordinate.key(),
                    field_name,
                    &dimensions.dimension,
                    variant,
                ) else {
                    continue;
                };
                let Some(path) = file_path_cache
                    .entry(source.display_path.clone())
                    .or_insert_with(|| git.diff_path(root, &source.display_path, snapshot))
                    .clone()
                else {
                    continue;
                };
                let dimension = dimensions.dimension.to_string();
                let state = records
                    .entry((coordinate.clone(), Some(dimension.clone()), path.clone()))
                    .or_insert_with(|| RecordState {
                        file_path: path,
                        values: BTreeMap::new(),
                        dimension: Some(dimension),
                        actual_type: record.actual_type().to_string(),
                        schema: Arc::clone(&session.schema),
                    });
                let name = format!("{}[{}={}]", field_name, dimensions.dimension, variant);
                state
                    .values
                    .insert(name, normalized_semantic_value(&value.value));
            }
        }
    }
    records
}

// 展示元数据绑定各自端点的 schema，删除字段和旧类型仍保留原始 label/描述。
fn diff_display(
    schema: &CftSchema,
    actual_type: &str,
    field: &str,
    value: &CfdValue,
) -> ProjectDiffDisplay {
    let display = schema
        .field(actual_type, field)
        .and_then(|field| field.display.as_ref());
    let mut display = ProjectDiffDisplay {
        label: display.and_then(|display| display.label.clone()),
        description: display.and_then(|display| display.description.clone()),
        children: BTreeMap::new(),
    };
    fn children(schema: &CftSchema, value: &CfdValue) -> BTreeMap<String, ProjectDiffDisplay> {
        match value {
            CfdValue::OptionSome(inner) => children(schema, inner),
            CfdValue::Object(object) => object
                .fields()
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string(),
                        diff_display(schema, object.actual_type(), name, value),
                    )
                })
                .collect(),
            CfdValue::Array(items) => items
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    (
                        index.to_string(),
                        ProjectDiffDisplay {
                            label: None,
                            description: None,
                            children: children(schema, value),
                        },
                    )
                })
                .collect(),
            CfdValue::Dict(items) => items
                .iter()
                .map(|(key, value)| {
                    (
                        crate::dict_key_path_text(key),
                        ProjectDiffDisplay {
                            label: None,
                            description: None,
                            children: children(schema, value),
                        },
                    )
                })
                .collect(),
            _ => BTreeMap::new(),
        }
    }
    display.children = children(schema, value);
    display
}

fn normalized_semantic_value(value: &CfdValue) -> CfdValue {
    match value {
        CfdValue::FormattedString(value) => CfdValue::FormattedString(crate::CfdFormattedString {
            source: normalized_line_endings(&value.source),
            rendered: normalized_line_endings(&value.rendered),
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
    before: &BTreeMap<RecordStateKey, RecordState>,
    after: &BTreeMap<RecordStateKey, RecordState>,
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
        .filter_map(|key| match (before.get(&key), after.get(&key)) {
            (None, Some(after)) => Some(ProjectRecordDiff {
                coordinate: key.0,
                change: ProjectDiffChange::Added,
                before: None,
                after: Some(snapshot_of(after)),
                fields: field_diffs(&BTreeMap::new(), &after.values),
            }),
            (Some(before), None) => Some(ProjectRecordDiff {
                coordinate: key.0,
                change: ProjectDiffChange::Deleted,
                before: Some(snapshot_of(before)),
                after: None,
                fields: field_diffs(&before.values, &BTreeMap::new()),
            }),
            (Some(before), Some(after))
                if before.values != after.values || before.file_path != after.file_path =>
            {
                Some(ProjectRecordDiff {
                    coordinate: key.0,
                    change: ProjectDiffChange::Modified,
                    before: Some(snapshot_of(before)),
                    after: Some(snapshot_of(after)),
                    fields: field_diffs(&before.values, &after.values),
                })
            }
            _ => None,
        })
        .collect()
}

fn snapshot_of(state: &RecordState) -> ProjectRecordSnapshot {
    // 变化记录的唯一克隆点，由值映射生成有序快照。
    ProjectRecordSnapshot {
        file_path: state.file_path.clone(),
        dimension: state.dimension.clone(),
        values: state
            .values
            .iter()
            .map(|(path, value)| {
                // 仅为确认变化的记录构造展示元数据，未变化记录只持有共享 schema。
                let name = if state.dimension.is_some() {
                    path.split('[').next().unwrap_or(path)
                } else {
                    path
                };
                let mut display = diff_display(&state.schema, &state.actual_type, name, value);
                if state.dimension.is_some() {
                    display.label = Some(format!(
                        "{}{}",
                        display.label.as_deref().unwrap_or(name),
                        &path[name.len()..]
                    ));
                }
                ProjectDiffValue {
                    path: path.clone(),
                    value: value.clone(),
                    display: Some(display),
                }
            })
            .collect(),
    }
}

fn field_diffs(
    before: &BTreeMap<String, CfdValue>,
    after: &BTreeMap<String, CfdValue>,
) -> Vec<ProjectFieldDiff> {
    let mut changes = Vec::new();
    for path in before.keys().chain(after.keys()).collect::<BTreeSet<_>>() {
        value_diffs(path, before.get(path), after.get(path), &mut changes);
    }
    changes
}

// 嵌套值按稳定字段/索引/字典键比较，前端可直接定位并展开变化的子字段。
fn value_diffs(
    path: &str,
    before: Option<&CfdValue>,
    after: Option<&CfdValue>,
    changes: &mut Vec<ProjectFieldDiff>,
) {
    if before == after {
        return;
    }
    match (before, after) {
        (Some(CfdValue::OptionSome(left)), Some(CfdValue::OptionSome(right))) => {
            value_diffs(path, Some(left), Some(right), changes);
            return;
        }
        (Some(CfdValue::Object(left)), Some(CfdValue::Object(right)))
            if left.actual_type() == right.actual_type() =>
        {
            for name in left
                .fields()
                .keys()
                .chain(right.fields().keys())
                .collect::<BTreeSet<_>>()
            {
                value_diffs(
                    &format!("{path}.{name}"),
                    left.fields().get(name),
                    right.fields().get(name),
                    changes,
                );
            }
            return;
        }
        (Some(CfdValue::Array(left)), Some(CfdValue::Array(right))) => {
            for index in 0..left.len().max(right.len()) {
                value_diffs(
                    &format!("{path}[{index}]"),
                    left.get(index),
                    right.get(index),
                    changes,
                );
            }
            return;
        }
        (Some(CfdValue::Dict(left)), Some(CfdValue::Dict(right))) => {
            let left: BTreeMap<_, _> = left
                .iter()
                .map(|(key, value)| (crate::dict_key_path_text(key), value))
                .collect();
            let right: BTreeMap<_, _> = right
                .iter()
                .map(|(key, value)| (crate::dict_key_path_text(key), value))
                .collect();
            for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
                value_diffs(
                    &format!("{path}[{key}]"),
                    left.get(key).copied(),
                    right.get(key).copied(),
                    changes,
                );
            }
            // 字典重排没有子值变化，仍保留容器级差异。
            if left == right {
                changes.push(ProjectFieldDiff {
                    path: path.into(),
                    change: ProjectDiffChange::Modified,
                    before: before.cloned(),
                    after: after.cloned(),
                });
            }
            return;
        }
        _ => {}
    }
    changes.push(ProjectFieldDiff {
        path: path.to_string(),
        change: if before.is_none() {
            ProjectDiffChange::Added
        } else if after.is_none() {
            ProjectDiffChange::Deleted
        } else {
            ProjectDiffChange::Modified
        },
        before: before.cloned(),
        after: after.cloned(),
    });
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

pub(crate) fn unified_hunks(before: &str, after: &str) -> Result<String, DiagnosticSet> {
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
                    .chain(config.data.iter().map(crate::SourceConfig::path))
                    .chain(
                        config
                            .dimensions
                            .values()
                            .filter_map(|dimension| dimension.dir.as_ref()),
                    );
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
        for dimension in project.config.dimensions.values_mut() {
            if let Some(dir) = &mut dimension.dir {
                *dir = rebase(dir)?;
            }
        }

        Ok(())
    }
}

fn is_project_text_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name == "coflow.enum.lock.json")
        || matches!(
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
