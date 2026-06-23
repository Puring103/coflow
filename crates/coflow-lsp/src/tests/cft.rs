use super::common::*;
use super::*;

#[test]
fn hover_and_definition_ignore_comment_and_string_words() {
    let source = "type Monster { key: string; }\n\
type Item {\n\
  note: string = \"Monster\";\n\
  # Monster\n\
  target: Monster;\n\
}\n";
    let (_cleanup, project) = test_project("lsp-trivia", source);
    let build = LspBuild::new(
        compile_schema_project_with_overrides(&project, &[]).expect("compile schema"),
        None,
    );
    let document = build
        .documents
        .values()
        .next()
        .expect("document should exist");

    let string_position =
        position_from_byte(source, position_inside(source, "\"Monster\"", "Monster", 1));
    let comment_position =
        position_from_byte(source, position_inside(source, "# Monster", "Monster", 1));

    assert_eq!(hover_at(&build, document, &string_position), None);
    assert_eq!(hover_at(&build, document, &comment_position), None);
    assert!(definitions_at(&build, document, &string_position).is_empty());
    assert!(definitions_at(&build, document, &comment_position).is_empty());
}

#[test]
fn hover_and_definition_cover_symbol_resolution_boundaries() {
    let source = "const LIMIT: int = 5;\n\
@display(\"target\")\n\
type Target { key: string; value: int; }\n\
enum Kind { One = 1, Two = 2, }\n\
type Item {\n\
  @display(\"kind\")\n\
  kind: Kind = Kind.One;\n\
  target: Target;\n\
  count: int = LIMIT;\n\
  check {\n\
    target.value >= LIMIT;\n\
    kind == Kind.Two;\n\
    count > 0;\n\
    true;\n\
  }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-symbol-boundaries", source);
    let document = first_document(&build);

    let hover_cases = [
        (
            position_inside(source, "@display(\"target\")", "@display", 1),
            "display",
        ),
        (position_inside(source, "type Target", "type", 1), "Define"),
        (
            position_inside(source, "Kind.Two", "Two", 1),
            "enum variant",
        ),
        (
            position_inside(source, "target.value", "value", 1),
            "Target`.`value",
        ),
        (
            position_inside(source, "target: Target", "Target", 1),
            "CFT type",
        ),
        (position_inside(source, "kind: Kind", "Kind", 1), "CFT enum"),
        (
            position_inside(source, "LIMIT;", "LIMIT", 1),
            "CFT constant",
        ),
        (
            position_inside(source, "count > 0", "count", 1),
            "Item`.`count",
        ),
    ];

    for (offset, expected) in hover_cases {
        let hover = hover_at(&build, document, &position_from_byte(source, offset))
            .unwrap_or_else(|| panic!("expected hover containing {expected}"));
        assert!(
            hover["contents"]["value"]
                .as_str()
                .is_some_and(|text| text.contains(expected)),
            "hover {hover:?} did not contain {expected}"
        );
    }

    for offset in [
        position_inside(source, "Kind.Two", "Two", 1),
        position_inside(source, "target.value", "value", 1),
        position_inside(source, "LIMIT;", "LIMIT", 1),
        position_inside(source, "count > 0", "count", 1),
    ] {
        assert!(
            !definitions_at(&build, document, &position_from_byte(source, offset)).is_empty(),
            "definition should resolve at offset {offset}"
        );
    }
    assert!(definitions_at(
        &build,
        document,
        &position_from_byte(source, position_inside(source, "true;", "true", 1))
    )
    .is_empty());
}

#[test]
fn completion_scope_uses_boundary_offsets_and_missing_ast_as_top_level() {
    let source = "enum Kind { One = 1, }\n\
type Item {\n\
  key: string;\n\
  check { key != \"\"; }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-completion-scope", source);
    let document = first_document(&build);
    let ast = document.ast.as_ref().expect("ast");
    let enum_def = ast
        .items
        .iter()
        .find_map(|item| match item {
            Item::Enum(enum_def) => Some(enum_def),
            _ => None,
        })
        .expect("enum");
    let type_def = ast
        .items
        .iter()
        .find_map(|item| match item {
            Item::Type(ty) => Some(ty),
            _ => None,
        })
        .expect("type");
    let check = type_def.check.as_ref().expect("check");
    let no_ast_document = LspDocument {
        module_id: document.module_id.clone(),
        uri: document.uri.clone(),
        source: document.source.clone(),
        ast: None,
    };

    assert_eq!(
        completion_scope(document, enum_def.span.start),
        CompletionScope::EnumBody
    );
    assert_eq!(
        completion_scope(document, enum_def.span.end),
        CompletionScope::EnumBody
    );
    assert_eq!(
        completion_scope(document, type_def.span.start),
        CompletionScope::TypeBody
    );
    assert_eq!(
        completion_scope(document, check.span.start),
        CompletionScope::CheckBlock
    );
    assert_eq!(
        completion_scope(document, check.span.end),
        CompletionScope::CheckBlock
    );
    assert_eq!(
        completion_scope(document, source.len()),
        CompletionScope::TopLevel
    );
    // Without an AST we fall back to a brace+keyword scan so the user still
    // gets context-appropriate completions while the document is mid-edit.
    let inside_check = source.find("key != ").expect("check body") + "key != ".len();
    assert_eq!(
        completion_scope(&no_ast_document, inside_check),
        CompletionScope::CheckBlock
    );
    assert_eq!(
        completion_scope(&no_ast_document, type_def.span.start + "type Item {".len()),
        CompletionScope::TypeBody
    );
    assert_eq!(
        completion_scope(&no_ast_document, enum_def.span.start + "enum Kind {".len()),
        CompletionScope::EnumBody
    );
    assert_eq!(
        completion_scope(&no_ast_document, 0),
        CompletionScope::TopLevel
    );
}

#[test]
fn completion_items_suppress_trivia_and_restrict_predicate_context() {
    let source = "type Target { key: string; }\n\
type Item {\n\
  key: string;\n\
  target: Target;\n\
  note: string = \"tar\";\n\
  # tar\n\
  check { target is Target; }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-completion-context", source);
    let document = first_document(&build);

    let string_position = position_from_byte(source, position_inside(source, "\"tar\"", "tar", 1));
    let comment_position = position_from_byte(source, position_inside(source, "# tar", "tar", 1));
    let predicate_position = position_from_byte(
        source,
        source.find("target is Target").expect("predicate") + "target is ".len(),
    );

    assert!(completion_items(&build, document, &string_position).is_empty());
    assert!(completion_items(&build, document, &comment_position).is_empty());

    let labels = completion_labels(completion_items(&build, document, &predicate_position));
    assert!(labels.contains(&"Target".to_string()));
    assert!(labels.contains(&"Item".to_string()));
    assert!(labels.contains(&"null".to_string()));
    assert!(!labels.contains(&"when".to_string()));
    assert!(!labels.contains(&"check".to_string()));
}

#[test]
fn completion_items_cover_context_filters_and_default_boundaries() {
    let source = "const LIMIT: int = 5;\n\
const NAME: string = \"boss\";\n\
enum Kind { One = 1, Two = 2, }\n\
type Target { key: string; value: int; }\n\
type Item {\n\
  enabled: bool = true;\n\
  kind: Kind = Kind.One;\n\
  maybe: int? = null;\n\
  xs: [int] = [];\n\
  attrs: {string: int} = {};\n\
  target: Target;\n\
  other: Target;\n\
  check { all value in xs { value > LIMIT; } }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-completion-boundaries", source);
    let document = first_document(&build);

    let top_labels = completion_labels(annotation_completion_items(CompletionScope::TopLevel));
    assert!(top_labels.contains(&"@struct".to_string()));
    assert!(top_labels.contains(&"@keyAsEnum".to_string()));
    assert!(!top_labels.contains(&"@id".to_string()));
    assert!(!top_labels.contains(&"@ref".to_string()));
    assert!(!top_labels.contains(&"@index".to_string()));

    let type_labels = completion_labels(annotation_completion_items(CompletionScope::TypeBody));
    assert!(type_labels.contains(&"@display".to_string()));
    assert!(!type_labels.contains(&"@id".to_string()));
    assert!(!type_labels.contains(&"@ref".to_string()));
    assert!(!type_labels.contains(&"@index".to_string()));
    assert!(!type_labels.contains(&"@keyAsEnum".to_string()));
    assert!(!type_labels.contains(&"@struct".to_string()));

    let enum_labels = completion_labels(annotation_completion_items(CompletionScope::EnumBody));
    assert!(enum_labels.is_empty());
    assert!(!enum_labels.contains(&"@id".to_string()));
    assert!(!enum_labels.contains(&"@ref".to_string()));
    assert!(!enum_labels.contains(&"@index".to_string()));

    assert_eq!(
        completion_labels(top_level_completion_items("abstract ")),
        vec!["type".to_string()]
    );

    let type_ref_position = position_from_byte(
        source,
        source.find("target: Target").expect("target") + "target: ".len(),
    );
    let type_ref_labels = completion_labels(completion_items(&build, document, &type_ref_position));
    assert!(type_ref_labels.contains(&"Target".to_string()));
    assert!(type_ref_labels.contains(&"Kind".to_string()));
    assert!(type_ref_labels.contains(&"string".to_string()));

    let const_position = position_from_byte(
        source,
        source.find("const LIMIT: int = 5").expect("const") + "const LIMIT: int = ".len(),
    );
    let const_labels = completion_labels(completion_items(&build, document, &const_position));
    assert!(const_labels.contains(&"true".to_string()));
    assert!(!const_labels.contains(&"null".to_string()));

    let bool_position = position_from_byte(
        source,
        source.find("enabled: bool = true").expect("bool") + "enabled: bool = ".len(),
    );
    let bool_labels = completion_labels(completion_items(&build, document, &bool_position));
    assert!(bool_labels.contains(&"true".to_string()));
    assert!(bool_labels.contains(&"false".to_string()));
    assert!(!bool_labels.contains(&"null".to_string()));

    let enum_position = position_from_byte(
        source,
        source.find("kind: Kind = Kind.One").expect("kind") + "kind: Kind = ".len(),
    );
    let enum_labels = completion_labels(completion_items(&build, document, &enum_position));
    assert!(enum_labels.contains(&"Kind.One".to_string()));
    assert!(enum_labels.contains(&"Kind.Two".to_string()));
    assert!(!enum_labels.contains(&"LIMIT".to_string()));

    let nullable_position = position_from_byte(
        source,
        source.find("maybe: int? = null").expect("nullable") + "maybe: int? = ".len(),
    );
    let nullable_labels = completion_labels(completion_items(&build, document, &nullable_position));
    assert!(nullable_labels.contains(&"null".to_string()));
    assert!(nullable_labels.contains(&"LIMIT".to_string()));
    assert!(!nullable_labels.contains(&"NAME".to_string()));

    let array_position = position_from_byte(
        source,
        source.find("xs: [int] = []").expect("array") + "xs: [int] = ".len(),
    );
    assert!(
        completion_labels(completion_items(&build, document, &array_position))
            .contains(&"[]".to_string())
    );

    let dict_position = position_from_byte(
        source,
        source.find("attrs: {string: int} = {}").expect("dict") + "attrs: {string: int} = ".len(),
    );
    assert!(
        completion_labels(completion_items(&build, document, &dict_position))
            .contains(&"{}".to_string())
    );

    let check_offset = source.find("value > LIMIT").expect("check body");
    let check_labels = completion_labels(check_expression_completion_items(
        &build,
        document,
        check_offset,
    ));
    assert!(check_labels.contains(&"id".to_string()));
    assert!(check_labels.contains(&"value".to_string()));
    assert!(check_labels.contains(&"target".to_string()));
    assert!(check_labels.contains(&"LIMIT".to_string()));
    assert!(!check_labels.contains(&"len".to_string()));

    let method_source = source.replacen("value > LIMIT", "xs.", 1);
    let method_offset = method_source.find("xs.").expect("method receiver") + "xs.".len();
    let method_document = LspDocument {
        module_id: document.module_id.clone(),
        uri: document.uri.clone(),
        source: method_source,
        ast: document.ast.clone(),
    };
    let method_labels = completion_labels(check_expression_completion_items(
        &build,
        &method_document,
        method_offset,
    ));
    assert!(method_labels.contains(&"len".to_string()));
    assert!(method_labels.contains(&"contains".to_string()));

    assert_eq!(
        completion_labels(dot_completion_items(
            &build,
            document,
            check_offset,
            &[s("Kind")]
        )),
        vec!["One".to_string(), "Two".to_string()]
    );
    let ref_field_labels = completion_labels(dot_completion_items(
        &build,
        document,
        check_offset,
        &[s("target")],
    ));
    assert!(ref_field_labels.contains(&"key".to_string()));
    assert!(ref_field_labels.contains(&"value".to_string()));
    assert!(dot_completion_items(&build, document, check_offset, &[s("missing")]).is_empty());
}

#[test]
fn scope_type_helpers_return_none_for_invalid_or_non_object_chains() {
    let source = "type Target { key: string; value: int; }\n\
type Holder {\n\
  key: string;\n\
  target: Target;\n\
  count: int;\n\
  check { target.value == 1; }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-scope-type", source);
    let document = first_document(&build);
    let offset = source.find("target.value").expect("chain");

    assert_eq!(
        type_name_of_schema_ref(
            &type_of_chain(&build, document, offset, &[s("target")]).expect("target type")
        ),
        Some("Target")
    );
    assert!(matches!(
        type_of_chain(&build, document, offset, &[s("target"), s("value")]),
        Some(CftSchemaTypeRef::Int)
    ));
    assert!(type_of_chain(&build, document, offset, &[]).is_none());
    assert!(type_of_chain(&build, document, offset, &[s("missing")]).is_none());
    assert!(type_of_chain(&build, document, offset, &[s("count"), s("value")]).is_none());
    assert!(field_by_chain(&build, document, offset, &[]).is_none());
    assert!(field_by_chain(&build, document, offset, &[s("target"), s("missing")]).is_none());
    assert!(field_location_by_chain(&build, document, offset, &[s("count"), s("value")]).is_none());
}

#[test]
fn dotted_word_parsing_rejects_partial_empty_or_punctuated_chains() {
    assert_eq!(
        parse_dotted_ident_chain(" target . child_1 "),
        Some(vec![s("target"), s("child_1")])
    );
    assert_eq!(parse_dotted_ident_chain(""), None);
    assert_eq!(parse_dotted_ident_chain("target."), None);
    assert_eq!(parse_dotted_ident_chain("target..child"), None);
    assert_eq!(parse_dotted_ident_chain("target.child!"), None);

    let prefix_input = "  target.child.  partial";
    assert_eq!(
        receiver_chain_before_dot(prefix_input, prefix_input.len()),
        Some(vec![s("target"), s("child")])
    );
    let punctuated_input = "target.child.!";
    assert_eq!(
        receiver_chain_before_dot(punctuated_input, punctuated_input.len()),
        None
    );

    let source = "check { target . child; other }";
    let word = word_at(source, source.find("child").expect("child")).expect("word");
    assert_eq!(
        dotted_chain_at(source, &word),
        Some(vec![s("target"), s("child")])
    );

    let punctuated = "check { target . child + other }";
    let word = word_at(punctuated, punctuated.find("child").expect("child")).expect("word");
    assert_eq!(
        dotted_chain_at(punctuated, &word),
        Some(vec![s("target"), s("child")])
    );
}

#[test]
fn completion_scope_fallback_recovers_inside_unclosed_type_body() {
    // The `name` field is being typed; the closing `}` does not exist yet so
    // the parser will reject the file. We still want field-name completion to
    // know the cursor is inside the `Item` body, not at the top level.
    let source = "type Item {\n  na";
    let scope = completion_scope_fallback(source, source.len());
    assert_eq!(scope, CompletionScope::TypeBody);

    let check_source = "type Item {\n  key: string;\n  check {\n    ke";
    let scope = completion_scope_fallback(check_source, check_source.len());
    assert_eq!(scope, CompletionScope::CheckBlock);

    let enum_source = "enum Kind {\n  One = 1,\n  Tw";
    let scope = completion_scope_fallback(enum_source, enum_source.len());
    assert_eq!(scope, CompletionScope::EnumBody);

    let nested_source = "type Item {\n  default: int = 1;\n  ot";
    let scope = completion_scope_fallback(nested_source, nested_source.len());
    assert_eq!(scope, CompletionScope::TypeBody);
}

#[test]
fn receiver_chain_before_dot_walks_across_lines() {
    let multi_line = "check {\n  drop\n    .rewards\n    .";
    let chain = receiver_chain_before_dot(multi_line, multi_line.len());
    assert_eq!(chain, Some(vec![s("drop"), s("rewards")]));

    let with_comment = "check {\n  drop // comment\n    .rewards\n    .";
    let chain = receiver_chain_before_dot(with_comment, with_comment.len());
    assert_eq!(chain, Some(vec![s("drop"), s("rewards")]));

    let inside_string = "check {\n  drop // \"masked.dot\"\n    .";
    let chain = receiver_chain_before_dot(inside_string, inside_string.len());
    assert_eq!(chain, Some(vec![s("drop")]));
}

#[test]
fn fallback_container_keeps_completion_when_recompile_fails() {
    let good_source = "type Target { key: string; value: int; }\n\
type Item {\n\
  target: Target;\n\
  check { target.value > 0; }\n\
}\n";
    let (_cleanup, project) = test_project("lsp-fallback-container", good_source);
    let good_build =
        compile_schema_project_with_overrides(&project, &[]).expect("compile good schema");
    let last_good = good_build.container.expect("first compile succeeds");

    // Simulate the user introducing a type error: the `Target` type is removed
    // while still referenced from `Item`. The compile produces no container,
    // but the LSP should fall back to the previous container so completions
    // still resolve `target.<dot>`.
    let main_path = project.root_dir.join("schema").join("main.cft");
    let bad_source = "type Item {\n  target: Target;\n  check { target.value > 0; }\n}\n";
    let bad_build = compile_schema_project_with_overrides(
        &project,
        &[SchemaSourceOverride {
            requested_module: None,
            normalized_path: normalize_path(&main_path),
            source: bad_source.to_string(),
        }],
    )
    .expect("compile bad schema");
    assert!(bad_build.container.is_none());

    let build = LspBuild::new(bad_build, Some(last_good));
    let document = build
        .documents
        .values()
        .next()
        .expect("document should exist");
    let dot_offset = document
        .source
        .find("target.value")
        .expect("dot expression")
        + "target.".len();
    let position = position_from_byte(&document.source, dot_offset);
    let labels = completion_labels(completion_items(&build, document, &position));
    assert!(
        labels.contains(&"value".to_string()),
        "expected `value` field; got {labels:?}"
    );
    assert!(
        labels.contains(&"key".to_string()),
        "expected `key` field; got {labels:?}"
    );
}

#[test]
fn current_type_at_uses_ast_when_schema_is_missing() {
    let source = "type Item {\n  key: string;\n  unresolved: Missing;\n}\n";
    let (_cleanup, project) = test_project("lsp-current-type-ast", source);
    let raw_build =
        compile_schema_project_with_overrides(&project, &[]).expect("compile schema");
    // Schema compilation fails (Missing is not a known type), but the AST is
    // still parsed and we want the local Item type to be available.
    assert!(raw_build.container.is_none());
    let build = LspBuild::new(raw_build, None);
    let document = build
        .documents
        .values()
        .next()
        .expect("document should exist");

    let inside_offset = source.find("key: string").expect("key field");
    let name = current_type_name_from_ast(document, inside_offset);
    assert_eq!(name, Some("Item"));
}

#[test]
fn completion_lists_const_and_type_from_ast_when_container_missing() {
    let source = "const LIMIT: int = 5;\n\
type Item { unresolved: Missing; }\n";
    let (_cleanup, project) = test_project("lsp-const-fallback", source);
    let raw_build =
        compile_schema_project_with_overrides(&project, &[]).expect("compile schema");
    assert!(raw_build.container.is_none());
    let build = LspBuild::new(raw_build, None);

    let const_labels = completion_labels(const_completion_items(&build));
    assert!(const_labels.contains(&"LIMIT".to_string()));

    let type_labels = completion_labels(type_completion_items(&build));
    assert!(type_labels.contains(&"Item".to_string()));
    assert!(type_labels.contains(&"int".to_string()));
}

#[test]
fn formatter_ignores_delimiters_inside_strings_and_comments() {
    let source = "type Item {\n\
values: [string] = [\n\
\"{\" # string brace does not indent\n\
] # closing bracket in comment } }\n\
}\n";

    assert_eq!(
            format_cft(source),
            "type Item {\n  values: [string] = [\n    \"{\" # string brace does not indent\n  ] # closing bracket in comment } }\n}\n"
        );
    assert_eq!(
        format_cft("type Item {\n\nkey: string;\n}"),
        "type Item {\n\n  key: string;\n}"
    );
}
