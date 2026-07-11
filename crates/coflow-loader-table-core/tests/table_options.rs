#![allow(clippy::panic)]

use coflow_loader_table_core::TableSourceOptions;
use serde_json::json;

#[test]
fn rejects_duplicate_sheet_names() {
    let error = TableSourceOptions::decode(
        &json!({
            "sheets": [
                { "sheet": "Items", "type": "Item" },
                { "sheet": "Items", "type": "ArchivedItem" }
            ]
        }),
        "test source",
    )
    .expect_err("duplicate sheet should fail");

    assert_eq!(error.message, "test source defines duplicate sheet `Items`");
}

#[test]
fn rejects_multiple_source_columns_for_one_field() {
    let error = TableSourceOptions::decode(
        &json!({
            "sheets": [{
                "sheet": "Items",
                "columns": { "Name": "name", "Display Name": "name" }
            }]
        }),
        "test source",
    )
    .expect_err("duplicate field mapping should fail");

    assert_eq!(
        error.message,
        "test source sheet `Items` maps multiple columns to field `name`"
    );
}

#[test]
fn explicit_sheet_lookup_never_falls_back_to_a_type_match() {
    let options = TableSourceOptions::decode(
        &json!({
            "sheets": [
                { "sheet": "Active", "type": "Item", "key": "active_id" },
                { "sheet": "Archive", "type": "Item", "key": "archive_id" }
            ]
        }),
        "test source",
    )
    .expect("valid options");

    let archive = options
        .sheet_config("Archive", "Item")
        .expect("exact sheet lookup");
    assert_eq!(archive.key.as_deref(), Some("archive_id"));
}

#[test]
fn rejects_type_mismatch_for_an_explicit_sheet() {
    let options = TableSourceOptions::decode(
        &json!({ "sheets": [{ "sheet": "Items", "type": "Item" }] }),
        "test source",
    )
    .expect("valid options");

    let error = options
        .sheet_config("Items", "Skill")
        .expect_err("sheet/type mismatch should fail");
    assert_eq!(
        error.message,
        "test source sheet `Items` is configured for type `Item`, not `Skill`"
    );
}

#[test]
fn ambiguous_type_lookup_requires_an_explicit_sheet() {
    let options = TableSourceOptions::decode(
        &json!({
            "sheets": [
                { "sheet": "Active", "type": "Item" },
                { "sheet": "Archive", "type": "Item" }
            ]
        }),
        "test source",
    )
    .expect("valid options");

    let error = options
        .sheet_for_type("Item")
        .expect_err("ambiguous type lookup should fail");
    assert_eq!(
        error.message,
        "test source type `Item` is configured for multiple sheets (Active, Archive); specify a sheet"
    );
}

#[test]
fn missing_sheet_is_ambiguous_when_the_source_has_multiple_sheets() {
    let options = TableSourceOptions::decode(
        &json!({
            "sheets": [
                { "sheet": "Items", "type": "Item" },
                { "sheet": "Skills", "type": "Skill" }
            ]
        }),
        "test source",
    )
    .expect("valid options");

    let error = options
        .type_for_sheet(None)
        .expect_err("missing sheet should fail");
    assert_eq!(
        error.message,
        "test source defines multiple sheets; specify a sheet"
    );
}
