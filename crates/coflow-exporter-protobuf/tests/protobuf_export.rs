#![allow(clippy::expect_used)]

use coflow_api::ArtifactContent;
use coflow_cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, DimensionName, FieldName, ModuleId,
    RecordKey, TypeName, VariantName,
};
use coflow_data_model::{CfdDataModel, DimensionValueDraft, LoadedValueDraft, RecordOrigin};
use coflow_exporter_protobuf::export_protobuf_artifacts;

#[test]
fn emits_contract_and_deterministic_sint64_payload() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "enum Rarity { Common = 0, Rare = 10, } type Item { rarity: Rarity; }",
    )]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "x",
        "Item",
        [("rarity", LoadedValueDraft::enum_variant("Rarity", "Rare"))],
    );
    let model = builder.build().expect("model");

    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let contract = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "_schema/coflow.proto")
        .expect("contract");
    let ArtifactContent::Text(contract) = &contract.content else {
        panic!("contract should be text");
    };
    assert!(contract.contains("sint64 rarity = 16;"));
    assert!(contract.contains("reserved 2 to 15;"));

    let data = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "Item.pb")
        .expect("table data");
    let ArtifactContent::Bytes(data) = &data.content else {
        panic!("table should contain bytes");
    };
    assert_eq!(data, &[0x0a, 0x06, 0x0a, 0x01, b'x', 0x80, 0x01, 0x14]);
}

#[test]
fn assigns_user_tags_by_canonical_field_name() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type Item { zed: int; alpha: string; }",
    )]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let model = CfdDataModel::builder(&schema).build().expect("model");
    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let contract = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "_schema/coflow.proto")
        .expect("contract");
    let ArtifactContent::Text(contract) = &contract.content else {
        panic!("contract should be text");
    };
    assert!(contract.contains("string alpha = 16;"));
    assert!(contract.contains("sint64 zed = 17;"));
}

#[test]
fn nested_nullable_collection_bytes_match_generated_wrapper_contract() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type Item { attrs: {string: [string]?}; }",
    )]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "x",
        "Item",
        [(
            "attrs",
            LoadedValueDraft::dict([(
                "slot".into(),
                LoadedValueDraft::Array(vec!["a".into(), "b".into()]),
            )]),
        )],
    );
    let model = builder.build().expect("model");

    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let contract = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "_schema/coflow.proto")
        .expect("contract");
    let ArtifactContent::Text(contract) = &contract.content else {
        panic!("contract should be text");
    };
    assert!(contract.contains("ItemAttrsEntryValueNullable value = 2;"));
    assert!(contract.contains("optional ItemAttrsEntryValueValueArray value = 1;"));
    assert!(contract.contains("repeated string items = 1;"));

    let data = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "Item.pb")
        .expect("table data");
    let ArtifactContent::Bytes(data) = &data.content else {
        panic!("table should contain bytes");
    };
    assert_eq!(
        data,
        &[
            0x0a, 0x16, 0x0a, 0x01, b'x', 0x82, 0x01, 0x10, 0x0a, 0x04, b's', b'l', b'o', b't',
            0x12, 0x08, 0x0a, 0x06, 0x0a, 0x01, b'a', 0x0a, 0x01, b'b',
        ]
    );
}

#[test]
fn rejects_projected_protobuf_name_collisions() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type FooBar {} type foo_bar {}",
    )]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let model = CfdDataModel::builder(&schema).build().expect("model");

    let error = export_protobuf_artifacts(&schema, &model).expect_err("collision");
    assert!(error
        .to_string()
        .contains("duplicate Protobuf name `FooBar`"));
}

#[test]
fn struct_types_only_emit_inline_messages() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "@struct sealed type Details { note: string; } type Item { details: Details; }",
    )]);
    let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
    let model = CfdDataModel::builder(&schema).build().expect("model");

    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let contract = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "_schema/coflow.proto")
        .expect("contract");
    let ArtifactContent::Text(contract) = &contract.content else {
        panic!("contract should be text");
    };

    assert!(contract.contains("message Details {"));
    assert!(!contract.contains("message DetailsTable {"));
    assert!(!artifacts
        .files()
        .iter()
        .any(|file| file.relative_path.to_string_lossy() == "Details.pb"));
}

#[test]
fn exports_dimension_variant_tables() {
    let dimensions = CftDimensionInputs::try_new([(
        "language",
        vec!["en".to_string(), "zh".to_string()],
    )])
    .expect("dimensions");
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "type Item { @localized name: string; }",
    )]);
    let schema = build_schema(&modules, &dimensions).expect("schema");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("potion", "Item", [("name", LoadedValueDraft::from("Potion"))]);
    builder.add_dimension_value_draft(DimensionValueDraft {
        source_type: TypeName::new("Item").expect("type"),
        source_key: RecordKey::new("potion").expect("key"),
        field: FieldName::new("name").expect("field"),
        dimension: DimensionName::new("language").expect("dimension"),
        variant: VariantName::new("zh").expect("variant"),
        value: LoadedValueDraft::from("药水"),
        origin: RecordOrigin::None,
    });
    let model = builder.build().expect("model");

    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let contract = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "_schema/coflow.proto")
        .expect("contract");
    let ArtifactContent::Text(contract) = &contract.content else {
        panic!("contract should be text");
    };
    assert!(contract.contains("message ItemNameVariants {"));
    assert!(contract.contains("optional string default = 16;"));
    assert!(contract.contains("optional string en = 17;"));
    assert!(contract.contains("optional string zh = 18;"));
    assert!(contract.contains("message ItemNameVariantsTable {"));

    let data = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "Item_nameVariants.pb")
        .expect("dimension table data");
    let ArtifactContent::Bytes(data) = &data.content else {
        panic!("dimension table should contain bytes");
    };
    assert!(data.windows(b"Potion".len()).any(|bytes| bytes == b"Potion"));
    assert!(data.windows("药水".len()).any(|bytes| bytes == "药水".as_bytes()));
    assert!(!data.windows(2).any(|bytes| bytes == [0x8a, 0x01]));
}

#[test]
fn exports_singleton_dimension_variant_tables() {
    let dimensions = CftDimensionInputs::try_new([(
        "language",
        vec!["en".to_string(), "zh".to_string()],
    )])
    .expect("dimensions");
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        "@singleton type Settings { @localized title: string; }",
    )]);
    let schema = build_schema(&modules, &dimensions).expect("schema");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Settings",
        "Settings",
        [("title", LoadedValueDraft::from("Default title"))],
    );
    builder.add_dimension_value_draft(DimensionValueDraft {
        source_type: TypeName::new("Settings").expect("type"),
        source_key: RecordKey::new("Settings").expect("key"),
        field: FieldName::new("title").expect("field"),
        dimension: DimensionName::new("language").expect("dimension"),
        variant: VariantName::new("zh").expect("variant"),
        value: LoadedValueDraft::from("设置"),
        origin: RecordOrigin::None,
    });
    let model = builder.build().expect("model");

    let artifacts = export_protobuf_artifacts(&schema, &model).expect("protobuf export");
    let data = artifacts
        .files()
        .iter()
        .find(|file| file.relative_path.to_string_lossy() == "Settings_titleVariants.pb")
        .expect("singleton dimension table data");
    let ArtifactContent::Bytes(data) = &data.content else {
        panic!("dimension table should contain bytes");
    };
    assert!(data.windows(b"title".len()).any(|bytes| bytes == b"title"));
    assert!(data
        .windows("Default title".len())
        .any(|bytes| bytes == b"Default title"));
    assert!(data.windows("设置".len()).any(|bytes| bytes == "设置".as_bytes()));
}
