use std::{collections::BTreeMap, fs, path::{Path, PathBuf, Component}};
use serde::{Serialize, Deserialize};
use serde_json::{Value, json};
use crate::{Project, ProjectConfig, SchemaConfig, SourceConfig, Runtime};
use super::{Failure, Result, Status, InputSnapshot, Arguments};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeSet {
    Files { protocol_version: u32, changes: Vec<FileOperation> },
    Operations { protocol_version: u32, ops: Vec<super::operations::Operation> },
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileOperation {
    Write { file: String, #[serde(default)] content: Option<String>, #[serde(default)] content_file: Option<String>, #[serde(default)] expected_fingerprint: Option<String> },
    Create { file: String, #[serde(default)] content: Option<String>, #[serde(default)] content_file: Option<String> },
    Delete { file: String, #[serde(default)] expected_fingerprint: Option<String> },
    AddInput { kind: String, path: String },
    RemoveInput { kind: String, path: String },
}

/// 候选项目在独立目录中执行；绝对和外部输入映射后仍保持相对拓扑。
/// 正式目录只在最终校验通过且快照一致时，由 staging 一次性发布。
#[derive(Debug)]
pub struct Candidate {
    temp: tempfile::TempDir,
    pub original: Project,
    pub snapshot: InputSnapshot,
    config_path: PathBuf,
    originals: BTreeMap<PathBuf, Option<Vec<u8>>>,
    pub operations: Vec<Value>,
}

impl Candidate {
    pub fn new(project: &Project, snapshot: &InputSnapshot) -> Result<Self> {
        let mut candidate = Self { temp: tempfile::tempdir()?, original: project.clone(), snapshot: snapshot.clone(),
            config_path: PathBuf::new(), originals: BTreeMap::new(), operations: Vec::new() };
        candidate.config_path = candidate.shadow(project.config_path());
        for file in &snapshot.files {
            candidate.track(&file.absolute)?;
            candidate.write(&file.absolute, &file.contents)?;
        }
        let config = candidate.map_config(project.config().clone(), true);
        candidate.write_config(&config)?;
        Ok(candidate)
    }
    pub fn project(&self) -> Result<Project> { Ok(Project::open_schema_only(Some(&self.config_path))?) }
    pub fn shadow(&self, original: &Path) -> PathBuf {
        let mut mapped = self.temp.path().join("inputs");
        for component in crate::normalize_path(original).components() {
            match component { Component::Prefix(p) => mapped.push(p.as_os_str().to_string_lossy().replace([':', '\\', '/'], "_")),
                Component::Normal(p) => mapped.push(p), _ => {} }
        }
        mapped
    }
    pub fn unshadow(&self, path: &Path) -> PathBuf {
        let mapped_root = self.shadow(self.original.root_dir());
        if let Ok(relative) = path.strip_prefix(&mapped_root) { return self.original.root_dir().join(relative); }
        for root in self.original.source_roots().iter().chain(std::iter::once(&self.original.config_path().to_path_buf())) {
            if let Ok(relative) = path.strip_prefix(self.shadow(root)) { return root.join(relative); }
        }
        // 跨目录候选路径以已记录的原始文件及祖先恢复，绝不发布到临时目录。
        for original in self.originals.keys() {
            for ancestor in original.ancestors() {
                if let Ok(relative) = path.strip_prefix(self.shadow(ancestor)) { return ancestor.join(relative); }
            }
        }
        path.to_path_buf()
    }
    fn map_config(&self, mut config: ProjectConfig, into_shadow: bool) -> ProjectConfig {
        let map = |p: &Path| if !p.is_absolute() { p.to_path_buf() } else if into_shadow { self.shadow(p) } else { self.unshadow(p) };
        config.schema = SchemaConfig::many(config.schema.paths().iter().map(|p| map(p)).collect());
        config.data = config.data.iter().map(|s| SourceConfig::from_path(map(s.path()))).collect();
        for dimension in config.dimensions.values_mut() { dimension.dir = dimension.dir.as_ref().map(|p| map(p)); }
        for target in &mut config.codegen { target.dir = map(&target.dir); }
        config
    }
    fn write_config(&self, config: &ProjectConfig) -> Result<()> {
        let text = serde_yaml::to_string(config).map_err(|e| Failure::invalid(e.to_string()))?;
        if let Some(parent) = self.config_path.parent() { fs::create_dir_all(parent)?; }
        fs::write(&self.config_path, text)?; Ok(())
    }
    pub fn track(&mut self, original: &Path) -> Result<()> {
        if !self.originals.contains_key(original) { self.originals.insert(original.to_path_buf(), optional_read(original)?); }
        Ok(())
    }
    pub fn write(&self, original: &Path, contents: &[u8]) -> Result<()> {
        let target = self.shadow(original);
        if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
        fs::write(target, contents)?; Ok(())
    }
    pub fn resolve(&self, file: &str, create: bool) -> Result<PathBuf> {
        let original = crate::resolve_existing_or_future_path(&self.original.resolve_path(Path::new(file)))?;
        let inside = crate::path_is_same_or_descendant(&original, self.original.root_dir());
        if !inside && !self.original.tracks_path(&original) { return Err(Failure::invalid(format!("path `{file}` is outside project input scope"))); }
        if self.original.config().codegen.iter().any(|t| crate::path_is_same_or_descendant(&original, &self.original.resolve_path(&t.dir))) {
            return Err(Failure::invalid("source writes cannot target generated directories"));
        }
        if original != self.original.config_path() && !original.extension().is_some_and(|e| e == "cft" || e == "cfd") { return Err(Failure::invalid("source file must be CFT or CFD")); }
        if !create && !self.shadow(&original).is_file() { return Err(Failure::missing(format!("file `{file}` does not exist in candidate"))); }
        Ok(original)
    }
    pub fn include(&mut self, original: &Path) -> Result<()> {
        let mut project = self.project()?;
        let path = self.shadow(original);
        if project.tracks_path(&path) { return Ok(()); }
        let relative = PathBuf::from(crate::project_path(project.root_dir(), &path));
        if original.extension().is_some_and(|e| e == "cft") {
            let mut paths = project.config().schema.paths().to_vec(); paths.push(relative);
            project.config.schema = SchemaConfig::many(paths);
        } else { project.config.data.push(SourceConfig::from_path(relative)); }
        self.write_config(project.config())
    }
    pub fn input(&mut self, kind: &str, path: &str, add: bool) -> Result<()> {
        if !["data", "schema"].contains(&kind) { return Err(Failure::invalid("input kind must be data or schema")); }
        let original = crate::resolve_existing_or_future_path(&self.original.resolve_path(Path::new(path)))?;
        let mut project = self.project()?;
        let target = self.shadow(&original);
        let mut paths = if kind == "schema" { project.config().schema.paths().to_vec() } else { project.config().data.iter().map(|s| s.path().clone()).collect() };
        let existing = paths.iter().position(|p| crate::normalize_path(&project.resolve_path(p)) == crate::normalize_path(&target));
        if add {
            if existing.is_some() || paths.iter().any(|p| {
                let p = project.resolve_path(p); crate::path_is_same_or_descendant(&p,&target) || crate::path_is_same_or_descendant(&target,&p)
            }) { return Err(Failure::invalid("duplicate or overlapping input")); }
            let extension = if kind == "schema" { "cft" } else { "cfd" };
            if original.is_dir() {
                fs::create_dir_all(&target)?;
                for entry in walkdir::WalkDir::new(&original) {
                    let entry = entry.map_err(|e| Failure::from(std::io::Error::other(e)))?;
                    if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == extension) {
                        self.track(entry.path())?; self.write(entry.path(), &fs::read(entry.path())?)?;
                    }
                }
            } else if original.is_file() { self.track(&original)?; self.write(&original,&fs::read(&original)?)?; }
            paths.push(PathBuf::from(crate::project_path(project.root_dir(), &target)));
        } else {
            let index = existing.ok_or_else(|| Failure::missing("input entry does not exist"))?;
            paths.remove(index);
        }
        if kind == "schema" { project.config.schema = SchemaConfig::many(paths); }
        else { project.config.data = paths.into_iter().map(SourceConfig::from_path).collect(); }
        self.write_config(project.config())
    }
    pub fn apply_files(&mut self, operations: &[FileOperation], carrier_dir: &Path) -> Result<()> {
        for (index, operation) in operations.iter().enumerate() {
            match operation {
                FileOperation::Write { file, content, content_file, expected_fingerprint } => {
                    let path = self.resolve(file,false)?; self.track(&path)?;
                    self.expect(&path, expected_fingerprint.as_deref())?;
                    self.write(&path, &read_content(content,content_file,carrier_dir)?)?;
                }
                FileOperation::Create { file, content, content_file } => {
                    let path = self.resolve(file,true)?; self.track(&path)?;
                    if self.shadow(&path).exists() || path.exists() { return Err(Failure::conflict(format!("file `{file}` already exists"))); }
                    self.write(&path,&read_content(content,content_file,carrier_dir)?)?; self.include(&path)?;
                }
                FileOperation::Delete { file, expected_fingerprint } => {
                    let path = self.resolve(file,false)?; self.track(&path)?; self.expect(&path,expected_fingerprint.as_deref())?;
                    fs::remove_file(self.shadow(&path))?;
                    let mut project = self.project()?;
                    let target = self.shadow(&path);
                    project.config.schema = SchemaConfig::many(project.config().schema.paths().iter().filter(|p| crate::normalize_path(&project.resolve_path(p)) != target).cloned().collect());
                    let data: Vec<_> = project.config().data.iter().filter(|s| crate::normalize_path(&project.resolve_path(s.path())) != target).cloned().collect();
                    project.config.data = data; self.write_config(project.config())?;
                }
                FileOperation::AddInput {kind,path} => self.input(kind,path,true)?,
                FileOperation::RemoveInput {kind,path} => self.input(kind,path,false)?,
            }
            self.operations.push(json!({"index": index, "status": "success", "operation": operation}));
        }
        Ok(())
    }
    fn expect(&self, path: &Path, expected: Option<&str>) -> Result<()> {
        if let Some(expected) = expected {
            if super::snapshot::fingerprint(&fs::read(self.shadow(path))?) != expected { return Err(Failure::conflict(format!("content precondition failed: {}",path.display()))); }
        }
        Ok(())
    }
    pub fn changes(&mut self) -> Result<Vec<coflow_staging::transaction::FileChange>> {
        let project = self.project()?;
        for entry in walkdir::WalkDir::new(self.temp.path().join("inputs")) {
            let entry = entry.map_err(|e| Failure::from(std::io::Error::other(e)))?;
            if entry.file_type().is_file() { let path = self.unshadow(entry.path()); self.track(&path)?; }
        }
        let config = self.map_config(project.config().clone(),false);
        let config_bytes = if serde_json::to_value(&config)? == serde_json::to_value(self.original.config())? {
            self.snapshot.files.iter().find(|f| f.absolute == self.original.config_path()).map(|f| f.contents.clone()).unwrap_or_default()
        } else { serde_yaml::to_string(&config).map_err(|e| Failure::invalid(e.to_string()))?.into_bytes() };
        let mut changes = Vec::new();
        for (path,before) in &self.originals {
            let after = if path == self.original.config_path() { Some(config_bytes.clone()) } else { optional_read(&self.shadow(path))? };
            if &after != before { changes.push(coflow_staging::transaction::FileChange { path: path.clone(), before: before.clone(), after }); }
        }
        Ok(changes)
    }
    pub fn finish(&mut self, args: &Arguments, force_preview: bool) -> Result<(Value, crate::DiagnosticSet)> {
        let project = self.project()?;
        let session = Runtime::new().open_data_session(project)?;
        let (validation, diagnostics) = super::rules::check(&session,args)?;
        let changes = self.changes()?;
        let dry_run = force_preview || args.flag("dry-run");
        let files: Vec<_> = changes.iter().map(|c| json!({"path": crate::project_path(self.original.root_dir(),&c.path),
            "change": if c.before.is_none() { "added" } else if c.after.is_none() { "deleted" } else { "modified" },
            "before": c.before.as_ref().map(|b| String::from_utf8_lossy(b)), "after": c.after.as_ref().map(|b| String::from_utf8_lossy(b))})).collect();
        let mut written = Vec::new();
        if diagnostics.is_empty() && !dry_run && !changes.is_empty() {
            self.snapshot.verify(&self.original)?;
            for (path,before) in &self.originals { if optional_read(path)? != *before { return Err(Failure::conflict(format!("input changed: {}",path.display()))); } }
            written = coflow_staging::transaction::publish(&changes).map_err(|e| Failure::new(if e.conflict { Status::Conflict } else { Status::Error }, "CLI-PUBLISH",
                format!("{}; written={:?}; restoration_failed={:?}",e.message,e.written,e.restoration_failed)))?;
        }
        Ok((json!({"changed": !changes.is_empty(), "written": !written.is_empty(), "dry_run": dry_run, "validation": validation,
            "operations": self.operations, "files": files, "written_files": written.iter().map(|p| crate::project_path(self.original.root_dir(),p)).collect::<Vec<_>>()}), diagnostics))
    }
}

fn read_content(content: &Option<String>, file: &Option<String>, base: &Path) -> Result<Vec<u8>> {
    match (content,file) { (Some(content),None) => Ok(content.as_bytes().to_vec()), (None,Some(file)) => Ok(fs::read(base.join(file))?),
        _ => Err(Failure::invalid("exactly one of content and content_file is required")) }
}
fn optional_read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) { Ok(v) => Ok(Some(v)), Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None), Err(e) => Err(e.into()) }
}
