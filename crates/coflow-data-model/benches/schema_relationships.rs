#![allow(clippy::cast_possible_wrap, clippy::expect_used, clippy::print_stdout)]

use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, DimensionName, FieldName, ModuleId,
    RecordKey, TypeName, VariantName,
};
use coflow_data_model::{
    CfdDataModel, DimensionValueDraft, LoadedRecordDraft, LoadedValueDraft, RecordOrigin,
};
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
    let representative_schema = representative_schema();
    let (representative_records, dimension_values) = representative_records();
    let representative_model = build_model_with_dimensions(
        &representative_schema,
        representative_records.clone(),
        dimension_values.clone(),
    );
    assert_eq!(
        representative_model.direct_ref_edges().count(),
        RECORD_COUNT - 1
    );
    assert_eq!(
        representative_model.spread_edges().count(),
        RECORD_COUNT - 1
    );
    let representative_build_elapsed = measure(|| {
        for _ in 0..BUILD_ROUNDS {
            black_box(build_model_with_dimensions(
                &representative_schema,
                representative_records.clone(),
                dimension_values.clone(),
            ));
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
    println!(
        "representative model build: depth={INHERITANCE_DEPTH}, records={RECORD_COUNT}, refs={}, spreads={}, dimensions=2, variants=5, dimension_values={}, builds={BUILD_ROUNDS}, elapsed={representative_build_elapsed:?}",
        RECORD_COUNT - 1,
        RECORD_COUNT - 1,
        dimension_values.len(),
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

fn representative_schema() -> coflow_cft::CftSchema {
    let mut source = String::from(
        r#"type Root {
    value: int;
    target: &Root? = null;
    @localized name: string;
    @dimension("platform") label: string;
}
"#,
    );
    for depth in 1..=INHERITANCE_DEPTH {
        let parent = if depth == 1 {
            "Root".to_string()
        } else {
            format!("Type{}", depth - 1)
        };
        writeln!(source, "type Type{depth} : {parent} {{}}").expect("write benchmark schema");
    }
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("representative"),
        source,
    )]);
    let dimensions = CftDimensionInputs::try_new([
        (
            "language",
            vec!["zh".to_string(), "en".to_string(), "jp".to_string()],
        ),
        ("platform", vec!["pc".to_string(), "console".to_string()]),
    ])
    .expect("benchmark dimensions must be valid");
    build_schema(&modules, &dimensions).expect("representative benchmark schema must compile")
}

fn representative_records() -> (Vec<LoadedRecordDraft>, Vec<DimensionValueDraft>) {
    let mut records = Vec::with_capacity(RECORD_COUNT);
    records.push(LoadedRecordDraft::new(
        "record_0",
        "Type64",
        [
            ("value", LoadedValueDraft::from(0_i64)),
            ("target", LoadedValueDraft::Null),
            ("name", LoadedValueDraft::from("Record 0")),
            ("label", LoadedValueDraft::from("record_0")),
        ],
    ));
    for index in 1..RECORD_COUNT {
        records.push(LoadedRecordDraft::with_spreads(
            format!("record_{index}"),
            "Type64",
            [LoadedValueDraft::record_ref("record_0")],
            [
                ("value", LoadedValueDraft::from(index as i64)),
                ("target", LoadedValueDraft::record_ref("record_0")),
            ],
        ));
    }

    let mut dimension_values = Vec::with_capacity(RECORD_COUNT * 5);
    for index in 0..RECORD_COUNT {
        let key = RecordKey::new(format!("record_{index}")).expect("benchmark record key");
        for (field, dimension, variants) in [
            ("name", "language", ["zh", "en", "jp"].as_slice()),
            ("label", "platform", ["pc", "console"].as_slice()),
        ] {
            for variant in variants {
                dimension_values.push(DimensionValueDraft {
                    source_type: TypeName::new("Root").expect("benchmark source type"),
                    source_key: key.clone(),
                    field: FieldName::new(field).expect("benchmark field"),
                    dimension: DimensionName::new(dimension).expect("benchmark dimension"),
                    variant: VariantName::new(*variant).expect("benchmark variant"),
                    value: LoadedValueDraft::from(format!("{variant}_{index}")),
                    origin: RecordOrigin::None,
                });
            }
        }
    }
    (records, dimension_values)
}

fn build_model(schema: &coflow_cft::CftSchema, records: Vec<LoadedRecordDraft>) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_loaded_record(record);
    }
    builder.build().expect("benchmark model must build")
}

fn build_model_with_dimensions(
    schema: &coflow_cft::CftSchema,
    records: Vec<LoadedRecordDraft>,
    dimension_values: Vec<DimensionValueDraft>,
) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    for record in records {
        builder.add_loaded_record(record);
    }
    builder.add_dimension_value_drafts(dimension_values);
    builder
        .build()
        .expect("representative benchmark model must build")
}
