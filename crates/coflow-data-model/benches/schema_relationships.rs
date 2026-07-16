#![allow(clippy::cast_possible_wrap, clippy::expect_used, clippy::print_stdout)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
use coflow_data_model::{CfdDataModel, LoadedRecordDraft, LoadedValueDraft};
use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

const INHERITANCE_DEPTH: usize = 64;
const RECORD_COUNT: usize = 512;
const LOOKUP_ROUNDS: usize = 200_000;
const BUILD_ROUNDS: usize = 50;

fn main() {
    let schema = deep_schema();
    let records = loaded_records();
    let model = build_model(&schema, records.clone());
    let keys = (0..RECORD_COUNT)
        .map(|index| format!("record_{index}"))
        .collect::<Vec<_>>();

    let schema_elapsed = measure(|| {
        for _ in 0..LOOKUP_ROUNDS {
            black_box(schema.inheritance_root("Type64"));
            black_box(schema.ancestor_type_names("Type64"));
            black_box(schema.is_assignable("Type64", "Root"));
        }
    });
    let lookup_elapsed = measure(|| {
        for index in 0..LOOKUP_ROUNDS {
            black_box(model.lookup_assignable(&schema, "Root", &keys[index % RECORD_COUNT]));
        }
    });
    let build_elapsed = measure(|| {
        for _ in 0..BUILD_ROUNDS {
            black_box(build_model(&schema, records.clone()));
        }
    });

    println!(
        "schema relationships: depth={INHERITANCE_DEPTH}, operations={}, elapsed={schema_elapsed:?}",
        LOOKUP_ROUNDS * 3
    );
    println!(
        "assignable record lookup: records={RECORD_COUNT}, operations={LOOKUP_ROUNDS}, elapsed={lookup_elapsed:?}"
    );
    println!(
        "model index build: depth={INHERITANCE_DEPTH}, records={RECORD_COUNT}, builds={BUILD_ROUNDS}, elapsed={build_elapsed:?}"
    );
}

fn measure(operation: impl FnOnce()) -> Duration {
    let started = Instant::now();
    operation();
    started.elapsed()
}

fn deep_schema() -> coflow_cft::CftSchema {
    let mut source = String::from("type Root { value: int; }\n");
    for depth in 1..=INHERITANCE_DEPTH {
        let parent = if depth == 1 {
            "Root".to_string()
        } else {
            format!("Type{}", depth - 1)
        };
        writeln!(source, "type Type{depth} : {parent} {{}}").expect("write benchmark schema");
    }
    let modules = parse_modules([CftFile::from_source(ModuleId::from("bench"), source)]);
    build_schema(&modules, &CftDimensionInputs::default()).expect("benchmark schema must compile")
}

fn loaded_records() -> Vec<LoadedRecordDraft> {
    (0..RECORD_COUNT)
        .map(|index| {
            LoadedRecordDraft::new(
                format!("record_{index}"),
                "Type64",
                [("value", LoadedValueDraft::from(index as i64))],
            )
        })
        .collect()
}

fn build_model(schema: &coflow_cft::CftSchema, records: Vec<LoadedRecordDraft>) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_loaded_record(record);
    }
    builder.build().expect("benchmark model must build")
}
