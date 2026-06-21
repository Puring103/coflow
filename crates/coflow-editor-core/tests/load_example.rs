use std::path::PathBuf;

use coflow_editor_core::SessionStore;

fn example_yaml() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("cfd")
        .join("coflow.yaml")
}

#[test]
fn load_example_project() {
    let store = SessionStore::new();
    let snapshot = store.load_project(&example_yaml()).unwrap();
    assert!(!snapshot.file_tree.is_empty(), "file tree should not be empty");
    let session_id = snapshot.session_id;
    println!("Diagnostics ({}):", snapshot.diagnostics.len());
    for d in &snapshot.diagnostics {
        println!("  [{}] {} ({})", d.severity, d.message, d.code);
    }
    println!("File tree:");
    fn dump(n: &coflow_editor_core::FileTreeNode, indent: usize) {
        println!("{}{} (dir={}, src={})", "  ".repeat(indent), n.path, n.is_dir, n.in_sources);
        for c in &n.children { dump(c, indent + 1); }
    }
    for n in &snapshot.file_tree { dump(n, 0); }

    // Find first .cfd file under sources
    let mut paths = Vec::new();
    fn walk(node: &coflow_editor_core::FileTreeNode, out: &mut Vec<String>) {
        if !node.is_dir && node.in_sources {
            out.push(node.path.clone());
        }
        for c in &node.children {
            walk(c, out);
        }
    }
    for n in &snapshot.file_tree {
        walk(n, &mut paths);
    }
    assert!(!paths.is_empty(), "expected at least one source .cfd file");

    let file = &paths[0];
    let records = store.get_file_records(session_id, file).unwrap();
    println!("File: {} types={:?} records={}", file, records.type_names, records.records.len());
    assert!(!records.records.is_empty());

    let first_key = &records.records[0].key;
    let record = store.get_record(session_id, file, first_key).unwrap();
    println!("Record `{}` fields={}", first_key, record.fields.len());

    let graph = store.get_graph(session_id, file).unwrap();
    println!("Graph nodes={} edges={}", graph.nodes.len(), graph.edges.len());
    assert!(!graph.nodes.is_empty());

    // Also try the rpg example which uses Excel.
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rpg_yaml = std::path::PathBuf::from(manifest)
        .parent().unwrap().parent().unwrap()
        .join("examples").join("rpg").join("coflow.yaml");
    println!("\n=== RPG (excel) project ===");
    let rpg_snap = store.load_project(&rpg_yaml).unwrap();
    println!("File tree:");
    fn dump2(n: &coflow_editor_core::FileTreeNode, indent: usize) {
        println!("{}{} (dir={}, src={})", "  ".repeat(indent), n.path, n.is_dir, n.in_sources);
        for c in &n.children { dump2(c, indent + 1); }
    }
    for n in &rpg_snap.file_tree { dump2(n, 0); }
    println!("Diagnostics ({}):", rpg_snap.diagnostics.len());
    for d in rpg_snap.diagnostics.iter().take(5) {
        println!("  [{}] {} ({})", d.severity, d.message, d.code);
    }

    // Try reading the xlsx file
    let xlsx_recs = store
        .get_file_records(rpg_snap.session_id, "data/rpg.xlsx")
        .unwrap();
    println!("rpg.xlsx types={:?} records={}",
        xlsx_recs.type_names, xlsx_recs.records.len());
    if let Some(r) = xlsx_recs.records.first() {
        println!("  first record key={} type={} fields={}",
            r.key, r.actual_type, r.fields.len());
    }
    assert!(!xlsx_recs.records.is_empty(), "xlsx should have records");

    let xlsx_graph = store.get_graph(rpg_snap.session_id, "data/rpg.xlsx").unwrap();
    println!("rpg.xlsx graph nodes={} edges={}", xlsx_graph.nodes.len(), xlsx_graph.edges.len());

    // progression.cfd lives under sources/data; verify it loads with Stage records.
    let prog = store
        .get_file_records(rpg_snap.session_id, "data/cfd/progression.cfd")
        .unwrap();
    let stage_count = prog.records.iter().filter(|r| r.actual_type == "Stage").count();
    assert_eq!(stage_count, 3, "expected 3 Stage records in progression.cfd, got {stage_count}");

    // Inspect the first Item record to confirm featured_stage resolved
    let xlsx_recs2 = store.get_file_records(rpg_snap.session_id, "data/rpg.xlsx").unwrap();
    let healing = xlsx_recs2.records.iter().find(|r| r.key == "healing_potion").unwrap();
    let fs = healing.fields.iter().find(|f| f.name == "featured_stage").unwrap();
    println!("healing_potion.featured_stage = {:?}", fs.value);

    println!("rpg diagnostics dump ({}):", rpg_snap.diagnostics.len());
    for d in &rpg_snap.diagnostics {
        println!("  [{}] {} ({}/{})", d.severity, d.message, d.code, d.stage);
    }

    // Try every source file
    for path in &paths {
        println!("--- {path} ---");
        let recs = store.get_file_records(session_id, path).unwrap();
        println!("  types={:?} records={}", recs.type_names, recs.records.len());
        for r in &recs.records {
            for f in &r.fields {
                let _ = serde_json::to_string(&f.value).unwrap();
            }
        }
        let g = store.get_graph(session_id, path).unwrap();
        println!("  graph nodes={} edges={}", g.nodes.len(), g.edges.len());
    }
}
