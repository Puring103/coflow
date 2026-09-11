//! Headless performance smoke test for large editor projects.
//!
//! Generates a synthetic project whose size is controlled by environment
//! variables, loads it through `SessionStore`, and times the editor commands
//! that gate view opening: project load, file records, and graph.
//!
//! ```text
//! PERF_FILES=5 PERF_RECORDS=1000 \
//!   cargo run --release -p cfd-editor-core --example perf_smoke
//! ```
//!
//! `PERF_PROJECT` pins the output directory (defaults to a temp dir);
//! `PERF_KEEP=1` keeps that directory for inspection.

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::print_stdout)]

use cfd_editor_core::editor::{GraphQuery, SessionStore};
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Config {
    files: usize,
    records: usize,
    project_root: PathBuf,
    keep: bool,
}

impl Config {
    fn from_env() -> Self {
        let files = env_usize("PERF_FILES", 5);
        let records = env_usize("PERF_RECORDS", 1000);
        let project_root = std::env::var_os("PERF_PROJECT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!("coflow-perf-{}", std::process::id()))
            });
        let keep = std::env::var_os("PERF_KEEP").is_some();
        Self {
            files,
            records,
            project_root,
            keep,
        }
    }
}

fn env_usize(key: &str, fallback: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn main() {
    let config = Config::from_env();
    if config.project_root.exists() {
        fs::remove_dir_all(&config.project_root).expect("reset perf project");
    }
    generate_project(&config);

    let total_records = config.files * config.records;
    println!(
        "project: {} files x {} records = {} records at {}",
        config.files,
        config.records,
        total_records,
        config.project_root.display()
    );

    breakdown(&config);

    let store = SessionStore::new().expect("session store");
    let (bootstrap, load) = timed(|| {
        store
            .load_project(&config.project_root.join("coflow.yaml"))
            .expect("load project")
    });
    println!("load_project            : {}", format_duration(load));

    let session_id = bootstrap.session_id;
    let file_path = format!("data/{:02}.cfd", 0);

    // Cold file records (first open of a large file).
    let (records, first) = timed(|| {
        store
            .get_file_records(session_id, &file_path)
            .expect("get file records")
    });
    println!(
        "get_file_records cold   : {} ({} rows, {} columns)",
        format_duration(first),
        records.records.len(),
        records.columns.len()
    );

    // Warm file records (same revision; exercises any backend cache).
    let (_, second) = timed(|| {
        store
            .get_file_records(session_id, &file_path)
            .expect("get file records")
    });
    println!("get_file_records warm   : {}", format_duration(second));

    let source = store
        .read_source_text(session_id, &file_path)
        .expect("read source");
    let (_, sync_time) = timed(|| {
        store
            .sync_language_document(session_id, &file_path, &source, 1)
            .expect("sync language document")
    });
    println!("sync_language_document  : {}", format_duration(sync_time));
    let (_, validate_time) = timed(|| {
        store
            .validate_source_text(session_id, &file_path, &source)
            .expect("validate source")
    });
    println!(
        "validate_source_text    : {}",
        format_duration(validate_time)
    );

    let (graph, graph_time) = timed(|| {
        store
            .get_graph(
                session_id,
                &GraphQuery {
                    file_path: file_path.clone(),
                    depth: Some(3),
                    limit: Some(1000),
                },
            )
            .expect("get graph")
    });
    println!(
        "get_graph               : {} ({} nodes, {} edges)",
        format_duration(graph_time),
        graph.nodes.len(),
        graph.edges.len()
    );

    if !config.keep {
        fs::remove_dir_all(&config.project_root).expect("remove perf project");
    } else {
        println!("kept project at {}", config.project_root.display());
    }
}

/// Replicates `build_session`'s runtime stages so load cost can be attributed.
fn breakdown(config: &Config) {
    let yaml_path = config.project_root.join("coflow.yaml");
    let (project, open) = timed(|| {
        coflow_runtime::Project::open_schema_only(Some(&yaml_path)).expect("open project")
    });
    println!("  [breakdown] open      : {}", format_duration(open));

    let (schema_runtime, compile) = timed(|| {
        let mut runtime = coflow_runtime::ProjectRuntime::new(project.clone());
        let _ = runtime.refresh();
        runtime
    });
    println!("  [breakdown] schema    : {}", format_duration(compile));

    let schema = schema_runtime
        .into_latest_attempt()
        .expect("compiled schema");
    let (engine, data) = timed(|| {
        coflow_runtime::Runtime::new()
            .open_write_session_from_schema(schema)
            .expect("write session")
    });
    println!("  [breakdown] data      : {}", format_duration(data));
    drop(engine);

    let (_, lsp) = timed(|| coflow_lsp::EmbeddedLsp::new(project.clone()));
    println!("  [breakdown] lsp setup : {}", format_duration(lsp));
}

fn timed<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = work();
    (value, start.elapsed())
}

fn format_duration(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}

fn generate_project(config: &Config) {
    let data_dir = config.project_root.join("data");
    let schema_dir = config.project_root.join("schema");
    fs::create_dir_all(&data_dir).expect("create data dir");
    fs::create_dir_all(&schema_dir).expect("create schema dir");
    fs::write(config.project_root.join("coflow.yaml"), PROJECT_YAML).expect("write config");
    fs::write(schema_dir.join("schema.cft"), SCHEMA).expect("write schema");
    for file_index in 0..config.files {
        let text = generate_data_file(config, file_index);
        fs::write(data_dir.join(format!("{file_index:02}.cfd")), text).expect("write data file");
    }
}

const PROJECT_YAML: &str = "\
schema: schema
data:
  - data/
codegen:
  - language: csharp
    dir: generated/csharp
";

const SCHEMA: &str = "\
@struct
sealed type Vec2 {
  x: float = 0.0;
  y: float = 0.0;
}

@struct
sealed type Stats {
  health: int = 100;
  speed: float = 1.0;
  resistances: {string: float} = {};
}

enum Rarity {
  Common,
  Rare,
  Epic,
}

type Entity {
  name: string;
  level: int = 1;
  rarity: Rarity = Common;
  tags: [string] = [];
  position: Vec2 = Vec2 {};
  stats: Stats = Stats {};
  related: [&Entity] = [];
  parent: Option<&Entity> = None;
  notes: {string: string} = {};
  description: string;
  weight: float = 1.0;
  enabled: bool = true;
  bonus: Stats = Stats {};
  optionalBonus: Option<Stats> = None;
  history: [Stats] = [];
  lookup: {string: Stats} = {};
  matrix: [[int]] = [];
  rating: Rarity = Rare;
  secondaryParent: Option<&Entity> = None;
  flags: [string] = [];
}

type Item {
  name: string;
  value: int = 0;
  owner: &Entity;
  tags: [string] = [];
  stats: Stats = Stats {};
}
";

fn generate_data_file(config: &Config, file_index: usize) -> String {
    let mut text = String::new();
    for record_index in 0..config.records {
        let global = file_index * config.records + record_index;
        let key = format!("e{global:05}");
        // Cross-file refs keep the graph connected while staying acyclic-ish
        // and always resolvable.
        let ref_a = format!("e{:05}", (global + 7) % (config.files * config.records));
        let ref_b = format!("e{:05}", (global + 31) % (config.files * config.records));
        let parent = format!("e{:05}", global.saturating_sub(3));
        let rarity = ["Common", "Rare", "Epic"][global % 3];
        writeln!(text, "{key}: Entity {{").expect("write entity");
        writeln!(text, "  name: \"Entity {global}\",").expect("write name");
        writeln!(text, "  level: {},", (global % 100) + 1).expect("write level");
        writeln!(text, "  rarity: {rarity},").expect("write rarity");
        writeln!(text, "  tags: [\"alpha\", \"beta\", \"gamma\"],").expect("write tags");
        writeln!(
            text,
            "  position: Vec2 {{ x: {}.0, y: {}.0 }},",
            global % 50,
            global % 37
        )
        .expect("write position");
        writeln!(
            text,
            "  stats: Stats {{ health: {}, speed: 1.{} }},",
            (global % 900) + 100,
            global % 10
        )
        .expect("write stats");
        writeln!(text, "  related: [&{ref_a}, &{ref_b}],").expect("write related");
        if global >= 3 {
            writeln!(text, "  parent: &{parent},").expect("write parent");
        }
        writeln!(
            text,
            "  notes: {{ \"origin\": \"file-{file_index}\", \"index\": \"{record_index}\" }},"
        )
        .expect("write notes");
        writeln!(text, "  description: \"Entity {global} description\",")
            .expect("write description");
        writeln!(text, "  weight: {}.5,", global % 100).expect("write weight");
        writeln!(text, "  enabled: {},", global % 2 == 0).expect("write enabled");
        writeln!(
            text,
            "  bonus: Stats {{ health: {}, speed: 2.0 }},",
            global % 50
        )
        .expect("write bonus");
        writeln!(text, "  optionalBonus: Stats {{ health: 5, speed: 0.1 }},")
            .expect("write optional bonus");
        writeln!(
            text,
            "  history: [Stats {{ health: 1 }}, Stats {{ health: 2 }}],"
        )
        .expect("write history");
        writeln!(
            text,
            "  lookup: {{ \"a\": Stats {{ health: 3 }}, \"b\": Stats {{ health: 4 }} }},"
        )
        .expect("write lookup");
        writeln!(text, "  matrix: [[1, 2], [3, 4]],").expect("write matrix");
        writeln!(text, "  rating: {rarity},").expect("write rating");
        writeln!(text, "  secondaryParent: &{ref_a},").expect("write secondary parent");
        writeln!(text, "  flags: [\"a\", \"b\", \"c\"],").expect("write flags");
        writeln!(text, "}}\n").expect("close entity");
    }
    // A handful of items per file referencing the entities above.
    for item_index in 0..(config.records / 10).max(1) {
        let global = file_index * config.records + item_index * 10;
        let owner = format!("e{global:05}");
        writeln!(text, "item{item_index:05}: Item {{").expect("write item");
        writeln!(text, "  name: \"Item {file_index}-{item_index}\",").expect("write item name");
        writeln!(text, "  value: {},", item_index * 3).expect("write item value");
        writeln!(text, "  owner: &{owner},").expect("write item owner");
        writeln!(text, "  tags: [\"loot\"],").expect("write item tags");
        writeln!(text, "  stats: Stats {{ health: 10, speed: 0.5 }},").expect("write item stats");
        writeln!(text, "}}\n").expect("close item");
    }
    text
}
