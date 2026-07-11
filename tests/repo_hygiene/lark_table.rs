use super::*;

#[test]

fn lark_loader_dto_types_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let dto =

        std::fs::read_to_string("crates/coflow-loader-lark/src/dto.rs").expect("read lark dto");



    for expected in [

        "pub(crate) struct AuthResponse",

        "pub(crate) struct ApiEnvelope",

        "pub(crate) struct WikiNodeData",

        "pub(crate) struct SheetsQueryData",

        "pub(crate) struct LarkSheetMetadata",

        "pub(crate) struct ValuesData",

        "pub(crate) struct ValueRange",

    ] {

        assert!(

            dto.contains(expected),

            "Lark loader DTO `{expected}` should live in dto.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark loader DTO `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lark_loader_diagnostics_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let diagnostics = std::fs::read_to_string("crates/coflow-loader-lark/src/diagnostics.rs")

        .expect("read lark diagnostics");



    for expected in [

        "pub struct LarkDiagnostics",

        "pub struct LarkDiagnostic",

        "pub(crate) fn lark_diagnostics_to_api",

        "pub(crate) fn table_diagnostics_to_api",

        "pub(crate) fn table_write_diagnostics_to_api",

    ] {

        assert!(

            diagnostics.contains(expected),

            "Lark diagnostics helper `{expected}` should live in diagnostics.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark diagnostics helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lark_loader_source_parsing_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let source = std::fs::read_to_string("crates/coflow-loader-lark/src/source.rs")

        .expect("read lark source");



    for expected in [

        "pub struct LarkSheetSource",

        "pub enum LarkSheetLocator",

        "pub(crate) fn decode_lark_source_options",

        "pub(crate) fn lark_source_options",

        "pub(crate) fn lark_source_from_spec",

        "pub(crate) fn sheet_config_from_options",

        "pub(crate) fn sheet_for_type_from_options",

        "TableSourceOptions::decode(raw, \"lark source\")",

        "pub(crate) fn lark_document",

        "pub(crate) fn lark_document_spreadsheet_token",

    ] {

        assert!(

            source.contains(expected),

            "Lark source helper `{expected}` should live in source.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark source helper `{expected}` should not live in lib.rs"

        );

    }

    for forbidden in [

        "fn table_sheet_config_from_value",

        "fn optional_string_field",

        "options.get(\"sheets\")",

        ".as_array()",

    ] {

        assert!(

            !source.contains(forbidden),

            "Lark source should use the shared table options decoder instead of `{forbidden}`"

        );

    }

}



#[test]

fn table_source_options_decode_lives_in_table_core() {

    let table_options = std::fs::read_to_string("crates/coflow-loader-table-core/src/options.rs")

        .expect("read table options");

    let table_lib = std::fs::read_to_string("crates/coflow-loader-table-core/src/lib.rs")

        .expect("read table core lib");

    let csv_writer = std::fs::read_to_string("crates/coflow-loader-csv/src/writer.rs")

        .expect("read csv writer");

    let excel_writer = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")

        .expect("read excel writer");

    let lark_write = std::fs::read_to_string("crates/coflow-loader-lark/src/write.rs")

        .expect("read lark writer");



    for expected in [

        "pub struct TableSourceOptions",

        "pub fn decode(",

        "pub fn sheet_config(",

        "pub fn sheet_for_type(",

        "fn table_sheet_config_from_value",

        "fn optional_string_field",

    ] {

        assert!(

            table_options.contains(expected),

            "shared table option helper `{expected}` should live in table-core options.rs"

        );

    }

    assert!(

        table_lib.contains("pub use options::{TableOptionsError, TableSourceOptions};"),

        "table-core should expose the typed table source options facade"

    );

    for (name, writer) in [

        ("CSV writer", csv_writer.as_str()),

        ("Excel writer", excel_writer.as_str()),

        ("Lark writer", lark_write.as_str()),

    ] {

        for forbidden in ["options.get(\"sheets\")", ".as_array()", "Value::as_object"] {

            assert!(

                !writer.contains(forbidden),

                "{name} should not parse table source option JSON with `{forbidden}`"

            );

        }

    }

}



#[test]

fn lark_loader_http_client_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let http =

        std::fs::read_to_string("crates/coflow-loader-lark/src/http.rs").expect("read lark http");



    for expected in [

        "pub trait LarkHttpClient",

        "pub struct UreqLarkHttpClient",

        "impl LarkHttpClient for UreqLarkHttpClient",

        "fn ureq_error_message",

    ] {

        assert!(

            http.contains(expected),

            "Lark HTTP helper `{expected}` should live in http.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark HTTP helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lark_loader_load_pipeline_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let load =

        std::fs::read_to_string("crates/coflow-loader-lark/src/load.rs").expect("read lark load");



    for expected in [

        "pub fn load_lark_table_source",

        "pub fn load_lark_table_source_with_client",

        "pub struct LarkSheetLoader",

        "pub const LARK_SHEET_LOADER_DESCRIPTOR",

        "fn tenant_access_token",

        "fn spreadsheet_metadata",

        "fn sheet_values",

    ] {

        assert!(

            load.contains(expected),

            "Lark load helper `{expected}` should live in load.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark load helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lark_loader_writer_cache_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let writer_cache = std::fs::read_to_string("crates/coflow-loader-lark/src/writer_cache.rs")

        .expect("read lark writer cache");



    for expected in [

        "pub(crate) struct LarkWriterCache",

        "struct CachedToken",

        "pub(crate) struct LarkWriteAuth",

        "pub(crate) fn cached_tenant_token",

        "pub(crate) fn cached_sheet_id",

        "pub(crate) fn invalidate_caches",

        "pub(crate) fn lark_write_auth",

        "fn lark_tenant_token_with_ttl",

        "pub(crate) fn fetch_sheet_id_map",

    ] {

        assert!(

            writer_cache.contains(expected),

            "Lark writer cache helper `{expected}` should live in writer_cache.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark writer cache helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lark_loader_write_operations_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-loader-lark/src/lib.rs")

        .expect("read lark loader lib");

    let write =

        std::fs::read_to_string("crates/coflow-loader-lark/src/write.rs").expect("read lark write");

    let write_http = std::fs::read_to_string("crates/coflow-loader-lark/src/write_http.rs")

        .expect("read lark write http");

    let write_layout = std::fs::read_to_string("crates/coflow-loader-lark/src/write_layout.rs")

        .expect("read lark write layout");



    for expected in [

        "impl<C> SourceWriter for LarkSheetWriter<C>",

        "pub static LARK_SHEET_WRITER_DESCRIPTOR",

    ] {

        assert!(

            write.contains(expected),

            "Lark writer operation `{expected}` should live in write.rs"

        );

        assert!(

            !lib.contains(expected),

            "Lark writer operation `{expected}` should not live in lib.rs"

        );

    }



    for expected in [

        "fn append_lark_row",

        "fn create_lark_sheet",

        "fn write_lark_header",

        "fn delete_lark_row",

        "fn read_lark_cell",

        "fn read_lark_header",

        "fn send_lark_write",

        "fn send_values_batch_update",

        "fn parse_write_envelope",

        "enum LarkWriteFailure",

        "enum LarkHttpMethod",

    ] {

        assert!(

            write_http.contains(expected),

            "Lark HTTP write helper `{expected}` should live in write_http.rs"

        );

        assert!(

            !lib.contains(expected) && !write.contains(expected),

            "Lark HTTP write helper `{expected}` should not live in lib.rs or write.rs"

        );

    }



    for expected in [

        "struct LarkInsertLayoutRequest",

        "fn lark_insert_layout",

    ] {

        assert!(

            write_layout.contains(expected),

            "Lark write layout helper `{expected}` should live in write_layout.rs"

        );

        assert!(

            !lib.contains(expected) && !write.contains(expected),

            "Lark write layout helper `{expected}` should not live in lib.rs or write.rs"

        );

    }

    assert!(
        write.contains("plan_field_write"),
        "Lark field writes should reuse the shared table-core field write planner"
    );
    assert!(
        !write.contains("resolve_lark_column"),
        "Lark field writes should not duplicate table-core column resolution"
    );

}



#[test]

fn table_writers_use_shared_cell_renderer() {

    let excel = std::fs::read_to_string("crates/coflow-loader-excel/src/writer.rs")

        .expect("read excel writer");

    let table_writer = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer.rs")

        .expect("read table writer");

    let table_writer_cells =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/cells.rs")

            .expect("read table writer cells");

    let lark = std::fs::read_to_string("crates/coflow-loader-lark/src/write.rs")

        .expect("read lark writer");



    assert!(

        excel.contains("coflow_loader_table_core::writer::{")

            && table_writer.contains("mod cells;")

            && table_writer_cells.contains("use crate::cell_value::render_cell_value;")

            && table_writer_cells.contains("render_cell_value(value).map_err(table_render_error)"),

        "Excel writer should use the shared table-core writer and renderer"

    );

    assert!(

        lark.contains("plan_field_write") && lark.contains("table_write_diagnostics_to_api"),

        "Lark writer should use the shared table-core field planner and renderer"

    );

    for forbidden in ["fn render_cell_value(value:", "fn render_lark_cell_value"] {

        assert!(

            !excel.contains(forbidden),

            "Excel writer should not duplicate shared renderer `{forbidden}`"

        );

        assert!(

            !lark.contains(forbidden),

            "Lark writer should not duplicate shared renderer `{forbidden}`"

        );

    }

}



#[test]

fn table_core_writer_diagnostics_do_not_live_in_writer_rs() {

    let writer = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer.rs")

        .expect("read table core writer");

    let cells = std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/cells.rs")

        .expect("read table core writer cell rendering");

    let diagnostics =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/writer/diagnostics.rs")

            .expect("read table core writer diagnostics");



    for expected in [

        "pub struct TableWriteDiagnostics",

        "pub struct TableWriteDiagnostic",

        "pub(super) fn one_error",

        "pub(super) fn table_render_error",

    ] {

        assert!(

            diagnostics.contains(expected),

            "table writer diagnostic item `{expected}` should live in writer/diagnostics.rs"

        );

        assert!(

            !writer.contains(expected),

            "table writer diagnostic item `{expected}` should not live in writer.rs"

        );

    }

    assert!(

        writer.lines().count() < 330,

        "coflow-loader-table-core writer.rs should stay below the 450-line focused-module threshold"

    );

    for expected in [

        "pub(super) fn render_insert_value",

        "pub(super) fn render_field_cells",

        "fn root_value_for_path",

        "fn replace_subvalue",

        "fn resolve_column",

        "fn direct_child_columns",

        "format_cfd_dict_key",

    ] {

        assert!(

            cells.contains(expected),

            "table writer cell item `{expected}` should live in writer/cells.rs"

        );

        assert!(

            !writer.contains(expected),

            "table writer cell item `{expected}` should not live in writer.rs"

        );

    }

}



#[test]

fn table_loader_core_table_rs_is_split_by_responsibility() {

    let table = std::fs::read_to_string("crates/coflow-loader-table-core/src/table.rs")

        .expect("read table core loader");

    let types = std::fs::read_to_string("crates/coflow-loader-table-core/src/table/types.rs")

        .expect("read table core types");

    let diagnostics =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/table/diagnostics.rs")

            .expect("read table core diagnostics");

    let columns = std::fs::read_to_string("crates/coflow-loader-table-core/src/table/columns.rs")

        .expect("read table core columns");



    for expected in [

        "pub struct TableSheetConfig",

        "pub struct TableSource",

        "pub struct TableDiagnostic",

        "pub struct TableLocation",

    ] {

        assert!(

            types.contains(expected),

            "table type `{expected}` should live in table/types.rs"

        );

        assert!(

            !table.contains(expected),

            "table type `{expected}` should not live in table.rs"

        );

    }

    for expected in [

        "pub(super) enum TableLoadError",

        "pub(super) fn table_load_error_diagnostics",

    ] {

        assert!(

            diagnostics.contains(expected),

            "table diagnostic item `{expected}` should live in table/diagnostics.rs"

        );

        assert!(

            !table.contains(expected),

            "table diagnostic item `{expected}` should not live in table.rs"

        );

    }

    for expected in [

        "struct ColumnResolution",

        "pub(super) fn resolve_columns",

        "pub(super) fn field_columns_from_resolved",

    ] {

        assert!(

            columns.contains(expected),

            "table column item `{expected}` should live in table/columns.rs"

        );

        assert!(

            !table.contains(expected),

            "table column item `{expected}` should not live in table.rs"

        );

    }

    assert!(

        table.lines().count() < 800,

        "coflow-loader-table-core table.rs should stay below the 800-line large-module threshold"

    );

    assert!(

        table.contains("pub fn resolve_table_write_layout(\r\n    schema: &CompiledSchema")

            || table.contains("pub fn resolve_table_write_layout(\n    schema: &CompiledSchema"),

        "table write layout should receive the shared schema view instead of rebuilding it"

    );

    assert!(

        !table.contains("CompiledSchema::new(schema)"),

        "table write layout should not rebuild schema view internally"

    );

}



#[test]

fn table_cell_value_is_split_by_responsibility() {

    let cell_value =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/mod.rs")

            .expect("read table core cell value parser");

    let collections =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/collections.rs")

            .expect("read table core cell value collections");

    let diagnostics =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/diagnostics.rs")

            .expect("read table core cell value diagnostics");

    let markers =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/markers.rs")

            .expect("read table core cell value markers");

    let objects =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/objects.rs")

            .expect("read table core cell value objects");

    let refs = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/refs.rs")

        .expect("read table core cell value refs");

    let render =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/render.rs")

            .expect("read table core cell value renderer");

    let scan = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/scan.rs")

        .expect("read table core cell value scanner");

    let strings =

        std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/strings.rs")

            .expect("read table core cell value strings");

    let types = std::fs::read_to_string("crates/coflow-loader-table-core/src/cell_value/types.rs")

        .expect("read table core cell value type parser");



    for expected in [

        "pub struct CellValueDiagnostics",

        "pub struct CellValueDiagnostic",

        "pub enum CellValueErrorCode",

        "pub(super) fn syntax",

        "pub(super) fn type_mismatch",

    ] {

        assert!(

            diagnostics.contains(expected),

            "cell value diagnostic item `{expected}` should live in cell_value/diagnostics.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value diagnostic item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub enum CellRenderError",

        "pub fn render_cell_value",

        "fn render_array",

        "fn render_dict",

        "pub(super) fn render_string",

    ] {

        assert!(

            render.contains(expected),

            "cell value render item `{expected}` should live in cell_value/render.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value render item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) fn split_top_level",

        "pub(super) fn find_top_level_char",

        "pub(super) fn strip_outer_pair",

        "pub(super) fn find_marker_open_brace",

        "struct ScanState",

    ] {

        assert!(

            scan.contains(expected),

            "cell value scanner item `{expected}` should live in cell_value/scan.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value scanner item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) enum CellType",

        "struct TypeParser",

        "pub(super) struct FieldMeta",

        "pub(super) fn full_fields",

        "fn field_meta",

    ] {

        assert!(

            types.contains(expected),

            "cell value type item `{expected}` should live in cell_value/types.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value type item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_string",

        "pub(super) fn string_needs_quotes",

    ] {

        assert!(

            strings.contains(expected),

            "cell value string item `{expected}` should live in cell_value/strings.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value string item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) fn looks_like_bare_record_key",

        "pub(super) fn is_type_marker_name",

    ] {

        assert!(

            markers.contains(expected),

            "cell value marker item `{expected}` should live in cell_value/markers.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value marker item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in ["pub(super) fn parse_ref"] {

        assert!(

            refs.contains(expected),

            "cell value ref item `{expected}` should live in cell_value/refs.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value ref item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_object",

        "fn validate_actual_type",

        "fn parse_named_object",

        "fn parse_positional_object",

        "fn object_value",

        "struct ObjectContent",

        "fn object_content",

    ] {

        assert!(

            objects.contains(expected),

            "cell value object item `{expected}` should live in cell_value/objects.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value object item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    for expected in [

        "pub(super) fn parse_array",

        "fn reject_comma_array_item",

        "pub(super) fn parse_dict",

        "fn parse_dict_key",

    ] {

        assert!(

            collections.contains(expected),

            "cell value collection item `{expected}` should live in cell_value/collections.rs"

        );

        assert!(

            !cell_value.contains(expected),

            "cell value collection item `{expected}` should not live in cell_value/mod.rs"

        );

    }

    assert!(

        cell_value.lines().count() < 800,

        "coflow-loader-table-core cell_value/mod.rs should stay below the 800-line large-module threshold"

    );

}



