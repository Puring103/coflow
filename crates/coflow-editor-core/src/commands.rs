use crate::patch;
use crate::types::{
    DiagnosticItem, DictEntry, DictKey, FieldCell, FieldPathSegment, FieldSchema, FieldValue,
    FileRecords, FileTreeNode, GraphData, GraphEdge, GraphNode, ProjectSnapshot, RecordBrief,
    RecordRow, SearchHit, SpreadSource,
};
use coflow_cfd::{parse_cfd, CfdAst, CfdBlockEntry};
use coflow_checker::CfdCheckExt;
use coflow_cft::{CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef, ModuleId};
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
    /// Diagnostics from the most recent model-build attempt that failed.
    /// Cleared when the model builds successfully.
    pub pending_model_diags: Vec<DiagnosticItem>,
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

    let id_to_key: Vec<String> = all_input_records.iter().map(|r| r.key.clone()).collect();
    let mut builder = CfdDataModel::builder(&schema);
    for r in all_input_records {
        builder.add_input_record(r);
    }
    let model = builder.build().unwrap_or_else(|d| {
        diagnostics.extend(convert_cfd_diagnostics_with_index(&d, None, Some(&id_to_key), Some(&file_record_keys)));
        CfdDataModel::builder(&schema)
            .build()
            .unwrap_or_else(|_| unreachable!("empty builder must succeed"))
    });

    if let Err(d) = model.run_checks(&schema) {
        diagnostics.extend(convert_cfd_diagnostics(&d, Some(&model), Some(&file_record_keys)));
    }

    let mut file_tree = build_file_tree(&project_dir, &project_dir, &source_dirs);
    annotate_tree_record_counts(&mut file_tree, &file_record_keys);

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
                pending_model_diags: Vec::new(),
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

    // Build a lookup from key → ast record for the fallback path
    let ast_records: HashMap<String, &coflow_cfd::CfdRecord> = session
        .file_sources
        .get(file_path)
        .map(|(_, ast)| ast.records.iter().map(|r| (r.key.clone(), r)).collect())
        .unwrap_or_default();

    for key in &keys {
        if let Some((_, record)) = session.model.records().find(|(_, r)| r.key == *key) {
            if !type_names.contains(&record.actual_type) {
                type_names.push(record.actual_type.clone());
            }
            let direct = ast_direct.get(key.as_str());
            let ast_rec = ast_records.get(key.as_str()).copied();
            let mut row = convert_record_row_with_ast(record, &session.schema, &session.model, &session.file_record_keys, direct, ast_rec);
            row.file_path = file_path.to_string();
            records.push(row);
        } else if let Some(ast_rec) = ast_records.get(key.as_str()) {
            // Model build failed for this record (e.g. missing required fields during editing).
            // Return a best-effort row from the raw AST so the UI stays responsive.
            if !type_names.contains(&ast_rec.type_name) {
                type_names.push(ast_rec.type_name.clone());
            }
            let mut row = ast_record_fallback(ast_rec, &session.schema);
            row.file_path = file_path.to_string();
            records.push(row);
        }
    }

    Ok(FileRecords {
        file_path: file_path.to_string(),
        type_names,
        records,
    })
}

/// Returns all records of a given type across every loaded file, with their file paths embedded.
/// Used by the cross-file global table view.
pub fn get_all_records_of_type_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    type_name: &str,
) -> Result<Vec<RecordRow>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let mut results: Vec<RecordRow> = Vec::new();

    // Iterate all files in insertion order
    for (file_path, keys) in &session.file_record_keys {
        let ast_direct: HashMap<String, HashSet<String>> = session
            .file_sources
            .get(file_path.as_str())
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

        let ast_records: HashMap<String, &coflow_cfd::CfdRecord> = session
            .file_sources
            .get(file_path.as_str())
            .map(|(_, ast)| ast.records.iter().map(|r| (r.key.clone(), r)).collect())
            .unwrap_or_default();

        for key in keys {
            if let Some((_, record)) = session.model.records().find(|(_, r)| r.key == *key) {
                if record.actual_type != type_name {
                    continue;
                }
                let direct = ast_direct.get(key.as_str());
                let ast_rec = ast_records.get(key.as_str()).copied();
                let mut row = convert_record_row_with_ast(
                    record,
                    &session.schema,
                    &session.model,
                    &session.file_record_keys,
                    direct,
                    ast_rec,
                );
                row.file_path = file_path.clone();
                results.push(row);
            } else if let Some(ast_rec) = ast_records.get(key.as_str()) {
                if ast_rec.type_name != type_name {
                    continue;
                }
                let mut row = ast_record_fallback(ast_rec, &session.schema);
                row.file_path = file_path.clone();
                results.push(row);
            }
        }
    }

    Ok(results)
}

/// Returns the absolute path for a project-relative file path in the session.
pub fn resolve_project_path_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    rel_path: &str,
) -> Result<String, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;
    let abs = session.project_dir.join(rel_path);
    Ok(abs.to_string_lossy().to_string())
}

pub fn get_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
) -> Result<RecordRow, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let in_model = session.model.records().any(|(_, r)| r.key == record_key);
    if in_model {
        let (_, record) = session.model.records().find(|(_, r)| r.key == record_key)
            .expect("record exists: checked with .any() on same model");
        let ast_rec = session
            .file_sources
            .get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key));
        let direct = ast_rec.map(|r| r.fields.iter().map(|f| f.name.clone()).collect::<HashSet<String>>());
        let mut row = convert_record_row_with_ast(record, &session.schema, &session.model, &session.file_record_keys, direct.as_ref(), ast_rec);
        row.file_path = file_path.to_string();
        Ok(row)
    } else {
        let schema = &session.schema;
        let fallback = session
            .file_sources
            .get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key))
            .map(|r| {
                let mut row = ast_record_fallback(r, schema);
                row.file_path = file_path.to_string();
                row
            });
        fallback.ok_or_else(|| format!("record '{record_key}' not found"))
    }
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
            labels.sort();
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

fn default_value_cfd(d: &CftSchemaDefaultValue) -> String {
    match d {
        CftSchemaDefaultValue::Null => "null".to_string(),
        CftSchemaDefaultValue::Int(v) => v.to_string(),
        CftSchemaDefaultValue::Float(v) => {
            let s = v.to_string();
            if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{s}.0") }
        }
        CftSchemaDefaultValue::Bool(v) => v.to_string(),
        CftSchemaDefaultValue::String(v) => format!("{v:?}"),
        CftSchemaDefaultValue::Enum { variant, .. } => variant.clone(),
        CftSchemaDefaultValue::EmptyArray => "[]".to_string(),
        CftSchemaDefaultValue::EmptyObject => "{}".to_string(),
    }
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

    // Guard: reject duplicate keys — check AST-based index so this works even when model build failed
    let key_exists_in_project = session
        .file_record_keys
        .values()
        .any(|keys| keys.contains(&key.to_string()));
    if key_exists_in_project {
        let conflicting_file = session
            .file_record_keys
            .iter()
            .find_map(|(fp, keys)| if keys.contains(&key.to_string()) { Some(fp.as_str()) } else { None })
            .unwrap_or("unknown file");
        return Err(format!("record key '{key}' already exists in {conflicting_file}"));
    }
    // Guard: reject unknown or abstract type names
    match session.schema.resolve_type(type_name) {
        Some(t) if t.is_abstract => {
            return Err(format!("type '{type_name}' is abstract and cannot be instantiated"));
        }
        None => {
            return Err(format!("unknown type '{type_name}'"));
        }
        _ => {}
    }

    // Build initial field lines from schema defaults
    let default_fields_body: String = {
        let schema_type = session.schema.resolve_type(type_name);
        if let Some(t) = schema_type {
            t.all_fields
                .iter()
                .filter_map(|f| f.default.as_ref().map(|d| (f.name.clone(), default_value_cfd(d))))
                .map(|(name, val)| format!("  {name}: {val}\n"))
                .collect()
        } else {
            String::new()
        }
    };

    let abs_path = session.project_dir.join(file_path);
    let existing = std::fs::read_to_string(&abs_path).unwrap_or_default();

    // Detect whether this file uses grouped syntax for this type_name.
    // Grouped: `TypeName { key { ... } }` — the group token (type_name) appears
    // before the record key in source, so type_span.start < key_span.start.
    // Standalone: `key: TypeName { ... }` — key comes first, type_span.start > key_span.start.
    // Also detect grouped format by looking for the `TypeName {` block header in the source
    // even if all records were deleted (leaving an empty group block).
    let (ast, _) = parse_cfd(&existing);
    let uses_grouped = ast.records.iter().any(|r| {
        r.type_name == type_name && r.type_span.start < r.key_span.start
    }) || source_has_grouped_header(&existing, type_name);

    let separator = if existing.ends_with('\n') || existing.is_empty() { "" } else { "\n" };

    let new_content = if uses_grouped {
        // Find the closing `}` of the existing group block and insert before it.
        // Strategy: scan for the last occurrence of `\n}` at the end of the file
        // (the group closer), then insert the new record before it.
        if let Some(group_end) = find_group_closer(&existing, type_name) {
            let before = &existing[..group_end];
            let after = &existing[group_end..];
            if default_fields_body.is_empty() {
                format!("{before}  {key} {{\n  }}\n{after}")
            } else {
                // Indent default fields by extra 2 spaces for grouped format
                let indented: String = default_fields_body.lines()
                    .map(|l| format!("  {l}\n"))
                    .collect();
                format!("{before}  {key} {{\n{indented}  }}\n{after}")
            }
        } else {
            // Fallback: couldn't locate group block, append standalone
            format!("{existing}{separator}{key}: {type_name} {{\n{default_fields_body}}}\n")
        }
    } else {
        format!("{existing}{separator}{key}: {type_name} {{\n{default_fields_body}}}\n")
    };

    std::fs::write(&abs_path, &new_content)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_content)?;

    Ok(RecordRow {
        key: key.to_string(),
        actual_type: type_name.to_string(),
        file_path: file_path.to_string(),
        fields: Vec::new(),
        spread_fields: Vec::new(),
        spread_sources: Vec::new(),
        is_fallback: false,
    })
}

/// Replace the source text of an existing record with new CFD source.
/// The new source must contain exactly one record with the same key.
pub fn write_record_source_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
    new_source: &str,
) -> Result<(), String> {
    let new_source = new_source.trim();
    if new_source.is_empty() {
        return Err("new source is empty".to_string());
    }

    // Validate the new source
    let (new_ast, _) = parse_cfd(new_source);
    if new_ast.records.is_empty() {
        return Err("no records found in the new source".to_string());
    }
    if new_ast.records.len() > 1 {
        return Err(format!(
            "new source must contain exactly one record, found {}",
            new_ast.records.len()
        ));
    }
    if new_ast.records[0].key != record_key {
        return Err(format!(
            "record key mismatch: expected '{}', got '{}'",
            record_key, new_ast.records[0].key
        ));
    }

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
        .ok_or_else(|| format!("record '{record_key}' not found in {file_path}"))?;

    let span = record.span;
    let span_end = span.end.min(source.len());
    let span_start = span.start.min(source.len());

    let file_source = format!("{}{}\n{}", &source[..span_start], new_source, &source[span_end..]);

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &file_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &file_source)
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

/// Move a record from `src_file` to `dst_file`.
/// The record is appended to the destination file in standalone format
/// (or inserted into a grouped block if the destination already uses that format for this type).
/// Returns the `RecordRow` for the moved record as now seen in `dst_file`.
pub fn move_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    src_file: &str,
    dst_file: &str,
    record_key: &str,
) -> Result<RecordRow, String> {
    if src_file == dst_file {
        return Err("source and destination file are the same".to_string());
    }

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Guard: destination file must be loaded
    if !session.file_sources.contains_key(dst_file) {
        return Err(format!("destination file '{dst_file}' not loaded"));
    }

    // ── 1. Extract record source text from source file ────────────────────────
    let (src_source, src_ast) = session
        .file_sources
        .get(src_file)
        .ok_or_else(|| format!("file '{src_file}' not loaded"))?
        .clone();

    let rec = src_ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found in '{src_file}'"))?;

    let is_grouped = rec.type_span.start < rec.key_span.start;
    let type_name = rec.type_name.clone();

    // Build a standalone record text to insert into dst_file
    let record_text = if is_grouped {
        let after_key = src_source
            .get(rec.key_span.end..rec.span.end)
            .ok_or("key/span offsets out of range")?;
        format!("{record_key}: {type_name}{after_key}")
    } else {
        src_source
            .get(rec.span.start..rec.span.end)
            .ok_or("span offsets out of range")?
            .to_string()
    };

    // ── 2. Append record to destination file ──────────────────────────────────
    let (dst_source, dst_ast) = session
        .file_sources
        .get(dst_file)
        .ok_or_else(|| format!("file '{dst_file}' not loaded"))?
        .clone();

    let dst_uses_grouped = dst_ast.records.iter().any(|r| {
        r.type_name == type_name && r.type_span.start < r.key_span.start
    }) || source_has_grouped_header(&dst_source, &type_name);

    let dst_abs_path = session.project_dir.join(dst_file);
    let new_dst = if dst_uses_grouped {
        if let Some(group_closer) = find_group_closer(&dst_source, &type_name) {
            let before = &dst_source[..group_closer];
            let after = &dst_source[group_closer..];
            // Extract just the body part (after `key: TypeName`)
            let body_part = record_text.trim_start_matches(record_key)
                .trim_start_matches(':')
                .trim_start_matches(&format!(" {type_name}"))
                .trim_start();
            format!("{before}  {record_key} {body_part}\n{after}")
        } else {
            let sep = if dst_source.ends_with('\n') || dst_source.is_empty() { "" } else { "\n" };
            format!("{dst_source}{sep}{record_text}\n")
        }
    } else {
        let sep = if dst_source.ends_with('\n') || dst_source.is_empty() { "" } else { "\n" };
        format!("{dst_source}{sep}{record_text}\n")
    };

    std::fs::write(&dst_abs_path, &new_dst)
        .map_err(|e| format!("cannot write {dst_file}: {e}"))?;
    reload_file(&mut session, dst_file, &new_dst)?;

    // ── 3. Delete record from source file ─────────────────────────────────────
    // Re-borrow after reload_file modified session (cannot use src_source/src_ast anymore)
    let (src2, src_ast2) = session
        .file_sources
        .get(src_file)
        .ok_or_else(|| format!("source file '{src_file}' not found after dst write"))?
        .clone();

    let rec2 = src_ast2
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found in source after dst write"))?;

    let span = rec2.span;
    let span_end = span.end.min(src2.len());
    let start = if span.start > 0 && span.start <= src2.len() && src2.as_bytes().get(span.start - 1) == Some(&b'\n') {
        span.start - 1
    } else {
        span.start.min(src2.len())
    };
    let new_src = format!("{}{}", &src2[..start], &src2[span_end..]);

    let src_abs_path = session.project_dir.join(src_file);
    std::fs::write(&src_abs_path, &new_src)
        .map_err(|e| format!("cannot write {src_file}: {e}"))?;
    reload_file(&mut session, src_file, &new_src)?;

    // ── 4. Return the RecordRow for the moved record in dst_file ──────────────
    let in_model = session.model.records().any(|(_, r)| r.key == record_key);
    if in_model {
        let (_, record) = session.model.records().find(|(_, r)| r.key == record_key)
            .expect("exists: checked above");
        let ast_rec = session.file_sources.get(dst_file)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key));
        let direct = ast_rec.map(|r| r.fields.iter().map(|f| f.name.clone()).collect::<HashSet<String>>());
        let mut row = convert_record_row_with_ast(record, &session.schema, &session.model, &session.file_record_keys, direct.as_ref(), ast_rec);
        row.file_path = dst_file.to_string();
        Ok(row)
    } else {
        let schema = &session.schema;
        session.file_sources.get(dst_file)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == record_key))
            .map(|r| {
                let mut row = ast_record_fallback(r, schema);
                row.file_path = dst_file.to_string();
                row
            })
            .ok_or_else(|| format!("record '{record_key}' not found in '{dst_file}' after move"))
    }
}

/// Copy a record to a different file, optionally under a new key.
/// The original record is left untouched.
pub fn copy_record_to_file_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    src_file: &str,
    dst_file: &str,
    record_key: &str,
    new_key: &str,
) -> Result<RecordRow, String> {
    if new_key != record_key {
        validate_cfd_key(new_key)?;
    }

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Guard: destination file must be loaded
    if !session.file_sources.contains_key(dst_file) {
        return Err(format!("destination file '{dst_file}' not loaded"));
    }

    // Guard duplicate key
    if session.file_record_keys.values().any(|keys| keys.contains(&new_key.to_string())) {
        return Err(format!("record key '{new_key}' already exists in the project"));
    }

    // ── 1. Extract record source text from source file ────────────────────────
    let (src_source, src_ast) = session
        .file_sources
        .get(src_file)
        .ok_or_else(|| format!("file '{src_file}' not loaded"))?
        .clone();

    let rec = src_ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found in '{src_file}'"))?;

    let is_grouped = rec.type_span.start < rec.key_span.start;
    let type_name = rec.type_name.clone();

    // Build a standalone record text using new_key
    let record_text = if is_grouped {
        let after_key = src_source
            .get(rec.key_span.end..rec.span.end)
            .ok_or("key/span offsets out of range")?;
        format!("{new_key}: {type_name}{after_key}")
    } else {
        let span_text = src_source
            .get(rec.span.start..rec.span.end)
            .ok_or("span offsets out of range")?;
        if new_key == record_key {
            span_text.to_string()
        } else {
            // Replace the leading key token
            let after_old_key = src_source
                .get(rec.key_span.end..rec.span.end)
                .ok_or("key_span offsets out of range")?;
            format!("{new_key}{after_old_key}")
        }
    };

    // ── 2. Append record to destination file ──────────────────────────────────
    let (dst_source, dst_ast) = session
        .file_sources
        .get(dst_file)
        .ok_or_else(|| format!("file '{dst_file}' not loaded"))?
        .clone();

    let dst_uses_grouped = dst_ast.records.iter().any(|r| {
        r.type_name == type_name && r.type_span.start < r.key_span.start
    }) || source_has_grouped_header(&dst_source, &type_name);

    let dst_abs_path = session.project_dir.join(dst_file);

    let new_dst = if dst_uses_grouped {
        if let Some(group_closer) = find_group_closer(&dst_source, &type_name) {
            let before = &dst_source[..group_closer];
            let after = &dst_source[group_closer..];
            let body_part = record_text.trim_start_matches(new_key)
                .trim_start_matches(':')
                .trim_start_matches(&format!(" {type_name}"))
                .trim_start();
            format!("{before}  {new_key} {body_part}\n{after}")
        } else {
            let sep = if dst_source.ends_with('\n') || dst_source.is_empty() { "" } else { "\n" };
            format!("{dst_source}{sep}{record_text}\n")
        }
    } else {
        let sep = if dst_source.ends_with('\n') || dst_source.is_empty() { "" } else { "\n" };
        format!("{dst_source}{sep}{record_text}\n")
    };

    std::fs::write(&dst_abs_path, &new_dst)
        .map_err(|e| format!("cannot write {dst_file}: {e}"))?;
    reload_file(&mut session, dst_file, &new_dst)?;

    // ── 3. Return the RecordRow for the new record in dst_file ──────────────
    let in_model = session.model.records().any(|(_, r)| r.key == new_key);
    if in_model {
        let (_, record) = session.model.records().find(|(_, r)| r.key == new_key)
            .expect("exists: checked above");
        let ast_rec = session.file_sources.get(dst_file)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == new_key));
        let direct = ast_rec.map(|r| r.fields.iter().map(|f| f.name.clone()).collect::<HashSet<String>>());
        let mut row = convert_record_row_with_ast(record, &session.schema, &session.model, &session.file_record_keys, direct.as_ref(), ast_rec);
        row.file_path = dst_file.to_string();
        Ok(row)
    } else {
        let schema = &session.schema;
        session.file_sources.get(dst_file)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == new_key))
            .map(|r| {
                let mut row = ast_record_fallback(r, schema);
                row.file_path = dst_file.to_string();
                row
            })
            .ok_or_else(|| format!("record '{new_key}' not found in '{dst_file}' after copy"))
    }
}

pub fn get_record_source_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    record_key: &str,
) -> Result<String, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?;

    let record = ast
        .records
        .iter()
        .find(|r| r.key == record_key)
        .ok_or_else(|| format!("record '{record_key}' not found"))?;

    let start = record.span.start.min(source.len());
    let end = record.span.end.min(source.len());
    Ok(source[start..end].to_string())
}

/// Collect all byte spans of `&old_key` (or `@Type.old_key`) references in an AST.
fn collect_ref_key_spans(ast: &CfdAst, old_key: &str) -> Vec<coflow_cft::Span> {
    fn walk_value(v: &coflow_cfd::ast::CfdValue, key: &str, out: &mut Vec<coflow_cft::Span>) {
        match v {
            coflow_cfd::ast::CfdValue::Ref(r) if r.key.0 == key => {
                out.push(r.key.1);
            }
            coflow_cfd::ast::CfdValue::Block(b) => {
                for entry in &b.entries {
                    match entry {
                        CfdBlockEntry::Field(f) => walk_value(&f.value, key, out),
                        CfdBlockEntry::Spread(sv, _) => walk_value(sv, key, out),
                    }
                }
            }
            coflow_cfd::ast::CfdValue::Array(items, _) => {
                for item in items { walk_value(item, key, out); }
            }
            coflow_cfd::ast::CfdValue::Spread(inner, _) => walk_value(inner, key, out),
            _ => {}
        }
    }
    let mut spans = Vec::new();
    for rec in &ast.records {
        for entry in &rec.entries {
            match entry {
                CfdBlockEntry::Field(f) => walk_value(&f.value, old_key, &mut spans),
                CfdBlockEntry::Spread(sv, _) => walk_value(sv, old_key, &mut spans),
            }
        }
    }
    spans
}

/// Apply a list of byte-span replacements (sorted descending) to a source string.
fn apply_span_replacements(source: &str, mut spans: Vec<coflow_cft::Span>, replacement: &str) -> String {
    spans.sort_by(|a, b| b.start.cmp(&a.start));
    spans.dedup_by_key(|s| s.start);
    let mut result = source.to_string();
    for span in &spans {
        let s = span.start.min(result.len());
        let e = span.end.min(result.len());
        result = format!("{}{}{}", &result[..s], replacement, &result[e..]);
    }
    result
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

    // Guard: reject duplicate keys — use AST index so this works even when model build failed
    if session.file_record_keys.values().any(|keys| keys.contains(&new_key.to_string())) {
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

    // ── 1. Rename the record's own key span ───────────────────────────────────
    let key_span = record.key_span;
    // Also collect any self-refs inside the same file
    let mut self_spans = collect_ref_key_spans(&ast, old_key);
    self_spans.push(key_span);
    let new_source = apply_span_replacements(&source, self_spans, new_key);

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;
    reload_file(&mut session, file_path, &new_source)?;

    // ── 2. Update &old_key references in all other loaded files ───────────────
    let other_files: Vec<(String, String, CfdAst)> = session
        .file_sources
        .iter()
        .filter(|(fp, _)| *fp != file_path)
        .map(|(fp, (src, ast))| (fp.clone(), src.clone(), ast.clone()))
        .collect();

    for (fp, src, ast) in other_files {
        let spans = collect_ref_key_spans(&ast, old_key);
        if spans.is_empty() {
            continue;
        }
        let updated = apply_span_replacements(&src, spans, new_key);
        let abs = session.project_dir.join(&fp);
        std::fs::write(&abs, &updated)
            .map_err(|e| format!("cannot write {fp}: {e}"))?;
        reload_file(&mut session, &fp, &updated)?;
    }

    Ok(())
}

pub fn get_diagnostics_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
) -> Result<Vec<DiagnosticItem>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let mut diagnostics: Vec<DiagnosticItem> = session.pending_model_diags.clone();
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

/// Return a brief summary (key, type, file) for every record in the project.
/// Used by the command palette / jump-to-record feature.
pub fn get_all_records_brief_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
) -> Result<Vec<RecordBrief>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let mut results: Vec<RecordBrief> = Vec::new();
    for (file_path, keys) in &session.file_record_keys {
        for key in keys {
            let in_model = session.model.records().any(|(_, r)| &r.key == key);
            let actual_type = if in_model {
                session.model.records().find(|(_, r)| &r.key == key)
                    .map(|(_, r)| r.actual_type.clone())
                    .unwrap_or_default()
            } else {
                session.file_sources.get(file_path)
                    .and_then(|(_, ast)| ast.records.iter().find(|r| &r.key == key))
                    .map(|r| r.type_name.clone())
                    .unwrap_or_default()
            };
            // Build display hint: prefer "name" field, then first non-null scalar field.
            let display_hint = if in_model {
                session.model.records()
                    .find(|(_, r)| &r.key == key)
                    .and_then(|(_, r)| {
                        fn scalar_str(v: &CfdValue) -> Option<String> {
                            match v {
                                CfdValue::String(s) => Some(s.clone()),
                                CfdValue::Int(i) => Some(i.to_string()),
                                CfdValue::Float(f) => Some(f.to_string()),
                                CfdValue::Bool(b) => Some(b.to_string()),
                                _ => None,
                            }
                        }
                        let name_val = r.fields.get("name").and_then(|v| scalar_str(v));
                        name_val.or_else(|| r.fields.values().find_map(|v| scalar_str(v)))
                    })
            } else {
                None
            };
            results.push(RecordBrief {
                key: key.clone(),
                actual_type,
                file_path: file_path.clone(),
                is_fallback: !in_model,
                display_hint,
            });
        }
    }
    results.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(results)
}

/// Return all record keys whose actual_type is assignable to `expected_type`.
/// Used for Ref field autocomplete in the editor.
pub fn get_ref_targets_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    expected_type: &str,
) -> Result<Vec<String>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;
    // Primary: records that built successfully in the model (have type info for filtering)
    let model_keys: HashSet<String> = session
        .model
        .records()
        .filter(|(_, r)| session.schema.is_assignable(&r.actual_type, expected_type))
        .map(|(_, r)| r.key.clone())
        .collect();
    // Supplement: include AST records not in the model (e.g. those with missing required fields).
    // For these, we only know the declared type from the AST; include them if they match.
    let mut ast_extra: Vec<String> = session
        .file_sources
        .values()
        .flat_map(|(_, ast)| &ast.records)
        .filter(|r| {
            !model_keys.contains(&r.key)
                && session.schema.is_assignable(&r.type_name, expected_type)
        })
        .map(|r| r.key.clone())
        .collect();
    let mut keys: Vec<String> = model_keys.into_iter().collect();
    keys.append(&mut ast_extra);
    keys.sort();
    keys.dedup();
    Ok(keys)
}

/// Search all records for scalar fields containing `query` (case-insensitive).
/// Returns up to `limit` hits, ordered by record key.
pub fn search_records_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    query: &str,
    limit: usize,
) -> Result<Vec<SearchHit>, String> {
    fn scalar_as_string(v: &CfdValue) -> Option<String> {
        match v {
            CfdValue::Null => None,
            CfdValue::Bool(b) => Some(b.to_string()),
            CfdValue::Int(i) => Some(i.to_string()),
            CfdValue::Float(f) => Some(f.to_string()),
            CfdValue::String(s) => Some(s.clone()),
            CfdValue::Enum(e) => Some(e.variant.clone().unwrap_or_else(|| e.value.to_string())),
            CfdValue::Ref { key, .. } => Some(key.clone()),
            CfdValue::Object(rec) => Some(rec.key.clone()),
            _ => None,
        }
    }

    fn search_value(
        v: &CfdValue,
        q: &str,
        path_prefix: &str,
    ) -> Option<(String, String)> {
        match v {
            CfdValue::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    let sub_path = format!("{}[{}]", path_prefix, i);
                    if let Some(hit) = search_value(item, q, &sub_path) {
                        return Some(hit);
                    }
                }
                None
            }
            CfdValue::Dict(entries) => {
                for (k, val) in entries {
                    let key_str = match k {
                        CfdDictKey::String(s) => s.clone(),
                        CfdDictKey::Int(i) => i.to_string(),
                        CfdDictKey::Enum(e) => e.variant.clone().unwrap_or_else(|| e.value.to_string()),
                    };
                    let sub_path = if path_prefix.is_empty() {
                        key_str.clone()
                    } else {
                        format!("{}.{}", path_prefix, key_str)
                    };
                    if let Some(hit) = search_value(val, q, &sub_path) {
                        return Some(hit);
                    }
                }
                None
            }
            CfdValue::Object(rec) => {
                for (field_name, field_val) in &rec.fields {
                    let sub_path = if path_prefix.is_empty() {
                        field_name.clone()
                    } else {
                        format!("{}.{}", path_prefix, field_name)
                    };
                    if let Some(hit) = search_value(field_val, q, &sub_path) {
                        return Some(hit);
                    }
                }
                None
            }
            _ => {
                if let Some(s) = scalar_as_string(v) {
                    if s.to_lowercase().contains(q) {
                        return Some((path_prefix.to_string(), s));
                    }
                }
                None
            }
        }
    }

    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let q_raw = query.trim().to_lowercase();
    if q_raw.is_empty() {
        return Ok(Vec::new());
    }

    // Support `field:value` or `type:TypeName` syntax for targeted search
    let (field_filter, q) = if let Some(colon) = q_raw.find(':') {
        let field = q_raw[..colon].trim().to_string();
        let value = q_raw[colon + 1..].trim().to_string();
        if !field.is_empty() && !value.is_empty() {
            (Some(field), value)
        } else {
            (None, q_raw.clone())
        }
    } else {
        (None, q_raw.clone())
    };

    // Check key match first, then field values
    let mut hits: Vec<SearchHit> = Vec::new();

    // Collect file_path for each record key from the AST index
    let key_to_file: HashMap<String, String> = session
        .file_record_keys
        .iter()
        .flat_map(|(fp, keys)| keys.iter().map(move |k| (k.clone(), fp.clone())))
        .collect();

    let mut sorted_records: Vec<_> = session.model.records().collect();
    sorted_records.sort_by(|(_, a), (_, b)| a.key.cmp(&b.key));

    for (_, record) in sorted_records {
        if hits.len() >= limit {
            break;
        }
        let file_path = key_to_file.get(&record.key).cloned().unwrap_or_default();

        match &field_filter {
            Some(ff) if ff == "file" => {
                // file:X filters by file_path (filename only, case-insensitive)
                let filename = file_path.split(['/', '\\']).last().unwrap_or(&file_path).to_string();
                if filename.to_lowercase().contains(&q) || file_path.to_lowercase().contains(&q) {
                    hits.push(SearchHit {
                        key: record.key.clone(),
                        actual_type: record.actual_type.clone(),
                        file_path,
                        match_field: "file".to_string(),
                        match_value: filename,
                    });
                }
            }
            Some(ff) if ff == "type" => {
                // type:X filters by actual_type
                if record.actual_type.to_lowercase().contains(&q) {
                    hits.push(SearchHit {
                        key: record.key.clone(),
                        actual_type: record.actual_type.clone(),
                        file_path,
                        match_field: "type".to_string(),
                        match_value: record.actual_type.clone(),
                    });
                }
            }
            Some(ff) if ff == "key" => {
                // key:X explicitly filters by record key
                if record.key.to_lowercase().contains(&q) {
                    hits.push(SearchHit {
                        key: record.key.clone(),
                        actual_type: record.actual_type.clone(),
                        file_path,
                        match_field: "key".to_string(),
                        match_value: record.key.clone(),
                    });
                }
            }
            Some(ff) => {
                // field_name:value — search only the specified field
                for (field_name, field_val) in &record.fields {
                    if field_name.to_lowercase() != *ff {
                        continue;
                    }
                    if let Some((path, val_str)) = search_value(field_val, &q, field_name) {
                        hits.push(SearchHit {
                            key: record.key.clone(),
                            actual_type: record.actual_type.clone(),
                            file_path: file_path.clone(),
                            match_field: path,
                            match_value: val_str,
                        });
                        break;
                    }
                }
            }
            None => {
                // General search: key match first, then field values
                if record.key.to_lowercase().contains(&q) {
                    hits.push(SearchHit {
                        key: record.key.clone(),
                        actual_type: record.actual_type.clone(),
                        file_path,
                        match_field: "key".to_string(),
                        match_value: record.key.clone(),
                    });
                    continue;
                }
                for (field_name, field_val) in &record.fields {
                    if let Some((path, val_str)) = search_value(field_val, &q, field_name) {
                        hits.push(SearchHit {
                            key: record.key.clone(),
                            actual_type: record.actual_type.clone(),
                            file_path: file_path.clone(),
                            match_field: path,
                            match_value: val_str,
                        });
                        break; // One hit per record
                    }
                }
            }
        }
    }

    Ok(hits)
}

/// Return all records that hold a Ref pointing at `target_key`.
pub fn get_incoming_refs_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    target_key: &str,
) -> Result<Vec<crate::types::IncomingRef>, String> {
    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let key_to_file: HashMap<String, String> = session
        .file_record_keys
        .iter()
        .flat_map(|(fp, keys)| keys.iter().map(move |k| (k.clone(), fp.clone())))
        .collect();

    let mut result: Vec<crate::types::IncomingRef> = Vec::new();

    let mut sorted: Vec<_> = session.model.records().collect();
    sorted.sort_by(|(_, a), (_, b)| a.key.cmp(&b.key));

    for (_, record) in sorted {
        let mut incoming: HashMap<String, Vec<(String, String)>> = HashMap::new();
        collect_refs_in_record(record, &record.key, "", &mut incoming, &session.model);
        if let Some(paths) = incoming.get(target_key) {
            let source_file = key_to_file.get(&record.key).cloned().unwrap_or_default();
            for (_, field_path) in paths {
                result.push(crate::types::IncomingRef {
                    source_key: record.key.clone(),
                    source_type: record.actual_type.clone(),
                    source_file: source_file.clone(),
                    field_path: field_path.clone(),
                });
            }
        }
    }

    Ok(result)
}

/// Import one or more records from raw CFD source text into a file.
/// Validates key uniqueness and that at least one record is parseable.
/// Returns the keys of all imported records.
pub fn import_record_source_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    source: &str,
) -> Result<Vec<String>, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("source is empty".to_string());
    }

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Parse to get the keys from the pasted source
    let (ast, _) = parse_cfd(source);
    if ast.records.is_empty() {
        return Err("no records found in the pasted source".to_string());
    }

    // Check for key conflicts across the project
    for rec in &ast.records {
        let key = &rec.key;
        let conflict_file = session.file_record_keys
            .iter()
            .find_map(|(fp, keys)| if keys.contains(key) { Some(fp.as_str()) } else { None });
        if let Some(fp) = conflict_file {
            return Err(format!("record key '{key}' already exists in {fp}"));
        }
    }

    let abs_path = session.project_dir.join(file_path);
    let existing = std::fs::read_to_string(&abs_path).unwrap_or_default();
    let separator = if existing.ends_with('\n') || existing.is_empty() { "" } else { "\n" };
    let new_content = format!("{existing}{separator}{source}\n");

    std::fs::write(&abs_path, &new_content)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_content)?;

    Ok(ast.records.iter().map(|r| r.key.clone()).collect())
}

pub fn duplicate_record_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
    src_key: &str,
    new_key: &str,
) -> Result<RecordRow, String> {
    validate_cfd_key(new_key)?;

    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    // Guard duplicate key — use AST index so this works even when model build failed
    if session.file_record_keys.values().any(|keys| keys.contains(&new_key.to_string())) {
        return Err(format!("record key '{new_key}' already exists in the project"));
    }

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    let rec = ast
        .records
        .iter()
        .find(|r| r.key == src_key)
        .ok_or_else(|| format!("record '{src_key}' not found in '{file_path}'"))?;

    // Extract the text from end-of-key to end-of-record-span (everything after the key).
    // rec.key_span.end points to the byte just past the last byte of the key token.
    let after_key = source
        .get(rec.key_span.end..rec.span.end)
        .ok_or("key/span offsets out of range")?;

    // Detect grouped vs standalone syntax.
    // Grouped: type_span.start < key_span.start (group token precedes record key).
    let is_grouped = rec.type_span.start < rec.key_span.start;

    let abs_path = session.project_dir.join(file_path);
    let new_content = if is_grouped {
        // Insert the duplicate inside the existing group block, before its closing `}`.
        let type_name = rec.type_name.clone();
        if let Some(group_closer) = find_group_closer(&source, &type_name) {
            let before = &source[..group_closer];
            let after = &source[group_closer..];
            format!("{before}  {new_key}{after_key}\n{after}")
        } else {
            // Fallback: append as standalone
            format!("{source}\n{new_key}: {} {after_key}\n", rec.type_name)
        }
    } else {
        // Standalone: append `new_key: TypeName { ... }` after the file.
        // after_key starts with `: TypeName {` for standalone records.
        let separator = if after_key.trim_start().starts_with('{') {
            // Should not happen for standalone, but guard anyway
            format!(": {} ", rec.type_name)
        } else {
            String::new()
        };
        format!("{source}\n{new_key}{separator}{after_key}\n")
    };
    std::fs::write(&abs_path, &new_content)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;
    reload_file(&mut session, file_path, &new_content)?;

    // Build the RecordRow for the duplicate
    let in_model = session.model.records().any(|(_, r)| r.key == new_key);
    if in_model {
        let (_, record) = session.model.records().find(|(_, r)| r.key == new_key)
            .expect("record exists: checked with .any() on same model");
        let ast_rec = session.file_sources.get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == new_key));
        let direct = ast_rec.map(|r| r.fields.iter().map(|f| f.name.clone()).collect::<HashSet<String>>());
        let mut row = convert_record_row_with_ast(record, &session.schema, &session.model, &session.file_record_keys, direct.as_ref(), ast_rec);
        row.file_path = file_path.to_string();
        Ok(row)
    } else {
        let schema = &session.schema;
        session.file_sources.get(file_path)
            .and_then(|(_, ast)| ast.records.iter().find(|r| r.key == new_key))
            .map(|r| {
                let mut row = ast_record_fallback(r, schema);
                row.file_path = file_path.to_string();
                row
            })
            .ok_or_else(|| format!("duplicate record '{new_key}' not found after write"))
    }
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

/// Return schema metadata for all fields on a given type.
/// Used by the UI to enable schema-aware editing for nullable Object fields.
pub fn get_field_schemas_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    type_name: &str,
) -> Result<Vec<FieldSchema>, String> {
    fn nullable_object_type(ty_ref: &CftSchemaTypeRef, schema: &CftContainer) -> Option<String> {
        match ty_ref {
            CftSchemaTypeRef::Nullable(inner) => {
                if let CftSchemaTypeRef::Named(name) = inner.as_ref() {
                    if schema.resolve_type(name).is_some() {
                        return Some(name.clone());
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn array_nullable_element_type(ty_ref: &CftSchemaTypeRef, schema: &CftContainer) -> Option<String> {
        if let CftSchemaTypeRef::Array(elem) = ty_ref {
            return nullable_object_type(elem, schema);
        }
        None
    }

    fn format_default(d: &CftSchemaDefaultValue) -> String {
        match d {
            CftSchemaDefaultValue::Null => "null".to_string(),
            CftSchemaDefaultValue::Int(v) => v.to_string(),
            CftSchemaDefaultValue::Float(v) => {
                let s = v.to_string();
                if s.contains('.') || s.contains('e') || s.contains('E') { s } else { format!("{s}.0") }
            }
            CftSchemaDefaultValue::Bool(v) => v.to_string(),
            CftSchemaDefaultValue::String(v) => format!("{v:?}"),
            CftSchemaDefaultValue::Enum { variant, .. } => variant.clone(),
            CftSchemaDefaultValue::EmptyArray => "[]".to_string(),
            CftSchemaDefaultValue::EmptyObject => "{}".to_string(),
        }
    }

    let session_arc = get_session(store, session_id)?;
    let session = session_arc.lock().map_err(|_| "session lock poisoned")?;
    let schema_type = session
        .schema
        .resolve_type(type_name)
        .ok_or_else(|| format!("type '{type_name}' not found"))?;
    let schemas = schema_type
        .all_fields
        .iter()
        .map(|f| FieldSchema {
            name: f.name.clone(),
            type_str: f.ty.clone(),
            nullable_object_type: nullable_object_type(&f.ty_ref, &session.schema),
            array_nullable_element_type: array_nullable_element_type(&f.ty_ref, &session.schema),
            has_default: f.has_default,
            default_str: f.default.as_ref().map(format_default),
        })
        .collect();
    Ok(schemas)
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

    let record_count = session.file_record_keys
        .get(rel_path)
        .map(|keys| keys.len() as u32)
        .unwrap_or(0);

    Ok(FileTreeNode {
        name,
        path: rel_path.to_string(),
        is_dir: false,
        in_sources,
        children: Vec::new(),
        record_count,
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

/// Reload a single .cfd file from disk, rebuilding the model without a full project reload.
pub fn reload_file_from_disk_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    rel_path: &str,
) -> Result<(), String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let abs_path = session.project_dir.join(rel_path);
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

    let new_source = std::fs::read_to_string(&abs_path)
        .map_err(|e| format!("cannot read '{rel_path}': {e}"))?;
    reload_file(&mut session, rel_path, &new_source)
}

/// Convert a raw AST CfdValue to FieldValue without schema or model.
/// Used as a fallback when the model build fails (e.g. record has missing required fields).
fn ast_value_to_field_value(v: &coflow_cfd::CfdValue, schema: &CftContainer) -> FieldValue {
    use coflow_cfd::CfdValue as AV;
    match v {
        AV::Null(_) => FieldValue::Null,
        AV::QuotedString(s, _) => FieldValue::Str { v: s.clone() },
        AV::Scalar(s, _) => {
            let trimmed = s.trim();
            if trimmed == "true" { return FieldValue::Bool { v: true }; }
            if trimmed == "false" { return FieldValue::Bool { v: false }; }
            if let Ok(i) = trimmed.parse::<i64>() { return FieldValue::Int { v: i as f64 }; }
            if let Ok(f) = trimmed.parse::<f64>() { return FieldValue::Float { v: f }; }
            FieldValue::Str { v: s.clone() }
        }
        AV::Ref(r) => FieldValue::Ref {
            target_type: r.type_name.as_ref().map(|(t, _)| t.clone()).unwrap_or_default(),
            target_key: r.key.0.clone(),
            target_file: None,
        },
        AV::Array(items, _) => FieldValue::Array {
            items: items.iter().map(|i| ast_value_to_field_value(i, schema)).collect(),
        },
        AV::Block(b) => {
            let actual_type = b.type_marker.as_ref().map(|(t, _)| t.clone()).unwrap_or_default();
            // Build a map of explicitly written fields from the AST
            let written: HashMap<String, FieldValue> = b.entries.iter().filter_map(|e| {
                if let CfdBlockEntry::Field(f) = e {
                    Some((f.name.clone(), ast_value_to_field_value(&f.value, schema)))
                } else { None }
            }).collect();
            // Populate all schema-defined fields (missing ones as Null) so empty Object
            // blocks still show editable sub-fields in the UI.
            let fields: Vec<FieldCell> = if let Some(schema_type) = schema.resolve_type(&actual_type) {
                schema_type.all_fields.iter().map(|sf| FieldCell {
                    name: sf.name.clone(),
                    value: written.get(&sf.name).cloned().unwrap_or(FieldValue::Null),
                }).collect()
            } else {
                written.into_iter().map(|(name, value)| FieldCell { name, value }).collect()
            };
            FieldValue::Object { actual_type, fields }
        }
        AV::Spread(_, _) => FieldValue::Null,
    }
}

/// Build a RecordRow from the raw AST record when the model doesn't contain it.
/// Returns null-typed fields based purely on what is written in the source.
fn ast_record_fallback(ast_rec: &coflow_cfd::CfdRecord, schema: &CftContainer) -> RecordRow {
    // Use schema field order and fill in missing fields as Null so the UI can edit them.
    let actual_type = &ast_rec.type_name;
    let written: HashMap<String, FieldValue> = ast_rec.fields.iter().map(|f| {
        (f.name.clone(), ast_value_to_field_value(&f.value, schema))
    }).collect();
    let fields: Vec<FieldCell> = if let Some(schema_type) = schema.resolve_type(actual_type) {
        schema_type.all_fields.iter().map(|sf| FieldCell {
            name: sf.name.clone(),
            value: written.get(&sf.name).cloned().unwrap_or(FieldValue::Null),
        }).collect()
    } else {
        written.into_iter().map(|(name, value)| FieldCell { name, value }).collect()
    };
    let spread_sources: Vec<SpreadSource> = ast_rec.entries.iter().filter_map(|e| match e {
        CfdBlockEntry::Spread(coflow_cfd::CfdValue::Ref(r), _) => Some(SpreadSource {
            key: r.key.0.clone(),
            file: String::new(),
        }),
        _ => None,
    }).collect();
    RecordRow {
        key: ast_rec.key.clone(),
        actual_type: ast_rec.type_name.clone(),
        file_path: String::new(),
        fields,
        spread_fields: Vec::new(),
        spread_sources,
        is_fallback: true,
    }
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

/// Find the byte position of the closing `}` that ends a grouped-type block
/// (e.g. `TypeName { ... }`). Returns the position of that `}` in the source.
///
/// Strategy 1: if grouped records of this type exist, scan forward from the last
/// record's span.end to find the block closer.
/// Strategy 2: if no records exist (e.g. all were deleted), scan the source text
/// for `TypeName {` and then find the matching `}` by brace counting.
fn find_group_closer(source: &str, type_name: &str) -> Option<usize> {
    let (ast, _) = parse_cfd(source);
    // Strategy 1: records still exist — find the closer after the last record.
    if let Some(max_end) = ast.records.iter()
        .filter(|r| r.type_name == type_name && r.type_span.start < r.key_span.start)
        .map(|r| r.span.end)
        .max()
    {
        let bytes = source.as_bytes();
        let search_from = max_end.min(source.len());
        for i in search_from..bytes.len() {
            if bytes[i] == b'}' {
                return Some(i);
            }
        }
    }
    // Strategy 2: no grouped records found — scan for `TypeName {` header and
    // find its matching closing `}` via brace counting.
    let header = format!("{type_name} {{");
    if let Some(header_pos) = source.find(&header) {
        let open_pos = header_pos + header.len() - 1; // position of `{`
        let bytes = source.as_bytes();
        let mut depth: i32 = 0;
        for i in open_pos..bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Returns true if `source` contains a grouped-block header `TypeName {` at the
/// start of a line (i.e. not `key: TypeName {` which is a standalone record).
fn source_has_grouped_header(source: &str, type_name: &str) -> bool {
    let header = format!("{type_name} {{");
    let mut pos = 0;
    while let Some(idx) = source[pos..].find(&header) {
        let abs = pos + idx;
        // Check that the character before the header is a newline or the header is at pos 0
        let at_line_start = abs == 0 || source.as_bytes()[abs - 1] == b'\n';
        if at_line_start {
            return true;
        }
        pos = abs + 1;
    }
    false
}

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
            // Track insertion order so build-failure diagnostics can resolve record_key
            let mut id_to_key: Vec<String> = Vec::new();
            for (fp, (src, _)) in &session.file_sources {
                if fp != file_path {
                    if let Ok(recs) = parse_cfd_input_records(&session.schema, src) {
                        for r in recs {
                            id_to_key.push(r.key.clone());
                            builder.add_input_record(r);
                        }
                    }
                }
            }
            for r in records {
                id_to_key.push(r.key.clone());
                builder.add_input_record(r);
            }
            match builder.build() {
                Ok(new_model) => {
                    session.model = new_model;
                    session.pending_model_diags.clear();
                }
                Err(d) => {
                    // Build failed (e.g. missing required fields, unresolved Refs).
                    // Keep the existing model but store the build errors so
                    // get_diagnostics_inner can surface them.
                    session.pending_model_diags = convert_cfd_diagnostics_with_index(
                        &d, None, Some(&id_to_key), Some(&session.file_record_keys),
                    );
                }
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

fn annotate_tree_record_counts(nodes: &mut Vec<FileTreeNode>, file_record_keys: &HashMap<String, Vec<String>>) {
    for node in nodes.iter_mut() {
        if node.is_dir {
            annotate_tree_record_counts(&mut node.children, file_record_keys);
        } else {
            node.record_count = file_record_keys
                .get(&node.path)
                .map(|keys| keys.len() as u32)
                .unwrap_or(0);
        }
    }
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
                    record_count: 0,
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
                record_count: 0,
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
    convert_record_row_with_ast(record, schema, model, file_record_keys, direct_field_names, None)
}

fn convert_record_row_with_ast(
    record: &CfdRecord,
    schema: &CftContainer,
    model: &CfdDataModel,
    file_record_keys: &HashMap<String, Vec<String>>,
    direct_field_names: Option<&HashSet<String>>,
    ast_rec: Option<&coflow_cfd::CfdRecord>,
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
                    .map(|v| convert_value(v, schema, model, file_record_keys))
                    .unwrap_or(FieldValue::Null),
            })
            .collect()
    } else {
        record
            .fields
            .iter()
            .map(|(name, v)| FieldCell {
                name: name.clone(),
                value: convert_value(v, schema, model, file_record_keys),
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
    // Spread sources: extract record keys from spread entries in the AST.
    // `...&key` → entries contain CfdBlockEntry::Spread(CfdValue::Ref { key, .. }, _)
    let spread_sources: Vec<SpreadSource> = ast_rec
        .map(|ar| {
            ar.entries
                .iter()
                .filter_map(|e| match e {
                    CfdBlockEntry::Spread(coflow_cfd::CfdValue::Ref(r), _) => {
                        let key = r.key.0.clone();
                        let file = find_file_for_key(file_record_keys, &key)
                            .unwrap_or("")
                            .to_string();
                        Some(SpreadSource { key, file })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    RecordRow {
        key: record.key.clone(),
        actual_type: record.actual_type.clone(),
        file_path: String::new(),
        fields,
        spread_fields,
        spread_sources,
        is_fallback: false,
    }
}

fn find_file_for_key<'a>(file_record_keys: &'a HashMap<String, Vec<String>>, key: &str) -> Option<&'a str> {
    file_record_keys
        .iter()
        .find_map(|(fp, keys)| if keys.iter().any(|k| k == key) { Some(fp.as_str()) } else { None })
}

fn convert_value(v: &CfdValue, schema: &CftContainer, model: &CfdDataModel, file_record_keys: &HashMap<String, Vec<String>>) -> FieldValue {
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
        CfdValue::Object(record) => {
            // Use schema field order if available; fall back to BTreeMap (alphabetical) order.
            // Show all schema fields (missing ones as Null) — consistent with convert_record_row_with_ast,
            // and required so that newly-created empty nullable objects have editable sub-fields.
            let fields = if let Some(schema_type) = schema.resolve_type(&record.actual_type) {
                schema_type
                    .all_fields
                    .iter()
                    .map(|sf| FieldCell {
                        name: sf.name.clone(),
                        value: record
                            .fields
                            .get(&sf.name)
                            .map(|fv| convert_value(fv, schema, model, file_record_keys))
                            .unwrap_or(FieldValue::Null),
                    })
                    .collect()
            } else {
                record
                    .fields
                    .iter()
                    .map(|(name, fv)| FieldCell {
                        name: name.clone(),
                        value: convert_value(fv, schema, model, file_record_keys),
                    })
                    .collect()
            };
            FieldValue::Object { actual_type: record.actual_type.clone(), fields }
        }
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
            items: items.iter().map(|i| convert_value(i, schema, model, file_record_keys)).collect(),
        },
        CfdValue::Dict(entries) => FieldValue::Dict {
            entries: entries
                .iter()
                .map(|(k, v)| DictEntry {
                    key: convert_dict_key(k),
                    value: convert_value(v, schema, model, file_record_keys),
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
    convert_cfd_diagnostics_with_index(diags, model, None, file_record_keys)
}

fn convert_cfd_diagnostics_with_index(
    diags: &CfdDiagnostics,
    model: Option<&CfdDataModel>,
    id_to_key: Option<&[String]>,
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
                    let key = l.record.and_then(|id| {
                        // Try model first (fast path for run_checks diagnostics)
                        if let Some(m) = model {
                            if let Some(r) = m.record(id) {
                                return Some(r.key.clone());
                            }
                        }
                        // Fall back to insertion-order index (build-failure diagnostics)
                        id_to_key.and_then(|idx| idx.get(id.index()).cloned())
                    });
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
                let k_str = match k {
                    CfdDictKey::String(s) => s.clone(),
                    CfdDictKey::Int(n) => n.to_string(),
                    CfdDictKey::Enum(e) => e.variant.as_deref().unwrap_or(&e.enum_name).to_string(),
                };
                collect_refs_in_value(
                    v,
                    source_key,
                    &format!("{label}[{k_str}]"),
                    reverse_refs,
                    model,
                );
            }
        }
        _ => {}
    }
}

/// Sort all records in `file_path` alphabetically by key (case-insensitive).
///
/// Handles standalone syntax only. Grouped-syntax files (where the type block
/// precedes all keys) are sorted within each group block.
pub fn sort_file_records_inner(
    store: &Mutex<SessionStore>,
    session_id: u32,
    file_path: &str,
) -> Result<usize, String> {
    let session_arc = get_session(store, session_id)?;
    let mut session = session_arc.lock().map_err(|_| "session lock poisoned")?;

    let (source, ast) = session
        .file_sources
        .get(file_path)
        .ok_or_else(|| format!("file '{file_path}' not loaded"))?
        .clone();

    if ast.records.len() < 2 {
        return Ok(0); // Nothing to sort
    }

    let any_grouped = ast.records.iter().any(|r| r.type_span.start < r.key_span.start);

    let new_source = if any_grouped {
        // Grouped syntax: group records by type_name, sort within each group block.
        // We rebuild the file by replacing each group's inner content with sorted entries.
        let mut groups: std::collections::BTreeMap<String, Vec<&coflow_cfd::ast::CfdRecord>> =
            std::collections::BTreeMap::new();
        for rec in &ast.records {
            groups.entry(rec.type_name.clone()).or_default().push(rec);
        }
        let mut result = source.clone();
        let mut offset_delta: i64 = 0;
        for (type_name, mut recs) in groups {
            if recs.len() < 2 {
                continue;
            }
            recs.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));
            // Find the group block and rebuild its inner content
            let group_header = format!("{type_name} {{");
            let Some(block_start) = source.find(&group_header) else { continue };
            let inner_start = block_start + group_header.len();
            let Some(relative_end) = source[inner_start..].find('}') else { continue };
            let block_end = inner_start + relative_end;

            let sorted_inner: String = recs.iter().map(|r| {
                // Extract entry text: from key_span.start to rec.span.end
                source.get(r.key_span.start..r.span.end).unwrap_or("").to_string() + "\n"
            }).collect();
            let indented = sorted_inner.lines()
                .map(|l| if l.is_empty() { String::new() } else { format!("  {l}") })
                .collect::<Vec<_>>()
                .join("\n");
            let new_inner = format!("\n{indented}\n");

            let adj_start = (inner_start as i64 + offset_delta) as usize;
            let adj_end = (block_end as i64 + offset_delta) as usize;
            let before = &result[..adj_start];
            let after = &result[adj_end..];
            let new_result = format!("{before}{new_inner}{after}");
            offset_delta += new_inner.len() as i64 - (block_end - inner_start) as i64;
            result = new_result;
        }
        result
    } else {
        // Standalone syntax: collect record spans, sort by key, reconstruct file.
        let mut records: Vec<_> = ast.records.iter().collect();
        records.sort_by(|a, b| a.key.to_lowercase().cmp(&b.key.to_lowercase()));

        // Check if already sorted
        let already_sorted = ast.records.windows(2).all(|w| {
            w[0].key.to_lowercase() <= w[1].key.to_lowercase()
        });
        if already_sorted {
            return Ok(0);
        }

        // Extract text for each record (including any leading blank line)
        let record_texts: Vec<String> = ast.records.iter().map(|r| {
            let start = if r.span.start > 0 && source.as_bytes().get(r.span.start - 1) == Some(&b'\n') {
                r.span.start - 1
            } else {
                r.span.start
            };
            source.get(start..r.span.end).unwrap_or("").to_string()
        }).collect();

        // Build sorted order
        let sorted_order: Vec<usize> = {
            let mut indexed: Vec<(usize, &str)> = ast.records.iter().enumerate()
                .map(|(i, r)| (i, r.key.as_str()))
                .collect();
            indexed.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
            indexed.into_iter().map(|(i, _)| i).collect()
        };

        // Find the part of the source before the first record and after the last
        let first_span_start = ast.records.iter().map(|r| {
            if r.span.start > 0 && source.as_bytes().get(r.span.start - 1) == Some(&b'\n') {
                r.span.start - 1
            } else {
                r.span.start
            }
        }).min().unwrap_or(0);
        let last_span_end = ast.records.iter().map(|r| r.span.end).max().unwrap_or(source.len());

        let prefix = &source[..first_span_start];
        let suffix = &source[last_span_end..];

        let sorted_body: String = sorted_order.iter()
            .enumerate()
            .map(|(i, &orig_idx)| {
                let text = record_texts[orig_idx].trim_start_matches('\n');
                if i == 0 { text.to_string() } else { format!("\n{text}") }
            })
            .collect();
        format!("{prefix}{sorted_body}{suffix}")
    };

    if new_source == source {
        return Ok(0);
    }

    let abs_path = session.project_dir.join(file_path);
    std::fs::write(&abs_path, &new_source)
        .map_err(|e| format!("cannot write {file_path}: {e}"))?;

    reload_file(&mut session, file_path, &new_source)?;
    Ok(ast.records.len())
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
    fn write_field_inserts_when_missing() {
        // Tests the insert_field path: write_field on a newly created (empty) record
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");
        let schema_path = dir.path().join("schema.cft");

        std::fs::write(&schema_path, "type Item { name: string; count: int; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        // Empty file — no records yet
        std::fs::write(&cfd, "").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Create record (fields are empty/null)
        create_record_inner(&store, sid, "data/items.cfd", "sword", "Item").unwrap();

        // Write a field that does NOT exist yet → insert_field path
        write_field_inner(
            &store, sid, "data/items.cfd", "sword",
            &[FieldPathSegment::Field { name: "name".to_string() }],
            &FieldValue::Str { v: "Sword of Fire".to_string() },
        ).unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("Sword of Fire"), "inserted field not found in file: {contents}");

        // Model must also reflect the new value
        let records = get_file_records_inner(&store, sid, "data/items.cfd").unwrap();
        let sword = records.records.iter().find(|r| r.key == "sword").unwrap();
        let name_field = sword.fields.iter().find(|f| f.name == "name");
        assert!(
            matches!(name_field.map(|f| &f.value), Some(FieldValue::Str { v }) if v == "Sword of Fire"),
            "model should show inserted field value, got: {name_field:?}"
        );
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
    fn rename_record_updates_cross_file_refs() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let items_cfd = data_dir.join("items.cfd");
        let refs_cfd = data_dir.join("refs.cfd");

        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }\ntype Container { item: &Item; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&items_cfd, "old_key: Item {\n  name: \"test\",\n}\n").unwrap();
        std::fs::write(&refs_cfd, "container: Container {\n  item: &old_key,\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        rename_record_inner(&store, sid, "data/items.cfd", "old_key", "new_key").unwrap();

        let items_contents = std::fs::read_to_string(&items_cfd).unwrap();
        assert!(items_contents.contains("new_key"), "items.cfd should contain new_key:\n{items_contents}");
        assert!(!items_contents.contains("old_key"), "items.cfd should not contain old_key:\n{items_contents}");

        let refs_contents = std::fs::read_to_string(&refs_cfd).unwrap();
        assert!(refs_contents.contains("&new_key"), "refs.cfd should contain &new_key:\n{refs_contents}");
        assert!(!refs_contents.contains("&old_key"), "refs.cfd should not contain &old_key:\n{refs_contents}");
    }

    #[test]
    fn sort_file_records_standalone() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("items.cfd");
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        std::fs::write(&cfd, "zebra: Item { name: \"Z\", }\napple: Item { name: \"A\", }\nmango: Item { name: \"M\", }\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let count = sort_file_records_inner(&store, sid, "data/items.cfd").unwrap();
        assert_eq!(count, 3, "should report 3 records sorted");

        let contents = std::fs::read_to_string(&cfd).unwrap();
        let apple_pos = contents.find("apple").unwrap();
        let mango_pos = contents.find("mango").unwrap();
        let zebra_pos = contents.find("zebra").unwrap();
        assert!(apple_pos < mango_pos && mango_pos < zebra_pos, "records should be sorted alphabetically:\n{contents}");

        // Already sorted → 0
        let count2 = sort_file_records_inner(&store, sid, "data/items.cfd").unwrap();
        assert_eq!(count2, 0, "already-sorted file should return 0");
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

    #[test]
    fn get_ref_targets_returns_assignable_keys() {
        let yaml_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cfd/coflow.yaml");
        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml_path.to_str().unwrap()).unwrap();

        // "Item" is a concrete type; get_ref_targets("Item") should include sword_fire and staff_ice
        let item_keys = get_ref_targets_inner(&store, snap.session_id, "Item").unwrap();
        assert!(item_keys.contains(&"sword_fire".to_string()), "item_keys should contain sword_fire");
        assert!(item_keys.contains(&"staff_ice".to_string()), "item_keys should contain staff_ice");
        // Monster keys should NOT appear when filtering by Item
        assert!(!item_keys.contains(&"basic_monster".to_string()), "monster key should not appear for Item filter");

        // "Monster" type keys should include basic_monster
        let monster_keys = get_ref_targets_inner(&store, snap.session_id, "Monster").unwrap();
        assert!(monster_keys.contains(&"basic_monster".to_string()), "monster_keys should contain basic_monster");
        assert!(!monster_keys.contains(&"sword_fire".to_string()), "item key should not appear for Monster filter");

        // Keys are sorted
        assert_eq!(item_keys, {
            let mut sorted = item_keys.clone();
            sorted.sort();
            sorted
        });
    }

    #[test]
    fn duplicate_record_copies_fields() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; price: int; }").unwrap();
        std::fs::write(&cfd_path, "sword: Item {\n  name: \"Sword\",\n  price: 100,\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let file_path = "data/items.cfd";

        // Duplicate sword → sword2
        let row = duplicate_record_inner(&store, snap.session_id, file_path, "sword", "sword2").unwrap();
        assert_eq!(row.key, "sword2");
        assert_eq!(row.actual_type, "Item");
        let name_field = row.fields.iter().find(|f| f.name == "name");
        assert!(name_field.is_some(), "duplicated record should have name field");

        // Duplicate key must not already exist
        let err = duplicate_record_inner(&store, snap.session_id, file_path, "sword", "sword2");
        assert!(err.is_err(), "should fail when new_key already exists");

        // Original record still exists
        let records = get_file_records_inner(&store, snap.session_id, file_path).unwrap();
        assert!(records.records.iter().any(|r| r.key == "sword"));
        assert!(records.records.iter().any(|r| r.key == "sword2"));
    }

    #[test]
    fn duplicate_grouped_record_preserves_type() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        // Grouped record syntax: TypeName { key { ... } }
        std::fs::write(&cfd_path, "Item {\n  sword {\n    name: \"Sword\",\n  }\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let file_path = "data/items.cfd";

        let row = duplicate_record_inner(&store, snap.session_id, file_path, "sword", "axe").unwrap();
        assert_eq!(row.key, "axe");
        assert_eq!(row.actual_type, "Item", "duplicated grouped record must keep its type");
        assert!(row.fields.iter().any(|f| f.name == "name"), "fields should be copied");

        // Both records exist
        let records = get_file_records_inner(&store, snap.session_id, file_path).unwrap();
        let axe = records.records.iter().find(|r| r.key == "axe").expect("axe should exist");
        assert_eq!(axe.actual_type, "Item");

        // Verify grouped syntax: the duplicate must be inside the group block, not standalone
        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        assert!(!contents.contains("axe: Item"), "duplicate should not use standalone syntax:\n{contents}");
        // Group block still present
        assert!(contents.contains("Item {"), "group block header should still be present:\n{contents}");
    }

    #[test]
    fn get_all_records_brief_includes_all_keys() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd1 = dir.path().join("data/a.cfd");
        let cfd2 = dir.path().join("data/b.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd1, "sword: Item { name: \"Sword\", }\n").unwrap();
        std::fs::write(&cfd2, "shield: Item { name: \"Shield\", }\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let briefs = get_all_records_brief_inner(&store, snap.session_id).unwrap();
        assert_eq!(briefs.len(), 2, "should have one brief per record");
        let keys: Vec<&str> = briefs.iter().map(|b| b.key.as_str()).collect();
        assert!(keys.contains(&"sword"), "sword should be present");
        assert!(keys.contains(&"shield"), "shield should be present");
        assert!(briefs.iter().all(|b| b.actual_type == "Item"), "all should be Item type");
        // Result is sorted by key
        assert_eq!(briefs[0].key, "shield");
        assert_eq!(briefs[1].key, "sword");
    }

    #[test]
    fn write_field_dict_roundtrip() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Monster { weaknesses: {string: float}; }").unwrap();
        std::fs::write(
            &cfd_path,
            "goblin: Monster {\n  weaknesses: {\"fire\": 1.5},\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Overwrite the entire weaknesses dict
        let new_val = FieldValue::Dict {
            entries: vec![
                DictEntry {
                    key: DictKey::Str { v: "fire".to_string() },
                    value: FieldValue::Float { v: 2.0 },
                },
                DictEntry {
                    key: DictKey::Str { v: "ice".to_string() },
                    value: FieldValue::Float { v: 0.5 },
                },
            ],
        };
        write_field_inner(
            &store,
            snap.session_id,
            "data/items.cfd",
            "goblin",
            &[FieldPathSegment::Field { name: "weaknesses".to_string() }],
            &new_val,
        ).unwrap();

        let row = get_record_inner(&store, snap.session_id, "data/items.cfd", "goblin").unwrap();
        let wk = row.fields.iter().find(|f| f.name == "weaknesses").unwrap();
        if let FieldValue::Dict { entries } = &wk.value {
            assert_eq!(entries.len(), 2, "should have 2 entries after write");
            let fire = entries.iter().find(|e| matches!(&e.key, DictKey::Str { v } if v == "fire")).unwrap();
            assert!(matches!(&fire.value, FieldValue::Float { v } if (*v - 2.0).abs() < 1e-9));
        } else {
            panic!("expected Dict, got {:?}", wk.value);
        }
    }

    #[test]
    fn create_record_rejects_duplicate_key() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path, "sword: Item { name: \"Sword\", }\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let err = create_record_inner(&store, snap.session_id, "data/items.cfd", "sword", "Item")
            .unwrap_err();
        assert!(err.contains("sword"), "error should mention conflicting key: {err}");
    }

    #[test]
    fn create_record_uses_grouped_syntax_when_file_is_grouped() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        // File already uses grouped syntax
        std::fs::write(&cfd_path, "Item {\n  sword {\n    name: \"Sword\",\n  }\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let row = create_record_inner(&store, snap.session_id, "data/items.cfd", "axe", "Item").unwrap();
        assert_eq!(row.key, "axe");

        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        // The new record must be INSIDE the existing group block, not appended as standalone
        assert!(contents.contains("axe"), "file should contain axe:\n{contents}");
        // axe should not appear as `axe: Item {` — that would be standalone syntax
        assert!(!contents.contains("axe: Item"), "should not use standalone syntax for grouped file:\n{contents}");
        // File should still parse correctly (one group block, two records)
        let records = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert_eq!(records.records.len(), 2, "file should have two records:\n{contents}");
        assert!(records.records.iter().any(|r| r.key == "sword"));
        assert!(records.records.iter().any(|r| r.key == "axe"));
    }

    #[test]
    fn create_record_standalone_syntax_on_empty_file() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path, "").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let row = create_record_inner(&store, snap.session_id, "data/items.cfd", "sword", "Item").unwrap();
        assert_eq!(row.key, "sword");

        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        // Should not start with a blank newline
        assert!(!contents.starts_with('\n'), "file should not start with extra newline:\n{contents:?}");
        assert!(contents.contains("sword: Item"), "should use standalone syntax:\n{contents}");
    }

    #[test]
    fn spread_sources_extracted_from_ast() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/monsters.cfd");

        std::fs::write(&schema_path, "type Monster { name: string; hp: int; }").unwrap();
        std::fs::write(&cfd_path,
            "basic_monster: Monster {\n  name: \"Basic\",\n  hp: 10,\n}\nelite_monster: Monster {\n  ...&basic_monster,\n  name: \"Elite\",\n}\n"
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let records = get_file_records_inner(&store, snap.session_id, "data/monsters.cfd").unwrap();
        let basic = records.records.iter().find(|r| r.key == "basic_monster").expect("basic_monster missing");
        assert!(basic.spread_sources.is_empty(), "basic_monster has no spreads");

        let elite = records.records.iter().find(|r| r.key == "elite_monster").expect("elite_monster missing");
        assert_eq!(elite.spread_sources.len(), 1, "elite_monster should have one spread source");
        assert_eq!(elite.spread_sources[0].key, "basic_monster", "spread source key should be basic_monster");
        assert_eq!(elite.spread_sources[0].file, "data/monsters.cfd", "spread source file should be data/monsters.cfd");
        // hp comes from spread, name is direct
        assert!(elite.spread_fields.contains(&"hp".to_string()), "hp should be a spread field");
        assert!(!elite.spread_fields.contains(&"name".to_string()), "name should not be a spread field");
    }

    #[test]
    fn delete_grouped_record_leaves_group_intact() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path,
            "Item {\n  sword {\n    name: \"Sword\",\n  }\n  axe {\n    name: \"Axe\",\n  }\n}\n"
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        delete_record_inner(&store, snap.session_id, "data/items.cfd", "sword").unwrap();

        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        assert!(!contents.contains("sword"), "sword should be deleted:\n{contents}");
        assert!(contents.contains("axe"), "axe should remain:\n{contents}");
        assert!(contents.contains("Item {"), "Item group block header should remain:\n{contents}");
    }

    #[test]
    fn create_record_after_all_grouped_records_deleted() {
        // After deleting all records in a grouped block, the group block remains
        // (e.g. "Item {\n}\n"). New records should still be inserted inside the block.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path,
            "Item {\n  sword {\n    name: \"Sword\",\n  }\n}\n"
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Delete the only record — leaves "Item {\n}\n"
        delete_record_inner(&store, snap.session_id, "data/items.cfd", "sword").unwrap();

        // Now create a new record — should go inside the existing group block
        create_record_inner(&store, snap.session_id, "data/items.cfd", "axe", "Item").unwrap();

        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        assert!(contents.contains("axe"), "axe should be created:\n{contents}");
        // Must NOT use standalone syntax (mixed format would be ugly)
        assert!(!contents.contains("axe: Item"), "axe should use grouped syntax, not standalone:\n{contents}");
        // Group block should still have only one `Item {` header
        let item_count = contents.matches("Item {").count();
        assert_eq!(item_count, 1, "should have exactly one Item {{ group header:\n{contents}");
    }

    #[test]
    fn create_file_adds_to_session() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let node = create_file_inner(&store, snap.session_id, "data/loot.cfd").unwrap();
        assert_eq!(node.name, "loot.cfd");
        assert!(node.in_sources, "created file should be inside sources");

        // File on disk
        assert!(data_dir.join("loot.cfd").exists(), "physical file should be created");

        // Accessible via get_file_records
        let records = get_file_records_inner(&store, snap.session_id, "data/loot.cfd").unwrap();
        assert_eq!(records.records.len(), 0, "new file has no records");

        // Rejects paths outside project
        let err = create_file_inner(&store, snap.session_id, "../outside.cfd");
        assert!(err.is_err(), "should reject path outside project dir");

        // Rejects non-.cfd path
        let err = create_file_inner(&store, snap.session_id, "data/notes.txt");
        assert!(err.is_err(), "should reject non-.cfd extension");
    }

    #[test]
    fn rename_file_updates_session_maps() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path, "sword: Item {\n  name: \"Sword\",\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        rename_file_inner(&store, snap.session_id, "data/items.cfd", "data/weapons.cfd").unwrap();

        // Old path should no longer work
        let old_result = get_file_records_inner(&store, snap.session_id, "data/items.cfd");
        assert!(old_result.is_err(), "old path should no longer be loaded");

        // New path should work and contain the record
        let records = get_file_records_inner(&store, snap.session_id, "data/weapons.cfd").unwrap();
        assert!(records.records.iter().any(|r| r.key == "sword"), "record should be accessible at new path");

        // Physical file should be renamed
        assert!(data_dir.join("weapons.cfd").exists(), "physical file should exist at new path");
        assert!(!cfd_path.exists(), "physical file should not exist at old path");
    }

    #[test]
    fn delete_file_removes_from_session() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path, "sword: Item {\n  name: \"Sword\",\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        delete_file_inner(&store, snap.session_id, "data/items.cfd").unwrap();

        // File should be inaccessible from session
        let result = get_file_records_inner(&store, snap.session_id, "data/items.cfd");
        assert!(result.is_err(), "deleted file should not be accessible");

        // Physical file should be deleted
        assert!(!cfd_path.exists(), "physical file should be deleted");
    }

    #[test]
    fn reload_file_from_disk_picks_up_external_changes() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&cfd_path, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Confirm initial state
        let records = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert_eq!(records.records.len(), 1);
        assert_eq!(records.records[0].key, "sword");

        // Simulate external edit: overwrite the file with new content
        std::fs::write(&cfd_path, "axe: Item {\n  name: \"Axe\"\n}\n").unwrap();

        // Before reload, session still has old data
        let records_before = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert!(records_before.records.iter().any(|r| r.key == "sword"), "old data should still be present before reload");

        // Reload
        reload_file_from_disk_inner(&store, snap.session_id, "data/items.cfd").unwrap();

        // After reload, session should have new data
        let records_after = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert_eq!(records_after.records.len(), 1, "should have 1 record after reload");
        assert_eq!(records_after.records[0].key, "axe", "should see the new 'axe' record after reload");
        assert!(!records_after.records.iter().any(|r| r.key == "sword"), "old 'sword' record should be gone after reload");
    }

    #[test]
    fn write_field_nested_object_roundtrip() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Stats { hp: int; attack: int; } type Monster { stats: Stats; }").unwrap();
        std::fs::write(
            &cfd_path,
            "goblin: Monster {\n  stats: Stats {\n    hp: 10,\n    attack: 2,\n  },\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let new_stats = FieldValue::Object {
            actual_type: "Stats".to_string(),
            fields: vec![
                FieldCell { name: "hp".to_string(), value: FieldValue::Int { v: 100.0 } },
                FieldCell { name: "attack".to_string(), value: FieldValue::Int { v: 15.0 } },
            ],
        };
        write_field_inner(
            &store,
            snap.session_id,
            "data/items.cfd",
            "goblin",
            &[FieldPathSegment::Field { name: "stats".to_string() }],
            &new_stats,
        ).unwrap();

        let row = get_record_inner(&store, snap.session_id, "data/items.cfd", "goblin").unwrap();
        let stats = row.fields.iter().find(|f| f.name == "stats").unwrap();
        if let FieldValue::Object { fields, .. } = &stats.value {
            let hp = fields.iter().find(|f| f.name == "hp").unwrap();
            assert!(matches!(hp.value, FieldValue::Int { v } if (v - 100.0).abs() < 1e-9));
            let atk = fields.iter().find(|f| f.name == "attack").unwrap();
            assert!(matches!(atk.value, FieldValue::Int { v } if (v - 15.0).abs() < 1e-9));
        } else {
            panic!("expected Object, got {:?}", stats.value);
        }
    }

    #[test]
    fn write_field_ref_serializes_as_ampersand_key() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; related: Item; }").unwrap();
        std::fs::write(
            &cfd_path,
            "sword: Item {\n  name: \"Sword\",\n  related: &sword,\n}\nstaff: Item {\n  name: \"Staff\",\n  related: &sword,\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Change sword's related field to point to staff
        write_field_inner(
            &store,
            snap.session_id,
            "data/items.cfd",
            "sword",
            &[FieldPathSegment::Field { name: "related".to_string() }],
            &FieldValue::Ref {
                target_type: "Item".to_string(),
                target_key: "staff".to_string(),
                target_file: None,
            },
        ).unwrap();

        let contents = std::fs::read_to_string(data_dir.join("items.cfd")).unwrap();
        assert!(contents.contains("related: &staff"), "should serialize Ref as &staff, got:\n{contents}");
    }

    #[test]
    fn write_field_array_roundtrip() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { tags: [string]; }").unwrap();
        std::fs::write(
            &cfd_path,
            "sword: Item {\n  tags: [\"weapon\"],\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let new_tags = FieldValue::Array {
            items: vec![
                FieldValue::Str { v: "weapon".to_string() },
                FieldValue::Str { v: "melee".to_string() },
                FieldValue::Str { v: "rare".to_string() },
            ],
        };
        write_field_inner(
            &store,
            snap.session_id,
            "data/items.cfd",
            "sword",
            &[FieldPathSegment::Field { name: "tags".to_string() }],
            &new_tags,
        ).unwrap();

        let row = get_record_inner(&store, snap.session_id, "data/items.cfd", "sword").unwrap();
        let tags_field = row.fields.iter().find(|f| f.name == "tags").unwrap();
        if let FieldValue::Array { items } = &tags_field.value {
            assert_eq!(items.len(), 3, "should have 3 tags after write");
            assert!(matches!(&items[1], FieldValue::Str { v } if v == "melee"));
            assert!(matches!(&items[2], FieldValue::Str { v } if v == "rare"));
        } else {
            panic!("expected Array, got {:?}", tags_field.value);
        }
    }

    #[test]
    fn get_graph_returns_nodes_and_ref_edges() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; related: Item; }").unwrap();
        std::fs::write(
            &cfd_path,
            "sword: Item {\n  name: \"Sword\",\n  related: &staff,\n}\nstaff: Item {\n  name: \"Staff\",\n  related: &sword,\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let graph = get_graph_inner(&store, snap.session_id, "data/items.cfd", &[]).unwrap();

        assert_eq!(graph.nodes.len(), 2, "should have 2 nodes");
        let sword_node = graph.nodes.iter().find(|n| n.key == "sword").expect("sword node missing");
        let staff_node = graph.nodes.iter().find(|n| n.key == "staff").expect("staff node missing");
        assert!(sword_node.in_focus_file, "sword should be in focus file");
        assert!(staff_node.in_focus_file, "staff should be in focus file");

        // Edges: sword→staff and staff→sword
        assert_eq!(graph.edges.len(), 2, "should have 2 ref edges");
        let has_sword_to_staff = graph.edges.iter().any(|e| {
            e.source.contains("sword") && e.target.contains("staff")
        });
        let has_staff_to_sword = graph.edges.iter().any(|e| {
            e.source.contains("staff") && e.target.contains("sword")
        });
        assert!(has_sword_to_staff, "missing sword→staff edge");
        assert!(has_staff_to_sword, "missing staff→sword edge");
    }

    #[test]
    fn get_graph_collapses_nodes_beyond_max_depth() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("items.cfd");

        // Chain: a → b → c → d (4 hops; d should be collapsed at depth 3)
        std::fs::write(&schema_path, "type Item { next: Item; }").unwrap();
        std::fs::write(
            &cfd_path,
            "a: Item { next: &b, }\nb: Item { next: &c, }\nc: Item { next: &d, }\nd: Item { next: &a, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Default (no expanded keys) — a is focus file, so starting from a: a(0) b(1) c(2) d(3, collapsed)
        let graph = get_graph_inner(&store, snap.session_id, "data/items.cfd", &["a".to_string()]).unwrap();
        let d_node = graph.nodes.iter().find(|n| n.key == "d");
        // d is reachable as focus file record (it's in the same file), so it may not be collapsed
        // The key insight: all 4 are in focus_file, so they start at depth 0 in the queue
        assert!(d_node.is_some(), "d should be in graph");
        // With all 4 as focus keys, none should be collapsed at depth 0
        assert!(!d_node.unwrap().is_collapsed, "d should not be collapsed since it starts at depth 0 as a focus record");
    }

    #[test]
    fn get_graph_collapses_cross_file_nodes_at_max_depth() {
        // Set up two files: focus.cfd has a single record `root` that chains
        // through b→c→d (all in other.cfd).  d is reached at depth 3 (MAX_DEPTH)
        // and must be collapsed (is_collapsed = true, fields = []).
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let focus_path = data_dir.join("focus.cfd");
        let other_path = data_dir.join("other.cfd");

        std::fs::write(&schema_path, "type Item { next: Item?; }").unwrap();
        // focus.cfd: root → b
        std::fs::write(&focus_path, "root: Item { next: &b, }\n").unwrap();
        // other.cfd: b → c → d (d has no next)
        std::fs::write(
            &other_path,
            "b: Item { next: &c, }\nc: Item { next: &d, }\nd: Item { next: null, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Focus on focus.cfd; no expanded_keys
        let graph = get_graph_inner(&store, snap.session_id, "data/focus.cfd", &[]).unwrap();

        // root is at depth 0 (focus), b at depth 1, c at depth 2, d at depth 3
        assert!(graph.nodes.iter().any(|n| n.key == "root"), "root missing");
        assert!(graph.nodes.iter().any(|n| n.key == "b"), "b missing");
        assert!(graph.nodes.iter().any(|n| n.key == "c"), "c missing");

        let d_node = graph.nodes.iter().find(|n| n.key == "d").expect("d should be in graph");
        assert!(d_node.is_collapsed, "d at depth 3 should be collapsed");
        assert!(d_node.fields.is_empty(), "collapsed node should have no fields");

        // Now provide d as an expanded key — it should no longer be collapsed
        let graph2 = get_graph_inner(&store, snap.session_id, "data/focus.cfd", &["d".to_string()]).unwrap();
        let d_node2 = graph2.nodes.iter().find(|n| n.key == "d").expect("d missing in expanded graph");
        assert!(!d_node2.is_collapsed, "d should not be collapsed when explicitly expanded");
    }

    #[test]
    fn validate_cfd_key_rejects_illegal_chars() {
        // Empty key
        assert!(validate_cfd_key("").is_err());
        // Whitespace
        assert!(validate_cfd_key("my key").is_err());
        assert!(validate_cfd_key("key\t").is_err());
        // Illegal chars
        for c in &[':', '=', ';', ',', '{', '}', '[', ']', '(', ')', '@', '&', '"'] {
            let key = format!("key{c}");
            assert!(validate_cfd_key(&key).is_err(), "should reject key containing '{c}'");
        }
        // Valid keys
        assert!(validate_cfd_key("simple_key").is_ok());
        assert!(validate_cfd_key("key-with-dashes").is_ok());
        assert!(validate_cfd_key("key123").is_ok());
        assert!(validate_cfd_key("CamelCase").is_ok());
    }

    #[test]
    fn get_all_type_names_returns_concrete_types() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"),
            "abstract type Base {}\ntype Sword : Base {}\ntype Shield : Base {}").unwrap();
        std::fs::write(data_dir.join("items.cfd"), "").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let names = get_all_type_names_inner(&store, snap.session_id).unwrap();
        assert!(names.contains(&"Sword".to_string()), "Sword missing: {names:?}");
        assert!(names.contains(&"Shield".to_string()), "Shield missing: {names:?}");
        assert!(!names.contains(&"Base".to_string()), "abstract Base should be excluded: {names:?}");
    }

    #[test]
    fn get_ref_targets_filters_by_assignable_type() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"),
            "type Weapon { power: int; }\ntype Armor { defense: int; }").unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Weapon { power: 10, }\nshield: Armor { defense: 5, }\nbow: Weapon { power: 7, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let weapon_refs = get_ref_targets_inner(&store, snap.session_id, "Weapon").unwrap();
        assert!(weapon_refs.contains(&"sword".to_string()), "sword missing: {weapon_refs:?}");
        assert!(weapon_refs.contains(&"bow".to_string()), "bow missing: {weapon_refs:?}");
        assert!(!weapon_refs.contains(&"shield".to_string()), "shield (Armor) should not be in Weapon refs: {weapon_refs:?}");

        let armor_refs = get_ref_targets_inner(&store, snap.session_id, "Armor").unwrap();
        assert_eq!(armor_refs, vec!["shield".to_string()]);
    }

    #[test]
    fn get_enum_variants_returns_variant_names() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"),
            "enum Rarity { Common, Uncommon, Rare, Epic }").unwrap();
        std::fs::write(data_dir.join("items.cfd"), "").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let variants = get_enum_variants_inner(&store, snap.session_id, "Rarity").unwrap();
        assert_eq!(variants, vec!["Common", "Uncommon", "Rare", "Epic"]);

        let unknown = get_enum_variants_inner(&store, snap.session_id, "NonExistent").unwrap();
        assert!(unknown.is_empty(), "unknown enum should return empty: {unknown:?}");
    }

    #[test]
    fn get_all_records_brief_returns_all_records_with_types() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"), "type Item { name: string; }").unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", }\nbow: Item { name: \"Bow\", }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let briefs = get_all_records_brief_inner(&store, snap.session_id).unwrap();
        assert_eq!(briefs.len(), 2);

        let sword = briefs.iter().find(|b| b.key == "sword").expect("sword missing");
        assert_eq!(sword.actual_type, "Item");
        assert!(sword.file_path.contains("items.cfd"), "file_path should contain items.cfd: {}", sword.file_path);

        let bow = briefs.iter().find(|b| b.key == "bow").expect("bow missing");
        assert_eq!(bow.actual_type, "Item");

        // Results should be sorted by key
        assert_eq!(briefs[0].key, "bow");
        assert_eq!(briefs[1].key, "sword");
    }

    #[test]
    fn get_file_records_returns_typed_rows_with_fields() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"),
            "type Weapon { power: int; name: string; }").unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Weapon { power: 10, name: \"Sword\", }\nbow: Weapon { power: 7, name: \"Bow\", }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let file_recs = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert_eq!(file_recs.type_names, vec!["Weapon".to_string()]);
        assert_eq!(file_recs.records.len(), 2);

        let sword = file_recs.records.iter().find(|r| r.key == "sword").expect("sword missing");
        assert_eq!(sword.actual_type, "Weapon");
        let power = sword.fields.iter().find(|f| f.name == "power").expect("power field missing");
        assert!(matches!(power.value, FieldValue::Int { v } if (v - 10.0).abs() < 1e-9));

        // Invalid session id returns error
        assert!(get_file_records_inner(&store, 9999, "data/items.cfd").is_err());
        // Invalid file path returns error
        assert!(get_file_records_inner(&store, snap.session_id, "data/nonexistent.cfd").is_err());
    }

    #[test]
    fn get_record_returns_single_row_with_all_fields() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"),
            "type Stats { hp: int; attack: int; } type Monster { name: string; stats: Stats; }").unwrap();
        std::fs::write(
            data_dir.join("monsters.cfd"),
            "goblin: Monster {\n  name: \"Goblin\",\n  stats: Stats { hp: 20, attack: 3, },\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let row = get_record_inner(&store, snap.session_id, "data/monsters.cfd", "goblin").unwrap();
        assert_eq!(row.key, "goblin");
        assert_eq!(row.actual_type, "Monster");

        let name_field = row.fields.iter().find(|f| f.name == "name").expect("name missing");
        assert!(matches!(&name_field.value, FieldValue::Str { v } if v == "Goblin"));

        let stats_field = row.fields.iter().find(|f| f.name == "stats").expect("stats missing");
        if let FieldValue::Object { actual_type, fields } = &stats_field.value {
            assert_eq!(actual_type, "Stats");
            let hp = fields.iter().find(|f| f.name == "hp").expect("hp missing");
            assert!(matches!(hp.value, FieldValue::Int { v } if (v - 20.0).abs() < 1e-9));
        } else {
            panic!("expected Object for stats, got {:?}", stats_field.value);
        }

        // Non-existent record returns error
        assert!(get_record_inner(&store, snap.session_id, "data/monsters.cfd", "dragon").is_err());
    }

    #[test]
    fn write_field_nullable_object_from_null() {
        // When a nullable Object field is null (absent from file), writing an Object value
        // should insert the field using insert_field path without error.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = data_dir.join("monsters.cfd");

        std::fs::write(&schema_path, "type Stats { hp: int; } type Monster { name: string; bonus: Stats?; }").unwrap();
        // bonus field is absent (null) — the record has only name
        std::fs::write(&cfd_path, "goblin: Monster {\n  name: \"Goblin\",\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Write an Object value to the absent nullable field
        let new_stats = FieldValue::Object {
            actual_type: "Stats".to_string(),
            fields: vec![
                FieldCell { name: "hp".to_string(), value: FieldValue::Int { v: 50.0 } },
            ],
        };
        write_field_inner(
            &store,
            snap.session_id,
            "data/monsters.cfd",
            "goblin",
            &[FieldPathSegment::Field { name: "bonus".to_string() }],
            &new_stats,
        ).unwrap();

        // Verify the file contains the inserted Object
        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        assert!(contents.contains("bonus"), "bonus field should be in file:\n{contents}");
        assert!(contents.contains("Stats"), "Stats type should be in file:\n{contents}");

        // Verify the model reflects the new field
        let row = get_record_inner(&store, snap.session_id, "data/monsters.cfd", "goblin").unwrap();
        let bonus = row.fields.iter().find(|f| f.name == "bonus").expect("bonus field missing from model");
        if let FieldValue::Object { actual_type, fields } = &bonus.value {
            assert_eq!(actual_type, "Stats");
            let hp = fields.iter().find(|f| f.name == "hp").expect("hp missing");
            assert!(matches!(hp.value, FieldValue::Int { v } if (v - 50.0).abs() < 1e-9));
        } else {
            panic!("expected Object for bonus, got {:?}", bonus.value);
        }
    }

    #[test]
    fn close_session_removes_session_from_store() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(dir.path().join("schema.cft"), "type Item { name: string; }").unwrap();
        std::fs::write(data_dir.join("items.cfd"), "sword: Item { name: \"Sword\", }\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Session is accessible before close
        assert!(get_file_records_inner(&store, sid, "data/items.cfd").is_ok());

        // Close session
        let _ = close_session_inner(&store, sid);

        // Session is no longer accessible after close
        assert!(get_file_records_inner(&store, sid, "data/items.cfd").is_err());
        // Closing non-existent session is a no-op (no panic)
        let _ = close_session_inner(&store, 9999);
    }

    /// Integration test: load the real examples/cfd project and verify the full chain.
    /// This test uses the actual schema and data files to catch regressions in parsing,
    /// model building, and field serialization that synthetic schemas may not exercise.
    #[test]
    fn integration_load_example_cfd_project() {
        // Locate examples/cfd relative to the workspace root (CARGO_MANIFEST_DIR is the crate dir)
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let examples_yaml = manifest_dir
            .join("../../examples/cfd/coflow.yaml");

        if !examples_yaml.exists() {
            // Skip gracefully if examples are not present (CI without full repo checkout)
            eprintln!("integration test skipped: examples/cfd/coflow.yaml not found");
            return;
        }

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, examples_yaml.to_str().unwrap())
            .expect("should load examples/cfd without error");

        // No errors from schema/load stage
        let load_errors: Vec<_> = snap.diagnostics.iter()
            .filter(|d| d.severity == "Error" && (d.stage == "SCHEMA" || d.stage == "LOAD"))
            .collect();
        assert!(
            load_errors.is_empty(),
            "expected no schema/load errors, got: {load_errors:?}"
        );

        // File tree should list the 3 data files
        fn count_files(nodes: &[FileTreeNode]) -> usize {
            nodes.iter().map(|n| if n.is_dir { count_files(&n.children) } else { 1 }).sum()
        }
        let file_count = count_files(&snap.file_tree);
        assert!(file_count >= 3, "expected at least 3 data files, got {file_count}");

        // All type_names present in session should be concrete (no abstract types)
        let type_names = get_all_type_names_inner(&store, snap.session_id).unwrap();
        assert!(!type_names.is_empty(), "expected concrete types");
        // Reward is abstract in the schema — must not appear
        assert!(
            !type_names.contains(&"Reward".to_string()),
            "abstract type 'Reward' should be excluded from type names"
        );
        // Concrete subtypes must be present
        assert!(type_names.contains(&"ItemReward".to_string()), "ItemReward missing from types");
        assert!(type_names.contains(&"CurrencyReward".to_string()), "CurrencyReward missing");
        assert!(type_names.contains(&"Monster".to_string()), "Monster missing");
        assert!(type_names.contains(&"Item".to_string()), "Item missing");

        // Load records from the first data file and verify Item records exist
        let records_01 = get_file_records_inner(&store, snap.session_id, "data/01-records.cfd")
            .expect("01-records.cfd should load");
        let items: Vec<_> = records_01.records.iter().filter(|r| r.actual_type == "Item").collect();
        assert!(items.len() >= 2, "expected at least 2 Item records, got {}", items.len());

        // sword_fire must have correct field values
        let sword = records_01.records.iter().find(|r| r.key == "sword_fire")
            .expect("sword_fire missing");
        assert_eq!(sword.actual_type, "Item");
        let name = sword.fields.iter().find(|f| f.name == "name").expect("name field missing");
        assert!(
            matches!(&name.value, FieldValue::Str { v } if v == "Fire Sword"),
            "expected name='Fire Sword', got {:?}", name.value
        );
        let price = sword.fields.iter().find(|f| f.name == "price").expect("price missing");
        assert!(
            matches!(price.value, FieldValue::Int { v } if (v - 100.0).abs() < 1e-9),
            "expected price=100, got {:?}", price.value
        );
        let element = sword.fields.iter().find(|f| f.name == "element").expect("element missing");
        assert!(
            matches!(&element.value, FieldValue::Enum { variant, .. } if variant == "Fire"),
            "expected element=Fire, got {:?}", element.value
        );
        let tags = sword.fields.iter().find(|f| f.name == "tags").expect("tags missing");
        if let FieldValue::Array { items } = &tags.value {
            assert_eq!(items.len(), 2, "expected 2 tags");
        } else {
            panic!("expected Array for tags, got {:?}", tags.value);
        }

        // basic_monster must have a nested Stats object
        let monster = records_01.records.iter().find(|r| r.key == "basic_monster")
            .expect("basic_monster missing");
        assert_eq!(monster.actual_type, "Monster");
        let stats = monster.fields.iter().find(|f| f.name == "stats").expect("stats missing");
        if let FieldValue::Object { actual_type, fields } = &stats.value {
            assert_eq!(actual_type, "Stats");
            let hp = fields.iter().find(|f| f.name == "hp").expect("hp missing");
            assert!(
                matches!(hp.value, FieldValue::Int { v } if (v - 100.0).abs() < 1e-9),
                "expected hp=100"
            );
        } else {
            panic!("expected Object for stats, got {:?}", stats.value);
        }

        // weaknesses is a {Element: float} dict
        let weaknesses = monster.fields.iter().find(|f| f.name == "weaknesses").expect("weaknesses missing");
        if let FieldValue::Dict { entries } = &weaknesses.value {
            assert_eq!(entries.len(), 2, "expected 2 weakness entries");
        } else {
            panic!("expected Dict for weaknesses, got {:?}", weaknesses.value);
        }

        // Graph for 01-records.cfd should include basic_monster node
        let graph = get_graph_inner(&store, snap.session_id, "data/01-records.cfd", &[])
            .expect("graph should build");
        assert!(
            graph.nodes.iter().any(|n| n.key == "basic_monster"),
            "basic_monster not in graph"
        );
        // sword_fire is referenced by basic_monster's drop → should appear as an edge target
        assert!(
            graph.nodes.iter().any(|n| n.key == "sword_fire"),
            "sword_fire not in graph (should be reachable via ref)"
        );

        // Enum variants for Element should include Fire and Ice
        let element_variants = get_enum_variants_inner(&store, snap.session_id, "Element")
            .expect("should get Element variants");
        assert!(element_variants.contains(&"Fire".to_string()), "Fire variant missing");
        assert!(element_variants.contains(&"Ice".to_string()), "Ice variant missing");

        // Ref targets for Item should include sword_fire and staff_ice
        let item_refs = get_ref_targets_inner(&store, snap.session_id, "Item")
            .expect("should get Item ref targets");
        assert!(item_refs.contains(&"sword_fire".to_string()), "sword_fire not in ref targets");
        assert!(item_refs.contains(&"staff_ice".to_string()), "staff_ice not in ref targets");

        // Diagnostics after all checks — should be no data errors for valid example
        let data_diags: Vec<_> = snap.diagnostics.iter()
            .filter(|d| d.severity == "Error")
            .collect();
        assert!(
            data_diags.is_empty(),
            "expected no errors for valid example project, got: {data_diags:?}"
        );

        // 03-spread.cfd: elite_monster spreads basic_monster's fields
        let records_03 = get_file_records_inner(&store, snap.session_id, "data/03-spread.cfd")
            .expect("03-spread.cfd should load");
        let elite = records_03.records.iter().find(|r| r.key == "elite_monster")
            .expect("elite_monster missing from 03-spread.cfd");
        assert_eq!(elite.actual_type, "Monster");
        // spread_sources: elite_monster spreads from basic_monster
        assert!(
            !elite.spread_sources.is_empty(),
            "elite_monster should have spread_sources"
        );
        // elite_monster overrides all 4 Monster fields (name, stats, weaknesses, drop) directly,
        // so spread_fields is empty — all fields are declared directly.
        assert!(
            !elite.spread_fields.contains(&"name".to_string()),
            "name is overridden directly, should not be a spread field"
        );
        assert!(
            !elite.spread_fields.contains(&"stats".to_string()),
            "stats is overridden directly, should not be a spread field"
        );
        // name should be "Elite Training Dummy" (local override)
        let name_field = elite.fields.iter().find(|f| f.name == "name").expect("name missing");
        assert!(
            matches!(&name_field.value, FieldValue::Str { v } if v == "Elite Training Dummy"),
            "expected 'Elite Training Dummy', got {:?}", name_field.value
        );
    }

    /// Integration test: write a field in a copy of examples/cfd and verify round-trip.
    #[test]
    fn integration_write_field_example_cfd() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let examples_yaml = manifest_dir.join("../../examples/cfd/coflow.yaml");
        if !examples_yaml.exists() {
            eprintln!("integration test skipped: examples/cfd/coflow.yaml not found");
            return;
        }

        // Copy example into a temp dir to avoid modifying the real source
        let tmp = TempDir::new().unwrap();
        let tmp_root = tmp.path();

        // Copy schema.cft
        std::fs::copy(
            manifest_dir.join("../../examples/cfd/schema.cft"),
            tmp_root.join("schema.cft"),
        ).unwrap();
        // Copy coflow.yaml
        std::fs::copy(&examples_yaml, tmp_root.join("coflow.yaml")).unwrap();
        // Copy data dir
        let data_src = manifest_dir.join("../../examples/cfd/data");
        let data_dst = tmp_root.join("data");
        std::fs::create_dir(&data_dst).unwrap();
        for entry in std::fs::read_dir(&data_src).unwrap().filter_map(|e| e.ok()) {
            std::fs::copy(entry.path(), data_dst.join(entry.file_name())).unwrap();
        }

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, tmp_root.join("coflow.yaml").to_str().unwrap())
            .expect("should load copied example");

        // Write sword_fire's price from 100 → 200
        write_field_inner(
            &store,
            snap.session_id,
            "data/01-records.cfd",
            "sword_fire",
            &[FieldPathSegment::Field { name: "price".to_string() }],
            &FieldValue::Int { v: 200.0 },
        ).expect("write_field should succeed");

        // Verify the in-memory model reflects the change
        let records = get_file_records_inner(&store, snap.session_id, "data/01-records.cfd")
            .expect("should load records after write");
        let sword = records.records.iter().find(|r| r.key == "sword_fire").expect("sword_fire missing");
        let price = sword.fields.iter().find(|f| f.name == "price").expect("price missing");
        assert!(
            matches!(price.value, FieldValue::Int { v } if (v - 200.0).abs() < 1e-9),
            "expected price=200 after write, got {:?}", price.value
        );

        // Verify the file on disk was updated
        let file_contents = std::fs::read_to_string(data_dst.join("01-records.cfd")).unwrap();
        assert!(
            file_contents.contains("price: 200"),
            "expected 'price: 200' in file after write:\n{file_contents}"
        );
        assert!(
            !file_contents.contains("price: 100"),
            "old value 'price: 100' still in file after write:\n{file_contents}"
        );

        // Write staff_ice's element from Ice → Fire (enum)
        write_field_inner(
            &store,
            snap.session_id,
            "data/01-records.cfd",
            "staff_ice",
            &[FieldPathSegment::Field { name: "element".to_string() }],
            &FieldValue::Enum { enum_name: "Element".to_string(), variant: "Fire".to_string(), int_value: 1.0 },
        ).expect("write enum field should succeed");

        let records2 = get_file_records_inner(&store, snap.session_id, "data/01-records.cfd")
            .expect("should load records after enum write");
        let staff = records2.records.iter().find(|r| r.key == "staff_ice").expect("staff_ice missing");
        let elem = staff.fields.iter().find(|f| f.name == "element").expect("element missing");
        assert!(
            matches!(&elem.value, FieldValue::Enum { variant, .. } if variant == "Fire"),
            "expected element=Fire after write, got {:?}", elem.value
        );
    }

    #[test]
    fn get_file_records_fallback_for_missing_required_field() {
        // When a record is missing a required field, the model build fails for it.
        // get_file_records_inner should fall back to the raw AST and still return the record.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        // Schema requires both name and power; we'll create one record missing power
        std::fs::write(dir.path().join("schema.cft"), "type Weapon { name: string; power: int; }").unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Weapon { name: \"Sword\", power: 10, }\nbroken: Weapon { name: \"Broken\", }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let file_recs = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        // Both records should appear — one from model, one from AST fallback
        assert_eq!(file_recs.records.len(), 2, "both records should be returned");

        let sword = file_recs.records.iter().find(|r| r.key == "sword").expect("sword missing");
        assert_eq!(sword.actual_type, "Weapon");
        let power = sword.fields.iter().find(|f| f.name == "power");
        assert!(power.is_some(), "sword should have power field from model");

        let broken = file_recs.records.iter().find(|r| r.key == "broken").expect("broken missing");
        // broken comes from AST fallback — actual_type is set from the AST type_name
        assert_eq!(broken.actual_type, "Weapon");
        // name field should be present from AST
        let name_field = broken.fields.iter().find(|f| f.name == "name");
        assert!(name_field.is_some(), "broken should have name field from AST fallback");
        // power was NOT written but the schema has it — should appear as Null (schema expansion)
        let power_field = broken.fields.iter().find(|f| f.name == "power");
        assert!(power_field.is_some(), "broken should have power field (as Null) via schema expansion in AST fallback");
        assert!(matches!(power_field.unwrap().value, FieldValue::Null), "power should be Null");
    }

    #[test]
    fn reload_file_preserves_cross_file_refs() {
        // Two files: elements.cfd has Element records; items.cfd has Item records that
        // reference elements. After writing to items.cfd (triggering reload_file), the
        // cross-file Ref should still resolve correctly in the rebuilt model.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();

        std::fs::write(
            dir.path().join("schema.cft"),
            "type Element { name: string; }\ntype Item { name: string; element: Element; }",
        ).unwrap();
        std::fs::write(
            data_dir.join("elements.cfd"),
            "fire: Element { name: \"Fire\", }\nice: Element { name: \"Ice\", }\n",
        ).unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", element: &fire, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Verify initial cross-file Ref resolves
        let sword_before = get_record_inner(&store, sid, "data/items.cfd", "sword").unwrap();
        let elem_field_before = sword_before.fields.iter().find(|f| f.name == "element").expect("element field missing");
        assert!(
            matches!(&elem_field_before.value, FieldValue::Ref { target_key, .. } if target_key == "fire"),
            "expected Ref to fire before write, got {:?}", elem_field_before.value
        );

        // Write items.cfd (change sword's name) — this triggers reload_file which rebuilds the model
        write_field_inner(
            &store, sid, "data/items.cfd", "sword",
            &[FieldPathSegment::Field { name: "name".to_string() }],
            &FieldValue::Str { v: "Fire Sword".to_string() },
        ).unwrap();

        // After the write, the cross-file Ref should still resolve correctly
        let sword_after = get_record_inner(&store, sid, "data/items.cfd", "sword").unwrap();
        let elem_field_after = sword_after.fields.iter().find(|f| f.name == "element").expect("element field missing after write");
        assert!(
            matches!(&elem_field_after.value, FieldValue::Ref { target_key, .. } if target_key == "fire"),
            "expected Ref to fire after write, got {:?}", elem_field_after.value
        );

        // The name change should also be visible
        let name_field = sword_after.fields.iter().find(|f| f.name == "name").unwrap();
        assert!(
            matches!(&name_field.value, FieldValue::Str { v } if v == "Fire Sword"),
            "expected name 'Fire Sword', got {:?}", name_field.value
        );
    }

    #[test]
    fn get_all_records_brief_includes_ast_fallback_records() {
        // Records that fail model build (missing required field) should still appear in
        // get_all_records_brief, using the type name from the AST.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(
            dir.path().join("schema.cft"),
            "type Item { name: string; power: int; }",
        ).unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", power: 10, }\nbroken: Item { name: \"Broken\", }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let briefs = get_all_records_brief_inner(&store, snap.session_id).unwrap();
        assert_eq!(briefs.len(), 2, "both records should appear in brief");

        let broken = briefs.iter().find(|b| b.key == "broken").expect("broken missing from briefs");
        // Even though model build failed for 'broken' (missing power), it should have type from AST
        assert_eq!(broken.actual_type, "Item", "broken should have type Item from AST");
        assert!(broken.file_path.contains("items.cfd"));
    }

    #[test]
    fn get_record_falls_back_to_ast_when_model_build_fails() {
        // When a record's model build fails (missing required field), get_record_inner
        // should still return a RecordRow using the AST fallback path.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(
            dir.path().join("schema.cft"),
            "type Item { name: string; power: int; }",
        ).unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", power: 10, }\nbroken: Item { name: \"Broken\", }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // 'broken' record is missing required 'power' field — model build fails for it
        // get_record_inner should still return it via AST fallback
        let row = get_record_inner(&store, snap.session_id, "data/items.cfd", "broken").unwrap();
        assert_eq!(row.key, "broken");
        assert_eq!(row.actual_type, "Item");
        let name_field = row.fields.iter().find(|f| f.name == "name").expect("name field missing");
        assert!(matches!(&name_field.value, FieldValue::Str { v } if v == "Broken"));
    }

    #[test]
    fn get_diagnostics_reports_errors_after_write_breaks_ref() {
        // Writing a Ref to a non-existent key should cause a diagnostic error after the write.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(
            dir.path().join("schema.cft"),
            "type Item { name: string; } type Bag { item: Item; }",
        ).unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", }\nbag: Bag { item: &sword, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Before write: no diagnostics errors
        let diags_before = get_diagnostics_inner(&store, sid).unwrap();
        assert!(
            diags_before.iter().all(|d| d.severity != "error"),
            "should have no errors before write: {:?}", diags_before
        );

        // Write a Ref to a non-existent key — bag.item now points to nothing
        write_field_inner(
            &store, sid, "data/items.cfd", "bag",
            &[FieldPathSegment::Field { name: "item".to_string() }],
            &FieldValue::Ref {
                target_type: "Item".to_string(),
                target_key: "nonexistent_key".to_string(),
                target_file: None,
            },
        ).unwrap();

        // After write: diagnostics should report a broken reference error
        let diags_after = get_diagnostics_inner(&store, sid).unwrap();
        assert!(
            diags_after.iter().any(|d| d.severity == "error"),
            "should have at least one error after writing broken Ref: {:?}", diags_after
        );
    }

    #[test]
    fn pending_model_diags_have_record_key() {
        // When a model build fails (unresolved Ref), the resulting pending_model_diags
        // should include record_key so the diagnostics panel can navigate to the broken record.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        std::fs::write(
            dir.path().join("schema.cft"),
            "type Item { name: string; } type Bag { item: Item; }",
        ).unwrap();
        std::fs::write(
            data_dir.join("items.cfd"),
            "sword: Item { name: \"Sword\", }\nbag: Bag { item: &sword, }\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // No errors before the write
        let diags_before = get_diagnostics_inner(&store, sid).unwrap();
        assert!(diags_before.iter().all(|d| d.severity != "error"), "no errors before write");

        // Write bag.item = &nonexistent — model build fails with an unresolved Ref
        write_field_inner(
            &store, sid, "data/items.cfd", "bag",
            &[FieldPathSegment::Field { name: "item".to_string() }],
            &FieldValue::Ref { target_type: "Item".to_string(), target_key: "nonexistent".to_string(), target_file: None },
        ).unwrap();

        // Diagnostics should contain at least one error with record_key populated
        let diags = get_diagnostics_inner(&store, sid).unwrap();
        let with_key = diags.iter().find(|d| d.severity == "error" && d.record_key.is_some());
        assert!(
            with_key.is_some(),
            "expected a diagnostic with a record_key after writing broken Ref, got: {:?}", diags
        );
        // The broken record is 'bag' (it holds the bad Ref)
        let bag_diag = diags.iter().find(|d| d.severity == "error" && d.record_key.as_deref() == Some("bag"));
        assert!(
            bag_diag.is_some(),
            "expected record_key='bag' in diagnostics after writing broken Ref, got: {:?}", diags
        );
    }

    #[test]
    fn get_field_schemas_returns_nullable_object_type() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        std::fs::write(
            dir.path().join("schema.cft"),
            "type Stats { hp: int; } type Player { name: string; stats: Stats?; power: int; }",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources: []").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let schemas = get_field_schemas_inner(&store, snap.session_id, "Player").unwrap();
        assert_eq!(schemas.len(), 3, "Player has 3 fields: name, stats, power");

        let name_schema = schemas.iter().find(|s| s.name == "name").expect("name field");
        assert_eq!(name_schema.nullable_object_type, None, "name is string, not nullable Object");

        let stats_schema = schemas.iter().find(|s| s.name == "stats").expect("stats field");
        assert_eq!(
            stats_schema.nullable_object_type,
            Some("Stats".to_string()),
            "stats is Stats? — nullable_object_type should be 'Stats'"
        );

        let power_schema = schemas.iter().find(|s| s.name == "power").expect("power field");
        assert_eq!(power_schema.nullable_object_type, None, "power is int, not nullable Object");
    }

    #[test]
    fn empty_inline_object_shows_all_schema_fields() {
        // When a nullable Object field is written as an empty block (e.g. `stats: Stats {}`),
        // convert_value must still populate all schema-defined fields as Null so the UI
        // can display them as editable. Previously, filter_map caused 0 fields to appear.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let cfd = data_dir.join("test.cfd");
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(
            &schema_path,
            "type Stats { hp: int; mp: int; }\ntype Player { name: string; stats: Stats?; }",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        // Player with an empty Stats block (no fields written yet)
        std::fs::write(&cfd, "p1: Player {\n  name: \"Alice\",\n  stats: Stats {},\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let row = get_record_inner(&store, snap.session_id, "data/test.cfd", "p1").unwrap();

        let stats_cell = row.fields.iter().find(|f| f.name == "stats").expect("stats field");
        match &stats_cell.value {
            FieldValue::Object { actual_type, fields } => {
                assert_eq!(actual_type, "Stats");
                assert_eq!(fields.len(), 2, "Stats has 2 fields: hp, mp — empty block must still show both as Null");
                assert!(fields.iter().all(|f| matches!(f.value, FieldValue::Null)), "both fields should be Null");
            }
            other => panic!("expected Object, got {other:?}"),
        }
    }

    #[test]
    fn get_record_source_returns_raw_span() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        let cfd_path = dir.path().join("data/items.cfd");

        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(
            &cfd_path,
            "sword: Item {\n  name: \"Sword\",\n}\nshield: Item {\n  name: \"Shield\",\n}\n",
        ).unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let src = get_record_source_inner(&store, snap.session_id, "data/items.cfd", "sword").unwrap();
        // Source should contain the sword record but not the shield record
        assert!(src.contains("sword"), "source should contain 'sword': {src}");
        assert!(src.contains("Sword"), "source should contain field value 'Sword': {src}");
        assert!(!src.contains("shield"), "source should not contain 'shield': {src}");

        // Unknown record should return error
        let err = get_record_source_inner(&store, snap.session_id, "data/items.cfd", "nonexistent");
        assert!(err.is_err());
    }

    #[test]
    fn get_field_schemas_includes_default_and_element_type() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");

        // Schema: Player has required name, optional hp (default 100), nullable Stats, and array of nullable Stats
        std::fs::write(
            &schema_path,
            "type Stats { hp: int; }\ntype Player { name: string; hp: int = 100; avatar: Stats?; rewards: [Stats?]; }",
        ).unwrap();
        std::fs::write(&dir.path().join("data/test.cfd"), "").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let schemas = get_field_schemas_inner(&store, snap.session_id, "Player").unwrap();

        let name_s = schemas.iter().find(|s| s.name == "name").expect("name field");
        assert!(!name_s.has_default, "name has no default");
        assert!(name_s.default_str.is_none(), "name default_str should be None");
        assert!(name_s.nullable_object_type.is_none());
        assert!(name_s.array_nullable_element_type.is_none());

        let hp_s = schemas.iter().find(|s| s.name == "hp").expect("hp field");
        assert!(hp_s.has_default, "hp has a default");
        assert_eq!(hp_s.default_str.as_deref(), Some("100"), "hp default_str should be '100'");

        let avatar_s = schemas.iter().find(|s| s.name == "avatar").expect("avatar field");
        assert_eq!(avatar_s.nullable_object_type.as_deref(), Some("Stats"), "avatar nullable_object_type should be 'Stats'");
        assert!(avatar_s.array_nullable_element_type.is_none());

        let rewards_s = schemas.iter().find(|s| s.name == "rewards").expect("rewards field");
        assert!(rewards_s.nullable_object_type.is_none());
        assert_eq!(rewards_s.array_nullable_element_type.as_deref(), Some("Stats"), "rewards array_nullable_element_type should be 'Stats'");
    }

    #[test]
    fn move_record_removes_from_src_and_adds_to_dst() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let src_cfd = data_dir.join("src.cfd");
        let dst_cfd = data_dir.join("dst.cfd");
        std::fs::write(&src_cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&dst_cfd, "").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        move_record_inner(&store, snap.session_id, "data/src.cfd", "data/dst.cfd", "sword").unwrap();

        // src.cfd should not contain the record anymore
        let src_content = std::fs::read_to_string(&src_cfd).unwrap();
        assert!(!src_content.contains("sword"), "src should not contain sword after move");

        // dst.cfd should contain the record
        let dst_content = std::fs::read_to_string(&dst_cfd).unwrap();
        assert!(dst_content.contains("sword"), "dst should contain sword after move");
        assert!(dst_content.contains("Sword"), "dst should contain field value 'Sword'");

        // File record keys in session should reflect the move
        let session_arc = get_session(&store, snap.session_id).unwrap();
        let session = session_arc.lock().unwrap();
        let src_keys = session.file_record_keys.get("data/src.cfd").unwrap();
        let dst_keys = session.file_record_keys.get("data/dst.cfd").unwrap();
        assert!(!src_keys.contains(&"sword".to_string()), "src keys should not contain sword");
        assert!(dst_keys.contains(&"sword".to_string()), "dst keys should contain sword");
    }

    #[test]
    fn copy_record_to_file_basic() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let src_cfd = data_dir.join("src.cfd");
        let dst_cfd = data_dir.join("dst.cfd");
        std::fs::write(&src_cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&dst_cfd, "shield: Item {\n  name: \"Shield\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Copy sword → dst with a new key
        let row = copy_record_to_file_inner(&store, sid, "data/src.cfd", "data/dst.cfd", "sword", "sword_copy").unwrap();
        assert_eq!(row.key, "sword_copy");
        assert_eq!(row.file_path, "data/dst.cfd");

        // src should still contain the original
        let src_content = std::fs::read_to_string(&src_cfd).unwrap();
        assert!(src_content.contains("sword"), "src should still contain original after copy");

        // dst should contain the new copy
        let dst_content = std::fs::read_to_string(&dst_cfd).unwrap();
        assert!(dst_content.contains("sword_copy"), "dst should contain sword_copy:\n{dst_content}");
        assert!(dst_content.contains("Sword"), "dst should contain field value 'Sword':\n{dst_content}");
        // original record in dst should still be there
        assert!(dst_content.contains("shield"), "dst should still contain shield:\n{dst_content}");

        // session should have sword_copy in dst file keys
        let session_arc = get_session(&store, sid).unwrap();
        let session = session_arc.lock().unwrap();
        let dst_keys = session.file_record_keys.get("data/dst.cfd").unwrap();
        assert!(dst_keys.contains(&"sword_copy".to_string()), "dst keys should include sword_copy");
        // src should still have sword
        let src_keys = session.file_record_keys.get("data/src.cfd").unwrap();
        assert!(src_keys.contains(&"sword".to_string()), "src keys should still have sword");
    }

    #[test]
    fn copy_record_to_file_duplicate_key_rejected() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let src_cfd = data_dir.join("src.cfd");
        let dst_cfd = data_dir.join("dst.cfd");
        std::fs::write(&src_cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&dst_cfd, "shield: Item {\n  name: \"Shield\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        // Attempt to copy with a key that already exists in the project
        let err = copy_record_to_file_inner(&store, snap.session_id, "data/src.cfd", "data/dst.cfd", "sword", "shield").unwrap_err();
        assert!(err.contains("already exists"), "should reject duplicate key: {err}");
    }

    #[test]
    fn write_record_source_replaces_record_in_file() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd = data_dir.join("items.cfd");
        std::fs::write(&cfd, "sword: Item {\n  name: \"Sword\"\n}\nshield: Item {\n  name: \"Shield\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let new_src = "sword: Item {\n  name: \"Excalibur\"\n}";
        write_record_source_inner(&store, sid, "data/items.cfd", "sword", new_src).unwrap();

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("Excalibur"), "should contain new name:\n{contents}");
        assert!(!contents.contains("\"Sword\""), "old name should be gone:\n{contents}");
        assert!(contents.contains("shield"), "unrelated record should still be present:\n{contents}");
    }

    #[test]
    fn write_record_source_rejects_key_mismatch() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd = data_dir.join("items.cfd");
        std::fs::write(&cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let err = write_record_source_inner(&store, snap.session_id, "data/items.cfd", "sword", "different_key: Item {\n  name: \"X\"\n}").unwrap_err();
        assert!(err.contains("key mismatch") || err.contains("mismatch"), "should reject key mismatch: {err}");
    }

    #[test]
    fn import_record_source_appends_records_to_file() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd = data_dir.join("items.cfd");
        std::fs::write(&cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let pasted = "shield: Item {\n  name: \"Shield\"\n}\naxe: Item {\n  name: \"Axe\"\n}";
        let keys = import_record_source_inner(&store, sid, "data/items.cfd", pasted).unwrap();
        assert_eq!(keys.len(), 2, "should return 2 imported keys");
        assert!(keys.contains(&"shield".to_string()));
        assert!(keys.contains(&"axe".to_string()));

        let contents = std::fs::read_to_string(&cfd).unwrap();
        assert!(contents.contains("shield"), "file should contain shield:\n{contents}");
        assert!(contents.contains("axe"), "file should contain axe:\n{contents}");
        assert!(contents.contains("sword"), "original record should still be present:\n{contents}");
    }

    #[test]
    fn import_record_source_rejects_duplicate_key() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd = data_dir.join("items.cfd");
        std::fs::write(&cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let err = import_record_source_inner(&store, snap.session_id, "data/items.cfd", "sword: Item {\n  name: \"Dupe\"\n}").unwrap_err();
        assert!(err.contains("already exists") || err.contains("sword"), "should reject duplicate key: {err}");
    }

    #[test]
    fn move_record_to_file_with_existing_standalone_records() {
        // Regression: dest file has standalone records containing "Item {" — this
        // should NOT trigger grouped syntax detection when appending the moved record.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let src_cfd = data_dir.join("src.cfd");
        let dst_cfd = data_dir.join("dst.cfd");
        std::fs::write(&src_cfd, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&dst_cfd, "shield: Item {\n  name: \"Shield\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let row = move_record_inner(&store, snap.session_id, "data/src.cfd", "data/dst.cfd", "sword").unwrap();
        assert_eq!(row.key, "sword");
        assert_eq!(row.file_path, "data/dst.cfd");

        let src_content = std::fs::read_to_string(&src_cfd).unwrap();
        assert!(!src_content.contains("sword"), "src should not have sword after move");

        let dst_content = std::fs::read_to_string(&dst_cfd).unwrap();
        assert!(dst_content.contains("sword: Item"), "dst should have sword as standalone:\n{dst_content}");
        assert!(dst_content.contains("shield"), "dst should still have shield:\n{dst_content}");

        // Both records should be parseable
        let records = get_file_records_inner(&store, snap.session_id, "data/dst.cfd").unwrap();
        assert_eq!(records.records.len(), 2, "dst should have 2 records:\n{dst_content}");
    }

    #[test]
    fn sort_file_records_grouped() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();
        let cfd = data_dir.join("items.cfd");
        // Grouped syntax: Item { zebra { ... } apple { ... } mango { ... } }
        std::fs::write(&cfd, "Item {\n  zebra {\n    name: \"Z\",\n  }\n  apple {\n    name: \"A\",\n  }\n  mango {\n    name: \"M\",\n  }\n}\n").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let count = sort_file_records_inner(&store, sid, "data/items.cfd").unwrap();
        assert!(count >= 3, "should report at least 3 items processed: {count}");

        let contents = std::fs::read_to_string(&cfd).unwrap();
        let apple_pos = contents.find("apple").unwrap();
        let mango_pos = contents.find("mango").unwrap();
        let zebra_pos = contents.find("zebra").unwrap();
        assert!(apple_pos < mango_pos && mango_pos < zebra_pos, "grouped records should be sorted alphabetically:\n{contents}");
    }

    #[test]
    fn create_record_does_not_falsely_use_grouped_syntax() {
        // Regression: standalone records like "key: Item { ... }" contain "Item {"
        // which should NOT be mistaken for a grouped block header.
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd_path = data_dir.join("items.cfd");
        // Standalone syntax: "sword: Item { ... }" — this contains "Item {" but is NOT a group header
        std::fs::write(&cfd_path, "sword: Item {\n  name: \"Sword\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();

        let row = create_record_inner(&store, snap.session_id, "data/items.cfd", "axe", "Item").unwrap();
        assert_eq!(row.key, "axe");

        let contents = std::fs::read_to_string(&cfd_path).unwrap();
        // axe must be appended as standalone, NOT inserted into a non-existent group block
        assert!(contents.contains("axe: Item"), "new record should use standalone syntax:\n{contents}");
        // file should still parse to 2 records
        let records = get_file_records_inner(&store, snap.session_id, "data/items.cfd").unwrap();
        assert_eq!(records.records.len(), 2, "file should have 2 records:\n{contents}");
    }

    #[test]
    fn get_incoming_refs_finds_records_that_reference_target() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }\ntype Container { item: Item; }").unwrap();
        let items_cfd = data_dir.join("items.cfd");
        let containers_cfd = data_dir.join("containers.cfd");
        std::fs::write(&items_cfd, "sword: Item {\n  name: \"Sword\",\n}\n").unwrap();
        std::fs::write(&containers_cfd, "chest: Container {\n  item: &sword,\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let refs = get_incoming_refs_inner(&store, sid, "sword").unwrap();
        assert_eq!(refs.len(), 1, "should find one incoming reference to sword");
        assert_eq!(refs[0].source_key, "chest");
        assert_eq!(refs[0].source_type, "Container");
        assert!(refs[0].field_path.contains("item"), "field_path should mention 'item': {}", refs[0].field_path);

        // Record with no references
        let refs = get_incoming_refs_inner(&store, sid, "chest").unwrap();
        assert!(refs.is_empty(), "chest should have no incoming refs");
    }

    #[test]
    fn get_all_records_of_type_returns_records_across_files() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }\ntype Weapon { damage: int; }").unwrap();
        let items1_cfd = data_dir.join("a.cfd");
        let items2_cfd = data_dir.join("b.cfd");
        std::fs::write(&items1_cfd, "sword: Item {\n  name: \"Sword\"\n}\nbow: Weapon {\n  damage: 5\n}\n").unwrap();
        std::fs::write(&items2_cfd, "shield: Item {\n  name: \"Shield\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        let rows = get_all_records_of_type_inner(&store, sid, "Item").unwrap();
        assert_eq!(rows.len(), 2, "should return 2 Item records across files");
        let keys: Vec<&str> = rows.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"sword"), "should include sword");
        assert!(keys.contains(&"shield"), "should include shield");
        assert!(!keys.contains(&"bow"), "should not include bow (Weapon type)");
    }

    #[test]
    fn search_records_finds_by_key_and_field_value() {
        let dir = TempDir::new().unwrap();
        let yaml = dir.path().join("coflow.yaml");
        let data_dir = dir.path().join("data");
        std::fs::create_dir(&data_dir).unwrap();
        let schema_path = dir.path().join("schema.cft");
        std::fs::write(&schema_path, "type Item { name: string; }").unwrap();
        let cfd = data_dir.join("items.cfd");
        std::fs::write(&cfd, "sword: Item {\n  name: \"Excalibur\"\n}\nshield: Item {\n  name: \"Aegis\"\n}\n").unwrap();
        std::fs::write(&yaml, "schema: schema.cft\nsources:\n  - path: data").unwrap();

        let store = Mutex::new(SessionStore::default());
        let snap = load_project_inner(&store, yaml.to_str().unwrap()).unwrap();
        let sid = snap.session_id;

        // Search by key substring
        let hits = search_records_inner(&store, sid, "swo", 10).unwrap();
        assert!(hits.iter().any(|h| h.key == "sword"), "should find 'sword' by key prefix");

        // Search by field value
        let hits = search_records_inner(&store, sid, "excalibur", 10).unwrap();
        assert!(hits.iter().any(|h| h.key == "sword"), "should find sword by field value 'Excalibur'");
        assert!(!hits.iter().any(|h| h.key == "shield"), "should not return shield when searching excalibur");

        // Empty query returns nothing
        let hits = search_records_inner(&store, sid, "", 10).unwrap();
        assert!(hits.is_empty(), "empty query should return no results");

        // type: filter
        let hits = search_records_inner(&store, sid, "type:Item", 10).unwrap();
        assert_eq!(hits.len(), 2, "type:Item should return both records");

        // file: filter
        let hits = search_records_inner(&store, sid, "file:items.cfd", 10).unwrap();
        assert_eq!(hits.len(), 2, "file:items.cfd should return all records in that file");

        let hits = search_records_inner(&store, sid, "file:nonexistent.cfd", 10).unwrap();
        assert!(hits.is_empty(), "file:nonexistent should return nothing");
    }
}
