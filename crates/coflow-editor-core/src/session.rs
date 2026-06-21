//! Session state and command handlers.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use coflow_api::{
    LoadContext, ProjectSourceRef, ProviderRegistry, ResolvedSource,
    SourceLocationSpec, SourceResolveContext,
};
use coflow_cfd::{parse_cfd, CfdAst};
use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef};
use coflow_checker::CfdCheckExt;
use coflow_data_model::{CfdDataModel, CfdDictKey, CfdRecord, CfdRecordId, CfdValue};
use coflow_project::{compile_schema_project, Project, SourceConfig};

use crate::convert::{cfd_value_to_wire, record_to_field_cells_with_ast};
use crate::patch;
use crate::types::{
    DiagnosticItem, FieldCell, FieldPathSegment, FieldValue, FileRecords, FileTreeNode, GraphData,
    GraphEdge, GraphNode, ProjectSnapshot, RecordRow,
};

const GRAPH_DEPTH: usize = 3;

/// One loaded project.
pub struct EditorSession {
    pub project_root: PathBuf,
    /// Original `coflow.yaml` path — needed to fully reload the project after
    /// a write so all data sources (cfd / excel / lark) are re-resolved.
    pub yaml_path: PathBuf,
    pub schema: CftContainer,
    pub model: CfdDataModel,
    pub diagnostics: Vec<DiagnosticItem>,
    /// Files inside any `sources` dir, by relative slash path.
    pub source_files: BTreeSet<String>,
    /// `record_key` → relative slash path of the file it lives in.
    pub key_to_file: HashMap<String, String>,
    /// `file path → ordered record keys`.
    pub file_to_keys: BTreeMap<String, Vec<String>>,
    /// Original `.cfd` source text + parsed AST, keyed by relative path.
    /// Only populated for files loaded via the cfd loader (`.cfd` text files).
    /// Used by write commands for span-patch updates.
    pub cfd_sources: HashMap<String, (String, CfdAst)>,
}

impl std::fmt::Debug for EditorSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditorSession")
            .field("project_root", &self.project_root)
            .field("source_files", &self.source_files.len())
            .field("records", &self.key_to_file.len())
            .finish()
    }
}

#[derive(Debug, Default)]
pub struct SessionStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    next_id: u32,
    sessions: HashMap<u32, EditorSession>,
}

impl SessionStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a project; returns a new session id + snapshot.
    pub fn load_project(&self, yaml_path: &Path) -> Result<ProjectSnapshot, String> {
        let (session, snapshot_partial) = build_session(yaml_path)?;
        let mut inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        inner.next_id = inner.next_id.checked_add(1).unwrap_or(1);
        let id = inner.next_id;
        let project_root = strip_unc_prefix(&session.project_root.display().to_string());
        let diagnostics = session.diagnostics.clone();
        inner.sessions.insert(id, session);
        Ok(ProjectSnapshot {
            session_id: id,
            project_root,
            file_tree: snapshot_partial.file_tree,
            diagnostics,
        })
    }

    pub fn close_session(&self, id: u32) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        inner.sessions.remove(&id);
        Ok(())
    }

    pub fn get_file_records(&self, id: u32, file_path: &str) -> Result<FileRecords, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        let keys = session
            .file_to_keys
            .get(file_path)
            .cloned()
            .unwrap_or_default();

        let mut records = Vec::with_capacity(keys.len());
        let mut type_seen = Vec::new();
        let mut type_set = HashSet::new();
        let ast_records = session.cfd_sources.get(file_path).map(|(_, ast)| ast);
        for key in &keys {
            if let Some((_id, record)) = lookup_record_by_key(&session.model, key) {
                if type_set.insert(record.actual_type.clone()) {
                    type_seen.push(record.actual_type.clone());
                }
                let ast_rec = ast_records
                    .and_then(|ast| ast.records.iter().find(|r| r.key == *key));
                let mut row = RecordRow {
                    key: record.key.clone(),
                    actual_type: record.actual_type.clone(),
                    fields: record_to_field_cells_with_ast(record, &session.model, ast_rec),
                };
                annotate_ref_files(&mut row.fields, session);
                records.push(row);
            }
        }
        Ok(FileRecords {
            file_path: file_path.to_string(),
            type_names: type_seen,
            records,
        })
    }

    pub fn get_record(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
    ) -> Result<RecordRow, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        if session.key_to_file.get(record_key).map(String::as_str) != Some(file_path) {
            return Err(format!("record `{record_key}` not in `{file_path}`"));
        }
        let (_id, record) = lookup_record_by_key(&session.model, record_key)
            .ok_or_else(|| format!("record `{record_key}` not found"))?;
        let ast_rec = session
            .cfd_sources
            .get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key));
        let mut row = RecordRow {
            key: record.key.clone(),
            actual_type: record.actual_type.clone(),
            fields: record_to_field_cells_with_ast(record, &session.model, ast_rec),
        };
        annotate_ref_files(&mut row.fields, session);
        Ok(row)
    }

    /// Build a default `FieldValue::Object` for the given schema type, filling
    /// each field with either its declared default or a kind-appropriate zero.
    /// Used by the UI to switch a Ref field to an inline Object without
    /// producing schema-invalid empty `{}`.
    pub fn make_default_object(&self, id: u32, type_name: &str) -> Result<FieldValue, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        let ty = session
            .schema
            .resolve_type(type_name)
            .ok_or_else(|| format!("type `{type_name}` not found in schema"))?;
        let mut fields = Vec::new();
        for f in &ty.all_fields {
            let value = default_value_for_ty(&f.ty_ref, f.default.as_ref(), &session.schema);
            fields.push(FieldCell { name: f.name.clone(), value, is_spread: false });
        }
        Ok(FieldValue::Object {
            actual_type: type_name.to_string(),
            fields,
        })
    }

    pub fn get_enum_variants(&self, id: u32, enum_name: &str) -> Result<Vec<String>, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        Ok(session
            .schema
            .resolve_enum(enum_name)
            .map(|e| e.variants.iter().map(|v| v.name.clone()).collect())
            .unwrap_or_default())
    }

    pub fn get_ref_targets(
        &self,
        id: u32,
        expected_type: &str,
    ) -> Result<Vec<String>, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        let mut keys: Vec<String> = session
            .model
            .records()
            .filter(|(_, r)| session.schema.is_assignable(&r.actual_type, expected_type))
            .map(|(_, r)| r.key.clone())
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    pub fn write_field(
        &self,
        id: u32,
        file_path: &str,
        record_key: &str,
        field_path: &[FieldPathSegment],
        new_value: &FieldValue,
    ) -> Result<RecordRow, String> {
        let mut inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get_mut(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;

        let (source, ast) = session
            .cfd_sources
            .get(file_path)
            .ok_or_else(|| format!("file '{file_path}' is not an editable .cfd source"))?
            .clone();

        let field_exists = ast.records.iter().any(|r| {
            r.key == record_key
                && match field_path.first() {
                    Some(FieldPathSegment::Field { name }) => {
                        r.fields.iter().any(|f| &f.name == name)
                            || r.entries.iter().any(|e| matches!(
                                e,
                                coflow_cfd::CfdBlockEntry::Field(f) if &f.name == name
                            ))
                    }
                    _ => false,
                }
        });

        let result = if field_exists {
            patch::apply_patch(&source, &ast, record_key, field_path, new_value)?
        } else if let Some(FieldPathSegment::Field { name }) = field_path.first() {
            patch::insert_field(&source, &ast, record_key, name, new_value)?
        } else {
            return Err("cannot insert: field_path must start with a Field segment".to_string());
        };

        let abs_path = session.project_root.join(file_path);
        std::fs::write(&abs_path, &result.new_source)
            .map_err(|e| format!("cannot write {file_path}: {e}"))?;

        // Full project reload — re-resolve every source (cfd / excel / lark) so
        // the data model fully reflects the new on-disk state, including
        // cross-file Refs to records in other files.
        let yaml_path = session.yaml_path.clone();
        let (new_session, _) = build_session(&yaml_path)?;
        *session = new_session;

        // Re-fetch the updated record to return.
        let (_id, record) = lookup_record_by_key(&session.model, record_key)
            .ok_or_else(|| format!("record `{record_key}` not found after write"))?;
        let ast_rec = session
            .cfd_sources
            .get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key));
        let mut row = RecordRow {
            key: record.key.clone(),
            actual_type: record.actual_type.clone(),
            fields: record_to_field_cells_with_ast(record, &session.model, ast_rec),
        };
        annotate_ref_files(&mut row.fields, session);
        Ok(row)
    }

    pub fn get_graph(&self, id: u32, file_path: &str) -> Result<GraphData, String> {
        let inner = self.inner.lock().map_err(|_| "session store poisoned")?;
        let session = inner
            .sessions
            .get(&id)
            .ok_or_else(|| format!("unknown session id {id}"))?;
        Ok(build_graph(session, file_path))
    }
}

struct SessionSnapshotParts {
    file_tree: Vec<FileTreeNode>,
}

#[allow(clippy::too_many_arguments)]
fn load_one_source(
    project: &Project,
    schema: &CftContainer,
    registry: &ProviderRegistry,
    source: &SourceConfig,
    source_files: &mut BTreeSet<String>,
    model_builder: &mut coflow_data_model::CfdModelBuilder<'_>,
    file_to_keys: &mut BTreeMap<String, Vec<String>>,
    key_to_file: &mut HashMap<String, String>,
    diagnostics: &mut Vec<DiagnosticItem>,
) {
    let configured = configured_resolved_source(project, source);

    let resolve_ctx = SourceResolveContext {
        project_root: &project.root_dir,
        schema,
    };

    // Determine the (loader, sub_source) pairs. If the source is an untyped
    // directory, every loader gets a chance to resolve into files; otherwise
    // pick exactly one loader.
    type Pair = (std::sync::Arc<dyn coflow_api::DataLoader>, ResolvedSource);
    let mut pairs: Vec<Pair> = Vec::new();
    let is_untyped_dir = source.source_type.is_none()
        && matches!(&configured.location, SourceLocationSpec::Path(p) if p.is_dir());

    if is_untyped_dir {
        for loader in registry.loaders() {
            match loader.resolve(resolve_ctx, &configured) {
                Ok(subs) => {
                    for sub in subs {
                        pairs.push((std::sync::Arc::clone(&loader), sub));
                    }
                }
                Err(diag_set) => {
                    for d in diag_set.diagnostics {
                        diagnostics.push(diagnostic_from_api(&d));
                    }
                }
            }
        }
    } else {
        let option_keys: Vec<&str> = configured
            .options
            .as_object()
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let source_ref = ProjectSourceRef {
            source_type: source.source_type.as_deref(),
            location: &configured.location,
            option_keys: &option_keys,
        };
        let loader = match registry.select_loader(&source_ref) {
            Ok(loader) => loader,
            Err(err) => {
                diagnostics.push(DiagnosticItem {
                    severity: "error".to_string(),
                    code: "PROJECT-001".to_string(),
                    stage: "PROJECT".to_string(),
                    message: format!(
                        "source `{}` could not select a loader: {err:?}",
                        configured.display_name
                    ),
                    file_path: None,
                    record_key: None,
                    field_path: None,
                });
                return;
            }
        };
        match loader.resolve(resolve_ctx, &configured) {
            Ok(subs) => {
                for sub in subs {
                    pairs.push((std::sync::Arc::clone(&loader), sub));
                }
            }
            Err(diag_set) => {
                for d in diag_set.diagnostics {
                    diagnostics.push(diagnostic_from_api(&d));
                }
                return;
            }
        }
    }

    let load_ctx = LoadContext {
        project_root: &project.root_dir,
        schema,
    };

    for (loader, sub) in &pairs {
        // Each sub source maps to a single file (or remote URI). For Excel sheets
        // the same xlsx file appears multiple times, once per sheet — we still
        // dedupe to one tree entry but track records separately.
        let label = file_label_for(project, sub);
        source_files.insert(label.clone());

        match loader.load(load_ctx, sub) {
            Ok(loaded) => {
                let entry = file_to_keys.entry(label.clone()).or_default();
                for record in loaded.records {
                    let key = record.key.clone();
                    entry.push(key.clone());
                    key_to_file.insert(key, label.clone());
                    model_builder.add_input_record(record);
                }
            }
            Err(diag_set) => {
                for d in diag_set.diagnostics {
                    let mut item = diagnostic_from_api(&d);
                    if item.file_path.is_none() {
                        item.file_path = Some(label.clone());
                    }
                    diagnostics.push(item);
                }
            }
        }
    }
}

fn configured_resolved_source(project: &Project, source: &SourceConfig) -> ResolvedSource {
    let location = match source.location() {
        SourceLocationSpec::Path(path) => SourceLocationSpec::Path(project.resolve_path(path)),
        SourceLocationSpec::Uri(uri) => SourceLocationSpec::Uri(uri.clone()),
    };
    let display_name = match source.location() {
        SourceLocationSpec::Path(path) => path.display().to_string(),
        SourceLocationSpec::Uri(uri) => uri.clone(),
    };
    ResolvedSource {
        provider_id: source.source_type.clone().unwrap_or_default(),
        location,
        options: source.options().clone(),
        display_name,
    }
}

fn file_label_for(project: &Project, sub: &ResolvedSource) -> String {
    match &sub.location {
        SourceLocationSpec::Path(p) => {
            let rel = p.strip_prefix(&project.root_dir).unwrap_or(p.as_path());
            path_to_slash(rel)
        }
        SourceLocationSpec::Uri(uri) => uri.clone(),
    }
}

fn loader_register_diagnostic(err: &coflow_api::ProviderRegistrationError) -> DiagnosticItem {
    DiagnosticItem {
        severity: "warning".to_string(),
        code: "REGISTRY".to_string(),
        stage: "PROJECT".to_string(),
        message: format!("loader registration: {err}"),
        file_path: None,
        record_key: None,
        field_path: None,
    }
}

fn diagnostic_from_api(d: &coflow_api::Diagnostic) -> DiagnosticItem {
    use coflow_api::{Severity, SourceLocation};
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
    .to_string();
    let file_path = d.primary.as_ref().and_then(|label| match &label.location {
        SourceLocation::FileSpan { path, .. }
        | SourceLocation::TableCell { path, .. }
        | SourceLocation::ProjectConfig { path, .. }
        | SourceLocation::Artifact { path } => Some(path_to_slash(path)),
        SourceLocation::RemoteCell { document, .. } => Some(document.clone()),
    });
    DiagnosticItem {
        severity,
        code: d.code.clone(),
        stage: d.stage.clone(),
        message: d.message.clone(),
        file_path,
        record_key: None,
        field_path: None,
    }
}

fn build_session(yaml_path_in: &Path) -> Result<(EditorSession, SessionSnapshotParts), String> {
    let yaml_path = yaml_path_in.to_path_buf();
    let mut diagnostics: Vec<DiagnosticItem> = Vec::new();

    let project = Project::open_schema_only(Some(yaml_path.as_path()))
        .map_err(|err| format!("failed to open project: {err}"))?;
    let project_root = project.root_dir.clone();

    // Collect project-level diagnostics (config/source shape errors etc).
    for d in project.schema_diagnostics() {
        diagnostics.push(diagnostic_from_project(&d, &project_root));
    }
    for d in project.data_diagnostics() {
        diagnostics.push(diagnostic_from_project(&d, &project_root));
    }

    // Compile schema. Continue with empty container on failure.
    let schema = match compile_schema_project(&project, None) {
        Ok(build) => {
            for diag in build.diagnostics {
                diagnostics.push(DiagnosticItem {
                    severity: severity_str(&diag),
                    code: format!("{:?}", diag.code),
                    stage: "SCHEMA".to_string(),
                    message: diag.message,
                    file_path: None,
                    record_key: None,
                    field_path: None,
                });
            }
            build.container.unwrap_or_else(CftContainer::new)
        }
        Err(err) => {
            diagnostics.push(DiagnosticItem {
                severity: "error".to_string(),
                code: "SCHEMA-COMPILE".to_string(),
                stage: "SCHEMA".to_string(),
                message: err,
                file_path: None,
                record_key: None,
                field_path: None,
            });
            CftContainer::new()
        }
    };

    // Build provider registry with all known loaders. Failure to register one
    // (e.g. duplicate ids) is recorded as a diagnostic; we continue with what's
    // already in the registry.
    let mut registry = ProviderRegistry::default();
    if let Err(err) = registry.register_loader(coflow_loader_cfd::CfdLoader) {
        diagnostics.push(loader_register_diagnostic(&err));
    }
    if let Err(err) = registry.register_loader(coflow_loader_excel::ExcelLoader) {
        diagnostics.push(loader_register_diagnostic(&err));
    }
    // lark loader uses default ureq HTTP client.
    if let Err(err) = registry.register_loader(coflow_loader_lark::LarkSheetLoader::<
        coflow_loader_lark::UreqLarkHttpClient,
    >::default())
    {
        diagnostics.push(loader_register_diagnostic(&err));
    }

    let mut source_files: BTreeSet<String> = BTreeSet::new();
    let mut model_builder = CfdDataModel::builder(&schema);
    let mut file_to_keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut key_to_file: HashMap<String, String> = HashMap::new();

    for source in &project.config.sources {
        load_one_source(
            &project,
            &schema,
            &registry,
            source,
            &mut source_files,
            &mut model_builder,
            &mut file_to_keys,
            &mut key_to_file,
            &mut diagnostics,
        );
    }

    let model = match model_builder.build() {
        Ok(model) => {
            // Run runtime checks for diagnostics; ignore on failure.
            if let Err(diags) = model.run_checks(&schema) {
                for diag in diags.diagnostics {
                    diagnostics.push(diagnostic_from_cfd(&diag, &model, &key_to_file));
                }
            }
            model
        }
        Err(diags) => {
            // Re-run with empty schema to get an empty model so we can still browse files.
            let empty = CftContainer::new();
            for diag in diags.diagnostics {
                diagnostics.push(diagnostic_from_cfd(
                    &diag,
                    &CfdDataModel::builder(&empty).build().unwrap_or_else(|_| {
                        // Pseudo-empty model fallback; the tail .build() should succeed.
                        CfdDataModel::builder(&empty).build().unwrap_or_else(|_| panic!())
                    }),
                    &key_to_file,
                ));
            }
            CfdDataModel::builder(&empty).build().unwrap_or_else(|_| panic!())
        }
    };

    // Provider-derived extension whitelist for the file tree (e.g. cfd, xlsx).
    let mut ext_whitelist: BTreeSet<String> = BTreeSet::new();
    for loader in registry.loaders() {
        for ext in loader.descriptor().extensions {
            ext_whitelist.insert((*ext).to_string());
        }
    }

    let file_tree = build_file_tree(&project_root, &source_files, &ext_whitelist);

    // Capture original .cfd source + AST for files we loaded, so write commands
    // can do span-patches and reload. Files we can't read are skipped silently —
    // they simply won't be editable; the read path already issued a diagnostic.
    let mut cfd_sources: HashMap<String, (String, CfdAst)> = HashMap::new();
    for file_path in file_to_keys.keys() {
        if !file_path.to_lowercase().ends_with(".cfd") {
            continue;
        }
        let abs = project_root.join(file_path);
        if let Ok(src) = std::fs::read_to_string(&abs) {
            let (ast, _) = parse_cfd(&src);
            cfd_sources.insert(file_path.clone(), (src, ast));
        }
    }

    Ok((
        EditorSession {
            project_root,
            yaml_path,
            schema,
            model,
            diagnostics: diagnostics.clone(),
            source_files,
            key_to_file,
            file_to_keys,
            cfd_sources,
        },
        SessionSnapshotParts { file_tree },
    ))
}

fn build_file_tree(
    root: &Path,
    in_sources: &BTreeSet<String>,
    ext_whitelist: &BTreeSet<String>,
) -> Vec<FileTreeNode> {
    // First pass: gather every relative .cfd path as slash-separated parts.
    let mut files: Vec<Vec<String>> = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let rel_for_check = path
            .strip_prefix(root)
            .map(path_to_slash)
            .unwrap_or_default();
        // A file is shown if it's already source-tracked, or its extension
        // matches any registered loader.
        let by_extension = !ext.is_empty() && ext_whitelist.contains(ext);
        if !by_extension && !in_sources.contains(&rel_for_check) {
            continue;
        }
        let rel = match path.strip_prefix(root) {
            Ok(rel) => rel,
            Err(_) => continue,
        };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        if !parts.is_empty() {
            files.push(parts);
        }
    }

    // Second pass: insert each path one component at a time.
    let mut roots: Vec<FileTreeNode> = Vec::new();
    for parts in files {
        insert_path(&mut roots, &parts, 0, "", in_sources);
    }
    sort_tree(&mut roots);
    roots
}

fn insert_path(
    nodes: &mut Vec<FileTreeNode>,
    parts: &[String],
    idx: usize,
    parent_path: &str,
    in_sources: &BTreeSet<String>,
) {
    if idx >= parts.len() {
        return;
    }
    let name = &parts[idx];
    let path = if parent_path.is_empty() {
        name.clone()
    } else {
        format!("{parent_path}/{name}")
    };
    let is_dir = idx + 1 < parts.len();

    let existing = nodes.iter_mut().find(|n| n.name == *name);
    if let Some(node) = existing {
        if is_dir {
            insert_path(&mut node.children, parts, idx + 1, &path, in_sources);
        }
        return;
    }
    let in_src = if is_dir { true } else { in_sources.contains(&path) };
    let mut node = FileTreeNode {
        name: name.clone(),
        path: path.clone(),
        is_dir,
        in_sources: in_src,
        children: Vec::new(),
    };
    if is_dir {
        insert_path(&mut node.children, parts, idx + 1, &path, in_sources);
    }
    nodes.push(node);
}

fn sort_tree(nodes: &mut Vec<FileTreeNode>) {
    nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    for node in nodes {
        if !node.children.is_empty() {
            sort_tree(&mut node.children);
        }
    }
}

fn default_value_for_ty(
    ty: &CftSchemaTypeRef,
    declared_default: Option<&CftSchemaDefaultValue>,
    schema: &CftContainer,
) -> FieldValue {
    if let Some(d) = declared_default {
        return default_from_schema_default(d, schema);
    }
    default_zero_for_ty(ty, schema)
}

fn default_from_schema_default(
    d: &CftSchemaDefaultValue,
    schema: &CftContainer,
) -> FieldValue {
    let _ = schema;
    match d {
        CftSchemaDefaultValue::Null => FieldValue::Null,
        CftSchemaDefaultValue::Int(v) => FieldValue::Int { v: *v },
        CftSchemaDefaultValue::Float(v) => FieldValue::Float { v: *v },
        CftSchemaDefaultValue::Bool(v) => FieldValue::Bool { v: *v },
        CftSchemaDefaultValue::String(v) => FieldValue::Str { v: v.clone() },
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => FieldValue::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone(),
            int_value: *value,
        },
        CftSchemaDefaultValue::EmptyArray => FieldValue::Array { items: Vec::new() },
        CftSchemaDefaultValue::EmptyObject => FieldValue::Dict { entries: Vec::new() },
    }
}

fn default_zero_for_ty(ty: &CftSchemaTypeRef, schema: &CftContainer) -> FieldValue {
    match ty {
        CftSchemaTypeRef::Int => FieldValue::Int { v: 0 },
        CftSchemaTypeRef::Float => FieldValue::Float { v: 0.0 },
        CftSchemaTypeRef::Bool => FieldValue::Bool { v: false },
        CftSchemaTypeRef::String => FieldValue::Str { v: String::new() },
        CftSchemaTypeRef::Array(_) => FieldValue::Array { items: Vec::new() },
        CftSchemaTypeRef::Dict(_, _) => FieldValue::Dict { entries: Vec::new() },
        CftSchemaTypeRef::Nullable(_) => FieldValue::Null,
        CftSchemaTypeRef::Named(name) => {
            if let Some(en) = schema.resolve_enum(name) {
                if let Some(first) = en.variants.first() {
                    return FieldValue::Enum {
                        enum_name: name.clone(),
                        variant: first.name.clone(),
                        int_value: first.value,
                    };
                }
            }
            // Otherwise it's an Object type — best-effort empty object;
            // caller should usually pass through `make_default_object` to
            // fully populate fields.
            FieldValue::Object {
                actual_type: name.clone(),
                fields: Vec::new(),
            }
        }
    }
}

fn lookup_record_by_key<'a>(
    model: &'a CfdDataModel,
    key: &str,
) -> Option<(CfdRecordId, &'a CfdRecord)> {
    model.records().find(|(_, record)| record.key == key)
}

fn annotate_ref_files(fields: &mut [FieldCell], session: &EditorSession) {
    for cell in fields {
        annotate_value(&mut cell.value, session);
    }
}

fn annotate_value(value: &mut FieldValue, session: &EditorSession) {
    match value {
        FieldValue::Ref {
            target_key,
            target_file,
            ..
        } => {
            *target_file = session.key_to_file.get(target_key).cloned();
        }
        FieldValue::Object { fields, .. } => annotate_ref_files(fields, session),
        FieldValue::Array { items } => {
            for item in items {
                annotate_value(item, session);
            }
        }
        FieldValue::Dict { entries } => {
            for entry in entries {
                annotate_value(&mut entry.value, session);
            }
        }
        _ => {}
    }
}

fn build_graph(session: &EditorSession, file_path: &str) -> GraphData {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    let starts: Vec<String> = session
        .file_to_keys
        .get(file_path)
        .cloned()
        .unwrap_or_default();

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut depths: HashMap<String, usize> = HashMap::new();

    for key in &starts {
        queue.push_back((key.clone(), 0));
        depths.insert(key.clone(), 0);
    }

    while let Some((key, depth)) = queue.pop_front() {
        let Some((_id, record)) = lookup_record_by_key(&session.model, &key) else {
            continue;
        };
        let host_file = session
            .key_to_file
            .get(&key)
            .cloned()
            .unwrap_or_default();
        let id = format!("{host_file}::{key}");
        let in_focus = host_file == file_path;
        let is_collapsed = depth >= GRAPH_DEPTH;

        let fields = if is_collapsed {
            Vec::new()
        } else {
            let ast_rec = session
                .cfd_sources
                .get(&host_file)
                .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == key));
            let mut f = record_to_field_cells_with_ast(record, &session.model, ast_rec);
            annotate_ref_files(&mut f, session);
            f
        };

        nodes.entry(id.clone()).or_insert(GraphNode {
            id: id.clone(),
            key: record.key.clone(),
            actual_type: record.actual_type.clone(),
            file_path: host_file.clone(),
            in_focus_file: in_focus,
            is_collapsed,
            fields,
        });

        if is_collapsed {
            continue;
        }

        // Walk outgoing references.
        let refs = collect_refs_in_record(record);
        for (path_str, target_key) in refs {
            let Some(target_file) = session.key_to_file.get(&target_key).cloned() else {
                continue;
            };
            let target_id = format!("{target_file}::{target_key}");
            edges.push(GraphEdge {
                source: id.clone(),
                target: target_id.clone(),
                field_path: path_str,
            });
            if !depths.contains_key(&target_key) {
                depths.insert(target_key.clone(), depth + 1);
                queue.push_back((target_key, depth + 1));
            }
        }
    }

    GraphData {
        nodes: nodes.into_values().collect(),
        edges,
    }
}

// Only top-level field refs and direct members of top-level Array/Dict are
// edges in the graph. Nested Object fields are not traversed — those refs
// belong to the record's internal structure, not its outgoing relationships.
fn collect_refs_in_record(record: &CfdRecord) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, value) in &record.fields {
        match value {
            CfdValue::Ref { key, .. } => out.push((name.clone(), key.clone())),
            CfdValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let CfdValue::Ref { key, .. } = item {
                        out.push((format!("{name}[{i}]"), key.clone()));
                    }
                }
            }
            CfdValue::Dict(entries) => {
                for (k, v) in entries {
                    if let CfdValue::Ref { key, .. } = v {
                        let key_str = match k {
                            CfdDictKey::String(s) => format!("\"{s}\""),
                            CfdDictKey::Int(i) => i.to_string(),
                            CfdDictKey::Enum(e) => e
                                .variant
                                .clone()
                                .unwrap_or_else(|| format!("{}({})", e.enum_name, e.value)),
                        };
                        out.push((format!("{name}[{key_str}]"), key.clone()));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn diagnostic_from_project(
    diag: &coflow_project::DiagnosticJson,
    project_root: &Path,
) -> DiagnosticItem {
    let _ = project_root;
    let file_path = if diag.path.is_empty() {
        None
    } else {
        Some(path_to_slash(Path::new(&diag.path)))
    };
    DiagnosticItem {
        severity: diag.severity.clone(),
        code: diag.code.clone(),
        stage: diag.stage.clone(),
        message: diag.message.clone(),
        file_path,
        record_key: None,
        field_path: None,
    }
}

fn diagnostic_from_cfd(
    diag: &coflow_data_model::CfdDiagnostic,
    model: &CfdDataModel,
    key_to_file: &HashMap<String, String>,
) -> DiagnosticItem {
    let stage = diag.stage.to_string();
    let severity = match diag.severity {
        coflow_data_model::CfdSeverity::Error => "error",
    }
    .to_string();
    let mut record_key: Option<String> = None;
    let mut file_path: Option<String> = None;
    let mut field_path: Option<String> = None;
    if let Some(label) = &diag.primary {
        if let Some(rec_id) = label.record {
            if let Some(record) = model.record(rec_id) {
                record_key = Some(record.key.clone());
                file_path = key_to_file.get(&record.key).cloned();
            }
        }
        if !label.path.segments.is_empty() {
            field_path = Some(format_cfd_path(&label.path));
        }
    }
    DiagnosticItem {
        severity,
        code: diag.code.as_str().to_string(),
        stage,
        message: diag.message.clone(),
        file_path,
        record_key,
        field_path,
    }
}

fn format_cfd_path(path: &coflow_data_model::CfdPath) -> String {
    let mut out = String::new();
    for seg in &path.segments {
        match seg {
            coflow_data_model::CfdPathSegment::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            coflow_data_model::CfdPathSegment::Index(i) => {
                out.push_str(&format!("[{i}]"));
            }
            coflow_data_model::CfdPathSegment::DictKey(k) => {
                out.push_str(&format!("[{k}]"));
            }
        }
    }
    out
}

fn severity_str(diag: &coflow_cft::CftDiagnostic) -> String {
    let _ = diag;
    "error".to_string()
}

fn path_to_slash(path: &Path) -> String {
    strip_unc_prefix(&path.to_string_lossy().replace('\\', "/"))
}

fn strip_unc_prefix(path: &str) -> String {
    // Windows canonicalize prepends `\\?\` (or after slash conversion `//?/`).
    path.strip_prefix(r"\\?\")
        .or_else(|| path.strip_prefix("//?/"))
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_owned())
}

fn _ignore_unused_value_conversion(_: &CfdValue, _: &CfdDataModel) {
    let _ = cfd_value_to_wire;
}
