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
                Err(e) => diagnostics.push(DiagnosticItem {
                    severity: "error".into(),
                    code: "CFD-READ".into(),
                    stage: "LOAD".into(),
                    message: format!("cannot read {}: {e}", cfd_path.display()),
                    file_path: Some(rel),
                    record_key: None,
                    field_path: None,
                }),
            }
        }
    }

    let mut builder = CfdDataModel::builder(&schema);
    for r in all_input_records {
        builder.add_input_record(r);
    }
    let model = builder.build().unwrap_or_else(|d| {
        diagnostics.extend(convert_cfd_diagnostics(&d));
        CfdDataModel::builder(&schema)
            .build()
            .unwrap_or_else(|_| unreachable!("empty builder must succeed"))
    });

    if let Err(d) = model.run_checks(&schema) {
        diagnostics.extend(convert_cfd_diagnostics(&d));
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

    for key in &keys {
        if let Some((_, record)) = session.model.records().find(|(_, r)| r.key == *key) {
            if !type_names.contains(&record.actual_type) {
                type_names.push(record.actual_type.clone());
            }
            records.push(convert_record_row(record, &session.schema, &session.model));
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
    _file_path: &str,
    record_key: &str,
) -> Result<RecordRow, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (_, record) = session
        .model
        .records()
        .find(|(_, r)| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    Ok(convert_record_row(record, &session.schema, &session.model))
}

pub fn get_graph_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
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
        let is_collapsed = depth >= MAX_DEPTH;

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
                        convert_record_row(record, &session.schema, &session.model).fields
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

    let mut seen: HashSet<(String, String)> = HashSet::new();
    edges.retain(|e| seen.insert((e.source.clone(), e.target.clone())));

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
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

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
    let start = if span.start > 0 && source.as_bytes().get(span.start - 1) == Some(&b'\n') {
        span.start - 1
    } else {
        span.start
    };
    let new_source = format!("{}{}", &source[..start], &source[span.end..]);

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_source)
}

pub fn create_file_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    rel_path: &str,
) -> Result<FileTreeNode, String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let abs_path = session.project_dir.join(rel_path);
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
                    .map(|v| convert_value(v, model))
                    .unwrap_or(FieldValue::Null),
            })
            .collect()
    } else {
        record
            .fields
            .iter()
            .map(|(name, v)| FieldCell {
                name: name.clone(),
                value: convert_value(v, model),
            })
            .collect()
    };
    RecordRow {
        key: record.key.clone(),
        actual_type: record.actual_type.clone(),
        fields,
    }
}

fn convert_value(v: &CfdValue, model: &CfdDataModel) -> FieldValue {
    match v {
        CfdValue::Null => FieldValue::Null,
        CfdValue::Bool(b) => FieldValue::Bool { v: *b },
        CfdValue::Int(i) => FieldValue::Int { v: *i },
        CfdValue::Float(f) => FieldValue::Float { v: *f },
        CfdValue::String(s) => FieldValue::Str { v: s.clone() },
        CfdValue::Enum(e) => FieldValue::Enum {
            enum_name: e.enum_name.clone(),
            variant: e.variant.clone().unwrap_or_default(),
            int_value: e.value,
        },
        CfdValue::Object(record) => FieldValue::Object {
            actual_type: record.actual_type.clone(),
            fields: record
                .fields
                .iter()
                .map(|(name, v)| FieldCell {
                    name: name.clone(),
                    value: convert_value(v, model),
                })
                .collect(),
        },
        CfdValue::Ref { key, target } => {
            let target_type = model
                .record(*target)
                .map(|r| r.actual_type.clone())
                .unwrap_or_default();
            FieldValue::Ref {
                target_type,
                target_key: key.clone(),
                target_file: None,
            }
        }
        CfdValue::Array(items) => FieldValue::Array {
            items: items.iter().map(|i| convert_value(i, model)).collect(),
        },
        CfdValue::Dict(entries) => FieldValue::Dict {
            entries: entries
                .iter()
                .map(|(k, v)| DictEntry {
                    key: convert_dict_key(k),
                    value: convert_value(v, model),
                })
                .collect(),
        },
    }
}

fn convert_dict_key(k: &CfdDictKey) -> DictKey {
    match k {
        CfdDictKey::String(s) => DictKey::Str { v: s.clone() },
        CfdDictKey::Int(i) => DictKey::Int { v: *i },
        CfdDictKey::Enum(CfdEnumValue {
            enum_name,
            variant,
            value,
        }) => DictKey::Enum {
            enum_name: enum_name.clone(),
            variant: variant.clone().unwrap_or_default(),
            int_value: *value,
        },
    }
}

fn convert_cfd_diagnostics(diags: &CfdDiagnostics) -> Vec<DiagnosticItem> {
    diags
        .diagnostics
        .iter()
        .map(|d| {
            let field_path = d.primary.as_ref().map(|l| {
                l.path
                    .segments
                    .iter()
                    .map(|s| format!("{s:?}"))
                    .collect::<Vec<_>>()
                    .join(".")
            });
            DiagnosticItem {
                severity: format!("{:?}", d.severity).to_lowercase(),
                code: format!("{:?}", d.code),
                stage: d.stage.to_string(),
                message: d.message.clone(),
                file_path: None,
                record_key: None,
                field_path: field_path.filter(|s| !s.is_empty()),
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
    }
}
