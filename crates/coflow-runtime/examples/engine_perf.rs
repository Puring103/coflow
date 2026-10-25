//! 核心引擎基准：只在独立临时目录运行，不启动编辑器。
//! `ENGINE_MODE=model|pipeline，其他参数见归档测量报告`。

#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

// 默认与 Windows CLI/桌面宿主一致；显式 feature 用于隔离分配器的对照实验。
#[cfg(all(windows, not(feature = "engine-perf-system")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};
use coflow_runtime::{
    CfdDataModel, CfdPathSegment, CfdValue, DataSourceTextOverride, LoadedValueDraft, MutationOp,
    MutationRequest, MutationValue, Project, RecordCoordinate, Runtime,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name).map_or(default, |value| value.parse().expect("integer parameter"))
}

// 逐样本保存原始时间；返回值及前次结果析构不计时，闭包内部临时值析构计时。
// 预热一次，分位数可由原始样本重算；前次结果在下一次计时前释放。
fn measure<T>(name: &str, samples: usize, mut operation: impl FnMut() -> T) -> T {
    drop(operation());
    let mut times = Vec::with_capacity(samples);
    let mut last = None;
    for _ in 0..samples {
        drop(last.take());
        let start = Instant::now();
        let result = black_box(operation());
        times.push(start.elapsed().as_secs_f64() * 1000.0);
        last = Some(result);
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{}",
        json!({"metric": name, "samples_ms": times,
        "median_ms": sorted[samples / 2], "min_ms": sorted[0], "max_ms": sorted[samples - 1],
        "p95_ms": (samples >= 20).then(|| sorted[(samples * 95).div_ceil(100) - 1])})
    );
    last.unwrap()
}

fn compile(source: &str) -> coflow_language::cft::CftSchema {
    let parsed = parse_modules([CftFile::from_source(ModuleId::from("perf.cft"), source)]);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    build_schema(&parsed).expect("valid benchmark schema")
}

fn main() {
    let samples = env_usize("ENGINE_SAMPLES", 7);
    assert!(samples > 0);
    let mode = std::env::var("ENGINE_MODE").unwrap_or_else(|_| "pipeline".to_owned());
    println!(
        "{}",
        json!({"mode": mode, "samples": samples, "warmups": 1,
        "allocator": if cfg!(all(windows, not(feature = "engine-perf-system"))) { "mimalloc" } else { "system" },
        "timing": "closure body and internal drops included; returned output drop excluded; previous output dropped before next sample"})
    );
    match mode.as_str() {
        "pipeline" => pipeline(samples),
        "model" => model(samples),
        _ => panic!("unknown ENGINE_MODE"),
    }
}

fn model(samples: usize) {
    println!(
        "{}",
        json!({"sizes_bytes": {
        "CfdValue": size_of::<CfdValue>(), "LoadedValueDraft": size_of::<LoadedValueDraft>(),
        "CfdRecord": size_of::<coflow_runtime::CfdRecord>(),
        "CfdObject": size_of::<coflow_runtime::CfdObject>(),
        "CfdDataModel": size_of::<CfdDataModel>(),
        "CfdAstValue": size_of::<coflow_language::cfd::CfdValue>()}})
    );
    let records = env_usize("ENGINE_RECORDS", 2_000);
    assert!(records > 0);
    for fields in [8, 32, 128] {
        let mut schema_source = String::from("table Row {\n");
        for field in 0..fields {
            writeln!(schema_source, "f{field}: int;").unwrap();
        }
        schema_source.push_str("}\n");
        let schema = compile(&schema_source);
        // 模型构建只计 builder.build，输入构造在计时区间外。
        let mut times = Vec::new();
        let mut latest = None;
        for sample in 0..=samples {
            drop(latest.take());
            let mut builder = CfdDataModel::builder(&schema);
            for record in 0..records {
                builder.add_record(
                    format!("r{record}"),
                    "Row",
                    (0..fields).map(|field| {
                        (
                            format!("f{field}"),
                            LoadedValueDraft::from(i64::from(field)),
                        )
                    }),
                );
            }
            let start = Instant::now();
            let built = black_box(builder.build().expect("valid model"));
            if sample > 0 {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            assert_eq!(built.record_count(), records);
            latest = Some(built);
        }
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({"metric":"model_build", "records":records,"fields":fields,
            "samples_ms":times,"median_ms":sorted[samples/2]})
        );
        let model = latest.unwrap();
        measure(&format!("model_clone_10000_f{fields}"), samples, || {
            for _ in 0..10_000 {
                black_box(model.clone());
            }
        });
    }
    dimension_models(samples, records);
    // 同时测正确文本和错误密集文本，覆盖用户输入期间的恢复路径。
    // 固定错误条目上限，使恢复工作量与归档基线保持可比较。
    let broken_limit = env_usize("ENGINE_BROKEN_RECORDS_MAX", 1_000);
    for count in [100, 1_000, 5_000] {
        for broken in [false, true] {
            if broken && count > broken_limit {
                continue;
            }
            let source = (0..count)
                .map(|i| {
                    // `?` 是合法的 schema-free 标量，必须缺少值才能真实触发语法恢复。
                    if broken {
                        format!("r{i}: Row {{ value: , }}\n")
                    } else {
                        format!("r{i}: Row {{ value: 1, }}\n")
                    }
                })
                .collect::<String>();
            let output = measure(&format!("parse_{count}_broken_{broken}"), samples, || {
                coflow_language::cfd::parse_cfd(black_box(&source))
            });
            assert_eq!(output.1.is_empty(), !broken);
            assert_eq!(output.0.records.len(), if broken { 0 } else { count });
            if broken {
                assert_eq!(output.1.len(), count);
            }
            println!(
                "{}",
                json!({"parse_records":count,"broken":broken,
                "bytes":source.len(),"diagnostics":output.1.len()})
            );
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "基准完整保留维度夹具、计时边界和结果对照，避免隐藏测量成本。"
)]
fn dimension_models(samples: usize, default_records: usize) {
    let records = env_usize("ENGINE_DIMENSION_RECORDS", default_records.min(1_000));
    assert!(records > 0);
    let schema = compile(
        r"
        table Item {
            @localized name: string;
            caption: string;
            parent: &Item;
            f0: int; f1: int; f2: int; f3: int; f4: int;
        }
    ",
    );
    let coflow_runtime::cell_value::ParsedCell::Value(caption) =
        coflow_runtime::cell_value::parse_cell(&schema, "string", r#"f"{self.parent.name}""#)
            .expect("valid formatted string")
    else {
        panic!("formatted string is a value");
    };
    for variants in [1, 10] {
        for (coverage, stride) in [("sparse", 100), ("dense", 1)] {
            let covered = records.div_ceil(stride);
            let mut times = Vec::with_capacity(samples);
            let mut latest = None;
            for sample in 0..=samples {
                drop(latest.take());
                // 输入构造与模板解析不计时；计时包含 build 内部消耗的输入及临时值析构。
                let mut builder = CfdDataModel::builder(&schema);
                for index in 0..records {
                    let mut fields = BTreeMap::from([
                        ("name".to_owned(), format!("Base {index}").into()),
                        ("caption".to_owned(), caption.clone()),
                        (
                            "parent".to_owned(),
                            LoadedValueDraft::record_ref(format!("i{}", (index + 1) % records)),
                        ),
                    ]);
                    for field in 0..5 {
                        fields.insert(
                            format!("f{field}"),
                            LoadedValueDraft::from(i64::from(field)),
                        );
                    }
                    builder.add_record(format!("i{index}"), "Item", fields);
                }
                for variant in 0..variants {
                    let dimension = coflow_runtime::DimensionName::new("language").unwrap();
                    let name = coflow_runtime::VariantName::new(format!("v{variant}")).unwrap();
                    builder.add_dimension_variant(dimension.clone(), name.clone());
                    for index in (0..records).step_by(stride) {
                        builder.add_dimension_value_draft(coflow_runtime::DimensionValueDraft {
                            source_type: coflow_runtime::TypeName::new("Item").unwrap(),
                            source_id: coflow_runtime::RecordId::new(format!("i{index}")).unwrap(),
                            field: coflow_runtime::FieldName::new("name").unwrap(),
                            dimension: dimension.clone(),
                            variant: name.clone(),
                            value: format!("Variant {variant} {index}").into(),
                            origin: coflow_runtime::RecordOrigin::None,
                        });
                    }
                }
                let start = Instant::now();
                let built = black_box(builder.build().expect("valid dimension model"));
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                if sample > 0 {
                    times.push(elapsed);
                }
                assert_eq!(built.record_count(), records);
                assert_eq!(built.dimension_variants("language").count(), variants);
                assert_eq!(
                    built
                        .records()
                        .filter(|(id, _)| built.has_dimension_variant(*id, "name", "v0"))
                        .count(),
                    covered
                );
                latest = Some(built);
            }
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "{}",
                json!({"metric":format!("dimension_build_{coverage}_v{variants}"),
                "records":records,"fields":8,"variants":variants,"explicit_records_per_variant":covered,
                "samples_ms":times,"median_ms":sorted[samples/2]})
            );
            let model = latest.unwrap();
            let selections = BTreeMap::from([("language".to_owned(), "v0".to_owned())]);
            // 每次从基础模型投影，不能误测相同选择的缓存返回分支。
            let projected = measure(
                &format!("dimension_project_{coverage}_v{variants}"),
                samples,
                || {
                    model
                        .project_dimensions(&schema, &selections)
                        .expect("valid projection")
                },
            );
            assert_eq!(projected.record_count(), records);
            for index in 0..records {
                let id = projected
                    .record_by_type_id("Item", &format!("i{index}"))
                    .unwrap();
                let record = projected.record(id).unwrap();
                let expected = |target: usize| {
                    if target.is_multiple_of(stride) {
                        format!("Variant 0 {target}")
                    } else {
                        format!("Base {target}")
                    }
                };
                assert_eq!(
                    record.field("name"),
                    Some(&CfdValue::String(expected(index)))
                );
                let Some(CfdValue::FormattedString(actual)) = record.field("caption") else {
                    panic!("caption is formatted");
                };
                assert_eq!(actual.rendered, expected((index + 1) % records));
            }
            measure(
                &format!("dimension_project_same_{coverage}_v{variants}"),
                samples,
                || {
                    projected
                        .project_dimensions(&schema, &selections)
                        .expect("same projection")
                },
            );
        }
    }
}

const SCHEMA: &str = r#"
data Stats { health: int = 100; speed: float = 1.0; }
table Row {
    value: int;
    name: string;
    description: string;
    level: int;
    enabled: bool;
    weight: float;
    tags: [string];
    numbers: [int];
    metadata: {string: string};
    stats: Stats;
    bonus: Stats;
    optional: Option<Stats>;
    history: [Stats];
    lookup: {string: Stats};
    matrix: [[int]];
    flags: [string];
    related: [&Row];
    parent: &Row;
    rating: int;
    note: string;
    check Valid { value >= 0; name != ""; parent.value >= 0; all n in numbers { n >= 0; } }
}
check ProjectIntegrity { records(Row).len() > 0; }
"#;

#[expect(
    clippy::too_many_lines,
    reason = "基准完整保留夹具、计时边界与独立重载验证，避免隐藏测量成本。"
)]
fn pipeline(samples: usize) {
    let count = env_usize("ENGINE_RECORDS", 1_000);
    let files = env_usize("ENGINE_FILES", 10);
    let update_samples = env_usize("ENGINE_UPDATE_SAMPLES", samples);
    let validation_samples = env_usize("ENGINE_VALIDATION_SAMPLES", samples);
    let mutation_field = std::env::var("ENGINE_FIELD").unwrap_or_else(|_| "name".to_owned());
    assert!(matches!(
        mutation_field.as_str(),
        "name" | "value" | "rating"
    ));
    assert!(update_samples > 0);
    assert!(count > 0 && files > 0 && count.is_multiple_of(files));
    let per_file = count / files;
    let root = tempfile::tempdir().expect("isolated project");
    std::fs::create_dir(root.path().join("data")).unwrap();
    std::fs::write(root.path().join("schema.cft"), SCHEMA).unwrap();
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    let mut bytes = 0;
    for file in 0..files {
        let mut text = String::new();
        for local in 0..per_file {
            let id = file * per_file + local;
            let next = (id + 1) % count;
            writeln!(
                text,
                r#"r{id:06}: Row {{
  value: 0, name: "Row {id}", description: "Description {id}", level: 1,
  enabled: true, weight: 1.5, tags: ["alpha", "beta"], numbers: [0, 1, 2, 3, 4, 5, 6, 7],
  metadata: {{ "origin": "file{file}", "key": "value" }},
  stats: Stats {{ health: 100, speed: 1.0 }}, bonus: Stats {{ health: 5 }},
  optional: Stats {{ health: 10 }}, history: [Stats {{ health: 1 }}, Stats {{ health: 2 }}],
  lookup: {{ "a": Stats {{ health: 3 }} }}, matrix: [[1, 2], [3, 4]],
  flags: ["a", "b"], related: [&r{next:06}], parent: &r{next:06}, rating: 1, note: "note"
}}"#
            )
            .unwrap();
        }
        bytes += text.len();
        std::fs::write(root.path().join(format!("data/{file:04}.cfd")), text).unwrap();
    }
    println!(
        "{}",
        json!({"records":count,"files":files,"fields":20,"source_bytes":bytes,
        "mutation_field":mutation_field,"update_samples":update_samples})
    );
    let project = measure("project_open", samples, || {
        Project::open_schema_only(Some(root.path())).expect("project")
    });
    let schema = measure("schema_compile", samples, || {
        Runtime::open_schema_session(project.clone()).expect("schema")
    });
    let mut session = measure("data_open", samples, || {
        let session = Runtime::new()
            .open_write_session_from_schema(schema.clone())
            .expect("session");
        assert_eq!(session.queries().record_count(), count);
        assert!(
            !session.queries().has_diagnostics(),
            "{:?}",
            session.queries().diagnostics()
        );
        session
    });
    println!(
        "{}",
        json!({"metric":"data_open_stats","stats":session.queries().execution_stats()})
    );
    let source_path = root.path().join("data/0000.cfd");
    let source = std::fs::read_to_string(&source_path).unwrap();
    let context = session.validation_context();
    let changed_source = match mutation_field.as_str() {
        "name" => source.replacen("name: \"Row 0\"", "name: \"Edited Row 0\"", 1),
        "value" => source.replacen("value: 0", "value: 10", 1),
        "rating" => source.replacen("rating: 1", "rating: 10", 1),
        _ => panic!("unsupported benchmark field: {mutation_field}"),
    };
    assert_ne!(changed_source, source);
    // 此处固定在修改前代次测重复校验；每次均重新校验同一份实际有变更的覆盖文本。
    for (name, source) in [
        ("validate_same_text", source),
        ("validate_one_field", changed_source),
    ] {
        let overrides = [DataSourceTextOverride {
            normalized_path: coflow_runtime::normalize_path(&source_path),
            source,
            deleted: false,
        }];
        measure(name, validation_samples, || {
            let diagnostics = context.validate(&overrides);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
        });
    }
    let batches = std::env::var("ENGINE_BATCHES").unwrap_or_else(|_| "1,10,100,1000".to_owned());
    let field = [CfdPathSegment::Field(mutation_field.clone())];
    // 所有批量场景共享递增序列，预热和采样均必然产生不同于已发布代次的新值。
    let mut sequence = 0_i64;
    for batch in batches
        .split(',')
        .map(|v| v.parse::<usize>().expect("batch"))
    {
        assert!(batch > 0);
        if batch > count {
            continue;
        }
        for order in ["grouped", "interleaved"] {
            if order == "interleaved" && (batch == 1 || files == 1) {
                continue;
            }
            let ids: Vec<_> = if order == "grouped" {
                (0..batch).collect()
            } else {
                (0..batch)
                    .map(|i| (i % files) * per_file + i / files)
                    .collect()
            };
            assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), batch);
            assert!(ids.iter().all(|id| *id < count));
            measure(&format!("mutate_{batch}_{order}"), update_samples, || {
                sequence += 1;
                // 每次输入都改变，避免把无效写入当作热更新；请求构造在本计时内。
                // 结果查询断言也计时，报告返回值自身的析构在计时外。
                let value = if mutation_field == "name" {
                    CfdValue::String(format!("Edited {sequence}"))
                } else {
                    CfdValue::Int(1_000_000 + sequence)
                };
                let report = session.apply_mutation(MutationRequest {
                    stop_on_write_error: true,
                    ops: ids
                        .iter()
                        .map(|id| MutationOp::SetField {
                            record: RecordCoordinate::new(
                                "Row".try_into().unwrap(),
                                format!("r{id:06}").try_into().unwrap(),
                            ),
                            file: None,
                            path: field.to_vec(),
                            value: MutationValue::Cfd(value.clone()),
                        })
                        .collect(),
                });
                assert!(report.write_ok && report.check_ok, "{report:?}");
                assert!(
                    report.failed.is_empty() && report.diagnostics.is_empty(),
                    "{report:?}"
                );
                assert!(report.generation_changed);
                assert_eq!(report.applied.len(), batch);
                assert_eq!(session.queries().record_count(), count);
                for id in &ids {
                    assert_eq!(
                        session
                            .queries()
                            .field_value("Row", &format!("r{id:06}"), &field),
                        Some(&value)
                    );
                }
                black_box(report)
            });
            println!(
                "{}",
                json!({"metric":format!("mutate_{batch}_{order}_stats"),
                "stats":session.queries().execution_stats()})
            );
        }
    }
    // 独立全量重载验证真实落盘结果；不能只相信写入报告。
    let fresh = Runtime::new()
        .open_read_only_session(project)
        .expect("fresh session");
    assert_eq!(fresh.queries().record_count(), count);
    assert!(!fresh.queries().has_diagnostics());
    for id in 0..count {
        assert_eq!(
            session
                .queries()
                .field_value("Row", &format!("r{id:06}"), &field),
            fresh
                .queries()
                .field_value("Row", &format!("r{id:06}"), &field)
        );
    }
    println!(
        "{}",
        json!({"verification":"fresh_reload_matches_all_values_and_no_diagnostics"})
    );
}
