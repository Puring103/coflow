use super::*;

#[test]
fn table_provider_algorithms_are_not_reexported_by_excel_source_provider() {
    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/lib.rs")
        .expect("read excel loader");

    for forbidden in [
        "pub struct ExcelLoadOutput",
        "pub fn load_excel_model",
        "pub fn load_excel(",
        "pub struct TableSource",
        "pub fn collect_table_input_records",
        "pub use coflow_loader_table_core::TableSheet",
        "shared_table_source_from_excel_table_source",
    ] {
        assert!(
            !excel.contains(forbidden),
            "Excel loader should not expose table-core facade `{forbidden}`"
        );
    }
}

#[test]
fn excel_loader_options_do_not_live_in_lib_rs() {
    let lib =
        std::fs::read_to_string("crates/coflow-loader-excel/src/lib.rs").expect("read excel lib");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-excel/src/diagnostics.rs")
        .expect("read excel diagnostics");
    let options = std::fs::read_to_string("crates/coflow-loader-excel/src/options.rs")
        .expect("read excel options parser");
    let source = std::fs::read_to_string("crates/coflow-loader-excel/src/source.rs")
        .expect("read excel source");

    for expected in [
        "pub(crate) fn decode_excel_source_options",
        "pub(super) fn excel_sheets",
        "pub(crate) fn excel_source_options",
        "pub(crate) fn excel_sheet_config_from_options",
        "pub(crate) fn excel_sheet_for_type_from_options",
        "TableSourceOptions::decode(raw, \"excel source\")",
    ] {
        assert!(
            options.contains(expected),
            "Excel option parser item `{expected}` should live in options.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel option parser item `{expected}` should not live in lib.rs"
        );
    }
    for forbidden in [
        "fn excel_sheet_from_value",
        "fn optional_string_field",
        "options.get(\"sheets\")",
        ".as_array()",
    ] {
        assert!(
            !options.contains(forbidden),
            "Excel options should use the shared table options decoder instead of `{forbidden}`"
        );
    }
    for expected in [
        "pub struct ExcelDiagnostics",
        "pub struct ExcelDiagnostic",
        "pub struct ExcelLabel",
        "pub struct ExcelLocation",
        "pub fn map_label_with_record_offset",
        "pub(crate) fn excel_diagnostics_to_api",
        "fn excel_label_to_api",
    ] {
        assert!(
            diagnostics.contains(expected),
            "Excel diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct ExcelSource",
        "pub struct ExcelSheet",
        "pub struct ExcelInputRecords",
        "pub fn collect_input_records",
        "fn table_sources_from_excel",
        "fn table_source_from_excel",
        "fn cell_text",
        "fn unsupported_cell_diagnostic",
    ] {
        assert!(
            source.contains(expected),
            "Excel source item `{expected}` should live in source.rs"
        );
        assert!(
            !lib.contains(expected),
            "Excel source item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        lib.lines().count() < 360,
        "coflow-loader-excel lib.rs should stay below the 360-line focused-module threshold"
    );
}

#[test]
fn csv_dimension_source_sync_does_not_live_in_writer_rs() {
    let writer =
        std::fs::read_to_string("crates/coflow-loader-csv/src/writer.rs").expect("read csv writer");
    let dimensions = std::fs::read_to_string("crates/coflow-loader-csv/src/writer/dimensions.rs")
        .expect("read csv writer dimension source sync");
    let plan = std::fs::read_to_string("crates/coflow-loader-csv/src/writer/plan.rs")
        .expect("read csv writer plan helpers");
    let table_manager =
        std::fs::read_to_string("crates/coflow-loader-csv/src/writer/table_manager.rs")
            .expect("read csv writer table manager");

    for expected in [
        "impl DimensionSourceManager for CsvWriter",
        "fn sync_dimension_source",
        "struct DimensionCsvRow",
        "fn render_dimension_csv_value",
    ] {
        assert!(
            dimensions.contains(expected),
            "CSV dimension source item `{expected}` should live in writer/dimensions.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV dimension source item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) fn apply_plan",
        "fn mutate_csv",
        "fn set_csv_cell",
        "fn ensure_expected_key",
    ] {
        assert!(
            plan.contains(expected),
            "CSV writer plan item `{expected}` should live in writer/plan.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV writer plan item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "impl TableManager for CsvWriter",
        "pub static CSV_TABLE_MANAGER_DESCRIPTOR",
        "fn create_table",
        "fn sync_header",
        "fn added_columns",
        "fn removed_columns",
        "fn sync_rows_to_header",
    ] {
        assert!(
            table_manager.contains(expected),
            "CSV table manager item `{expected}` should live in writer/table_manager.rs"
        );
        assert!(
            !writer.contains(expected),
            "CSV table manager item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.lines().count() < 360,
        "coflow-loader-csv writer.rs should stay below the 360-line focused-module threshold"
    );
}

#[test]
fn excel_table_manager_does_not_live_in_writer_rs() {
    let writer = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")
        .expect("read excel writer");
    let table_manager =
        std::fs::read_to_string("crates/coflow-loader-excel/src/writer/table_manager.rs")
            .expect("read excel table manager");

    for expected in [
        "impl TableManager for ExcelWriter",
        "pub static EXCEL_TABLE_MANAGER_DESCRIPTOR",
        "fn create_table",
        "fn sync_header",
        "fn create_excel_file",
        "fn append_excel_sheet",
        "fn sync_excel_header",
    ] {
        assert!(
            table_manager.contains(expected),
            "Excel table manager item `{expected}` should live in writer/table_manager.rs"
        );
        assert!(
            !writer.contains(expected),
            "Excel table manager item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        writer.lines().count() < 650,
        "coflow-loader-excel writer.rs should stay below the 650-line focused-module threshold"
    );
}

#[test]
fn csv_loader_helpers_do_not_live_in_lib_rs() {
    let lib = std::fs::read_to_string("crates/coflow-loader-csv/src/lib.rs").expect("read csv lib");
    let format =
        std::fs::read_to_string("crates/coflow-loader-csv/src/format.rs").expect("read csv format");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-csv/src/diagnostics.rs")
        .expect("read csv diagnostics");
    let options = std::fs::read_to_string("crates/coflow-loader-csv/src/options.rs")
        .expect("read csv options");
    let source =
        std::fs::read_to_string("crates/coflow-loader-csv/src/source.rs").expect("read csv source");

    for expected in ["pub fn parse", "pub fn write", "fn write_cell"] {
        assert!(
            format.contains(expected),
            "CSV format item `{expected}` should live in format.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV format item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub(crate) fn decode_csv_source_options",
        "pub(super) fn csv_sheets",
        "pub(crate) fn csv_source_options",
        "pub(crate) fn csv_sheet_config_from_options",
        "TableSourceOptions::decode(raw, \"csv source\")",
    ] {
        assert!(
            options.contains(expected),
            "CSV option parser item `{expected}` should live in options.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV option parser item `{expected}` should not live in lib.rs"
        );
    }
    for forbidden in [
        "fn csv_sheet_from_value",
        "fn optional_string_field",
        "options.get(\"sheets\")",
        ".as_array()",
    ] {
        assert!(
            !options.contains(forbidden),
            "CSV options should use the shared table options decoder instead of `{forbidden}`"
        );
    }
    for expected in [
        "pub struct CsvDiagnostics",
        "pub struct CsvDiagnostic",
        "pub struct CsvLocation",
        "pub fn csv_diagnostics_to_api",
        "fn csv_label_to_api",
    ] {
        assert!(
            diagnostics.contains(expected),
            "CSV diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    for expected in [
        "pub struct CsvSource",
        "pub struct CsvSheet",
        "pub struct CsvInputRecords",
        "pub fn collect_input_records",
        "fn table_sources_from_csv",
        "fn table_source_from_csv",
        "fn default_sheet_name",
    ] {
        assert!(
            source.contains(expected),
            "CSV source item `{expected}` should live in source.rs"
        );
        assert!(
            !lib.contains(expected),
            "CSV source item `{expected}` should not live in lib.rs"
        );
    }
    assert!(
        lib.lines().count() < 800,
        "coflow-loader-csv lib.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_writer_is_split_by_responsibility() {
    let writer =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/writer.rs").expect("read cfd writer");
    let dimensions = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/dimensions.rs")
        .expect("read cfd writer dimension source sync");
    let patch = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/patch.rs")
        .expect("read cfd writer patch helpers");
    let render = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/render.rs")
        .expect("read cfd writer render helpers");
    let schema_nav = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/schema_nav.rs")
        .expect("read cfd writer schema navigation helpers");
    let target = std::fs::read_to_string("crates/coflow-loader-cfd/src/writer/target.rs")
        .expect("read cfd writer target locator helpers");

    for expected in [
        "impl DimensionSourceManager for CfdWriter",
        "fn sync_dimension_source",
        "struct DimensionCfdRow",
        "fn read_existing_dimension_cfd",
    ] {
        assert!(
            dimensions.contains(expected),
            "CFD dimension source item `{expected}` should live in writer/dimensions.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD dimension source item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) fn apply_patch",
        "pub(super) fn find_record",
        "pub(super) fn replace_spans",
    ] {
        assert!(
            patch.contains(expected),
            "CFD patch item `{expected}` should live in writer/patch.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD patch item `{expected}` should not live in writer.rs"
        );
    }
    for expected in [
        "pub(super) enum WriteTarget",
        "pub(super) fn locate_target",
        "pub(super) fn spread_entries_at_path",
        "fn block_entries_at_path",
    ] {
        assert!(
            target.contains(expected),
            "CFD target locator item `{expected}` should live in writer/target.rs"
        );
        assert!(
            !patch.contains(expected) && !writer.contains(expected),
            "CFD target locator item `{expected}` should not live in writer.rs or writer/patch.rs"
        );
    }
    for expected in [
        "pub(super) fn cfd_top_level_fields",
        "pub(super) fn rewrite_cfd_records",
        "pub(super) fn serialize_value",
        "pub(super) fn serialize_value_for_type",
    ] {
        assert!(
            render.contains(expected),
            "CFD render item `{expected}` should live in writer/render.rs"
        );
        assert!(
            !writer.contains(expected),
            "CFD render item `{expected}` should not live in writer.rs"
        );
    }
    assert!(
        schema_nav.contains("pub(super) fn type_after_field_segment")
            && schema_nav.contains("pub(super) fn dict_key_path_matches"),
        "CFD writer schema navigation helpers should live in writer/schema_nav.rs"
    );
    assert!(
        writer.lines().count() < 800,
        "coflow-loader-cfd writer.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_loader_lowers_the_canonical_syntax_tree() {
    let lib =
        std::fs::read_to_string("crates/coflow-loader-cfd/src/lib.rs").expect("read cfd loader");
    let lower = std::fs::read_to_string("crates/coflow-loader-cfd/src/lower.rs")
        .expect("read cfd lowering");
    let diagnostics = std::fs::read_to_string("crates/coflow-loader-cfd/src/diagnostics.rs")
        .expect("read cfd diagnostics");

    for expected in ["parse_cfd(source)", "lower_records"] {
        assert!(
            lib.contains(expected),
            "CFD loader entry should use canonical syntax step `{expected}`"
        );
    }
    assert!(
        lower.contains("pub(super) struct ParsedCfdInputRecord")
            && lower.contains("pub(super) fn lower_records"),
        "CFD schema-guided record lowering should live in lower.rs"
    );
    for expected in [
        "pub enum CfdTextLoadError",
        "pub struct CfdTextDiagnostics",
        "pub struct CfdTextDiagnostic",
        "pub enum CfdTextErrorCode",
        "pub(super) fn cfd_error_to_diagnostics",
    ] {
        assert!(
            diagnostics.contains(expected),
            "CFD diagnostic item `{expected}` should live in diagnostics.rs"
        );
        assert!(
            !lib.contains(expected),
            "CFD diagnostic item `{expected}` should not live in lib.rs"
        );
    }
    for obsolete in [
        "crates/coflow-loader-cfd/src/parser.rs",
        "crates/coflow-loader-cfd/src/parser/lexer.rs",
        "crates/coflow-loader-cfd/src/parser/schema.rs",
        "crates/coflow-loader-cfd/src/parser/value.rs",
    ] {
        assert!(
            !std::path::Path::new(obsolete).exists(),
            "CFD syntax must have one parser in coflow-cfd; obsolete loader parser remains at {obsolete}"
        );
    }
    assert!(
        lib.lines().count() < 800,
        "coflow-loader-cfd lib.rs should stay below the 800-line large-module threshold"
    );
}

#[test]
fn cfd_syntax_parser_token_helpers_are_split_out() {
    let parser =
        std::fs::read_to_string("crates/coflow-cfd/src/parser.rs").expect("read CFD syntax parser");
    let tokens = std::fs::read_to_string("crates/coflow-cfd/src/parser/tokens.rs")
        .expect("read CFD syntax parser token helpers");

    for expected in [
        "pub(super) struct Token",
        "pub(super) fn parse_key",
        "fn parse_name_token",
        "pub(super) fn parse_quoted_string",
        "pub(super) fn skip_ws_and_comments",
        "fn is_value_boundary",
    ] {
        assert!(
            tokens.contains(expected),
            "CFD syntax parser token helper `{expected}` should live in parser/tokens.rs"
        );
        assert!(
            !parser.contains(expected),
            "CFD syntax parser token helper `{expected}` should not live in parser.rs"
        );
    }
    assert!(
        parser.lines().count() < 400,
        "coflow-cfd parser.rs should stay focused on CFD AST structure parsing"
    );
}


#[test]
fn loaders_do_not_depend_on_checker_runtime_directly() {
    let excel_manifest = std::fs::read_to_string("crates/coflow-loader-excel/Cargo.toml")
        .expect("read excel loader manifest");

    assert!(
        !excel_manifest.contains("coflow-checker"),
        "loaders should only produce input records; model checks belong in coflow-runtime"
    );
}


