//! 同一源码同时编译 baseline/after；独立临时工程，不启动编辑器。
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#[cfg(windows)]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use coflow_language::cft::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use coflow_runtime::{
    CfdDataModel, CfdPathSegment, CfdValue, DefaultMaterialization, DimensionValueCoordinate,
    DimensionValueExpectation, LoadedValueDraft, MutationFields, MutationOp, MutationRequest,
    MutationValue, Project, RecordCoordinate, Runtime, WriteProjectSession,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _, hint::black_box, time::Instant};

fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn compile(source: &str) -> CftSchema {
    let parsed = parse_modules([CftFile::from_source(ModuleId::from("bench"), source)]);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    build_schema(&parsed).expect("valid fixture schema")
}
fn measure<T>(
    metric: &str,
    fixture: Value,
    samples: usize,
    mut run: impl FnMut(usize) -> T,
    mut verify: impl FnMut(&T, usize),
) -> T {
    let mut times = Vec::new();
    let mut last = None;
    for sample in 0..=samples {
        drop(last.take());
        let start = Instant::now();
        let result = black_box(run(sample));
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        verify(&result, sample);
        if sample > 0 {
            times.push(elapsed);
        }
        last = Some(result);
    }
    println!(
        "{}",
        json!({"metric":metric,"fixture":fixture,"warmups":1,
        "samples_ms":times,"verification":"passed"})
    );
    last.unwrap()
}
fn coordinate(id: impl AsRef<str>) -> RecordCoordinate {
    RecordCoordinate::try_new("Row", id.as_ref()).unwrap()
}
fn field(name: &str) -> Vec<CfdPathSegment> {
    vec![CfdPathSegment::Field(name.into())]
}
fn set(id: &str, name: &str, value: CfdValue) -> MutationOp {
    MutationOp::SetField {
        record: coordinate(id),
        file: None,
        path: field(name),
        value: MutationValue::Cfd(value),
    }
}
fn insert(id: String) -> MutationOp {
    MutationOp::InsertRecord {
        file: "data/0000.cfd".into(),
        actual_type: "Row".into(),
        id,
        fields: MutationFields::Cfd(BTreeMap::from([("number".into(), CfdValue::Int(7))])),
        materialization: DefaultMaterialization::Minimal,
    }
}
fn schema_cases(samples: usize) {
    for kind in ["primitive", "template", "alias_inheritance"] {
        let mut source = String::new();
        if kind == "alias_inheritance" {
            source.push_str("type Numbers = [int]; data Node { value: int = 0; }\n");
        }
        for index in 0..200 {
            if kind == "alias_inheritance" {
                writeln!(
                    source,
                    "table Base{index} {{ seed: int = 1; }} table Row{index}: Base{index} {{"
                )
                .unwrap();
            } else {
                writeln!(source, "table Row{index} {{ seed: int = 1;").unwrap();
            }
            for f in 0..12 {
                match kind {
                    "primitive" => writeln!(source, "f{f}: int = {f};").unwrap(),
                    "template" => {
                        writeln!(source, "f{f}: string = f\"Item {{self.seed}} field {f}\";")
                            .unwrap()
                    }
                    _ => match f % 3 {
                        0 => writeln!(source, "f{f}: Option<{{string: [Node]}}> = None;").unwrap(),
                        1 => writeln!(source, "f{f}: Numbers = [1,2,3];").unwrap(),
                        _ => writeln!(source, "f{f}: Node = Node {{}};").unwrap(),
                    },
                }
            }
            source.push_str("}\n");
        }
        measure(
            &format!("schema_compile_{kind}"),
            json!({"types":200,"fields_per_type":13,
            "bytes":source.len(),"sha256":digest(&source)}),
            samples,
            |_| compile(&source),
            |schema, _| assert!(schema.resolve_type("Row199").is_some()),
        );
    }
}
fn model_cases(samples: usize) {
    for kind in ["scalar", "array", "template"] {
        let records = 5_000;
        let mut source = String::from("table Row { seed: int = 1;\n");
        match kind {
            "scalar" => {
                for f in 0..16 {
                    writeln!(source, "f{f}: int = {f};").unwrap();
                }
            }
            "array" => {
                writeln!(
                    source,
                    "values: [int] = [{}];",
                    (0..128)
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
            }
            _ => {
                for f in 0..8 {
                    writeln!(source, "f{f}: string = f\"Item {{self.seed}} field {f}\";").unwrap();
                }
            }
        }
        source.push_str("}\n");
        let schema = compile(&source);
        let mut times = Vec::new();
        let mut last = None;
        for sample in 0..=samples {
            drop(last.take());
            let mut builder = CfdDataModel::builder(&schema);
            for n in 0..records {
                builder.add_record(
                    format!("r{n}"),
                    "Row",
                    std::iter::empty::<(String, LoadedValueDraft)>(),
                );
            }
            let start = Instant::now();
            let model = black_box(builder.build().expect("default model"));
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(model.record_count(), records);
            for (_, row) in model.records() {
                assert_eq!(row.field("seed"), Some(&CfdValue::Int(1)));
                match kind {
                    "scalar" => assert_eq!(row.field("f15"), Some(&CfdValue::Int(15))),
                    "array" => assert_eq!(
                        row.field("values"),
                        Some(&CfdValue::Array((0..128).map(CfdValue::Int).collect()))
                    ),
                    _ => assert!(matches!(
                        row.field("f7"),
                        Some(CfdValue::FormattedString(_))
                    )),
                }
            }
            if sample > 0 {
                times.push(elapsed);
            }
            last = Some(model);
        }
        let model = last.unwrap();
        let first = model.records().next().unwrap().1;
        println!(
            "{}",
            json!({"metric":format!("model_default_{kind}"),"fixture":{
            "records":records,"schema_sha256":digest(&source),"schema_bytes":source.len()},
            "warmups":1,"samples_ms":times,"verification":"all record values checked",
            "first_record_fields_sha256":digest(serde_json::to_vec(first.fields()).unwrap())})
        );
    }
}
fn fixture(schema: &str, records: usize, files: usize) -> (tempfile::TempDir, Project, String) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    std::fs::write(root.path().join("schema.cft"), schema).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(schema.as_bytes());
    for f in 0..files {
        let mut source = String::new();
        for row in (f * (records / files))..((f + 1) * (records / files)) {
            writeln!(source, "r{row}: Row {{ number: {row} }}").unwrap();
        }
        hasher.update(source.as_bytes());
        std::fs::write(root.path().join(format!("data/{f:04}.cfd")), source).unwrap();
    }
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    (root, project, format!("{:x}", hasher.finalize()))
}
fn cold_case(samples: usize) {
    let schema="data Node { count: int = 1; values: [int] = [1,2,3,4,5,6,7,8]; } table Row { number: int; node: Node = Node {}; title: string = f\"Row {self.number}\"; }";
    let (_root, project, hash) = fixture(schema, 5000, 50);
    let loaded = measure(
        "new_read_session_defaults",
        json!({"records":5000,"files":50,"sha256":hash}),
        samples,
        |_| {
            Runtime::new()
                .open_read_only_session(project.clone())
                .unwrap()
        },
        |session, _| {
            assert_eq!(session.queries().record_count(), 5000);
            assert!(!session.queries().has_diagnostics());
            assert_eq!(
                session
                    .queries()
                    .field_value("Row", "r4999", &field("number")),
                Some(&CfdValue::Int(4999))
            );
        },
    );
    assert!(matches!(
        loaded.queries().field_value("Row", "r1", &field("node")),
        Some(CfdValue::Object(_))
    ));
}
fn verify_reload(session: &WriteProjectSession, project: &Project) {
    let fresh = Runtime::new()
        .open_read_only_session(project.clone())
        .unwrap();
    assert!(!fresh.queries().has_diagnostics());
    assert_eq!(
        session.queries().record_count(),
        fresh.queries().record_count()
    );
    for file in session.queries().source_files() {
        let current = session
            .queries()
            .record_views_in_file(file)
            .collect::<Vec<_>>();
        let reloaded = fresh
            .queries()
            .record_views_in_file(file)
            .collect::<Vec<_>>();
        assert_eq!(current.len(), reloaded.len());
        for (left, right) in current.iter().zip(reloaded) {
            assert_eq!(left.record, right.record);
        }
    }
}
fn mutation_cases(samples: usize) {
    let mut schema = String::from("table Row { number: int; @localized name: string = \"Name\"; ");
    for f in 0..18 {
        write!(schema, "f{f}: int = 0; ").unwrap();
    }
    schema.push('}');
    for metric in [
        "set_scalar",
        "batch_same_file_1000",
        "batch_cross_file_1000",
        "insert_one",
        "insert_batch_10",
        "dimension_set",
    ] {
        let records = if metric.starts_with("insert") {
            5000
        } else {
            10000
        };
        let files = if metric.starts_with("insert") { 1 } else { 10 };
        let (_root, project, hash) = fixture(&schema, records, files);
        let mut session = Runtime::new().open_write_session(project.clone()).unwrap();
        let operations = if metric.starts_with("batch_") {
            1000
        } else if metric == "insert_batch_10" {
            10
        } else {
            1
        };
        let mut times = Vec::new();
        for sample in 0..=samples {
            let value = (sample + 1) as i64;
            let ops = match metric {
                "set_scalar" => vec![set("r0", "f0", CfdValue::Int(value))],
                "batch_same_file_1000" => (0..1000)
                    .map(|i| set(&format!("r{i}"), "f0", CfdValue::Int(value)))
                    .collect(),
                "batch_cross_file_1000" => (0..1000)
                    .map(|i| {
                        set(
                            &format!("r{}", (i % 10) * 1000 + i / 10),
                            "f0",
                            CfdValue::Int(value),
                        )
                    })
                    .collect(),
                "insert_one" => vec![insert(format!("new{sample}"))],
                "insert_batch_10" => (0..10)
                    .map(|i| insert(format!("new{sample}_{i}")))
                    .collect(),
                _ => vec![MutationOp::SetDimensionValue {
                    file: Some("data/ja.cfd".into()),
                    coordinate: DimensionValueCoordinate {
                        actual_type: "Row".try_into().unwrap(),
                        record_id: "r0".try_into().unwrap(),
                        field: "name".try_into().unwrap(),
                        dimension: "language".try_into().unwrap(),
                        variant: "ja".try_into().unwrap(),
                        path: vec![],
                    },
                    expected: DimensionValueExpectation::Any,
                    value: MutationValue::Cfd(CfdValue::String(format!("Name{sample}"))),
                }],
            };
            let start = Instant::now();
            let report = black_box(session.apply_mutation(MutationRequest {
                stop_on_write_error: true,
                ops,
            }));
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert!(
                report.write_ok && report.check_ok && report.failed.is_empty(),
                "{report:?}"
            );
            assert_eq!(report.applied.len(), operations);
            if metric.starts_with("insert") {
                assert_eq!(
                    session.queries().record_count(),
                    records + (sample + 1) * operations
                );
            }
            if metric == "set_scalar" || metric.starts_with("batch_") {
                for index in 0..records {
                    let changed = match metric {
                        "set_scalar" => index == 0,
                        "batch_same_file_1000" => index < 1000,
                        _ => index % 1000 < 100,
                    };
                    assert_eq!(
                        session
                            .queries()
                            .field_value("Row", &format!("r{index}"), &field("f0")),
                        Some(&CfdValue::Int(if changed { value } else { 0 }))
                    );
                }
            }
            if metric.starts_with("insert") {
                for row in 0..operations {
                    let id = if operations == 1 {
                        format!("new{sample}")
                    } else {
                        format!("new{sample}_{row}")
                    };
                    assert_eq!(
                        session.queries().field_value("Row", &id, &field("number")),
                        Some(&CfdValue::Int(7))
                    );
                }
            }
            if metric == "dimension_set" {
                let coordinate = DimensionValueCoordinate {
                    actual_type: "Row".try_into().unwrap(),
                    record_id: "r0".try_into().unwrap(),
                    field: "name".try_into().unwrap(),
                    dimension: "language".try_into().unwrap(),
                    variant: "ja".try_into().unwrap(),
                    path: vec![],
                };
                assert_eq!(
                    session
                        .queries()
                        .dimension_value(&coordinate)
                        .unwrap()
                        .state,
                    coflow_runtime::DimensionValueState::Value(CfdValue::String(format!(
                        "Name{sample}"
                    )))
                );
            }
            if sample > 0 {
                times.push(elapsed);
            }
        }
        verify_reload(&session, &project);
        println!(
            "{}",
            json!({"metric":metric,"fixture":{"records":records,"files":files,"fields":20,"sha256":hash},
            "operations_per_sample":operations,"samples_ms":times,"warmups":1,"verification":"successful report and independent full reload of all records"})
        );
    }
}
fn nested_case(samples: usize) {
    let (_root, project, _) = fixture("table Row { number: int; values: [int]; }", 1, 1);
    let source = format!(
        "r0: Row {{ number: 0, values: [{}] }}",
        vec!["0"; 100_000].join(",")
    );
    std::fs::write(project.root_dir().join("data/0000.cfd"), &source).unwrap();
    let mut session = Runtime::new().open_write_session(project.clone()).unwrap();
    let mut times = Vec::new();
    for sample in 0..=samples {
        let ops = (0..1000)
            .map(|i| MutationOp::SetField {
                record: coordinate("r0"),
                file: None,
                path: vec![
                    CfdPathSegment::Field("values".into()),
                    CfdPathSegment::Index(i),
                ],
                value: MutationValue::Cfd(CfdValue::Int((sample + 1) as i64)),
            })
            .collect();
        let start = Instant::now();
        let result = black_box(session.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops,
        }));
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(result.write_ok && result.check_ok, "{result:?}");
        if sample > 0 {
            times.push(elapsed);
        }
    }
    assert_eq!(
        session.queries().field_value("Row", "r0", &field("values")),
        Some(&CfdValue::Array(
            (0..100_000)
                .map(|i| CfdValue::Int(if i < 1000 { (samples + 1) as i64 } else { 0 }))
                .collect()
        ))
    );
    verify_reload(&session, &project);
    println!(
        "{}",
        json!({"metric":"nested_array_1000","fixture":{"array_elements":100000,"source_sha256":digest(source)},"operations_per_sample":1000,
        "samples_ms":times,"warmups":1,"verification":"all elements and independent full reload"})
    );
}
fn main() {
    let samples = std::env::var("CORE_SAMPLES")
        .unwrap_or_else(|_| "3".into())
        .parse::<usize>()
        .unwrap();
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    println!(
        "{}",
        json!({"mode":mode,"samples":samples,"allocator":"mimalloc on Windows",
        "timing":"fixture and verification outside; returned result destruction outside; internal destruction and publication inside"})
    );
    if mode == "all" || mode == "schema" {
        schema_cases(samples);
    }
    if mode == "all" || mode == "model" {
        model_cases(samples);
    }
    if mode == "all" || mode == "cold" {
        cold_case(samples);
    }
    if mode == "all" || mode == "mutation" {
        mutation_cases(samples);
    }
    if mode == "all" || mode == "nested" {
        nested_case(samples);
    }
}
