use super::*;

#[test]

fn lsp_is_documented_as_schema_only_not_engine_runtime_host() {

    let architecture = std::fs::read_to_string("website/docs/docs/reference/12-architecture.md")

        .expect("read architecture reference");



    assert!(

        !architecture.contains("LSP --> Engine"),

        "architecture reference should not show coflow-lsp depending on coflow-runtime while the LSP remains schema-only"

    );

    assert!(

        !architecture.contains("LSP --> Builtins"),

        "architecture reference should not show coflow-lsp depending on builtins/provider registry while the LSP remains schema-only"

    );

    assert!(

        architecture.contains("schema-only"),

        "architecture reference should explicitly document why coflow-lsp does not depend on engine/builtins"

    );

}



#[test]

fn lsp_protocol_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let protocol =

        std::fs::read_to_string("crates/coflow-lsp/src/protocol.rs").expect("read lsp protocol");



    for expected in [

        "pub(crate) struct TextRequest",

        "pub(crate) fn read_message",

        "pub(crate) fn did_open_document",

        "pub(crate) fn text_document_uri",

    ] {

        assert!(

            protocol.contains(expected),

            "LSP protocol helper `{expected}` should live in protocol.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP protocol helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_state_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let state = std::fs::read_to_string("crates/coflow-lsp/src/state.rs").expect("read lsp state");



    for expected in [

        "pub(crate) struct LspBuild",

        "pub(crate) struct LspDocument",

        "pub(crate) fn current_type_at",

        "pub(crate) fn current_field_at",

        "pub(crate) fn type_of_chain",

        "pub(crate) fn field_by_type",

        "pub(crate) fn field_by_chain",

        "pub(crate) fn enum_variant_by_chain",

        "pub(crate) fn enum_name_exists",

        "pub(crate) fn enum_variant_exists",

        "pub(crate) fn quantifier_bindings_at",

        "fn collect_quantifier_bindings",

    ] {

        assert!(

            state.contains(expected),

            "LSP state helper `{expected}` should live in state.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP state helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_validation_core_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let validation = std::fs::read_to_string("crates/coflow-lsp/src/validation.rs")

        .expect("read lsp validation");

    let snapshot = std::fs::read_to_string("crates/coflow-lsp/src/validation/snapshot.rs")

        .expect("read lsp validation snapshot");



    for expected in [

        "pub(crate) struct LspValidationCore",

        "pub(crate) struct OpenDocument",

        "pub(crate) struct DiagnosticPublication",

        "pub(crate) enum LspRequestDocument",

        "pub(crate) fn open_document",

        "pub(crate) fn validate_project",

        "pub(crate) fn ensure_build_publications",

        "pub(crate) fn prepare_request_document",

        "pub(crate) fn request_document",

        "pub(crate) fn is_cfd_path",

    ] {

        assert!(

            validation.contains(expected),

            "LSP validation core item `{expected}` should live in validation.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP validation core item `{expected}` should not live in lib.rs"

        );

    }

    for private in ["struct CfdProjectSource", "fn collect_cfd_sources"] {

        assert!(

            snapshot.contains(private) && !lib.contains(private),

            "LSP CFD source adapter `{private}` should remain private to validation snapshot construction"

        );

    }

    for expected in [

        "pub(crate) struct ValidationSnapshot",

        "pub(crate) struct ValidationInput",

        "pub(crate) fn build_snapshot",

    ] {

        assert!(

            snapshot.contains(expected) && !lib.contains(expected),

            "LSP revision item `{expected}` should live behind the validation module"

        );

    }



    assert!(

        lib.contains("core: LspValidationCore"),

        "LspServer should keep validation state behind LspValidationCore"

    );

    for forbidden in ["parse_cfd", "cfd_source_by_uri"] {
        assert!(
            !lib.contains(forbidden),
            "LSP request handlers should use validation request context instead of `{forbidden}`"
        );
    }

}



#[test]

fn lsp_text_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let text = std::fs::read_to_string("crates/coflow-lsp/src/text.rs").expect("read lsp text");



    for expected in [

        "pub(crate) struct WordAt",

        "pub(crate) fn is_trivia_position",

        "pub(crate) fn is_inside_string",

        "pub(crate) fn dotted_chain_at",

        "pub(crate) fn word_at",

        "pub(crate) fn parse_dotted_ident_chain",

        "pub(crate) fn previous_char",

        "pub(crate) fn is_ident_continue",

        "pub(crate) fn last_ident",

        "pub(crate) fn line_prefix_at",

        "pub(crate) fn is_after_line_comment",

    ] {

        assert!(

            text.contains(expected),

            "LSP text helper `{expected}` should live in text.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP text helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_position_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let position =

        std::fs::read_to_string("crates/coflow-lsp/src/position.rs").expect("read lsp position");



    for expected in [

        "pub(crate) struct LspPosition",

        "pub(crate) fn full_document_range",

        "pub(crate) fn byte_range",

        "pub(crate) fn range_from_span",

        "pub(crate) fn byte_offset_from_position",

        "pub(crate) fn position_from_byte",

    ] {

        assert!(

            position.contains(expected),

            "LSP position helper `{expected}` should live in position.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP position helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_formatting_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let formatting = std::fs::read_to_string("crates/coflow-lsp/src/formatting.rs")

        .expect("read lsp formatting");



    for expected in [

        "pub(crate) fn format_cft",

        "pub(crate) fn starts_with_closing_delimiter",

        "pub(crate) fn adjusted_indent",

    ] {

        assert!(

            formatting.contains(expected),

            "LSP formatting helper `{expected}` should live in formatting.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP formatting helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_document_symbol_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let document_symbols = std::fs::read_to_string("crates/coflow-lsp/src/document_symbols.rs")

        .expect("read lsp document symbols");



    for expected in [

        "pub(crate) fn document_symbols",

        "fn document_symbol_item",

        "const SYMBOL_KIND_CLASS",

        "const SYMBOL_KIND_ENUM_MEMBER",

    ] {

        assert!(

            document_symbols.contains(expected),

            "LSP document symbol helper `{expected}` should live in document_symbols.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP document symbol helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_definition_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let definition =

        std::fs::read_to_string("crates/coflow-lsp/src/definition.rs").expect("read definition");



    for expected in [

        "pub(crate) fn definitions_at",

        "pub(crate) fn cft_type_definition_location",

        "pub(crate) fn cft_schema_field_definition_location",

        "pub(crate) fn cfd_record_definition_location",

        "pub(crate) fn field_location_by_chain",

        "pub(crate) fn field_location",

        "pub(crate) fn ast_enum_variant_location",

        "pub(crate) struct CfdDefinitionIndex",

        "pub(crate) fn from_sources",

    ] {

        assert!(

            definition.contains(expected),

            "LSP definition helper `{expected}` should live in definition.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP definition helper `{expected}` should not live in lib.rs"

        );

    }

    for forbidden in ["SourceLocationSpec", "OpenDocument", "normalize_path"] {
        assert!(
            !definition.contains(forbidden),
            "LSP definition helpers should consume CFD source snapshots instead of project/open-document adapter `{forbidden}`"
        );
    }

}



#[test]

fn lsp_documentation_catalog_does_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let documentation = std::fs::read_to_string("crates/coflow-lsp/src/documentation.rs")

        .expect("read lsp documentation");



    for expected in [

        "pub(crate) const KEYWORDS",

        "pub(crate) const PRIMITIVE_TYPES",

        "pub(crate) const LITERALS",

        "pub(crate) const BUILTIN_FUNCTIONS",

        "pub(crate) const ANNOTATIONS",

        "pub(crate) struct AnnotationCompletion",

        "pub(crate) fn static_documentation",

        "pub(crate) fn is_builtin_name",

    ] {

        assert!(

            documentation.contains(expected),

            "LSP documentation catalog `{expected}` should live in documentation.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP documentation catalog `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_completion_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let completion = std::fs::read_to_string("crates/coflow-lsp/src/completion.rs")

        .expect("read lsp completion");



    for expected in [

        "pub(crate) fn completion_items",

        "pub(crate) enum CompletionScope",

        "pub(crate) fn completion_scope",

        "fn completion_item",

        "fn annotation_completion_item",

        "pub(crate) fn annotation_completion_items",

        "pub(crate) fn top_level_completion_items",

        "pub(crate) fn check_expression_completion_items",

        "pub(crate) fn dot_completion_items",

        "fn const_value_assignable_to_type",

        "pub(crate) fn is_annotation_completion_context",

        "pub(crate) fn is_type_predicate_context",

        "pub(crate) fn is_type_header_parent_context",

        "pub(crate) fn is_type_reference_context",

        "pub(crate) fn is_const_value_context",

        "pub(crate) fn is_field_default_context",

        "pub(crate) fn top_level_needs_type_keyword",

        "pub(crate) fn receiver_chain_before_dot",

        "fn trailing_dotted_ident_chain",

    ] {

        assert!(

            completion.contains(expected),

            "LSP completion helper `{expected}` should live in completion.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP completion helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_hover_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let hover = std::fs::read_to_string("crates/coflow-lsp/src/hover.rs").expect("read lsp hover");



    for expected in [

        "pub(crate) fn hover_at",

        "fn type_hover_text",

        "fn const_value_to_string",

        "fn hover_response",

        "fn annotation_at",

    ] {

        assert!(

            hover.contains(expected),

            "LSP hover helper `{expected}` should live in hover.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP hover helper `{expected}` should not live in lib.rs"

        );

    }

}



#[test]

fn lsp_semantic_token_helpers_do_not_live_in_lib_rs() {

    let lib = std::fs::read_to_string("crates/coflow-lsp/src/lib.rs").expect("read lsp lib");

    let semantic_tokens = std::fs::read_to_string("crates/coflow-lsp/src/semantic_tokens.rs")

        .expect("read lsp semantic tokens");



    for expected in [

        "pub(crate) const SEMANTIC_TOKEN_TYPES",

        "pub(crate) const SEMANTIC_TOKEN_MODIFIERS",

        "pub(crate) struct RawSemanticToken",

        "pub(crate) fn push_semantic_span",

        "pub(crate) fn push_semantic_span_plain",

        "pub(crate) fn encode_semantic_tokens",

        "pub(crate) fn add_comment_semantic_tokens",

        "pub(crate) fn comment_start_in_line",

        "pub(crate) fn semantic_token_data",

        "fn semantic_raw_tokens",

        "fn add_lex_semantic_token",

        "fn add_ast_semantic_tokens",

        "fn add_annotation_semantic",

        "fn add_type_ref_semantic",

        "fn add_const_literal_semantic",

        "fn add_default_expr_semantic",

        "fn add_check_stmt_semantic",

        "fn add_check_expr_semantic",

    ] {

        assert!(

            semantic_tokens.contains(expected),

            "LSP semantic token helper `{expected}` should live in semantic_tokens.rs"

        );

        assert!(

            !lib.contains(expected),

            "LSP semantic token helper `{expected}` should not live in lib.rs"

        );

    }

}



