//! 非编辑器 Runtime 操作矩阵；仅使用独立临时工程及公开会话 API。
//! 参数与计时边界见 `runtime_matrix/README.md`。
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[cfg(all(windows, not(feature = "engine-perf-system")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[path = "runtime_matrix/fixture.rs"]
mod fixture;

use coflow_runtime::{
    CfdDictKey, CfdPathSegment, CfdValue, DataSourceTextOverride, DimensionValueCoordinate,
    DimensionValueExpectation, DimensionValueState, MutationFields, MutationOp, MutationReport,
    MutationRequest, MutationValue, RecordCoordinate, Runtime, WriteProjectSession,
};
use fixture::{Config, Fixture};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::Instant;

const CASES: &[&str] = &[
    "load_checks",
    "load_no_checks",
    "field_lookup",
    "record_scan",
    "references",
    "validate_same",
    "validate_changed",
    "validate_invalid",
    "refresh_same",
    "error_recovery",
    "set_scalar",
    "set_dependency",
    "set_noop",
    "unset_field",
    "batch_grouped",
    "batch_interleaved",
    "array_element",
    "array_replace",
    "dict_element",
    "dict_replace",
    "nested_field",
    "option_field",
    "insert_record",
    "delete_record",
    "rename_record",
    "swap_records",
    "transfer_record",
    "dimension_set",
    "dimension_add",
    "dimension_clear",
    "dimension_lookup",
    "external_conflict",
];

fn emit(metric: &str, times: Vec<f64>, stats: Value, operations: usize) {
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let median = if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };
    println!(
        "{}",
        json!({"metric":metric,"samples_ms":times,"median_ms":median,
        "min_ms":sorted[0],"max_ms":sorted[n-1],
        "p95_ms":if n>=20 {Some(sorted[(n*95).div_ceil(100)-1])} else {None},
        "operations_per_sample":operations,"stats":stats})
    );
}

fn record(id: &str) -> RecordCoordinate {
    RecordCoordinate::new("Row".try_into().unwrap(), id.try_into().unwrap())
}
fn path(field: &str) -> Vec<CfdPathSegment> {
    vec![CfdPathSegment::Field(field.into())]
}
fn dimension() -> DimensionValueCoordinate {
    DimensionValueCoordinate {
        actual_type: "Row".try_into().unwrap(),
        record_id: "r000000".try_into().unwrap(),
        field: "name".try_into().unwrap(),
        dimension: "language".try_into().unwrap(),
        variant: "bench".try_into().unwrap(),
        path: vec![],
    }
}
fn set(id: &str, field: &str, value: CfdValue) -> MutationOp {
    MutationOp::SetField {
        record: record(id),
        file: None,
        path: path(field),
        value: MutationValue::Cfd(value),
    }
}
fn apply(session: &mut WriteProjectSession, ops: Vec<MutationOp>) -> MutationReport {
    session.apply_mutation(MutationRequest {
        stop_on_write_error: true,
        ops,
    })
}
fn healthy(report: &MutationReport) {
    assert!(
        report.write_ok
            && report.check_ok
            && report.failed.is_empty()
            && report.diagnostics.is_empty(),
        "{report:?}"
    );
}

fn main() {
    let config = Config::read();
    let selected = std::env::var("MATRIX_CASES").unwrap_or_else(|_| CASES.join(","));
    println!(
        "{}",
        json!({"tool":"runtime_matrix","config":config.metadata(),
        "timing":"fixture/setup/reset/assertions/output excluded; API call and internal drops included; returned output drop excluded",
        "allocator":if cfg!(all(windows, not(feature="engine-perf-system"))) {"mimalloc"} else {"system"}})
    );
    for case in selected.split(',') {
        assert!(CASES.contains(&case), "unknown case {case}");
        let fixture = Fixture::new(&config);
        println!(
            "{}",
            json!({"case":case,"source_bytes":fixture.bytes,"config":config.metadata()})
        );
        run(case, &config, &fixture);
    }
}

fn run(case: &str, config: &Config, fixture: &Fixture) {
    if matches!(case, "load_checks" | "load_no_checks") {
        loads(case, config, fixture);
        return;
    }
    let mut session = fixture.session();
    assert_eq!(session.queries().record_count(), config.records);
    if matches!(
        case,
        "field_lookup" | "record_scan" | "references" | "dimension_lookup"
    ) {
        queries(case, config, &mut session);
        return;
    }
    if matches!(
        case,
        "validate_same"
            | "validate_changed"
            | "validate_invalid"
            | "refresh_same"
            | "error_recovery"
    ) {
        validation(case, config, fixture, session);
        return;
    }
    mutations(case, config, fixture, &mut session);
}

fn loads(case: &str, config: &Config, fixture: &Fixture) {
    let mut times = Vec::new();
    let mut stats = Value::Null;
    for i in 0..config.warmups + config.samples {
        let start = Instant::now();
        let loaded = if case == "load_checks" {
            Runtime::new().open_read_only_session(fixture.project.clone())
        } else {
            Runtime::new().open_data_session(fixture.project.clone())
        }
        .expect("load");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(loaded.queries().record_count(), config.records);
        assert!(!loaded.queries().has_diagnostics());
        assert_eq!(
            loaded
                .queries()
                .field_value("Row", "r000000", &path("name")),
            Some(&CfdValue::String("Row 0".into()))
        );
        stats = serde_json::to_value(loaded.queries().execution_stats()).unwrap();
        if i >= config.warmups {
            times.push(elapsed);
        }
        // 新会话每次释放；操作系统文件缓存不清理，因此不冒称机器冷启动。
        drop(loaded);
    }
    emit(case, times, stats, 1);
}

fn queries(case: &str, config: &Config, session: &mut WriteProjectSession) {
    if case == "dimension_lookup" {
        healthy(&apply(
            session,
            vec![MutationOp::SetDimensionValue {
                file: Some("dimensions/language/bench.cfd".into()),
                coordinate: dimension(),
                expected: DimensionValueExpectation::Any,
                value: MutationValue::Cfd(CfdValue::String("Bench".into())),
            }],
        ));
    }
    let mut times = Vec::new();
    let keys = (0..config.query_batch)
        .map(|i| format!("r{:06}", i % config.records))
        .collect::<Vec<_>>();
    let coordinates = keys.iter().map(|id| record(id)).collect::<Vec<_>>();
    let field = path("value");
    let coordinate = dimension();
    for i in 0..config.warmups + config.samples {
        let start = Instant::now();
        let count: usize = match case {
            "field_lookup" => keys
                .iter()
                .map(|id| {
                    usize::from(
                        black_box(session.queries().field_value("Row", id, &field)).is_some(),
                    )
                })
                .sum(),
            "references" => coordinates
                .iter()
                .map(|id| black_box(session.queries().record_references(id)).len())
                .sum(),
            "record_scan" => session
                .queries()
                .source_files()
                .map(|file| {
                    session
                        .queries()
                        .record_views_in_file(file)
                        .map(|view| {
                            black_box(view.record.fields());
                            1
                        })
                        .sum::<usize>()
                })
                .sum(),
            "dimension_lookup" => (0..config.query_batch)
                .map(|_| {
                    usize::from(black_box(session.queries().dimension_value(&coordinate)).is_some())
                })
                .sum(),
            _ => panic!("query case"),
        };
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if case == "record_scan" {
            assert_eq!(count, config.records);
        } else {
            assert_eq!(count, config.query_batch);
        }
        if i >= config.warmups {
            times.push(elapsed);
        }
    }
    emit(
        case,
        times,
        json!({"verified":true}),
        if case == "record_scan" {
            config.records
        } else {
            config.query_batch
        },
    );
}

fn validation(case: &str, config: &Config, fixture: &Fixture, mut session: WriteProjectSession) {
    let original = std::fs::read_to_string(fixture.source()).unwrap();
    let mut times = Vec::new();
    let mut stats = Value::Null;
    for i in 0..config.warmups + config.samples {
        let changed = match case {
            "validate_changed" => original.replacen("value: 0", "value: 12345", 1),
            "validate_invalid" => original.replacen("value: 0", "value: \"bad\"", 1),
            _ => original.clone(),
        };
        if case == "error_recovery" {
            // 错误状态准备也可能发生性能退化，单独记录阶段以解释有界超时。
            println!(
                "{}",
                json!({"phase":"prepare_invalid_state","case":case,"iteration":i,
                "unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()})
            );
            std::fs::write(
                fixture.source(),
                original.replacen("value: 0", "value: \"bad\"", 1),
            )
            .unwrap();
            let invalid = session
                .refresh_context()
                .refresh_data_sources(&[fixture.source()])
                .expect("tolerant invalid session");
            assert!(invalid.queries().has_diagnostics());
            session = invalid;
            std::fs::write(fixture.source(), &original).unwrap();
        }
        let override_text = [DataSourceTextOverride {
            normalized_path: coflow_runtime::normalize_path(&fixture.source()),
            source: changed,
            deleted: false,
        }];
        if matches!(case, "refresh_same" | "error_recovery") {
            let context = session.refresh_context();
            let start = Instant::now();
            let refreshed = context
                .refresh_data_sources(&[fixture.source()])
                .expect("refresh");
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert!(!refreshed.queries().has_diagnostics());
            stats = serde_json::to_value(refreshed.queries().execution_stats()).unwrap();
            session = refreshed;
            if i >= config.warmups {
                times.push(elapsed);
            }
        } else {
            let context = session.validation_context();
            if case == "validate_invalid" {
                println!(
                    "{}",
                    json!({"phase":"validate_invalid_started","case":case,"iteration":i,
                    "unix_ms":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()})
                );
            }
            let start = Instant::now();
            let diagnostics = context.validate(&override_text);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(diagnostics.is_empty(), case != "validate_invalid");
            stats = json!({"diagnostic_count":diagnostics.len()});
            if i >= config.warmups {
                times.push(elapsed);
            }
        }
        assert_eq!(
            session
                .queries()
                .field_value("Row", "r000000", &path("value")),
            Some(&CfdValue::Int(0))
        );
        assert_eq!(std::fs::read_to_string(fixture.source()).unwrap(), original);
    }
    emit(case, times, stats, 1);
}

fn mutations(case: &str, config: &Config, fixture: &Fixture, session: &mut WriteProjectSession) {
    let mut times = Vec::new();
    let mut stats = Value::Null;
    let mut operation_count = 1;
    let original_fields = session
        .queries()
        .record_view("Row", "r000000")
        .unwrap()
        .record
        .fields()
        .iter()
        .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    // 保留真实不可变记录快照，操作完成后核对旧值；完整磁盘重载在每组采样末尾执行。
    let old_record = session
        .queries()
        .record_view("Row", "r000000")
        .unwrap()
        .record
        .clone();
    let original_source = std::fs::read_to_string(fixture.source()).unwrap();
    for i in 0..config.warmups + config.samples {
        let sequence = i64::try_from(i + 1).unwrap();
        let mut cleanup = Vec::new();
        let mut expected_fields = Vec::new();
        let ops = match case {
            "set_scalar" => {
                let v = CfdValue::String(format!("Edited {sequence}"));
                expected_fields.push(("r000000".to_owned(), path("name"), v.clone()));
                vec![set("r000000", "name", v)]
            }
            "set_dependency" => {
                let v = CfdValue::Int(sequence);
                expected_fields.push(("r000000".to_owned(), path("value"), v.clone()));
                vec![set("r000000", "value", v)]
            }
            "set_noop" => vec![set("r000000", "value", CfdValue::Int(0))],
            "unset_field" => {
                healthy(&apply(
                    session,
                    vec![set("r000000", "value", CfdValue::Int(sequence))],
                ));
                expected_fields.push(("r000000".into(), path("value"), CfdValue::Int(0)));
                vec![MutationOp::UnsetField {
                    record: record("r000000"),
                    file: None,
                    path: path("value"),
                }]
            }
            "batch_grouped" | "batch_interleaved" => {
                let per_file = config.records / config.files;
                let ids = (0..config.batch)
                    .map(|n| {
                        if case == "batch_grouped" {
                            n
                        } else {
                            (n % config.files) * per_file + n / config.files
                        }
                    })
                    .collect::<Vec<_>>();
                operation_count = ids.len();
                ids.into_iter()
                    .map(|n| {
                        let id = format!("r{n:06}");
                        let v = CfdValue::String(format!("Batch {sequence}"));
                        expected_fields.push((id.clone(), path("name"), v.clone()));
                        set(&id, "name", v)
                    })
                    .collect()
            }
            "array_element" => {
                let p = vec![
                    CfdPathSegment::Field("numbers".into()),
                    CfdPathSegment::Index(0),
                ];
                let v = CfdValue::Int(sequence + 100);
                expected_fields.push(("r000000".into(), p.clone(), v.clone()));
                vec![MutationOp::SetField {
                    record: record("r000000"),
                    file: None,
                    path: p,
                    value: MutationValue::Cfd(v),
                }]
            }
            "array_replace" => {
                let v = CfdValue::Array(
                    (0..config.collection)
                        .map(|n| CfdValue::Int(i64::try_from(n).unwrap() + sequence))
                        .collect(),
                );
                expected_fields.push(("r000000".into(), path("numbers"), v.clone()));
                vec![set("r000000", "numbers", v)]
            }
            "dict_element" => {
                let p = vec![
                    CfdPathSegment::Field("metadata".into()),
                    CfdPathSegment::DictKey(coflow_runtime::dict_key_path_text(
                        &CfdDictKey::String("a".into()),
                    )),
                ];
                let v = CfdValue::String(format!("Edited {sequence}"));
                expected_fields.push(("r000000".into(), p.clone(), v.clone()));
                vec![MutationOp::SetField {
                    record: record("r000000"),
                    file: None,
                    path: p,
                    value: MutationValue::Cfd(v),
                }]
            }
            "dict_replace" => {
                let v = CfdValue::Dict(
                    (0..config.collection)
                        .map(|n| {
                            (
                                CfdDictKey::String(format!("k{n}")),
                                CfdValue::String(format!("Value {sequence}")),
                            )
                        })
                        .collect(),
                );
                expected_fields.push(("r000000".into(), path("metadata"), v.clone()));
                vec![set("r000000", "metadata", v)]
            }
            "nested_field" => {
                let p = vec![
                    CfdPathSegment::Field("stats".into()),
                    CfdPathSegment::Field("health".into()),
                ];
                let v = CfdValue::Int(sequence + 100);
                expected_fields.push(("r000000".into(), p.clone(), v.clone()));
                vec![MutationOp::SetField {
                    record: record("r000000"),
                    file: None,
                    path: p,
                    value: MutationValue::Cfd(v),
                }]
            }
            "option_field" => {
                let v = if i % 2 == 0 {
                    CfdValue::OptionNone
                } else {
                    CfdValue::OptionSome(Box::new(CfdValue::Int(sequence)))
                };
                expected_fields.push(("r000000".into(), path("optional"), v.clone()));
                vec![set("r000000", "optional", v)]
            }
            "insert_record" => {
                cleanup.push(MutationOp::DeleteRecord {
                    record: record("bench_added"),
                    file: None,
                });
                vec![insert(&original_fields)]
            }
            "delete_record" => {
                healthy(&apply(session, vec![insert(&original_fields)]));
                vec![MutationOp::DeleteRecord {
                    record: record("bench_added"),
                    file: None,
                }]
            }
            "rename_record" => {
                cleanup.push(MutationOp::RenameRecord {
                    record: record("bench_renamed"),
                    file: None,
                    new_id: "r000000".into(),
                });
                vec![MutationOp::RenameRecord {
                    record: record("r000000"),
                    file: None,
                    new_id: "bench_renamed".into(),
                }]
            }
            "swap_records" => {
                assert!(
                    config.records / config.files >= 2,
                    "swap requires two records per file"
                );
                let op = MutationOp::SwapRecords {
                    first: record("r000000"),
                    second: record("r000001"),
                    file: None,
                };
                cleanup.push(op.clone());
                vec![op]
            }
            "transfer_record" => {
                assert!(config.files >= 2, "transfer requires two files");
                cleanup.push(MutationOp::TransferRecord {
                    record: record("r000000"),
                    destination_file: "data/0000.cfd".into(),
                    target_index: 0,
                    source_file: None,
                });
                vec![MutationOp::TransferRecord {
                    record: record("r000000"),
                    destination_file: "data/0001.cfd".into(),
                    target_index: 0,
                    source_file: None,
                }]
            }
            "dimension_set" | "dimension_add" => {
                if case == "dimension_add"
                    && session
                        .queries()
                        .dimension_source_file(&dimension())
                        .is_some()
                {
                    healthy(&apply(
                        session,
                        vec![MutationOp::ClearDimensionValue {
                            coordinate: dimension(),
                            expected: DimensionValueExpectation::Any,
                        }],
                    ));
                }
                vec![MutationOp::SetDimensionValue {
                    file: Some("dimensions/language/bench.cfd".into()),
                    coordinate: dimension(),
                    expected: DimensionValueExpectation::Any,
                    value: MutationValue::Cfd(CfdValue::String(format!("Translation {sequence}"))),
                }]
            }
            "dimension_clear" => {
                healthy(&apply(
                    session,
                    vec![MutationOp::SetDimensionValue {
                        file: Some("dimensions/language/bench.cfd".into()),
                        coordinate: dimension(),
                        expected: DimensionValueExpectation::Any,
                        value: MutationValue::Cfd(CfdValue::String(format!(
                            "Translation {sequence}"
                        ))),
                    }],
                ));
                vec![MutationOp::ClearDimensionValue {
                    coordinate: dimension(),
                    expected: DimensionValueExpectation::Any,
                }]
            }
            "external_conflict" => {
                std::fs::write(
                    fixture.source(),
                    format!("{original_source}\n// external modification {sequence}\n"),
                )
                .unwrap();
                vec![set("r000000", "value", CfdValue::Int(sequence))]
            }
            _ => panic!("mutation case"),
        };
        let start = Instant::now();
        let report = apply(session, ops);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if case == "external_conflict" {
            assert!(
                !report.write_ok && !report.failed.is_empty(),
                "external conflict rejected: {report:?}"
            );
            assert!(std::fs::read_to_string(fixture.source())
                .unwrap()
                .contains("external modification"));
            std::fs::write(fixture.source(), &original_source).unwrap();
        } else {
            healthy(&report);
            if case == "set_noop" {
                assert!(!report.generation_changed);
            } else {
                assert!(report.generation_changed);
            }
            for (id, p, v) in &expected_fields {
                assert_eq!(session.queries().field_value("Row", id, p), Some(v));
            }
            match case {
                "insert_record" => assert_eq!(session.queries().record_count(), config.records + 1),
                "delete_record" => assert!(session
                    .queries()
                    .record_view("Row", "bench_added")
                    .is_none()),
                "rename_record" => {
                    assert!(session.queries().record_view("Row", "r000000").is_none());
                    assert!(session
                        .queries()
                        .record_view("Row", "bench_renamed")
                        .is_some());
                }
                "transfer_record" => assert_eq!(
                    session.queries().file_for_record("Row", "r000000"),
                    Some("data/0001.cfd")
                ),
                "dimension_set" | "dimension_add" => assert!(
                    matches!(session.queries().dimension_value(&dimension()).unwrap().state,DimensionValueState::Value(CfdValue::String(ref s)) if s==&format!("Translation {sequence}"))
                ),
                "dimension_clear" => assert!(matches!(
                    session
                        .queries()
                        .dimension_value(&dimension())
                        .unwrap()
                        .state,
                    DimensionValueState::Missing
                )),
                _ => {}
            }
        }
        // 失败或同值操作没有发布代次；会话仍保存旧代次计数，不能当作本次工作量。
        stats = json!({"generation_changed":report.generation_changed,
            "last_published_generation":session.queries().execution_stats(),
            "written_files":report.written_files.len(),"affected_files":report.affected_files.len()});
        if i >= config.warmups {
            times.push(elapsed);
        }
        if !cleanup.is_empty() {
            healthy(&apply(session, cleanup));
        }
        assert_eq!(
            old_record.field("name"),
            Some(&CfdValue::String("Row 0".into()))
        );
        assert_eq!(old_record.field("value"), Some(&CfdValue::Int(0)));
    }
    verify(fixture, session, config.records);
    emit(case, times, stats, operation_count);
}

fn insert(fields: &BTreeMap<String, CfdValue>) -> MutationOp {
    MutationOp::InsertRecord {
        file: "data/0000.cfd".into(),
        actual_type: "Row".into(),
        id: "bench_added".into(),
        fields: MutationFields::Cfd(fields.clone()),
        materialization: Default::default(),
    }
}

fn verify(fixture: &Fixture, session: &WriteProjectSession, count: usize) {
    let fresh = Runtime::new()
        .open_read_only_session(fixture.project.clone())
        .expect("independent full reload");
    assert!(!fresh.queries().has_diagnostics());
    assert_eq!(fresh.queries().record_count(), count);
    for n in 0..count {
        let id = format!("r{n:06}");
        let current = session.queries().record_view("Row", &id).unwrap();
        let loaded = fresh.queries().record_view("Row", &id).unwrap();
        assert_eq!(current.record.fields().len(), loaded.record.fields().len());
        for (field, value) in current.record.fields() {
            assert!(
                value.same_stored_value(loaded.record.field(field.as_str()).unwrap()),
                "reload mismatch {id}.{field}"
            );
        }
    }
    // 覆盖写入还核对独立重载后的显式值，基础字段相等不能替代维度持久化检查。
    for info in session.queries().dimensions() {
        for variant in info.variants {
            let coordinate = DimensionValueCoordinate {
                actual_type: "Row".try_into().unwrap(),
                record_id: "r000000".try_into().unwrap(),
                field: (if info.name == "language" {
                    "name"
                } else {
                    "caption"
                })
                .try_into()
                .unwrap(),
                dimension: info.name.as_str().try_into().unwrap(),
                variant: variant.as_str().try_into().unwrap(),
                path: vec![],
            };
            assert_eq!(
                session
                    .queries()
                    .dimension_value(&coordinate)
                    .map(|view| view.state),
                fresh
                    .queries()
                    .dimension_value(&coordinate)
                    .map(|view| view.state)
            );
        }
    }
    println!(
        "{}",
        json!({"verification":"fresh_reload_all_fields_and_no_diagnostics","records":count})
    );
}
