use super::{Arguments, Failure, InputSnapshot, Result, Status};
use crate::{Project, ProjectConfig, Runtime, SchemaConfig, SourceConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeSet {
    Files {
        #[schemars(range(min = 1, max = 1))]
        protocol_version: u32,
        changes: Vec<FileOperation>,
    },
    Operations {
        #[schemars(range(min = 1, max = 1))]
        protocol_version: u32,
        ops: Vec<super::operations::Operation>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileOperation {
    Write {
        file: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        content_file: Option<String>,
        #[serde(default)]
        expected_fingerprint: Option<String>,
    },
    Create {
        file: String,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        content_file: Option<String>,
    },
    Delete {
        file: String,
        #[serde(default)]
        expected_fingerprint: Option<String>,
    },
    AddInput {
        kind: String,
        path: String,
    },
    RemoveInput {
        kind: String,
        path: String,
    },
}

/// 候选项目在独立目录中执行；绝对和外部输入映射后仍保持相对拓扑。
/// 正式目录只在最终校验通过且快照一致时，由 staging 一次性发布。
#[derive(Debug)]
pub(crate) struct Candidate {
    temp: tempfile::TempDir,
    pub original: Project,
    pub snapshot: InputSnapshot,
    config_path: PathBuf,
    // 正式配置源码与执行目录中的路径映射配置分离，发布时保留调用方提交的原文。
    config_contents: Vec<u8>,
    input_scopes: Vec<(Project, InputSnapshot)>,
    originals: BTreeMap<PathBuf, Option<Vec<u8>>>,
    pub operations: Vec<Value>,
    baseline: super::semantic_diff::SemanticSnapshot,
    pub materialized: Vec<Value>,
    pub automatic_updates: Vec<Value>,
    pub operation_details: Value,
    pub typed_references: Vec<super::typed_references::TypedReference>,
}

impl Candidate {
    pub fn new(project: &Project, snapshot: &InputSnapshot) -> Result<Self> {
        let mut candidate = Self {
            temp: tempfile::tempdir()?,
            original: project.clone(),
            snapshot: snapshot.clone(),
            config_path: PathBuf::new(),
            config_contents: Vec::new(),
            input_scopes: Vec::new(),
            originals: BTreeMap::new(),
            operations: Vec::new(),
            baseline: Default::default(),
            materialized: Vec::new(),
            automatic_updates: Vec::new(),
            operation_details: json!({}),
            typed_references: Vec::new(),
        };
        candidate.config_path = candidate.shadow(project.config_path());
        for file in &snapshot.files {
            candidate.track(&file.absolute)?;
            candidate.write(&file.absolute, &file.contents)?;
        }
        let config = snapshot
            .files
            .iter()
            .find(|file| file.absolute == project.config_path())
            .ok_or_else(|| {
                Failure::new(
                    Status::Error,
                    "CLI-CANDIDATE",
                    "project config is missing from input capture",
                )
            })?;
        candidate.set_config_source(config.contents.clone())?;
        candidate.baseline =
            super::semantic_diff::SemanticSnapshot::capture(&candidate.project()?, |path| {
                Ok(crate::project_path(
                    candidate.original.root_dir(),
                    &candidate.unshadow(path)?,
                ))
            })?;
        Ok(candidate)
    }
    pub fn project(&self) -> Result<Project> {
        Ok(Project::open_schema_only(Some(&self.config_path))?)
    }
    pub fn shadow(&self, original: &Path) -> PathBuf {
        let original = crate::normalize_path(original);
        let mut anchor = PathBuf::new();
        let mut suffix = PathBuf::new();
        for component in original.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
                Component::Normal(name) => suffix.push(name),
                _ => {}
            }
        }
        // 卷根保持独立身份，UNC 与不同盘符不会落入同一临时路径。
        self.temp
            .path()
            .join("inputs")
            .join(super::snapshot::fingerprint(
                anchor.as_os_str().as_encoded_bytes(),
            ))
            .join(suffix)
    }
    pub fn unshadow(&self, path: &Path) -> Result<PathBuf> {
        let project = self.original_view()?;
        for root in project
            .source_roots()
            .iter()
            .chain(self.original.source_roots().iter())
            .chain(self.originals.keys())
        {
            for ancestor in root.ancestors() {
                if let Ok(relative) = path.strip_prefix(self.shadow(ancestor)) {
                    // 精确匹配文件根时不能 join 空路径；Windows 会把它变成文件后的目录分隔符。
                    let original = if relative.as_os_str().is_empty() {
                        ancestor.to_path_buf()
                    } else {
                        ancestor.join(relative)
                    };
                    return crate::resolve_existing_or_future_path(&original).map_err(|error| {
                        Failure::new(
                            Status::Error,
                            "CLI-CANDIDATE",
                            format!(
                                "cannot map candidate path {} to {}: {error}",
                                path.display(),
                                original.display()
                            ),
                        )
                    });
                }
            }
        }
        Err(Failure::new(
            Status::Error,
            "CLI-CANDIDATE",
            format!(
                "candidate path has no original identity: {}",
                path.display()
            ),
        ))
    }
    fn original_view(&self) -> Result<Project> {
        let mut project = self.original.clone();
        project.config = serde_yaml::from_slice(&self.config_contents)
            .map_err(|e| Failure::new(Status::Invalid, "PROJECT-CONFIG-PARSE", e.to_string()))?;
        project.config_source = String::from_utf8(self.config_contents.clone())
            .map_err(|e| Failure::new(Status::Invalid, "PROJECT-CONFIG-PARSE", e.to_string()))?
            .into();
        Ok(project)
    }
    fn mapped_config(&self, project: &Project) -> ProjectConfig {
        let mut config = project.config().clone();
        let map = |p: &Path| self.shadow(&project.resolve_path(p));
        config.schema = if config.schema.is_list_shape() || config.schema.paths().len() != 1 {
            SchemaConfig::many(config.schema.paths().iter().map(|p| map(p)).collect())
        } else {
            SchemaConfig::one(map(&config.schema.paths()[0]))
        };
        config.data = config
            .data
            .iter()
            .map(|s| SourceConfig::from_path(map(s.path())))
            .collect();
        for dimension in config.dimensions.values_mut() {
            dimension.dir = dimension.dir.as_ref().map(|p| map(p));
        }
        for target in &mut config.codegen {
            target.dir = map(&target.dir);
        }
        config
    }
    fn set_config_source(&mut self, contents: Vec<u8>) -> Result<()> {
        // 解析失败的请求不能替换已经建立的候选配置身份。
        let _: ProjectConfig = serde_yaml::from_slice(&contents)
            .map_err(|e| Failure::new(Status::Invalid, "PROJECT-CONFIG-PARSE", e.to_string()))?;
        self.config_contents = contents;
        let project = self.original_view()?;
        let mapped = self.mapped_config(&project);
        let text = serde_yaml::to_string(&mapped).map_err(|e| Failure::request(e.to_string()))?;
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.config_path, text)?;
        // 新配置纳入的正式输入只复制一次；前序候选修改与删除不能被导入覆盖。
        let scope = InputSnapshot::capture(&project)?;
        for file in &scope.files {
            if file.absolute == self.original.config_path()
                || self.originals.contains_key(&file.absolute)
            {
                continue;
            }
            self.originals
                .insert(file.absolute.clone(), Some(file.contents.clone()));
            self.write(&file.absolute, &file.contents)?;
        }
        for root in project.source_roots().iter().filter(|root| root.is_dir()) {
            fs::create_dir_all(self.shadow(root))?;
        }
        self.input_scopes.push((project, scope));
        Ok(())
    }
    fn write_config(&mut self, config: &ProjectConfig) -> Result<()> {
        if serde_json::to_value(config)? == serde_json::to_value(self.original_view()?.config())? {
            return Ok(());
        }
        self.set_config_source(
            serde_yaml::to_string(config)
                .map_err(|e| Failure::request(e.to_string()))?
                .into_bytes(),
        )?;
        self.automatic_updates.push(json!({"kind":"inputs","file":crate::project_path(self.original.root_dir(),self.original.config_path())}));
        Ok(())
    }
    pub fn track(&mut self, original: &Path) -> Result<()> {
        if !self.originals.contains_key(original) {
            self.originals
                .insert(original.to_path_buf(), optional_read(original)?);
        }
        Ok(())
    }
    pub fn write(&self, original: &Path, contents: &[u8]) -> Result<()> {
        let target = self.shadow(original);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(target, contents)?;
        Ok(())
    }
    pub fn resolve(&mut self, file: &str, create: bool) -> Result<PathBuf> {
        let original =
            crate::resolve_existing_or_future_path(&self.original.resolve_path(Path::new(file)))?;
        let inside = crate::path_is_same_or_descendant(&original, self.original.root_dir());
        let candidate = self.original_view()?;
        if !inside
            && !source_scope_contains(&candidate, &original)
            && !source_scope_contains(&self.original, &original)
        {
            return Err(Failure::request(format!(
                "path `{file}` is outside project input scope"
            )));
        }
        if [self.original.clone(), candidate].iter().any(|project| {
            project.config().codegen.iter().any(|target| {
                crate::path_is_same_or_descendant(&original, &project.resolve_path(&target.dir))
            })
        }) {
            return Err(Failure::request(
                "source writes cannot target generated directories",
            ));
        }
        if original != self.original.config_path()
            && !original
                .extension()
                .is_some_and(|e| e == "cft" || e == "cfd")
        {
            return Err(Failure::request("source file must be CFT or CFD"));
        }
        if !self.originals.contains_key(&original) && original.is_file() {
            self.track(&original)?;
            self.write(&original, &fs::read(&original)?)?;
        }
        if !create && !self.shadow(&original).is_file() {
            return Err(Failure::missing(format!(
                "file `{file}` does not exist in candidate"
            )));
        }
        Ok(original)
    }
    pub fn include(&mut self, original: &Path) -> Result<()> {
        let mut project = self.original_view()?;
        if source_scope_contains(&project, original) {
            return Ok(());
        }
        let relative = PathBuf::from(crate::project_path(project.root_dir(), original));
        if original.extension().is_some_and(|e| e == "cft") {
            let mut paths = project.config().schema.paths().to_vec();
            paths.push(relative);
            project.config.schema = SchemaConfig::many(paths);
        } else {
            project.config.data.push(SourceConfig::from_path(relative));
        }
        self.write_config(project.config())
    }
    pub fn input(&mut self, kind: &str, path: &str, add: bool) -> Result<()> {
        if !["data", "schema"].contains(&kind) {
            return Err(Failure::request("input kind must be data or schema"));
        }
        let original =
            crate::resolve_existing_or_future_path(&self.original.resolve_path(Path::new(path)))?;
        let mut project = self.original_view()?;
        let mut paths = if kind == "schema" {
            project.config().schema.paths().to_vec()
        } else {
            project
                .config()
                .data
                .iter()
                .map(|s| s.path().clone())
                .collect()
        };
        let existing = paths
            .iter()
            .position(|p| crate::normalize_path(&project.resolve_path(p)) == original);
        if add {
            if existing.is_some()
                || paths.iter().any(|p| {
                    let p = project.resolve_path(p);
                    crate::path_is_same_or_descendant(&p, &original)
                        || crate::path_is_same_or_descendant(&original, &p)
                })
            {
                return Err(Failure::request("duplicate or overlapping input"));
            }
            paths.push(PathBuf::from(crate::project_path(
                project.root_dir(),
                &original,
            )));
        } else {
            let index = existing.ok_or_else(|| Failure::missing("input entry does not exist"))?;
            paths.remove(index);
        }
        if kind == "schema" {
            project.config.schema = SchemaConfig::many(paths);
        } else {
            project.config.data = paths.into_iter().map(SourceConfig::from_path).collect();
        }
        self.write_config(project.config())
    }
    pub fn apply_files(&mut self, operations: &[FileOperation], carrier_dir: &Path) -> Result<()> {
        let encoded = operations
            .iter()
            .map(super::json)
            .collect::<Result<Vec<_>>>()?;
        for (index, operation) in operations.iter().enumerate() {
            let result = (|| -> Result<()> {
                match operation {
                    FileOperation::Write {
                        file,
                        content,
                        content_file,
                        expected_fingerprint,
                    } => {
                        let path = self.resolve(file, false)?;
                        self.track(&path)?;
                        self.expect(&path, expected_fingerprint.as_deref())?;
                        let contents = read_content(content, content_file, carrier_dir)?;
                        if path == self.original.config_path() {
                            self.set_config_source(contents)?;
                        } else {
                            self.write(&path, &contents)?;
                            self.include(&path)?;
                        }
                    }
                    FileOperation::Create {
                        file,
                        content,
                        content_file,
                    } => {
                        let path = self.resolve(file, true)?;
                        self.track(&path)?;
                        if path == self.original.config_path() {
                            return Err(Failure::request(
                                "project configuration already exists; use source write",
                            ));
                        }
                        if self.shadow(&path).exists() || path.exists() {
                            return Err(Failure::conflict(format!("file `{file}` already exists")));
                        }
                        self.write(&path, &read_content(content, content_file, carrier_dir)?)?;
                        self.include(&path)?;
                    }
                    FileOperation::Delete {
                        file,
                        expected_fingerprint,
                    } => {
                        let path = self.resolve(file, false)?;
                        self.track(&path)?;
                        self.expect(&path, expected_fingerprint.as_deref())?;
                        if path == self.original.config_path() {
                            return Err(Failure::request(
                                "project configuration cannot be deleted",
                            ));
                        }
                        fs::remove_file(self.shadow(&path))?;
                        let mut project = self.original_view()?;
                        let target = path.clone();
                        project.config.schema.paths = project
                            .config()
                            .schema
                            .paths()
                            .iter()
                            .filter(|p| crate::normalize_path(&project.resolve_path(p)) != target)
                            .cloned()
                            .collect();
                        let data: Vec<_> = project
                            .config()
                            .data
                            .iter()
                            .filter(|s| {
                                crate::normalize_path(&project.resolve_path(s.path())) != target
                            })
                            .cloned()
                            .collect();
                        project.config.data = data;
                        self.write_config(project.config())?;
                    }
                    FileOperation::AddInput { kind, path } => self.input(kind, path, true)?,
                    FileOperation::RemoveInput { kind, path } => self.input(kind, path, false)?,
                }
                Ok(())
            })();
            if let Err(failure) = result {
                return Err(self.record_failed(index, &encoded, failure));
            }
            self.operations
                .push(json!({"index": index, "status": "success", "operation": operation}));
        }
        Ok(())
    }
    fn expect(&self, path: &Path, expected: Option<&str>) -> Result<()> {
        if let Some(expected) = expected {
            let contents = if path == self.original.config_path() {
                self.config_contents.clone()
            } else {
                fs::read(self.shadow(path))?
            };
            if super::snapshot::fingerprint(&contents) != expected {
                return Err(Failure::conflict(format!(
                    "content precondition failed: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }
    pub fn changes(&mut self) -> Result<Vec<coflow_staging::transaction::FileChange>> {
        for entry in walkdir::WalkDir::new(self.temp.path().join("inputs")) {
            let entry = entry.map_err(|e| Failure::from(std::io::Error::other(e)))?;
            if entry.file_type().is_file() {
                let path = self.unshadow(entry.path())?;
                self.track(&path)?;
            }
        }
        let mut changes = Vec::new();
        for (path, before) in &self.originals {
            let after = if path == self.original.config_path() {
                Some(self.config_contents.clone())
            } else {
                optional_read(&self.shadow(path))?
            };
            if &after != before {
                changes.push(coflow_staging::transaction::FileChange {
                    path: path.clone(),
                    before: before.clone(),
                    after,
                });
            }
        }
        Ok(changes)
    }
    pub(crate) fn remap_diagnostics(&self, diagnostics: &mut crate::DiagnosticSet) -> Result<()> {
        for diagnostic in &mut diagnostics.diagnostics {
            for label in diagnostic.primary.iter_mut().chain(&mut diagnostic.related) {
                let path = match &mut label.location {
                    crate::SourceLocation::FileSpan { path, .. }
                    | crate::SourceLocation::ProjectConfig { path, .. }
                    | crate::SourceLocation::Artifact { path } => path,
                };
                if path.starts_with(self.temp.path()) {
                    let original = self.unshadow(path)?;
                    diagnostic.message = diagnostic.message.replace(
                        &path.to_string_lossy().to_string(),
                        &original.to_string_lossy(),
                    );
                    *path = original;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn record_failed(
        &mut self,
        index: usize,
        operations: &[Value],
        failure: Failure,
    ) -> Failure {
        self.operations.push(json!({"index":index,"status":"failed","operation":operations[index],
            "reason":failure.diagnostics.diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>(),"details":failure.result}));
        for (index, operation) in operations.iter().enumerate().skip(index + 1) {
            self.operations
                .push(json!({"index":index,"status":"not_executed","operation":operation}));
        }
        failure
    }
    pub(crate) fn failure_result(&mut self, mut failure: Failure, args: &Arguments) -> Failure {
        if let Err(remap) = self.remap_diagnostics(&mut failure.diagnostics) {
            failure.diagnostics.extend(remap.diagnostics);
        }
        let mut result = failure.result.take().unwrap_or_else(|| json!({}));
        if result.get("written").is_none() {
            result["written"] = json!(false);
        }
        if result.get("written_files").is_none() {
            result["written_files"] = json!([]);
        }
        result["dry_run"] = json!(args.flag("dry-run"));
        result["operations"] = json!(self.operations);
        if result.get("files").is_none() {
            match self.changes() {
                Ok(changes) => {
                    result["changed"] = json!(!changes.is_empty());
                    result["files"] = self.file_report(&changes);
                }
                Err(report_failure) => {
                    result["changed"] = Value::Null;
                    result["files"] = Value::Null;
                    failure.diagnostics.extend(report_failure.diagnostics);
                }
            }
        }
        if result.get("validation").is_none() {
            result["validation"] =
                json!({"structure":"not_executed","rules":"not_executed","complete":false});
        }
        result["failed_operation"] = json!(self
            .operations
            .iter()
            .find(|operation| operation["status"] == "failed")
            .and_then(|operation| operation["index"].as_u64()));
        failure.result = Some(result);
        failure
    }
    fn file_report(&self, changes: &[coflow_staging::transaction::FileChange]) -> Value {
        json!(changes.iter().map(|c| json!({"path":crate::project_path(self.original.root_dir(),&c.path),
            "change":if c.before.is_none() {"added"} else if c.after.is_none() {"deleted"} else {"modified"},
            "before":c.before.as_ref().map(|b|String::from_utf8_lossy(b)),"after":c.after.as_ref().map(|b|String::from_utf8_lossy(b))})).collect::<Vec<_>>())
    }
    pub fn preview_schema(
        &mut self,
        args: &Arguments,
    ) -> Result<(Value, super::rules::Validation)> {
        // Schema 候选检查只编译声明；数据与业务规则不参与，也不会发布候选文件。
        let session = Runtime::open_schema_session(self.project()?)?;
        let mut validation = super::rules::check_schema(&session, args);
        self.remap_diagnostics(&mut validation.diagnostics)?;
        Self::refresh_explanation_messages(&mut validation);
        let changes = self.changes()?;
        let report = json!({"changed":!changes.is_empty(),"written":false,"dry_run":true,"validation":&validation.report,
            "operations":self.operations,"files":self.file_report(&changes),"written_files":[]});
        Ok((report, validation))
    }

    pub fn finish(
        &mut self,
        args: &Arguments,
        force_preview: bool,
    ) -> Result<(Value, super::rules::Validation)> {
        let project = self.project()?;
        let mut validation = match Runtime::new().open_data_session(project) {
            Ok(session) => {
                let typed = self.validate_typed_references(&session);
                if typed.is_empty() {
                    super::rules::check(&session, args)?
                } else {
                    let store = session.queries().diagnostics();
                    let mut diagnostics = store.as_set().clone();
                    let mut locations = (0..diagnostics.diagnostics.len())
                        .filter_map(|index| {
                            store
                                .logical_location(index)
                                .cloned()
                                .map(|location| (index, location))
                        })
                        .collect::<BTreeMap<_, _>>();
                    for (diagnostic, location) in typed {
                        locations.insert(diagnostics.diagnostics.len(), location);
                        diagnostics.push(diagnostic);
                    }
                    let mut validation = super::rules::Validation {
                        report: json!({"structure":"failed","rules":if args.flag("rules")||!args.all("rule").is_empty(){"skipped_dependency_error"}else{"not_executed"},
                        "complete":false,"diagnostic_count":diagnostics.diagnostics.len()}),
                        diagnostics,
                        logical_locations: locations,
                        explanations: Default::default(),
                    };
                    if args.flag("explain") {
                        super::rules::explain_diagnostics(&session, &mut validation);
                    }
                    validation
                }
            }
            Err(diagnostics) => {
                let failure = Failure::from(diagnostics.clone());
                if failure.status == Status::Error {
                    return Err(failure);
                }
                super::rules::Validation {
                    report: json!({"structure":"failed","rules":if args.flag("rules") || !args.all("rule").is_empty() {"skipped_dependency_error"} else {"not_executed"},
                        "complete":false,"diagnostic_count":diagnostics.diagnostics.len()}),
                    diagnostics,
                    logical_locations: Default::default(),
                    explanations: Default::default(),
                }
            }
        };
        self.remap_diagnostics(&mut validation.diagnostics)?;
        Self::refresh_explanation_messages(&mut validation);
        let changes = self.changes()?;
        let dry_run = force_preview || args.flag("dry-run");
        let mut report = json!({"changed":!changes.is_empty(),"written":false,"dry_run":dry_run,"validation":&validation.report,
            "operations":self.operations,"files":self.file_report(&changes),"written_files":[]});
        let after = super::semantic_diff::SemanticSnapshot::capture(&self.project()?, |path| {
            Ok(crate::project_path(
                self.original.root_dir(),
                &self.unshadow(path)?,
            ))
        })?;
        let difference = super::semantic_diff::compare(&self.baseline, &after);
        let config = crate::project_path(self.original.root_dir(), self.original.config_path());
        report["endpoints"] = json!({"base":{"config":config,"complete":self.baseline.available},"current":{"config":config,"complete":after.available}});
        report["changed_records"] = json!(difference.changed_records);
        report["changed_fields"] = json!(difference.changed_fields);
        report["materialized"] = json!(self.materialized);
        report["automatic_updates"] = json!(self.automatic_updates);
        let matched = self
            .operations
            .iter()
            .flat_map(|operation| {
                operation["operation"].get("record").into_iter().chain(
                    operation["details"]["targets"]
                        .as_array()
                        .into_iter()
                        .flatten(),
                )
            })
            .filter_map(|target| Some((target["type"].as_str()?, target["key"].as_str()?)))
            .collect::<std::collections::BTreeSet<_>>();
        report["matched_records"] = json!(matched.len());
        report["diff"] = super::semantic_diff::select(difference, args)?;
        // 源码差异默认只携带补丁，完整文件必须显式请求。
        for (file, change) in report["files"]
            .as_array_mut()
            .into_iter()
            .flatten()
            .zip(&changes)
        {
            if args.get("view").is_none_or(|view| view == "source") {
                file["patch"] = json!(crate::diff::unified_hunks(
                    &String::from_utf8_lossy(change.before.as_deref().unwrap_or_default()),
                    &String::from_utf8_lossy(change.after.as_deref().unwrap_or_default())
                )?);
            }
            if !args.flag("include-source") {
                if let Some(file) = file.as_object_mut() {
                    file.remove("before");
                    file.remove("after");
                }
            }
            if args.flag("summary") {
                if let Some(file) = file.as_object_mut() {
                    file.remove("patch");
                    file.remove("before");
                    file.remove("after");
                }
            }
        }
        if validation.passed() && !dry_run && !changes.is_empty() {
            let verify = (|| -> Result<()> {
                self.snapshot.verify(&self.original)?;
                for (project, scope) in &self.input_scopes {
                    scope.verify_scope(project)?;
                }
                for (path, before) in &self.originals {
                    if optional_read(path)? != *before {
                        return Err(Failure::conflict(format!(
                            "input changed: {}",
                            path.display()
                        )));
                    }
                }
                Ok(())
            })();
            verify.map_err(|failure| failure.with_result(report.clone()))?;
            let written = coflow_staging::transaction::publish(&changes).map_err(|error| {
                publication_failure(self.original.root_dir(), error, report.clone())
            })?;
            report["written"] = json!(true);
            report["written_files"] = json!(written
                .iter()
                .map(|path| crate::project_path(self.original.root_dir(), path))
                .collect::<Vec<_>>());
        }
        Ok((report, validation))
    }

    fn refresh_explanation_messages(validation: &mut super::rules::Validation) {
        // 候选路径映射之后同步上下文消息，不把临时执行路径留在公开解释中。
        for (index, explanation) in &mut validation.explanations {
            if let Some(diagnostic) = validation.diagnostics.diagnostics.get(*index) {
                explanation["message"] = json!(diagnostic.message);
            }
        }
    }
}

fn read_content(content: &Option<String>, file: &Option<String>, base: &Path) -> Result<Vec<u8>> {
    match (content, file) {
        (Some(content), None) => Ok(content.as_bytes().to_vec()),
        (None, Some(file)) => Ok(fs::read(base.join(file))?),
        _ => Err(Failure::request(
            "exactly one of content and content_file is required",
        )),
    }
}
fn optional_read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn source_scope_contains(project: &Project, path: &Path) -> bool {
    project.source_roots().iter().any(|root| {
        crate::normalized_path_identity(path) == crate::normalized_path_identity(root)
            || (root.is_dir() || !root.extension().is_some_and(|e| e == "cft" || e == "cfd"))
                && crate::path_is_same_or_descendant(path, root)
    })
}

pub(crate) fn publication_failure(
    root: &Path,
    error: coflow_staging::transaction::PublishFailure,
    mut report: Value,
) -> Failure {
    let paths = |items: &[PathBuf]| {
        items
            .iter()
            .map(|path| crate::project_path(root, path))
            .collect::<Vec<_>>()
    };
    report["written"] = json!(false);
    report["written_files"] = json!(paths(&error.written));
    report["restored_files"] = json!(paths(&error.restored));
    report["restoration_failed"] = json!(error
        .restoration_errors
        .iter()
        .map(
            |(path, reason)| json!({"path":crate::project_path(root,path),
        "reason":reason,"conflict":error.restoration_conflicts.contains(path)})
        )
        .collect::<Vec<_>>());
    Failure::new(
        if error.conflict {
            Status::Conflict
        } else {
            Status::Error
        },
        "CLI-PUBLISH",
        error.message,
    )
    .with_result(report)
}
