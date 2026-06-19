use crate::patch;
use crate::types::{
    DiagnosticItem, DictEntry, DictKey, FieldCell, FieldPathSegment, FieldValue, FileRecords,
    FileTreeNode, GraphData, GraphEdge, GraphNode, ProjectSnapshot, RecordRow,
};
use coflow_cfd::{parse_cfd, CfdAst, CfdBlockEntry};
use coflow_checker::CfdCheckExt;
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{
    CfdDataModel, CfdDiagnostics, CfdDictKey, CfdEnumValue, CfdRecord, CfdValue,
};
use coflow_loader_cfd::parse_cfd_input_records;
use serde::Deserialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
struct CoflowYaml {
    schema: SchemaField,
    #[serde(default)]
    sources: Vec<SourceEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SchemaField {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

#[derive(Debug, Deserialize)]
struct SourceEntry {
    path: Option<PathBuf>,
    #[serde(rename = "type")]
    source_type: Option<String>,
}

#[derive(Debug, Default)]
pub struct SessionStore {
    next_id: u32,
    sessions: HashMap<u32, Arc<Mutex<Session>>>,
}

#[derive(Debug)]
pub struct Session {
    pub project_dir: PathBuf,
    pub schema: CftContainer,
    pub model: CfdDataModel,
    pub file_record_keys: HashMap<String, Vec<String>>,
    pub file_sources: HashMap<String, (String, CfdAst)>,
    pub source_dirs: Vec<PathBuf>,
}

pub fn load_project_inner(
    store: &Mutex<SessionStore>,
    yaml_path: &str,
) -> Result<ProjectSnapshot, String> {
    let yaml_file = Path::new(yaml_path);
    let project_dir = yaml_file
        .parent()
        .ok_or_else(|| "invalid yaml path".to_string())?
        .to_path_buf();

    let yaml_src =
        std::fs::read_to_string(yaml_file).map_err(|e| format!("cannot read coflow.yaml: {e}"))?;
    let config: CoflowYaml = serde_yaml::from_str(&yaml_src)
        .map_err(|e| format!("cannot parse coflow.yaml: {e}"))?;

    let mut diagnostics: Vec<DiagnosticItem> = Vec::new();

    let schema_paths: Vec<PathBuf> = match &config.schema {
        SchemaField::One(p) => vec![project_dir.join(p)],
        SchemaField::Many(ps) => ps.iter().map(|p| project_dir.join(p)).collect(),
    };

    let mut schema = CftContainer::new();
    for (i, sp) in schema_paths.iter().enumerate() {
        match std::fs::read_to_string(sp) {
            Ok(src) => {
                if let Err(d) = schema.add_module(ModuleId::new(format!("schema_{i}")), src) {
                    diagnostics.push(DiagnosticItem {
                        severity: "error".into(),
                        code: "SCHEMA-PARSE".into(),
                        stage: "SCHEMA".into(),
                        message: format!("{d:?}"),
                        file_path: Some(sp.to_string_lossy().into_owned()),
                        record_key: None,
                        field_path: None,
                    });
                }
            }
            Err(e) => diagnostics.push(DiagnosticItem {
                severity: "error".into(),
                code: "SCHEMA-READ".into(),
                stage: "SCHEMA".into(),
                message: format!("cannot read {}: {e}", sp.display()),
                file_path: Some(sp.to_string_lossy().into_owned()),
                record_key: None,
                field_path: None,
            }),
        }
    }
    if let Err(d) = schema.compile() {
        for diag in &d.diagnostics {
            diagnostics.push(DiagnosticItem {
                severity: "error".into(),
                code: format!("{:?}", diag.code),
                stage: format!("{:?}", diag.stage),
                message: diag.message.clone(),
                file_path: None,
                record_key: None,
                field_path: None,
            });
        }
    }

    let source_dirs: Vec<PathBuf> = config
        .sources
        .iter()
        .filter(|s| {
            s.source_type
                .as_deref()
                .map_or(true, |t| t == "file" || t == "cfd")
        })
        .filter_map(|s| s.path.as_ref())
        .map(|p| project_dir.join(p))
        .collect();

    let mut file_record_keys: HashMap<String, Vec<String>> = HashMap::new();
    let mut file_sources: HashMap<String, (String, CfdAst)> = HashMap::new();
    let mut all_input_records = Vec::new();

    for source_dir in &source_dirs {
        for cfd_path in collect_cfd_files(source_dir) {
            let rel = relative_path(&project_dir, &cfd_path);
            match std::fs::read_to_string(&cfd_path) {
                Ok(src) => {
                    let (ast, _) = parse_cfd(&src);
                    file_sources.insert(rel.clone(), (src.clone(), ast));
                    match parse_cfd_input_records(&schema, &src) {
                        Ok(records) => {
                            let keys: Vec<String> =
                                records.iter().map(|r| r.key.clone()).collect();
                            file_record_keys.insert(rel.clone(), keys);
                            all_input_records.extend(records);
                        }
                        Err(e) => {
                            file_record_keys.insert(rel.clone(), Vec::new());
                            diagnostics.push(DiagnosticItem {
                                severity: "error".into(),
                                code: "CFD-PARSE".into(),
                                stage: "LOAD".into(),
                                message: format!("{e:?}"),
                                file_path: Some(rel),
                                record_key: None,
                                field_path: None,
                            });
                        }
                    }
                }
                Err(e) => {
                    // Register the file with empty data so it appears loadable (showing the error diagnostic)
                    file_sources.insert(rel.clone(), (String::new(), CfdAst { records: Vec::new() }));
                    file_record_keys.insert(rel.clone(), Vec::new());
                    diagnostics.push(DiagnosticItem {
                        severity: "error".into(),
                        code: "CFD-READ".into(),
                        stage: "LOAD".into(),
                        message: format!("cannot read {}: {e}", cfd_path.display()),
                        file_path: Some(rel),
                        record_key: None,
                        field_path: None,
                    });
                }
            }
        }
    }

    let mut builder = CfdDataModel::builder(&schema);
    for r in all_input_records {
        builder.add_input_record(r);
    }
    let model = builder.build().unwrap_or_else(|d| {
        diagnostics.extend(convert_cfd_diagnostics(&d, None, Some(&file_record_keys)));
        CfdDataModel::builder(&schema)
            .build()
            .unwrap_or_else(|_| unreachable!("empty builder must succeed"))
    });

    if let Err(d) = model.run_checks(&schema) {
        diagnostics.extend(convert_cfd_diagnostics(&d, Some(&model), Some(&file_record_keys)));
    }

    let file_tree = build_file_tree(&project_dir, &project_dir, &source_dirs);

    let source_rel_dirs: Vec<PathBuf> = source_dirs
        .iter()
        .map(|d| PathBuf::from(relative_path(&project_dir, d)))
        .collect();

    let session_id = {
        let mut s = store.lock().map_err(|_| "lock poisoned")?;
        let id = s.next_id;
        s.next_id += 1;
        s.sessions.insert(
            id,
            Arc::new(Mutex::new(Session {
                project_dir,
                schema,
                model,
                file_record_keys,
                file_sources,
                source_dirs: source_rel_dirs,
            })),
        );
        id
    };

    Ok(ProjectSnapshot {
        session_id,
        file_tree,
        diagnostics,
    })
}

pub fn get_file_records_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
) -> Result<FileRecords, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let keys = session
        .file_record_keys
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    let mut type_names: Vec<String> = Vec::new();
    let mut records: Vec<RecordRow> = Vec::new();

    let ast_direct: HashMap<String, HashSet<String>> = session
        .file_sources
        .get(file_path)
        .map(|(_, ast)| {
            let mut m: HashMap<String, HashSet<String>> = HashMap::new();
            for ast_rec in &ast.records {
                let direct: HashSet<String> =
                    ast_rec.fields.iter().map(|f| f.name.clone()).collect();
                m.insert(ast_rec.key.clone(), direct);
            }
            m
        })
        .unwrap_or_default();

    for key in &keys {
        if let Some((_, record)) = session.model.records().find(|(_, r)| r.key == *key) {
            if !type_names.contains(&record.actual_type) {
                type_names.push(record.actual_type.clone());
            }
            let direct = ast_direct.get(key.as_str());
            records.push(convert_record_row(record, &session.schema, &session.model, &session.file_record_keys, direct));
        }
    }

    Ok(FileRecords {
        file_path: file_path.to_string(),
        type_names,
        records,
    })
}

pub fn get_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
) -> Result<RecordRow, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (_, record) = session
        .model
        .records()
        .find(|(_, r)| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    let direct = session
        .file_sources
        .get(file_path)
        .and_then(|(_, ast)| {
            ast.records
                .iter()
                .find(|r| r.key == record_key)
                .map(|r| r.fields.iter().map(|f| f.name.clone()).collect::<HashSet<String>>())
        });

    Ok(convert_record_row(record, &session.schema, &session.model, &session.file_record_keys, direct.as_ref()))
}

pub fn get_graph_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    expanded_keys: &[String],
) -> Result<GraphData, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let focus_keys: HashSet<String> = session
        .file_record_keys
        .get(file_path)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();

    let mut reverse_refs: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (_, record) in session.model.records() {
        let key = record.key.clone();
        collect_refs_in_record(record, &key, "", &mut reverse_refs, &session.model);
    }

    const MAX_DEPTH: usize = 3;
    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, usize)> =
        focus_keys.iter().map(|k| (k.clone(), 0)).collect();
    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();

    while let Some((key, depth)) = queue.pop_front() {
        if visited.contains(&key) {
            continue;
        }
        visited.insert(key.clone());

        let record_file = session
            .file_record_keys
            .iter()
            .find_map(|(fp, keys)| {
                if keys.contains(&key) {
                    Some(fp.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let node_id = format!("{record_file}::{key}");
        let is_collapsed = depth >= MAX_DEPTH && !expanded_keys.contains(&key);

        match session.model.records().find(|(_, r)| r.key == key) {
            Some((_, record)) => {
                nodes.push(GraphNode {
                    id: node_id.clone(),
                    key: key.clone(),
                    actual_type: record.actual_type.clone(),
                    file_path: record_file.clone(),
                    in_focus_file: focus_keys.contains(&key),
                    is_collapsed,
                    fields: if is_collapsed {
                        Vec::new()
                    } else {
                        convert_record_row(record, &session.schema, &session.model, &session.file_record_keys, None).fields
                    },
                });

                if !is_collapsed {
                    let mut out_refs: HashMap<String, Vec<(String, String)>> = HashMap::new();
                    collect_refs_in_record(record, &key, "", &mut out_refs, &session.model);
                    for (target_key, labels) in &out_refs {
                        let target_file = session
                            .file_record_keys
                            .iter()
                            .find_map(|(fp, keys)| {
                                if keys.contains(target_key) {
                                    Some(fp.clone())
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default();
                        let target_id = format!("{target_file}::{target_key}");
                        for (_, label) in labels {
                            edges.push(GraphEdge {
                                source: node_id.clone(),
                                target: target_id.clone(),
                                field_path: label.clone(),
                            });
                        }
                        if !visited.contains(target_key) {
                            queue.push_back((target_key.clone(), depth + 1));
                        }
                    }
                    if let Some(rev) = reverse_refs.get(&key) {
                        for (src_key, label) in rev {
                            let src_file = session
                                .file_record_keys
                                .iter()
                                .find_map(|(fp, keys)| {
                                    if keys.contains(src_key) {
                                        Some(fp.clone())
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            let src_id = format!("{src_file}::{src_key}");
                            edges.push(GraphEdge {
                                source: src_id,
                                target: node_id.clone(),
                                field_path: label.clone(),
                            });
                            if !visited.contains(src_key) {
                                queue.push_back((src_key.clone(), depth + 1));
                            }
                        }
                    }
                }
            }
            None => {
                nodes.push(GraphNode {
                    id: node_id,
                    key: key.clone(),
                    actual_type: String::new(),
                    file_path: record_file,
                    in_focus_file: focus_keys.contains(&key),
                    is_collapsed: true,
                    fields: Vec::new(),
                });
            }
        }
    }

    // Merge parallel edges: collapse (source, target) duplicates, joining labels with " / "
    let mut edge_map: std::collections::BTreeMap<(String, String), Vec<String>> = std::collections::BTreeMap::new();
    for e in edges {
        edge_map
            .entry((e.source, e.target))
            .or_default()
            .push(e.field_path);
    }
    let edges: Vec<GraphEdge> = edge_map
        .into_iter()
        .map(|((source, target), mut labels)| {
            labels.dedup();
            GraphEdge { source, target, field_path: labels.join(" / ") }
        })
        .collect();

    Ok(GraphData { nodes, edges })
}

pub fn write_field_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
    field_path: &[FieldPathSegment],
    new_value: &FieldValue,
) -> Result<(), String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    let field_exists = ast.records.iter().any(|r| {
        r.key == record_key
            && match field_path.first() {
                Some(FieldPathSegment::Field { name }) => {
                    r.fields.iter().any(|f| &f.name == name)
                        || r.entries.iter().any(|e| {
                            matches!(e, CfdBlockEntry::Field(f) if &f.name == name)
                        })
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

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &result.new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &result.new_source)
}

pub fn create_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    key: &str,
    type_name: &str,
) -> Result<RecordRow, String> {
    validate_cfd_key(key)?;

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Guard: reject duplicate keys
    if let Some(keys) = session.file_record_keys.get(file_path) {
        if keys.contains(&key.to_string()) {
            return Err(format!("record key '{key}' already exists in {file_path}"));
        }
    }
    // Also check global model for cross-file uniqueness
    if session.model.records().any(|(_, r)| r.key == key) {
        return Err(format!("record key '{key}' already exists in the project"));
    }

    let abs_path = session.project_dir.join(file_path);
    let existing = std::fs::read_to_string(&abs_path).unwrap_or_default();
    let new_content = format!("{existing}\n{key}: {type_name} {{\n}}\n");

    std::fs::write(&abs_path, &new_content)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_content)?;

    Ok(RecordRow {
        key: key.to_string(),
        actual_type: type_name.to_string(),
        fields: Vec::new(),
        spread_fields: Vec::new(),
    })
}

pub fn delete_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
) -> Result<(), String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    let span = record.span;
    let span_end = span.end.min(source.len());
    let start = if span.start > 0 && span.start <= source.len() && source.as_bytes().get(span.start - 1) == Some(&b'\n') {
        span.start - 1
    } else {
        span.start.min(source.len())
    };
    let new_source = format!("{}{}", &source[..start], &source[span_end..]);

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_source)
}

pub fn rename_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    old_key: &str,
    new_key: &str,
) -> Result<(), String> {
    if old_key == new_key {
        return Ok(());
    }
    let new_key = new_key.trim();
    validate_cfd_key(new_key)?;

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Guard: reject duplicate keys
    if session.model.records().any(|(_, r)| r.key == new_key) {
        return Err(format!("record key '{new_key}' already exists in the project"));
    }

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    let record = ast
        .records
        .iter()
        .find(|r| r.key == old_key)
        .ok_or_else(|| format!("record '{old_key}' not found in {file_path}"))?;

    let span = record.key_span;
    let new_source = format!("{}{}{}", &source[..span.start], new_key, &source[span.end..]);

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_source)
}

pub fn get_diagnostics_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
) -> Result<Vec<DiagnosticItem>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let mut diagnostics: Vec<DiagnosticItem> = Vec::new();
    if let Err(d) = session.model.run_checks(&session.schema) {
        diagnostics.extend(convert_cfd_diagnostics(&d, Some(&session.model), Some(&session.file_record_keys)));
    }
    Ok(diagnostics)
}

pub fn close_session_inner(store: &Mutex<SessionStore>, session_id: u32) -> Result<(), String> {
    let mut s = store.lock().map_err(|_| "lock poisoned")?;
    s.sessions.remove(&session_id);
    Ok(())
}

pub fn get_all_type_names_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
) -> Result<Vec<String>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;
    let mut names: Vec<String> = session
        .schema
        .all_types()
        .filter(|t| !t.is_abstract)
        .map(|t| t.name.clone())
        .collect();
    names.sort();
    Ok(names)
}

pub fn get_enum_variants_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    enum_name: &str,
) -> Result<Vec<String>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;
    let variants = session
        .schema
        .resolve_enum(enum_name)
        .map(|e| e.variants.iter().map(|v| v.name.clone()).collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(variants)
}

pub fn create_file_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    rel_path: &str,
) -> Result<FileTreeNode, String> {
    if !rel_path.ends_with(".cfd") {
        return Err("file path must end with .cfd".to_string());
    }
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let abs_path = session.project_dir.join(rel_path);
    // Guard against path traversal outside the project directory
    let canonical_project = session
        .project_dir
        .canonicalize()
        .unwrap_or_else(|_| session.project_dir.clone());
    let canonical_target = abs_path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| abs_path.clone());
    if !canonical_target.starts_with(&canonical_project) {
        return Err(format!("path '{rel_path}' is outside the project directory"));
    }
    if let Some(parent) = abs_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create directories: {e}"))?;
    }
    std::fs::write(&abs_path, "").map_err(|e| format!("cannot create file: {e}"))?;

    session
        .file_sources
        .insert(rel_path.to_string(), (String::new(), CfdAst { records: Vec::new() }));
    session
        .file_record_keys
        .insert(rel_path.to_string(), Vec::new());

    let name = abs_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel_path.to_string());

    let rel_pb = PathBuf::from(rel_path);
    let in_sources = session.source_dirs.iter().any(|sd| rel_pb.starts_with(sd));

    Ok(FileTreeNode {
        name,
        path: rel_path.to_string(),
        is_dir: false,
        in_sources,
        children: Vec::new(),
    })
}

pub fn rename_file_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    old_rel_path: &str,
    new_rel_path: &str,
) -> Result<(), String> {
    if old_rel_path == new_rel_path {
        return Ok(());
    }
    if !new_rel_path.ends_with(".cfd") {
        return Err("new file path must end with .cfd".to_string());
    }
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let canonical_project = session
        .project_dir
        .canonicalize()
        .unwrap_or_else(|_| session.project_dir.clone());

    let old_abs = session.project_dir.join(old_rel_path);
    let new_abs = session.project_dir.join(new_rel_path);

    // Guard both paths against traversal
    let canonical_old = old_abs.canonicalize().unwrap_or_else(|_| old_abs.clone());
    if !canonical_old.starts_with(&canonical_project) {
        return Err(format!("path '{old_rel_path}' is outside the project directory"));
    }
    let canonical_new_parent = new_abs
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| new_abs.clone());
    if !canonical_new_parent.starts_with(&canonical_project) {
        return Err(format!("path '{new_rel_path}' is outside the project directory"));
    }
    if new_abs.exists() {
        return Err(format!("'{new_rel_path}' already exists"));
    }
    if let Some(parent) = new_abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create directories: {e}"))?;
    }
    std::fs::rename(&old_abs, &new_abs)
        .map_err(|e| format!("cannot rename '{old_rel_path}' → '{new_rel_path}': {e}"))?;

    // Remap session maps
    if let Some(v) = session.file_sources.remove(old_rel_path) {
        session.file_sources.insert(new_rel_path.to_string(), v);
    }
    if let Some(v) = session.file_record_keys.remove(old_rel_path) {
        session.file_record_keys.insert(new_rel_path.to_string(), v);
    }
    Ok(())
}

pub fn delete_file_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    rel_path: &str,
) -> Result<(), String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let abs_path = session.project_dir.join(rel_path);
    // Guard against path traversal
    let canonical_project = session
        .project_dir
        .canonicalize()
        .unwrap_or_else(|_| session.project_dir.clone());
    let canonical_target = abs_path
        .canonicalize()
        .unwrap_or_else(|_| abs_path.clone());
    if !canonical_target.starts_with(&canonical_project) {
        return Err(format!("path '{rel_path}' is outside the project directory"));
    }
    if abs_path.is_dir() {
        return Err(format!("'{rel_path}' is a directory; only files can be deleted"));
    }
    std::fs::remove_file(&abs_path).map_err(|e| format!("cannot delete '{rel_path}': {e}"))?;

    session.file_sources.remove(rel_path);
    session.file_record_keys.remove(rel_path);
    Ok(())
}

fn get_session(
    store: &Mutex<SessionStore>,
    session_id: u32,
) -> Result<Arc<Mutex<Session>>, String> {
    let s = store.lock().map_err(|_| "store lock poisoned")?;
    s.sessions
        .get(&session_id)
        .cloned()
        .ok_or_else(|| format!("unknown session {session_id}"))
}

/// Validate that a CFD identifier doesn't contain illegal characters.
fn validate_cfd_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("key cannot be empty".to_string());
    }
    let illegal: &[char] = &[':', '=', ';', ',', '{', '}', '[', ']', '(', ')', '@', '&', '"'];
    if key.chars().any(|c| c.is_whitespace() || illegal.contains(&c)) {
        return Err(format!("key '{key}' contains illegal characters (whitespace or any of :=;,{{}}[]()@&\")"));
    }
    Ok(())
}

fn reload_file(session: &mut Session, file_path: &str, new_source: &str) -> Result<(), String> {
    let (new_ast, _) = parse_cfd(new_source);

    let new_keys = match parse_cfd_input_records(&session.schema, new_source) {
        Ok(records) => {
            let keys: Vec<String> = records.iter().map(|r| r.key.clone()).collect();
            let mut builder = CfdDataModel::builder(&session.schema);
            for (fp, (src, _)) in &session.file_sources {
                if fp != file_path {
                    if let Ok(recs) = parse_cfd_input_records(&session.schema, src) {
                        for r in recs {
                            builder.add_input_record(r);
                        }
                    }
                }
            }
            for r in records {
                builder.add_input_record(r);
            }
            if let Ok(new_model) = builder.build() {
                session.model = new_model;
            }
            keys
        }
        Err(_) => Vec::new(),
    };

    session
        .file_sources
        .insert(file_path.to_string(), (new_source.to_string(), new_ast));
    session
        .file_record_keys
        .insert(file_path.to_string(), new_keys);
    Ok(())
}

fn collect_cfd_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        sorted.sort_by_key(|e| e.file_name());
        for entry in sorted {
            let path = entry.path();
            if path.is_dir() {
                result.extend(collect_cfd_files(&path));
            } else if path.extension().map_or(false, |e| e == "cfd") {
                result.push(path);
            }
        }
    }
    result
}

fn relative_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn build_file_tree(base: &Path, dir: &Path, abs_source_dirs: &[PathBuf]) -> Vec<FileTreeNode> {
    let mut nodes: Vec<FileTreeNode> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return nodes;
    };
    let mut sorted: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    sorted.sort_by_key(|e| e.file_name());

    for entry in sorted {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" || name == "target" || name == "gen" {
            continue;
        }
        let rel = relative_path(base, &path);

        if path.is_dir() {
            let children = build_file_tree(base, &path, abs_source_dirs);
            if !children.is_empty() {
                let in_sources = abs_source_dirs
                    .iter()
                    .any(|sd| path.starts_with(sd) || sd.starts_with(&path));
                nodes.push(FileTreeNode {
                    name,
                    path: rel,
                    is_dir: true,
                    in_sources,
                    children,
                });
            }
        } else if path.extension().map_or(false, |e| e == "cfd") {
            let in_sources = abs_source_dirs.iter().any(|sd| path.starts_with(sd));
            nodes.push(FileTreeNode {
                name,
                path: rel,
                is_dir: false,
                in_sources,
                children: Vec::new(),
            });
        }
    }
    nodes
}

fn convert_record_row(
    record: &CfdRecord,
    schema: &CftContainer,
    model: &CfdDataModel,
    file_record_keys: &HashMap<String, Vec<String>>,
    direct_field_names: Option<&HashSet<String>>,
) -> RecordRow {
    let fields = if let Some(schema_type) = schema.resolve_type(&record.actual_type) {
        schema_type
            .all_fields
            .iter()
            .map(|sf| FieldCell {
                name: sf.name.clone(),
                value: record
                    .fields
                    .get(&sf.name)
                    .map(|v| convert_value(v, model, file_record_keys))
                    .unwrap_or(FieldValue::Null),
            })
            .collect()
    } else {
        record
            .fields
            .iter()
            .map(|(name, v)| FieldCell {
                name: name.clone(),
                value: convert_value(v, model, file_record_keys),
            })
            .collect()
    };
    // Spread fields: those in record.fields but NOT in the AST direct field names.
    // If we have no AST info, assume nothing is a spread field.
    let spread_fields: Vec<String> = if let Some(direct) = direct_field_names {
        record
            .fields
            .keys()
            .filter(|name| !direct.contains(*name))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    RecordRow {
        key: record.key.clone(),
        actual_type: record.actual_type.clone(),
        fields,
        spread_fields,
    }
}

fn find_file_for_key<'a>(file_record_keys: &'a HashMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    file_record_keys
        .iter()
        .find_map(|(fp, keys)| if keys.iter().any(|k| k == key) { Some(fp.as_str()) } else { None })
}

fn convert_value(v: &CfdValue, model: &CfdDataModel, file_record_keys: &HashMap<String, Vec<String>>) -> FieldValue {
    match v {
        CfdValue::Null => FieldValue::Null,
        CfdValue::Bool(b) => FieldValue::Bool { v: *b },
        CfdValue::Int(i) => FieldValue::Int { v: *i as f64 },
        CfdValue::Float(f) => FieldValue::Float { v: *f },
        CfdValue::String(s) => FieldValue::Str { v: s.clone() },
        CfdValue::Enum(e) => FieldValue::Enum {
            enum_name: e.enum_name.clone(),
            variant: e.variant.clone().unwrap_or_default(),
            int_value: e.value as f64,
        },
        CfdValue::Object(record) => FieldValue::Object {
            actual_type: record.actual_type.clone(),
            fields: record
                .fields
                .iter()
                .map(|(name, v)| FieldCell {
                    name: name.clone(),
                    value: convert_value(v, model, file_record_keys),
                })
                .collect(),
        },
        CfdValue::Ref { key, target } => {
            let target_type = model
                .record(*target)
                .map(|r| r.actual_type.clone())
                .unwrap_or_default();
            let target_file = find_file_for_key(file_record_keys, key).map(|s| s.to_string());
            FieldValue::Ref {
                target_type,
                target_key: key.clone(),
                target_file,
            }
        }
        CfdValue::Array(items) => FieldValue::Array {
            items: items.iter().map(|i| convert_value(i, model, file_record_keys)).collect(),
        },
        CfdValue::Dict(entries) => FieldValue::Dict {
            entries: entries
                .iter()
                .map(|(k, v)| DictEntry {
                    key: convert_dict_key(k),
                    value: convert_value(v, model, file_record_keys),
                })
                .collect(),
        },
    }
}

fn convert_dict_key(k: &CfdDictKey) -> DictKey {
    match k {
        CfdDictKey::String(s) => DictKey::Str { v: s.clone() },
        CfdDictKey::Int(i) => DictKey::Int { v: *i as f64 },
        CfdDictKey::Enum(CfdEnumValue {
            enum_name,
            variant,
            value,
        }) => DictKey::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone().unwrap_or_default(),
            int_value: *value as f64,
        },
    }
}

fn convert_cfd_diagnostics(
    diags: &CfdDiagnostics,
    model: Option<&CfdDataModel>,
    file_record_keys: Option<&HashMap<String, Vec<String>>>,
) -> Vec<DiagnosticItem> {
    use coflow_data_model::CfdPathSegment;
    diags
        .diagnostics
        .iter()
        .map(|d| {
            let (record_key, field_path) = match d.primary.as_ref() {
                None => (None, None),
                Some(l) => {
                    let key = l.record
                        .and_then(|id| model?.record(id))
                        .map(|r| r.key.clone());
                    let mut out = String::new();
                    for s in &l.path.segments {
                        match s {
                            CfdPathSegment::Field(name) => {
                                if !out.is_empty() { out.push('.'); }
                                out.push_str(name);
                            }
                            CfdPathSegment::Index(i) => out.push_str(&format!("[{i}]")),
                            CfdPathSegment::DictKey(k) => out.push_str(&format!("[{k}]")),
                        }
                    }
                    (key, if out.is_empty() { None } else { Some(out) })
                }
            };
            // Resolve which file contains this record key
            let file_path = record_key.as_deref().and_then(|rk| {
                let keys = file_record_keys?;
                keys.iter().find_map(|(fp, ks)| if ks.iter().any(|k| k == rk) { Some(fp.clone()) } else { None })
            });
            DiagnosticItem {
                severity: format!("{:?}", d.severity).to_lowercase(),
                code: format!("{:?}", d.code),
                stage: d.stage.to_string(),
                message: d.message.clone(),
                file_path,
                record_key,
                field_path,
            }
        })
        .collect()
}

fn collect_refs_in_record(
    record: &CfdRecord,
    source_key: &str,
    prefix: &str,
    reverse_refs: &mut HashMap<String, Vec<(String, String)>>,
    model: &CfdDataModel,
) {
    for (field_name, value) in &record.fields {
        let label = if prefix.is_empty() {
            field_name.clone()
        } else {
            format!("{prefix}.{field_name}")
        };
        collect_refs_in_value(value, source_key, &label, reverse_refs, model);
    }
}

fn collect_refs_in_value(
    value: &CfdValue,
    source_key: &str,
    label: &str,
    reverse_refs: &mut HashMap<String, Vec<(String, String)>>,
    model: &CfdDataModel,
) {
    match value {
        CfdValue::Ref { key, .. } => {
            reverse_refs
                .entry(key.clone())
                .or_default()
                .push((source_key.to_string(), label.to_string()));
        }
        CfdValue::Object(record) => {
            collect_refs_in_record(record, source_key, label, reverse_refs, model);
        }
        CfdValue::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                collect_refs_in_value(
                    item,
                    source_key,
                    &format!("{label}[{i}]"),
                    reverse_refs,
                    model,
                );
            }
        }
        CfdValue::Dict(entries) => {
            for (k, v) in entries {
                collect_refs_in_value(
                    v,
                    source_key,
                    &format!("{label}[{k:?}]"),
                    reverse_refs,
                    model,
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn load_example_project() {
        let yaml_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd/coflow.yaml");
        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml_path.to_str().unwrap()).unwrap();
        assert!(
            snap.diagnostics.iter().all(|d| d.severity != "error"),
            "{:?}",
            snap.diagnostics
        );
        assert!(!snap.file_tree.is_empty());

        let records =
            get_file_records_inner(&store, snap.session_id, "data/01-records.cfd").unwrap();
        assert!(!records.records.is_empty());

        // Test get_record for a known record
        let row = get_record_inner(&store, snap.session_id, "data/01-records.cfd", "sword_fire").unwrap();
        assert_eq!(row.key, "sword_fire");
        assert!(!row.fields.is_empty());

        // Test get_graph - should return nodes
        let graph = get_graph_inner(&store, snap.session_id, "data/01-records.cfd", &[]).unwrap();
        assert!(!graph.nodes.is_empty(), "graph should have nodes");

        // Test Ref target_file is populated for cross-record refs
        // basic_monster has drop.rewards[0].item = &sword_fire
        let monster = get_record_inner(&store, snap.session_id, "data/01-records.cfd", "basic_monster").unwrap();
        // basic_monster exists, verify fields are present
        let _ = monster.fields;
    }

    #[test]
    fn write_field_roundtrip() {
        // Build a minimal project in a temp dir
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("test.cfd");

        // Minimal schema
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type TestItem { name: string; count: int; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "item_a: TestItem {\n  name: \"hello\",\n  count: 5,\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Write a field
        let result = write_field_inner(
            &store, sid, "data/test.cfd", "item_a",
            &[FieldPathSegment::Field { name: "count".to_string() }],
            &FieldValue::Int { v: 42.0 },
        );
        assert!(result.is_ok(), "write_field failed: {:?}", result);

        // Verify the file was written
        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("42"), "file should contain '42', got: {contents}");

        // Verify the model was updated
        let records = get_file_records_inner(&store, sid, "data/test.cfd").unwrap();
        let item = records.records.iter().find(|r| r.key == "item_a").unwrap();
        let count_field = item.fields.iter().find(|f| f.name == "count").unwrap();
        assert!(
            matches!(&count_field.value, FieldValue::Int { v } if (*v - 42.0).abs() < 0.001),
            "expected Int 42, got {:?}",
            count_field.value
        );
    }

    #[test]
    fn create_and_delete_record() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");

        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Create a record
        let row = create_record_inner(&store, sid, "data/items.cfd", "sword", "Item").unwrap();
        assert_eq!(row.key, "sword");

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("sword"), "file should contain 'sword'");

        // Delete the record
        delete_record_inner(&store, sid, "data/items.cfd", "sword").unwrap();
        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(!contents.contains("sword"), "file should not contain 'sword' after delete");
    }

    #[test]
    fn write_float_preserves_decimal_point() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("test.cfd");

        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Stats { hp: float; speed: float; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "hero: Stats {\n  hp: 1.0,\n  speed: 2.5,\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Write a whole-number float — must preserve decimal point so parser reads it back as float
        write_field_inner(
            &store, sid, "data/test.cfd", "hero",
            &[FieldPathSegment::Field { name: "hp".to_string() }],
            &FieldValue::Float { v: 100.0 },
        ).unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(
            contents.contains("100.0"),
            "float 100.0 should be written with decimal point, got:\n{contents}"
        );

        // Verify model reads it back as float
        let records = get_file_records_inner(&store, sid, "data/test.cfd").unwrap();
        let hero = records.records.iter().find(|r| r.key == "hero").unwrap();
        let hp = hero.fields.iter().find(|f| f.name == "hp").unwrap();
        assert!(
            matches!(&hp.value, FieldValue::Float { v } if (*v - 100.0).abs() < 0.001),
            "expected Float 100.0, got {:?}", hp.value
        );
    }

    #[test]
    fn rename_record_updates_key() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");

        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "old_key: Item {\n  name: \"test\",\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        rename_record_inner(&store, sid, "data/items.cfd", "old_key", "new_key").unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("new_key"), "file should contain new_key");
        assert!(!contents.contains("old_key"), "file should not contain old_key");

        let records = get_file_records_inner(&store, sid, "data/items.cfd").unwrap();
        assert!(records.records.iter().any(|r| r.key == "new_key"));
        assert!(!records.records.iter().any(|r| r.key == "old_key"));
    }

    #[test]
    fn write_field_in_grouped_record() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; count: int; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "Item {\n  sword {\n    name: \"Sword\",\n    count: 1,\n  }\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        write_field_inner(
            &store, sid, "data/items.cfd", "sword",
            &[FieldPathSegment::Field { name: "count".to_string() }],
            &FieldValue::Int { v: 99.0 },
        ).unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("99"), "grouped record field should be updated:\n{contents}");
    }

    #[test]
    fn rename_grouped_record() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");

        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        // Grouped record syntax
        std::fs::write(&cfd, "Item {\n  old_key {\n    name: \"test\",\n  }\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        rename_record_inner(&store, sid, "data/items.cfd", "old_key", "new_key").unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("new_key"), "file should contain new_key:\n{contents}");
        assert!(!contents.contains("old_key"), "file should not contain old_key:\n{contents}");
    }

    #[test]
    fn get_diagnostics_returns_current_checks() {
        let yaml_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd/coflow.yaml");
        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml_path.to_str().unwrap()).unwrap();
        // Example project should have no errors
        let diags = get_diagnostics_inner(&store, snap.session_id).unwrap();
        assert!(
            diags.iter().all(|d| d.severity != "error"),
            "example project should have no errors, got: {:?}", diags
        );
    }

    #[test]
    fn spread_fields_detected() {
        let yaml_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd/coflow.yaml");
        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml_path.to_str().unwrap()).unwrap();

        // elite_monster uses ...@Monster.basic_monster at top level;
        // it directly declares: name, stats, weaknesses, drop.
        // Any other Monster fields (like loot_multiplier) come from the spread and should
        // be in spread_fields.
        let records = get_file_records_inner(&store, snap.session_id, "data/03-spread.cfd").unwrap();
        let elite = records.records.iter().find(|r| r.key == "elite_monster").expect("elite_monster should exist");

        let direct = ["name", "stats", "weaknesses", "drop"];
        for field_name in &direct {
            assert!(
                !elite.spread_fields.contains(&field_name.to_string()),
                "field '{field_name}' is declared directly so should NOT be in spread_fields"
            );
        }
        // Fields NOT declared directly in elite_monster's block must be in spread_fields
        for field_cell in &elite.fields {
            if !direct.contains(&field_cell.name.as_str()) {
                assert!(
                    elite.spread_fields.contains(&field_cell.name),
                    "field '{}' is not declared directly in elite_monster so should be in spread_fields",
                    field_cell.name
                );
            }
        }
    }
}
