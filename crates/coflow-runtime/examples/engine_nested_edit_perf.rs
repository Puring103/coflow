//! 显式大数组的批量元素修改：包含准备、写入、重建和检查，独立重载对照。
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
#[cfg(all(windows, not(feature = "engine-perf-system")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;
use coflow_runtime::{
    CfdPathSegment, CfdValue, MutationOp, MutationRequest, MutationValue, Project,
    RecordCoordinate, Runtime,
};
use std::time::Instant;
fn main() {
    let count = 100_000;
    let batch = 1_000;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("schema.cft"),
        "table Row { values: [int]; }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("data.cfd"),
        format!("r: Row {{ values: [{}] }}", vec!["0"; count].join(",")),
    )
    .unwrap();
    let project = Project::open_schema_only(Some(root.path())).unwrap();
    let mut session = Runtime::new().open_write_session(project.clone()).unwrap();
    let mut times = Vec::new();
    for sample in 0..4 {
        let ops = (0..batch)
            .map(|index| MutationOp::SetField {
                record: RecordCoordinate::new("Row".try_into().unwrap(), "r".try_into().unwrap()),
                file: None,
                path: vec![
                    CfdPathSegment::Field("values".into()),
                    CfdPathSegment::Index(index),
                ],
                value: MutationValue::Cfd(CfdValue::Int(sample + 1)),
            })
            .collect();
        let start = Instant::now();
        let result = session.apply_mutation(MutationRequest {
            stop_on_write_error: true,
            ops,
        });
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        assert!(
            result.write_ok && result.check_ok && result.failed.is_empty(),
            "{result:?}"
        );
        if sample > 0 {
            times.push(elapsed);
        }
    }
    let fresh = Runtime::new().open_read_only_session(project).unwrap();
    let path = [CfdPathSegment::Field("values".into())];
    let expected = CfdValue::Array(
        (0..count)
            .map(|i| CfdValue::Int(if i < batch { 4 } else { 0 }))
            .collect(),
    );
    assert_eq!(
        session.queries().field_value("Row", "r", &path),
        Some(&expected)
    );
    assert_eq!(
        fresh.queries().field_value("Row", "r", &path),
        Some(&expected)
    );
    assert!(!fresh.queries().has_diagnostics());
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    println!(
        "{}",
        serde_json::json!({"array_elements":count,"batch":batch,"samples_ms":times,"median_ms":sorted[1],"verification":"all elements and independent full reload"})
    );
}
