//! 第二轮审查探针：相同边数的环形引用与高扇入引用，测量普通字段增量更新。
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[cfg(all(windows, not(feature = "engine-perf-system")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use coflow_language::cft::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use coflow_runtime::{CfdDataModel, CfdModelBuilder, LoadedRecordDraft, LoadedValueDraft};
use serde_json::json;
use std::hint::black_box;
use std::time::Instant;

fn builder<'a>(schema: &'a CftSchema, inputs: &'a [LoadedRecordDraft]) -> CfdModelBuilder<'a> {
    let mut builder = CfdDataModel::builder(schema);
    for input in inputs {
        builder.add_borrowed_record(input);
    }
    builder
}

fn main() {
    let parsed = parse_modules([CftFile::from_source(
        ModuleId::from("probe.cft"),
        "table Item { value: int; target: &Item; }",
    )]);
    assert!(parsed.diagnostics().is_empty());
    let schema = build_schema(&parsed).unwrap();
    for count in [1_000_usize, 2_000, 4_000, 8_000] {
        for topology in ["ring", "hub"] {
            let mut inputs = (0..count)
                .map(|index| {
                    LoadedRecordDraft::new(
                        format!("i{index}"),
                        "Item",
                        [
                            (
                                "value",
                                LoadedValueDraft::Int(
                                    i64::try_from(index).expect("fixture index fits i64"),
                                ),
                            ),
                            (
                                "target",
                                LoadedValueDraft::record_ref(format!(
                                    "i{}",
                                    if topology == "hub" {
                                        0
                                    } else {
                                        (index + 1) % count
                                    }
                                )),
                            ),
                        ],
                    )
                })
                .collect::<Vec<_>>();
            let previous = builder(&schema, &inputs).build().unwrap();
            assert_eq!(previous.ref_edges().count(), count);
            let changed = previous
                .records()
                .take(count / 10)
                .map(|(handle, _)| handle)
                .collect::<Vec<_>>();
            for input in inputs.iter_mut().take(count / 10) {
                input
                    .fields
                    .insert("value".into(), LoadedValueDraft::Int(-1));
            }
            // 独立完整构建和逐值、双向引用、来源比较都在计时范围外。
            let expected = builder(&schema, &inputs).build().unwrap();
            let rebuild = || {
                builder(&schema, &inputs)
                    .try_build_partial_incremental(&previous, &changed)
                    .unwrap()
            };
            let warmup = rebuild();
            assert_eq!(warmup.model, expected);
            drop(warmup);
            let mut times = Vec::new();
            for _ in 0..3 {
                let start = Instant::now();
                let output = black_box(rebuild());
                times.push(start.elapsed().as_secs_f64() * 1_000.0);
                assert!(output.diagnostics.is_empty());
                assert_eq!(output.statistics.rebuilt_ref_edges, count / 10);
                assert_eq!(output.model, expected);
                // 模型相等性也包含引用索引；显式迭代再次验证公开查询的稳定顺序。
                assert_eq!(
                    output.model.ref_edges().collect::<Vec<_>>(),
                    expected.ref_edges().collect::<Vec<_>>()
                );
                for (handle, _) in expected.records() {
                    assert_eq!(
                        output.model.ref_edges_to_target(handle).collect::<Vec<_>>(),
                        expected.ref_edges_to_target(handle).collect::<Vec<_>>()
                    );
                }
                drop(output);
            }
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "{}",
                json!({"metric": "model_reference_fanin", "topology": topology,
                "records": count, "edges": count, "changed_records": count / 10,
                "warmups": 1, "samples_ms": times, "median_ms": sorted[1],
                "full_comparison": true, "retained_old_snapshot": true,
                "timing": "borrowed builder setup + incremental build; output drop and full comparison excluded"})
            );
        }
    }
}
