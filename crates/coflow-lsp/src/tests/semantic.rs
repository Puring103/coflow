use super::common::*;
use super::*;
use crate::uri::{hex_value, percent_decode};

#[test]
fn semantic_range_helpers_ignore_empty_multiline_and_overlapping_tokens() {
    let source = "ab\ncd";
    let mut tokens = Vec::new();

    push_semantic_span_plain(source, Span::new(1, 1), SEM_TYPE, &mut tokens);
    push_semantic_span_plain(source, Span::new(1, 4), SEM_TYPE, &mut tokens);
    assert!(tokens.is_empty());

    push_semantic_span_plain(source, Span::new(0, 2), SEM_TYPE, &mut tokens);
    push_semantic_span_plain(source, Span::new(1, 2), SEM_PROPERTY, &mut tokens);
    push_semantic_span_plain(source, Span::new(3, 5), SEM_STRING, &mut tokens);

    let encoded = encode_semantic_tokens(tokens);

    assert_eq!(
        encoded,
        vec![
            0, 0, 2, SEM_TYPE, 0, // overlapping property token is dropped
            1, 0, 2, SEM_STRING, 0,
        ]
    );
    assert_eq!(
        range_from_span(source, Span::new(2, 2)),
        lsp_range(0, 2, 1, 0)
    );
}

#[test]
fn encoded_semantic_tokens_preserve_modifiers() {
    let source = "ab cd";
    let mut tokens = Vec::new();
    push_semantic_span(
        source,
        Span::new(0, 2),
        SEM_NAMESPACE,
        MOD_DECLARATION | MOD_RECORD,
        &mut tokens,
    );
    push_semantic_span(
        source,
        Span::new(3, 5),
        SEM_NAMESPACE,
        MOD_REFERENCE | MOD_RECORD,
        &mut tokens,
    );

    let encoded = encode_semantic_tokens(tokens);

    assert_eq!(
        encoded,
        vec![
            0,
            0,
            2,
            SEM_NAMESPACE,
            MOD_DECLARATION | MOD_RECORD,
            0,
            3,
            2,
            SEM_NAMESPACE,
            MOD_REFERENCE | MOD_RECORD,
        ]
    );
}

#[test]
fn cft_semantic_tokens_distinguish_schema_declarations_references_and_paths() {
    let source = "type Target { key: string; value: int; }\n\
type Holder {\n\
  target: Target;\n\
  check { target.value == 1; }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-cft-semantic-modifiers", source);
    let document = first_document(&build);
    let raw_tokens = semantic_raw_tokens(&build, document);

    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "type Target",
        "Target",
        SEM_TYPE,
        MOD_DECLARATION | MOD_SCHEMA,
    ));
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "target: Target",
        "target",
        SEM_PROPERTY,
        MOD_DECLARATION | MOD_SCHEMA,
    ));
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "target: Target",
        "Target",
        SEM_TYPE,
        MOD_REFERENCE | MOD_SCHEMA,
    ));
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "target.value",
        "target",
        SEM_VARIABLE,
        MOD_REFERENCE,
    ));
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "target.value",
        "value",
        SEM_PROPERTY,
        MOD_REFERENCE | MOD_PATH | MOD_SCHEMA,
    ));
}

#[test]
fn cfd_semantic_tokens_distinguish_record_refs_paths_and_schema_fields() {
    let source = "base: Monster { stats: { hp: 10 } }\n\
elite: Monster { target: @Monster.base.stats.hp }\n";
    let (ast, _) = parse_cfd(source);
    let result = cfd::semantic_tokens(source, &ast);
    let tokens = decode_semantic_tokens(source, &result["data"]);

    assert!(
        tokens.contains(&DecodedSemanticToken {
            text: "base".to_string(),
            token_type: SEM_NAMESPACE,
            modifiers: MOD_DECLARATION | MOD_RECORD,
        }),
        "{tokens:?}"
    );
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "target".to_string(),
        token_type: SEM_PROPERTY,
        modifiers: MOD_DECLARATION | MOD_SCHEMA,
    }));
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "@".to_string(),
        token_type: SEM_OPERATOR,
        modifiers: 0,
    }));
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "Monster".to_string(),
        token_type: SEM_TYPE,
        modifiers: MOD_REFERENCE | MOD_SCHEMA,
    }));
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "base".to_string(),
        token_type: SEM_NAMESPACE,
        modifiers: MOD_REFERENCE | MOD_RECORD,
    }));
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "stats".to_string(),
        token_type: SEM_PROPERTY,
        modifiers: MOD_REFERENCE | MOD_PATH | MOD_SCHEMA,
    }));
    assert!(tokens.contains(&DecodedSemanticToken {
        text: "hp".to_string(),
        token_type: SEM_PROPERTY,
        modifiers: MOD_REFERENCE | MOD_PATH | MOD_SCHEMA,
    }));
}

#[test]
fn semantic_tokens_and_protocol_helpers_cover_malformed_boundaries() {
    let (_cleanup, build) = test_lsp_build(
        "lsp-semantic-helper-boundaries",
        "type Item { key: string; }\n",
    );
    let invalid_document = LspDocument {
        module_id: "broken".to_string(),
        uri: "file:///broken.cft".to_string(),
        source: "type Broken { $".to_string(),
        ast: None,
    };

    assert!(document_symbols(&invalid_document).is_empty());
    assert!(semantic_token_data(&build, &invalid_document).is_empty());
    assert_eq!(comment_start_in_line("\"#\" # real"), Some(4));
    assert_eq!(comment_start_in_line("\"unterminated # still string"), None);
    assert!(is_inside_string(
        "value = \"unterminated",
        "value = \"unterminated".len()
    ));
    assert!(is_after_line_comment("\"#\" # real"));
    assert!(!is_after_line_comment("\"# not comment\""));
    assert_eq!(
        word_at("abc", 3).map(|word| word.text),
        Some("abc".to_string())
    );
    assert!(word_at("! ", 1).is_none());

    let mut missing_length = io::Cursor::new("X-Header: value\r\n\r\n");
    assert!(read_message(&mut missing_length)
        .expect_err("missing content length")
        .contains("missing"));

    let mut invalid_length = io::Cursor::new("Content-Length: NaN\r\n\r\n");
    assert!(read_message(&mut invalid_length)
        .expect_err("invalid content length")
        .contains("invalid"));

    let mut eof_headers = io::Cursor::new("Content-Length: 5\r\n");
    assert!(read_message(&mut eof_headers)
        .expect_err("unexpected header EOF")
        .contains("unexpected EOF"));

    let mut short_body = io::Cursor::new("Content-Length: 5\r\n\r\nhi");
    assert!(read_message(&mut short_body)
        .expect_err("short body")
        .contains("failed to read LSP body"));

    assert!(path_from_file_uri("file:///bad%ZZ").is_none());
    assert!(percent_decode("%E0%A4%A").is_none());
    assert_eq!(hex_value(b'f'), Some(15));
    assert_eq!(hex_value(b'F'), Some(15));
    assert_eq!(hex_value(b'?'), None);
}

#[test]
fn semantic_tokens_do_not_treat_legacy_ref_annotation_arg_as_type() {
    let source = "type Target { key: string; }\n\
type Item { @ref(Target) target: string; }\n";
    let (_cleanup, build) = test_lsp_build("lsp-semantic-legacy-ref-annotation", source);
    let document = first_document(&build);
    let target_offset = source.find("@ref(Target)").expect("legacy ref annotation") + "@ref(".len();
    let target_start = position_from_byte(source, target_offset);
    let target_len = "Target".len();
    let raw_tokens = semantic_raw_tokens(&build, document);

    let token_matches_target = |token: &&RawSemanticToken| {
        token.line == target_start.line
            && token.character == target_start.character
            && token.length == target_len
    };
    assert!(raw_tokens
        .iter()
        .filter(token_matches_target)
        .any(|token| token.token_type == SEM_VARIABLE));
    assert!(!raw_tokens
        .iter()
        .filter(token_matches_target)
        .any(|token| token.token_type == SEM_TYPE));
}
