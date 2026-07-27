#![allow(clippy::expect_used)]

use coflow_api::ArtifactContent;
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
use coflow_data_model::{CfdDataModel, LoadedValueDraft};
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
