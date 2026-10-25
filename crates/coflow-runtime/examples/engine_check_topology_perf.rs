//! 第二轮审查探针：对照同记录数、同引用边数的环状与集中引用拓扑。
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[cfg(all(windows, not(feature = "engine-perf-system")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use coflow_runtime::{CfdPathSegment, CfdValue, MutationOp, MutationRequest, MutationValue, Project, RecordCoordinate, Runtime};
use serde_json::json;
use std::fmt::Write as _;
use std::time::Instant;

fn main() {
    let sizes = std::env::var("AUDIT_SIZES").unwrap_or_else(|_| "1000,2000,4000".into());
    let samples = std::env::var("AUDIT_SAMPLES").map_or(3, |v| v.parse::<usize>().unwrap());
    assert!(samples > 0);
    println!("{}", json!({"probe":"check_reference_fanout", "warmups":1, "samples":samples,
        "timing":"mutation only; fresh full reload and value verification outside timing",
        "allocator":if cfg!(all(windows, not(feature="engine-perf-system"))) { "mimalloc" } else { "system" }}));
    for count in sizes.split(',').map(|v| v.parse::<usize>().unwrap()) {
        for topology in ["ring", "hub"] {
            run(count, topology, samples);
        }
    }
}

fn run(count: usize, topology: &str, samples: usize) {
    let root = tempfile::tempdir().expect("isolated project");
    std::fs::write(root.path().join("coflow.yaml"), "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n").unwrap();
    std::fs::write(root.path().join("schema.cft"), "table Row { value: int; next: &Row; check Reference { next.value >= 0; } }\n").unwrap();
    let mut source = String::new();
    for id in 0..count {
        let target = if topology == "hub" { 0 } else { (id + 1) % count };
        writeln!(source, "r{id:06}: Row {{ value: 0, next: &r{target:06} }}").unwrap();
    }
    std::fs::write(root.path().join("data.cfd"), &source).unwrap();
    let project = Project::open_schema_only(Some(root.path())).expect("project");
    let mut session = Runtime::new().open_write_session(project.clone()).expect("session");
    assert_eq!(session.queries().record_count(), count);
    assert!(!session.queries().has_diagnostics(), "{:?}", session.queries().diagnostics());
    let field = [CfdPathSegment::Field("value".into())];
    let mut times = Vec::new();
    let mut stats = Vec::new();
    for sample in 0..=samples {
        // 预热和每次采样均提交真实的新值，保留生产环境的发布与冲突核对成本。
        let value = CfdValue::Int(sample as i64 + 1);
        let request = MutationRequest { stop_on_write_error: true, ops: vec![MutationOp::SetField {
            record: RecordCoordinate::new("Row".try_into().unwrap(), "r000000".try_into().unwrap()),
            file: None, path: field.to_vec(), value: MutationValue::Cfd(value.clone()),
        }] };
        let start = Instant::now();
        let report = session.apply_mutation(request);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(report.write_ok && report.check_ok && report.generation_changed, "{report:?}");
        assert!(report.failed.is_empty() && report.diagnostics.is_empty(), "{report:?}");
        assert_eq!(session.queries().field_value("Row", "r000000", &field), Some(&value));
        if sample > 0 {
            times.push(elapsed);
            stats.push(serde_json::to_value(session.queries().execution_stats()).unwrap());
        }
    }
    // 独立加载已发布文件，对照所有值、引用字段和诊断，不把此验证计入修改耗时。
    let fresh = Runtime::new().open_read_only_session(project).expect("independent full load");
    assert_eq!(fresh.queries().record_count(), count);
    assert!(!fresh.queries().has_diagnostics(), "{:?}", fresh.queries().diagnostics());
    for id in 0..count {
        for name in ["value", "next"] {
            let field = [CfdPathSegment::Field(name.into())];
            assert_eq!(session.queries().field_value("Row", &format!("r{id:06}"), &field),
                fresh.queries().field_value("Row", &format!("r{id:06}"), &field));
        }
    }
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!("{}", json!({"records":count,"edges":count,"topology":topology,"source_bytes":source.len(),
        "samples_ms":times,"median_ms":sorted[samples / 2],"stats":stats,
        "verification":"fresh_reload_matches_all_values_references_and_no_diagnostics"}));
}
