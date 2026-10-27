use super::semantic_diff::SemanticSnapshot;
use super::{Arguments, Failure, InputSnapshot, Result, Status};
use crate::{Project, ProjectConfig, Runtime, SchemaConfig, SourceConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChangeSet {
    Files {
        #[schemars(range(min = 1, max = 1))]
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_revision: Option<String>,
        changes: Vec<FileOperation>,
    },
    Operations {
        #[schemars(range(min = 1, max = 1))]
        protocol_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_revision: Option<String>,
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

/// 一次候选事务共享编译结果、文件语法树和身份索引；只更新实际变化的文件。
#[derive(Debug, Default)]
struct OperationCache {
    schema: Option<crate::ProjectSchemaSession>,
    files: BTreeMap<PathBuf, Arc<CandidateSource>>,
    sources_loaded: bool,
    records: BTreeMap<(String, String), Vec<(PathBuf, usize)>>,
    malformed: BTreeSet<PathBuf>,
    #[cfg(test)]
    stats: CacheStats,
}

#[cfg(test)]
#[derive(Debug, Default)]
struct CacheStats {
    schema_compilations: usize,
    source_discoveries: usize,
    cfd_parses: usize,
}

#[derive(Debug)]
pub(crate) struct CandidateSource {
    pub absolute: PathBuf,
    pub path: String,
    pub text: String,
    pub ast: coflow_language::cfd::CfdAst,
    pub malformed: bool,
}

impl OperationCache {
    fn replace_source(&mut self, path: PathBuf, source: Option<CandidateSource>) {
        self.malformed.remove(&path);
        if source.as_ref().is_some_and(|source| source.malformed) {
            self.malformed.insert(path.clone());
        }
        if let Some(previous) = self.files.get(&path) {
            for record in &previous.ast.records {
                let identity = (record.type_name.clone(), record.id.clone());
                if let Some(locations) = self.records.get_mut(&identity) {
                    locations.retain(|(file, _)| file != &path);
                    if locations.is_empty() {
                        self.records.remove(&identity);
                    }
                }
            }
        }
        if let Some(source) = source {
            for (index, record) in source.ast.records.iter().enumerate() {
                self.records
                    .entry((record.type_name.clone(), record.id.clone()))
                    .or_default()
                    .push((path.clone(), index));
            }
            self.files.insert(path, Arc::new(source));
            #[cfg(test)]
            {
                self.stats.cfd_parses += 1;
            }
        } else {
            self.files.remove(&path);
        }
    }
}

/// 候选项目在独立目录中执行；绝对和外部输入映射后仍保持相对拓扑。
/// 正式目录只在最终校验通过且快照一致时，由 staging 一次性发布。
#[derive(Debug)]
pub(crate) struct Candidate {
    _temp: tempfile::TempDir,
    temp_root: PathBuf,
    cache: RefCell<OperationCache>,
    pub original: Project,
    pub snapshot: InputSnapshot,
    config_path: PathBuf,
    // 正式配置源码与执行目录中的路径映射配置分离，发布时保留调用方提交的原文。
    config_contents: Vec<u8>,
    input_scopes: Vec<(Project, InputSnapshot)>,
    originals: BTreeMap<PathBuf, Option<Vec<u8>>>,
    source_entries: BTreeMap<PathBuf, PathBuf>,
    pub operations: Vec<Value>,
    baseline: SemanticSnapshot,
    pub materialized: Vec<Value>,
    pub automatic_updates: Vec<Value>,
    pub operation_details: Value,
    pub typed_references: Vec<super::typed_references::TypedReference>,
}

impl Candidate {
    pub fn new(project: &Project, snapshot: &InputSnapshot) -> Result<Self> {
        let temp = tempfile::tempdir()?;
        // 临时目录可能使用 Windows 短路径或系统别名，候选映射统一使用规范路径。
        let temp_root = crate::canonicalize_path(temp.path())?;
        let mut candidate = Self {
            _temp: temp,
            temp_root,
            cache: RefCell::new(OperationCache::default()),
            original: project.clone(),
            snapshot: snapshot.clone(),
            config_path: PathBuf::new(),
            config_contents: Vec::new(),
            input_scopes: Vec::new(),
            originals: BTreeMap::new(),
            source_entries: snapshot
                .files
                .iter()
                .filter(|file| {
                    (file.kind == "schema" && file.absolute.extension().is_none_or(|e| e != "cft"))
                        || (file.kind == "data"
                            && file.absolute.extension().is_none_or(|e| e != "cfd"))
                })
                .map(|file| (file.absolute.clone(), file.selected.clone()))
                .collect(),
            operations: Vec::new(),
            baseline: SemanticSnapshot::default(),
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
            SemanticSnapshot::capture_from_schema(&candidate.schema_session()?, None, |path| {
                Ok(crate::project_path(
                    candidate.original.root_dir(),
                    &candidate.unshadow(path)?,
                ))
            })?;
        Ok(candidate)
    }
    pub(crate) fn schema_session(&self) -> Result<crate::ProjectSchemaSession> {
        let project = self.project()?;
        let mut cache = self.cache.borrow_mut();
        let mut session = if let Some(session) = &cache.schema {
            session.clone()
        } else {
            let session = Runtime::open_schema_session(project.clone())?;
            cache.schema = Some(session.clone());
            #[cfg(test)]
            {
                cache.stats.schema_compilations += 1;
            }
            session
        };
        // 新增数据输入不改变声明，但读取会话必须采用当前候选的输入范围。
        session.project = project;
        Ok(session)
    }

    pub(crate) fn data_session(&self) -> Result<crate::ReadOnlyProjectSession> {
        Ok(crate::ReadOnlyProjectSession {
            session: crate::sessions::session_build::open_data_session_from_schema(
                self.schema_session()?,
            )?,
        })
    }

    fn ensure_data_sources(&self) -> Result<()> {
        if !self.cache.borrow().sources_loaded {
            let snapshot = InputSnapshot::capture(&self.project()?)?;
            let mut cache = self.cache.borrow_mut();
            cache.files.clear();
            cache.records.clear();
            cache.malformed.clear();
            #[cfg(test)]
            {
                cache.stats.source_discoveries += 1;
            }
            for file in snapshot
                .files
                .into_iter()
                .filter(|file| file.kind == "data")
            {
                let text = String::from_utf8(file.contents)
                    .map_err(|error| Failure::request(error.to_string()))?;
                let (ast, errors) = coflow_language::cfd::parse_cfd(&text);
                cache.replace_source(
                    file.absolute.clone(),
                    Some(CandidateSource {
                        absolute: file.absolute,
                        path: file.path,
                        text,
                        ast,
                        malformed: !errors.is_empty(),
                    }),
                );
            }
            cache.sources_loaded = true;
        }
        Ok(())
    }

    pub(crate) fn data_sources(&self) -> Result<Vec<Arc<CandidateSource>>> {
        self.ensure_data_sources()?;
        Ok(self.cache.borrow().files.values().cloned().collect())
    }

    pub(crate) fn cached_record(
        &self,
        target: &super::RecordTarget,
    ) -> Result<(PathBuf, coflow_language::cfd::CfdRecord, String)> {
        self.ensure_data_sources()?;
        let cache = self.cache.borrow();
        let files = &cache.files;
        if let Some(source) = cache.malformed.first().and_then(|path| files.get(path)) {
            return Err(Failure::request(format!(
                "cannot edit malformed CFD `{}`",
                source.path
            )));
        }
        let locations = cache
            .records
            .get(&(target.actual_type.clone(), target.id.clone()));
        match locations.map(Vec::as_slice) {
            Some([(path, index)]) => {
                let source = &files[path];
                Ok((
                    path.clone(),
                    source.ast.records[*index].clone(),
                    source.text.clone(),
                ))
            }
            Some(_) => Err(Failure::conflict("record identity is ambiguous")),
            None => Err(Failure::missing(format!(
                "record `{}::{}` does not exist",
                target.actual_type, target.id
            ))),
        }
    }

    pub(crate) fn refresh_source(&self, path: &Path, text: Option<String>) -> Result<()> {
        if path.extension().is_some_and(|extension| extension == "cft") {
            self.cache.borrow_mut().schema = None;
            return Ok(());
        }
        if !self.cache.borrow().sources_loaded
            || path.extension().is_none_or(|extension| extension != "cfd")
        {
            return Ok(());
        }
        let mut cache = self.cache.borrow_mut();
        if cache
            .files
            .get(path)
            .is_some_and(|source| Some(&source.text) == text.as_ref())
        {
            return Ok(());
        }
        let source = if let Some(text) = text {
            let (ast, errors) = coflow_language::cfd::parse_cfd(&text);
            Some(CandidateSource {
                absolute: path.into(),
                path: crate::project_path(self.original.root_dir(), &self.unshadow(path)?),
                text,
                ast,
                malformed: !errors.is_empty(),
            })
        } else {
            None
        };
        cache.replace_source(path.into(), source);
        Ok(())
    }

    pub(crate) fn publish_writer(&self, writer: &crate::cfd_loader::CfdWriter) -> Result<()> {
        let updates = writer.source_overrides()?;
        writer.publish()?;
        for update in updates {
            self.refresh_source(
                &update.normalized_path,
                (!update.deleted).then_some(update.source),
            )?;
        }
        Ok(())
    }

    pub fn project(&self) -> Result<Project> {
        Ok(Project::open_schema_only(Some(&self.config_path))?)
    }
    pub fn shadow(&self, original: &Path) -> PathBuf {
        let original = crate::normalize_path(original);
        // 使用实际被发现的链接入口，既保留语言扩展名，也不与真实同名文件碰撞。
        let selected = self.source_entries.get(&original).unwrap_or(&original);
        let mut anchor = PathBuf::new();
        let mut suffix = PathBuf::new();
        for component in selected.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => anchor.push(component.as_os_str()),
                Component::Normal(name) => suffix.push(name),
                _ => {}
            }
        }
        // 卷根保持独立身份，UNC 与不同盘符不会落入同一临时路径。
        self.temp_root
            .join("inputs")
            .join(super::snapshot::fingerprint(
                anchor.as_os_str().as_encoded_bytes(),
            ))
            .join(suffix)
    }
    pub fn unshadow(&self, path: &Path) -> Result<PathBuf> {
        // 非标准扩展名实体的临时语言路径必须先做精确反向映射。
        if let Some(original) = self
            .source_entries
            .keys()
            .find(|original| self.shadow(original) == path)
        {
            return Ok(original.clone());
        }
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
        let previous_config = self.original_view().ok().map(|project| project.config);
        self.config_contents = contents;
        let project = self.original_view()?;
        let mut cache = self.cache.borrow_mut();
        if previous_config
            .as_ref()
            .map(|config| {
                serde_json::to_value((&config.schema, &config.dimensions, &config.codegen))
            })
            .transpose()?
            != Some(serde_json::to_value((
                &project.config.schema,
                &project.config.dimensions,
                &project.config.codegen,
            ))?)
            || cache
                .schema
                .as_ref()
                .is_some_and(crate::sessions::session::ProjectSchemaSession::has_diagnostics)
        {
            cache.schema = None;
        }
        cache.files.clear();
        cache.sources_loaded = false;
        cache.records.clear();
        cache.malformed.clear();
        drop(cache);
        let mapped = self.mapped_config(&project);
        let text = serde_yaml::to_string(&mapped).map_err(|e| Failure::request(e.to_string()))?;
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.config_path, text)?;
        // 新配置纳入的正式输入只复制一次；前序候选修改与删除不能被导入覆盖。
        let scope = InputSnapshot::capture(&project)?;
        for file in &scope.files {
            if (file.kind == "schema" && file.absolute.extension().is_none_or(|e| e != "cft"))
                || (file.kind == "data" && file.absolute.extension().is_none_or(|e| e != "cfd"))
            {
                self.source_entries
                    .insert(file.absolute.clone(), file.selected.clone());
            }
        }
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
        fs::write(&target, contents)?;
        self.refresh_source(&target, String::from_utf8(contents.to_vec()).ok())?;
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
            && !self.source_entries.contains_key(&original)
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
                        let contents =
                            read_content(content.as_ref(), content_file.as_ref(), carrier_dir)?;
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
                        self.write(
                            &path,
                            &read_content(content.as_ref(), content_file.as_ref(), carrier_dir)?,
                        )?;
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
                        self.refresh_source(&self.shadow(&path), None)?;
                        let mut project = self.original_view()?;
                        let target = path;
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
            let actual = super::snapshot::fingerprint(&contents);
            if actual != expected {
                return Err(Failure::conflict(format!("content precondition failed: {}", path.display()))
                    .with_result(json!({"precondition":{"kind":"fingerprint", "expected": expected, "actual": actual}})));
            }
        }
        Ok(())
    }
    pub fn changes(&mut self) -> Result<Vec<coflow_staging::transaction::FileChange>> {
        for entry in walkdir::WalkDir::new(self.temp_root.join("inputs")) {
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
                if path.starts_with(&self.temp_root) {
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
        mut failure: Failure,
    ) -> Failure {
        let mut details = failure.result.take().unwrap_or_else(|| json!({}));
        details["operation_index"] = json!(index);
        details["target"] = operations[index]
            .get("record")
            .or_else(|| operations[index].get("file"))
            .or_else(|| operations[index].get("selector"))
            .cloned()
            .unwrap_or(Value::Null);
        failure.result = Some(details);
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
        result["revision_before"] = json!(self.snapshot.revision());
        result["operations"] = json!(self.operations);
        if result.get("files").is_none() {
            match self
                .changes()
                .and_then(|changes| Ok((!changes.is_empty(), self.file_report(&changes, args)?)))
            {
                Ok((changed, files)) => {
                    result["changed"] = json!(changed);
                    result["files"] = files;
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
    /// 成功、失败与声明预览共享同一源码报告；默认补丁只保留变化附近的上下文。
    fn file_report(
        &self,
        changes: &[coflow_staging::transaction::FileChange],
        args: &Arguments,
    ) -> Result<Value> {
        let mut files = Vec::with_capacity(changes.len());
        for change in changes {
            let mut file = json!({"path":crate::project_path(self.original.root_dir(), &change.path),
                "change":if change.before.is_none() {"added"} else if change.after.is_none() {"deleted"} else {"modified"}});
            if !args.flag("summary") {
                if args.get("view").is_none_or(|view| view == "source") {
                    file["patch"] = json!(crate::inspection::diff::unified_hunks(
                        &String::from_utf8_lossy(change.before.as_deref().unwrap_or_default()),
                        &String::from_utf8_lossy(change.after.as_deref().unwrap_or_default())
                    )?);
                }
                if args.flag("include-source") {
                    file["before"] = json!(change
                        .before
                        .as_ref()
                        .map(|bytes| String::from_utf8_lossy(bytes)));
                    file["after"] = json!(change
                        .after
                        .as_ref()
                        .map(|bytes| String::from_utf8_lossy(bytes)));
                }
            }
            files.push(file);
        }
        Ok(json!(files))
    }
    pub fn preview_schema(
        &mut self,
        args: &Arguments,
    ) -> Result<(Value, super::rules::Validation)> {
        // Schema 候选检查只编译声明；数据与业务规则不参与，也不会发布候选文件。
        let session = self.schema_session()?;
        let mut validation = super::rules::check_schema(&session, args);
        self.remap_diagnostics(&mut validation.diagnostics)?;
        Self::refresh_explanation_messages(&mut validation);
        let changes = self.changes()?;
        let report = json!({"revision_before":self.snapshot.revision(),"revision_after":self.snapshot.revision(),"changed":!changes.is_empty(),"written":false,"dry_run":true,"validation":&validation.report,
            "operations":self.operations,"files":self.file_report(&changes, args)?,"written_files":[]});
        Ok((report, validation))
    }

    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    pub fn finish(
        &mut self,
        args: &Arguments,
        force_preview: bool,
    ) -> Result<(Value, super::rules::Validation)> {
        let session = self.data_session().map_err(|failure| failure.diagnostics);
        let mut validation = match session.as_ref() {
            Ok(session) => {
                let typed = self.validate_typed_references(session);
                if typed.is_empty() {
                    super::rules::check(session, args)?
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
                        explanations: BTreeMap::default(),
                    };
                    if args.flag("explain") {
                        super::rules::explain_diagnostics(session, &mut validation);
                    }
                    validation
                }
            }
            Err(diagnostics) => {
                let diagnostics = diagnostics.clone();
                let failure = Failure::from(diagnostics.clone());
                if failure.status == Status::Error {
                    return Err(failure);
                }
                super::rules::Validation {
                    report: json!({"structure":"failed","rules":if args.flag("rules") || !args.all("rule").is_empty() {"skipped_dependency_error"} else {"not_executed"},
                        "complete":false,"diagnostic_count":diagnostics.diagnostics.len()}),
                    diagnostics,
                    logical_locations: BTreeMap::default(),
                    explanations: BTreeMap::default(),
                }
            }
        };
        self.remap_diagnostics(&mut validation.diagnostics)?;
        Self::refresh_explanation_messages(&mut validation);
        let changes = self.changes()?;
        let dry_run = force_preview || args.flag("dry-run");
        let mut report = json!({"revision_before":self.snapshot.revision(),"revision_after":self.snapshot.revision(),"changed":!changes.is_empty(),"written":false,"dry_run":dry_run,"validation":&validation.report,
            "operations":self.operations,"files":self.file_report(&changes, args)?,"written_files":[]});
        let after = SemanticSnapshot::capture_from_schema(
            &self.schema_session()?,
            session.as_ref().ok(),
            |path| {
                Ok(crate::project_path(
                    self.original.root_dir(),
                    &self.unshadow(path)?,
                ))
            },
        )?;
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
            .filter_map(|target| Some((target["type"].as_str()?, target["id"].as_str()?)))
            .collect::<BTreeSet<_>>();
        report["matched_records"] = json!(matched.len());
        report["diff"] = super::semantic_diff::select(difference, args)?;
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
            verify.map_err(|mut failure| {
                let mut details = report.clone();
                if let (Value::Object(details), Some(Value::Object(extra))) =
                    (&mut details, failure.result.take())
                {
                    details.extend(extra);
                }
                failure.with_result(details)
            })?;
            let written = coflow_staging::transaction::publish(&changes).map_err(|error| {
                publication_failure(self.original.root_dir(), *error, report.clone())
            })?;
            report["written"] = json!(true);
            report["written_files"] = json!(written
                .iter()
                .map(|path| crate::project_path(self.original.root_dir(), path))
                .collect::<Vec<_>>());
            report["revision_after"] = json!(capture_published_revision(&self.original, &report)?);
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

/// 发布已经完成后，读取修订失败不能把成功写入伪装成未执行，调用方必须保留原发布结果。
pub(crate) fn capture_published_revision(project: &Project, report: &Value) -> Result<String> {
    let capture = (|| -> Result<String> {
        let current = Project::open_schema_only(Some(project.config_path()))?;
        Ok(InputSnapshot::capture(&current)?.revision())
    })();
    capture.map_err(|mut failure| {
        let mut report = report.clone();
        report["revision"] = Value::Null;
        if report.get("revision_after").is_some() {
            report["revision_after"] = Value::Null;
        }
        for diagnostic in &mut failure.diagnostics.diagnostics {
            diagnostic.message.push_str("; publication completed, but the current revision could not be read; inspect written_files before retrying");
        }
        failure.status = Status::Error;
        failure.error_category = Some(super::ErrorCategory::Execution);
        failure.with_result(report)
    })
}

fn read_content(content: Option<&String>, file: Option<&String>, base: &Path) -> Result<Vec<u8>> {
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

#[cfg(test)]
mod cache_tests {
    #![allow(clippy::unwrap_used, reason = "测试的前置步骤失败应直接报告失败。")]
    use super::*;
    use crate::automation::{FieldPath, InputValue, Operation, RecordTarget, Selector};

    fn fixture(files: usize) -> (tempfile::TempDir, Project, InputSnapshot) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("data")).unwrap();
        fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
        fs::write(
            root.path().join("schema.cft"),
            "table Item { value: int; }\n",
        )
        .unwrap();
        for index in 0..files {
            fs::write(
                root.path().join(format!("data/{index}.cfd")),
                format!("r{index}: Item {{ value: 1 }}\n"),
            )
            .unwrap();
        }
        let project = Project::open_schema_only(Some(root.path())).unwrap();
        let snapshot = InputSnapshot::capture(&project).unwrap();
        (root, project, snapshot)
    }

    #[test]
    fn candidate_maps_canonical_paths_back_to_original_inputs() {
        let (_root, project, snapshot) = fixture(1);
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        // 读取器返回规范路径，不能依赖临时目录环境变量中的别名表示。
        for file in &snapshot.files {
            let shadow = crate::canonicalize_path(candidate.shadow(&file.absolute)).unwrap();
            assert_eq!(candidate.unshadow(&shadow).unwrap(), file.absolute);
        }
        assert!(candidate.changes().unwrap().is_empty());
    }

    #[test]
    fn batch_reuses_schema_and_discovers_sources_once() {
        let (_root, project, snapshot) = fixture(64);
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        let operations = (0..64)
            .map(|index| Operation::SetField {
                record: RecordTarget {
                    actual_type: "Item".into(),
                    id: format!("r{index}"),
                },
                path: vec![FieldPath::Field {
                    name: "value".into(),
                }],
                value: InputValue::Expression { source: "2".into() },
                expected: None,
            })
            .collect::<Vec<_>>();
        candidate.apply_operations(&operations).unwrap();
        candidate.finish(&Arguments::default(), true).unwrap();
        let stats = &candidate.cache.borrow().stats;
        assert_eq!(stats.schema_compilations, 1);
        assert_eq!(stats.source_discoveries, 1);
        // 64 个初始文件各解析一次，之后只重新解析被修改的 64 个文件。
        assert_eq!(stats.cfd_parses, 128);
    }

    #[test]
    fn repeated_set_where_uses_current_candidate_and_shared_schema() {
        let (_root, project, snapshot) = fixture(3);
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        let operation = |source: &str, filter: &str| Operation::SetWhere {
            selector: Selector {
                filter: Some(filter.into()),
                ..Default::default()
            },
            path: vec![FieldPath::Field {
                name: "value".into(),
            }],
            value: InputValue::Expression {
                source: source.into(),
            },
            expected_count: 3,
        };
        candidate
            .apply_operations(&[operation("2", "value == 1"), operation("3", "value == 2")])
            .unwrap();
        let session = candidate.data_session().unwrap();
        assert!(session
            .model()
            .records()
            .all(|(_, record)| record.field("value") == Some(&crate::CfdValue::Int(3))));
        let stats = &candidate.cache.borrow().stats;
        assert_eq!(stats.schema_compilations, 1);
        assert_eq!(stats.source_discoveries, 1);
        assert_eq!(stats.cfd_parses, 9);
    }

    #[test]
    fn source_edits_invalidate_cached_schema_and_records() {
        let (root, project, snapshot) = fixture(1);
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        candidate.schema_session().unwrap();
        candidate.data_sources().unwrap();
        candidate
            .apply_files(
                &[
                    FileOperation::Write {
                        file: "schema.cft".into(),
                        content: Some("table Item { value: string; }".into()),
                        content_file: None,
                        expected_fingerprint: None,
                    },
                    FileOperation::Write {
                        file: "data/0.cfd".into(),
                        content: Some("renamed: Item { value: \"new\" }".into()),
                        content_file: None,
                        expected_fingerprint: None,
                    },
                ],
                root.path(),
            )
            .unwrap();
        candidate
            .apply_operations(&[Operation::SetField {
                record: RecordTarget {
                    actual_type: "Item".into(),
                    id: "renamed".into(),
                },
                path: vec![FieldPath::Field {
                    name: "value".into(),
                }],
                value: InputValue::Expression {
                    source: "\"after\"".into(),
                },
                expected: None,
            }])
            .unwrap();
        let session = candidate.data_session().unwrap();
        assert_eq!(
            session.model().records().next().unwrap().1.field("value"),
            Some(&crate::CfdValue::String("after".into()))
        );
        assert_eq!(candidate.cache.borrow().stats.schema_compilations, 2);
    }

    #[test]
    fn revision_read_failure_preserves_completed_publication() {
        let (root, project, snapshot) = fixture(1);
        let mut candidate = Candidate::new(&project, &snapshot).unwrap();
        candidate
            .apply_operations(&[Operation::SetField {
                record: RecordTarget {
                    actual_type: "Item".into(),
                    id: "r0".into(),
                },
                path: vec![FieldPath::Field {
                    name: "value".into(),
                }],
                value: InputValue::Expression { source: "2".into() },
                expected: None,
            }])
            .unwrap();
        let (report, _) = candidate.finish(&Arguments::default(), false).unwrap();
        assert_eq!(report["written"], true);
        // 真实发布后删除配置，稳定复现后续修订读取失败，不依赖线程竞态。
        fs::remove_file(root.path().join("coflow.yaml")).unwrap();
        let failure = capture_published_revision(&project, &report).unwrap_err();
        let failure = candidate.failure_result(failure, &Arguments::default());
        let failed = failure.result.unwrap();
        assert_eq!(failure.status, Status::Error);
        assert_eq!(failed["written"], true);
        assert_eq!(failed["written_files"], report["written_files"]);
        assert_eq!(failed["validation"], report["validation"]);
        assert_eq!(failed["revision"], Value::Null);
        assert_eq!(failed["revision_after"], Value::Null);
        assert!(fs::read_to_string(root.path().join("data/0.cfd"))
            .unwrap()
            .contains("value: 2"));
    }
}
