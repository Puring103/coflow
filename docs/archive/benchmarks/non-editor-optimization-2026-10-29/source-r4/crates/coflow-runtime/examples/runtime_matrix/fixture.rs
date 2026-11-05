use coflow_runtime::{Project, Runtime, WriteProjectSession};
use serde_json::json;
use std::fmt::Write as _;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub records: usize,
    pub files: usize,
    pub fields: usize,
    pub samples: usize,
    pub warmups: usize,
    pub batch: usize,
    pub collection: usize,
    pub variants: usize,
    pub stride: usize,
    pub topology: String,
    pub templates: bool,
    pub query_batch: usize,
}

pub fn number(name: &str, default: usize) -> usize {
    std::env::var(name).map_or(default, |value| value.parse().expect("integer parameter"))
}

impl Config {
    pub fn read() -> Self {
        let config = Self {
            records: number("MATRIX_RECORDS", 1_000),
            files: number("MATRIX_FILES", 10),
            fields: number("MATRIX_FIELDS", 20),
            samples: number("MATRIX_SAMPLES", 3),
            warmups: number("MATRIX_WARMUPS", 1),
            batch: number("MATRIX_BATCH", 100),
            collection: number("MATRIX_COLLECTION", 8),
            variants: number("MATRIX_VARIANTS", 0),
            stride: number("MATRIX_STRIDE", 100),
            topology: std::env::var("MATRIX_TOPOLOGY").unwrap_or_else(|_| "ring".into()),
            templates: number("MATRIX_TEMPLATES", 1) != 0,
            query_batch: number("MATRIX_QUERY_BATCH", 1_000),
        };
        assert!(
            config.records >= 2 && config.files > 0 && config.records.is_multiple_of(config.files)
        );
        assert!(config.fields >= 8 && config.samples > 0 && config.stride > 0);
        assert!(config.collection > 0 && config.query_batch > 0);
        assert!(config.batch > 0 && config.batch <= config.records);
        assert!(matches!(config.topology.as_str(), "ring" | "star"));
        config
    }

    pub fn metadata(&self) -> serde_json::Value {
        json!({"records":self.records,"files":self.files,"fields":self.fields,
            "samples":self.samples,"warmups":self.warmups,"batch":self.batch,
            "collection_length":self.collection,"variants":self.variants,
            "dimension_stride":self.stride,"topology":self.topology,
            "templates":self.templates,"query_batch":self.query_batch})
    }
}

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub project: Project,
    pub bytes: usize,
}

impl Fixture {
    pub fn new(config: &Config) -> Self {
        let root = tempfile::tempdir().expect("isolated matrix project");
        std::fs::create_dir(root.path().join("data")).unwrap();
        std::fs::create_dir_all(root.path().join("dimensions/language")).unwrap();
        std::fs::create_dir_all(root.path().join("dimensions/platform")).unwrap();
        let mut schema = String::from("data Stats { health: int = 100; speed: float = 1.0; }\ntable Row {\nvalue: int = 0;\n@localized name: string;\n@dimension(\"platform\") caption: string;\nparent: &Row;\nnumbers: [int];\nmetadata: {string: string};\noptional: Option<int> = None;\nstats: Stats;\n");
        for field in 8..config.fields {
            writeln!(schema, "f{field}: int = 0;").unwrap();
        }
        schema.push_str("check Valid { value >= 0; name != \"\"; parent.value >= 0; all n in numbers { n >= 0; } }\n}\ncheck Integrity { records(Row).len() > 0; }\n");
        std::fs::write(root.path().join("schema.cft"), schema).unwrap();
        std::fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data/\ncodegen: [{language: csharp, dir: generated}]\ndimensions:\n  language: {dir: dimensions/language}\n  platform: {dir: dimensions/platform}\n").unwrap();
        let per_file = config.records / config.files;
        let mut bytes = 0;
        let numbers = (0..config.collection)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        for file in 0..config.files {
            let mut text = String::new();
            for local in 0..per_file {
                let id = file * per_file + local;
                let target = if config.topology == "star" {
                    0
                } else {
                    (id + 1) % config.records
                };
                let caption = if config.templates {
                    "f\"{self.parent.name}\"".to_owned()
                } else {
                    format!("\"Caption {id}\"")
                };
                write!(text, "r{id:06}: Row {{ value: 0, name: \"Row {id}\", caption: {caption}, parent: &r{target:06}, numbers: [{numbers}], metadata: {{\"a\": \"base\"}}, optional: Some(1), stats: Stats {{health: 100, speed: 1.0}}").unwrap();
                for field in 8..config.fields {
                    write!(text, ", f{field}: {field}").unwrap();
                }
                text.push_str(" }\n");
            }
            bytes += text.len();
            std::fs::write(root.path().join(format!("data/{file:04}.cfd")), text).unwrap();
        }
        // 两个语义维度分别保存覆盖；稀疏度与变体数独立控制，不改变基础记录数量。
        for (dimension, field) in [("language", "name"), ("platform", "caption")] {
            let mut text = String::new();
            for variant in 0..config.variants {
                writeln!(text, "{dimension}[v{variant}] {{").unwrap();
                for id in (0..config.records).step_by(config.stride) {
                    writeln!(
                        text,
                        "r{id:06}: Row {{ {field}: \"{dimension} {variant} {id}\" }}"
                    )
                    .unwrap();
                }
                text.push_str("}\n");
            }
            if !text.is_empty() {
                bytes += text.len();
                std::fs::write(
                    root.path()
                        .join(format!("dimensions/{dimension}/variants.cfd")),
                    text,
                )
                .unwrap();
            }
        }
        let project = Project::open_schema_only(Some(root.path())).expect("project");
        Self {
            root,
            project,
            bytes,
        }
    }

    pub fn session(&self) -> WriteProjectSession {
        let session = Runtime::new()
            .open_write_session(self.project.clone())
            .expect("session");
        assert!(
            !session.queries().has_diagnostics(),
            "{:?}",
            session.queries().diagnostics()
        );
        session
    }

    pub fn source(&self) -> PathBuf {
        self.root.path().join("data/0000.cfd")
    }
}
