use super::super::completion::{function_completion_items_for_type, receiver_chain_before_dot};
use super::super::semantic_tokens::{
    semantic_raw_tokens, MOD_DECLARATION, SEM_PARAMETER, SEM_VARIABLE,
};
use super::common::*;
use super::*;
use coflow_language::cft::syntax::ast::Item;
use coflow_language::cft::CftValueType;

#[test]
fn hover_and_definition_ignore_comment_and_string_words() {
    let source = "type Monster { key: string; }\n\
type Item {\n\
  note: string = \"Monster\";\n\
  # Monster\n\
  target: Monster;\n\
}\n";
    let (_cleanup, project) = test_project("lsp-trivia", source);
    let mut runtime = coflow_runtime::ProjectRuntime::new(project);
    runtime.refresh().expect("compile schema");
    let build = LspBuild::new(runtime.into_latest_attempt().expect("schema attempt"));
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
type Target { key: string; value: int; }\n\
enum Kind { One = 1, Two = 2, }\n\
type Item {\n\
  kind: Kind = Kind::One;\n\
  target: Target;\n\
  count: int = LIMIT;\n\
  check {\n\
    target.value >= LIMIT;\n\
    kind == Kind::Two;\n\
    count > 0;\n\
    true;\n\
  }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-symbol-boundaries", source);
    let document = first_document(&build);

    let hover_cases = [
        (position_inside(source, "type Target", "type", 1), "Define"),
        (
            position_inside(source, "Kind::Two", "Two", 1),
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
        position_inside(source, "Kind::Two", "Two", 1),
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
    let (_invalid_cleanup, invalid_build) =
        test_lsp_build("lsp-completion-scope-invalid", "type Broken { $");
    let no_ast_document = first_document(&invalid_build);

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
    assert_eq!(
        completion_scope(no_ast_document, check.span.start),
        CompletionScope::TopLevel
    );
}

#[test]
fn incomplete_definitions_keep_contextual_completion_scopes() {
    for (name, source, expected_scope, expected_label) in [
        (
            "type",
            "type Broken {\n  ",
            CompletionScope::TypeBody,
            "field",
        ),
        (
            "enum",
            "enum Broken {\n  ",
            CompletionScope::EnumBody,
            "variant",
        ),
        (
            "check",
            "check Broken {\n  ",
            CompletionScope::CheckBlock,
            "when",
        ),
        (
            "type-check",
            "type Broken {\n  check {\n    ",
            CompletionScope::CheckBlock,
            "all",
        ),
    ] {
        let (_cleanup, build) = test_lsp_build(&format!("lsp-incomplete-{name}"), source);
        let document = first_document(&build);
        assert!(document.ast.is_none());
        assert_eq!(completion_scope(document, source.len()), expected_scope);
        let labels = completion_labels(completion_items(
            &build,
            document,
            &position_from_byte(source, source.len()),
        ));
        assert!(
            labels.contains(&expected_label.to_string()),
            "{name} completions were {labels:?}"
        );
    }
}

#[test]
fn named_top_level_check_uses_check_completion_scope() {
    let source = "check Integrity { true; }";
    let (_cleanup, build) = test_lsp_build("lsp-top-level-check-scope", source);
    let document = first_document(&build);
    let offset = source.find("true").expect("condition");

    assert_eq!(
        completion_scope(document, offset),
        CompletionScope::CheckBlock
    );
    let labels = completion_labels(top_level_completion_items(""));
    assert!(labels.iter().any(|label| label == "check"));
    assert!(!labels.iter().any(|label| label == "namespace"));
    assert!(!labels.iter().any(|label| label == "use"));
}

#[test]
fn records_query_completes_types_and_resolves_hover_and_definition() {
    let source = "type Item { value: int; check { value > 0; } }\n\
type Reward { amount: int; }\n\
check GlobalRules { all item in records(Item) { item.value > 0; } }\n";
    let (_cleanup, build) = test_lsp_build("lsp-records-query", source);
    let document = first_document(&build);
    let records_offset = source.find("records(Item)").expect("records query");
    let type_offset = records_offset + "records(I".len();
    let labels = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, type_offset),
    ));
    assert_eq!(labels, vec!["Item".to_string(), "Reward".to_string()]);

    let top_level_labels = completion_labels(check_expression_completion_items(
        &build,
        document,
        records_offset,
    ));
    assert!(top_level_labels.contains(&"records".to_string()));
    let type_local_offset = source.find("value > 0").expect("type-local check");
    let type_local_labels = completion_labels(check_expression_completion_items(
        &build,
        document,
        type_local_offset,
    ));
    assert!(!type_local_labels.contains(&"records".to_string()));

    let records_hover = hover_at(
        &build,
        document,
        &position_from_byte(source, records_offset + 1),
    )
    .expect("records hover");
    assert!(records_hover["contents"]["value"]
        .as_str()
        .is_some_and(|text| text.contains("all top-level records")));
    let item_position = position_from_byte(source, records_offset + "records(".len() + 1);
    assert!(!definitions_at(&build, document, &item_position).is_empty());
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
    assert!(!labels.contains(&"null".to_string()));
    assert!(!labels.contains(&"when".to_string()));
    assert!(!labels.contains(&"check".to_string()));
}

#[test]
fn completion_items_cover_context_filters_and_default_boundaries() {
    let source = "const LIMIT: int = 5;\n\
const NAME: string = \"boss\";\n\
const OUTCOME: Result<int, string> = Ok(1);\n\
enum Kind { One = 1, Two = 2, }\n\
type Target { key: string; value: int; }\n\
type Item {\n\
  enabled: bool = true;\n\
  kind: Kind = Kind::One;\n\
  maybe: Option<int> = None;\n\
  xs: [int] = [];\n\
  attrs: {string: int} = {};\n\
  target: Target;\n\
  other: Target;\n\
  check { all value, index in xs { value > LIMIT && index >= 0; } }\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-completion-boundaries", source);
    let document = first_document(&build);

    let top_labels = completion_labels(annotation_completion_items(CompletionScope::TopLevel));
    assert!(top_labels.contains(&"@struct".to_string()));
    assert!(top_labels.contains(&"@idAsEnum".to_string()));
    assert!(top_labels.contains(&"@Host".to_string()));
    assert!(top_labels.contains(&"@singleton".to_string()));
    assert!(top_labels.contains(&"@label".to_string()));
    assert!(!top_labels.contains(&"@id".to_string()));
    assert!(!top_labels.contains(&"@ref".to_string()));
    assert!(!top_labels.contains(&"@index".to_string()));

    let type_labels = completion_labels(annotation_completion_items(CompletionScope::TypeBody));
    assert!(type_labels.contains(&"@expand".to_string()));
    assert!(type_labels.contains(&"@localized".to_string()));
    assert!(type_labels.contains(&"@dimension".to_string()));
    assert!(type_labels.contains(&"@description".to_string()));
    assert!(!type_labels.contains(&"@id".to_string()));
    assert!(!type_labels.contains(&"@ref".to_string()));
    assert!(!type_labels.contains(&"@index".to_string()));
    assert!(!type_labels.contains(&"@idAsEnum".to_string()));
    assert!(!type_labels.contains(&"@struct".to_string()));

    let enum_labels = completion_labels(annotation_completion_items(CompletionScope::EnumBody));
    assert!(enum_labels.contains(&"@label".to_string()));
    assert!(enum_labels.contains(&"@description".to_string()));
    assert!(!enum_labels.contains(&"@id".to_string()));
    assert!(!enum_labels.contains(&"@ref".to_string()));
    assert!(!enum_labels.contains(&"@index".to_string()));

    assert_eq!(
        completion_labels(top_level_completion_items("abstract ")),
        vec!["type".to_string()]
    );
    let top_level_items = top_level_completion_items("");
    let type_item = top_level_items
        .iter()
        .find(|item| item["label"] == "type")
        .expect("type completion");
    assert_eq!(type_item["insertTextFormat"], 2);
    assert_eq!(
        type_item["insertText"],
        "type ${1:Name} {\n\t${2:field}: ${3:string};\n}"
    );

    let value_type_position = position_from_byte(
        source,
        source.find("target: Target").expect("target") + "target: ".len(),
    );
    let value_type_labels =
        completion_labels(completion_items(&build, document, &value_type_position));
    assert!(value_type_labels.contains(&"Target".to_string()));
    assert!(value_type_labels.contains(&"Kind".to_string()));
    assert!(value_type_labels.contains(&"string".to_string()));
    assert!(value_type_labels.contains(&"array".to_string()));
    assert!(value_type_labels.contains(&"dictionary".to_string()));
    assert!(value_type_labels.contains(&"reference".to_string()));
    assert!(value_type_labels.contains(&"()".to_string()));

    let const_position = position_from_byte(
        source,
        source.find("const LIMIT: int = 5").expect("const") + "const LIMIT: int = ".len(),
    );
    let const_labels = completion_labels(completion_items(&build, document, &const_position));
    assert!(!const_labels.contains(&"true".to_string()));
    assert!(const_labels.contains(&"LIMIT".to_string()));
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
        source.find("kind: Kind = Kind::One").expect("kind") + "kind: Kind = ".len(),
    );
    let enum_labels = completion_labels(completion_items(&build, document, &enum_position));
    assert!(enum_labels.contains(&"Kind::One".to_string()));
    assert!(enum_labels.contains(&"Kind::Two".to_string()));
    assert!(!enum_labels.contains(&"LIMIT".to_string()));

    let option_position = position_from_byte(
        source,
        source.find("maybe: Option<int> = None").expect("Option")
            + "maybe: Option<int> = ".len(),
    );
    let option_labels = completion_labels(completion_items(&build, document, &option_position));
    assert!(option_labels.contains(&"None".to_string()));
    assert!(option_labels.contains(&"Some".to_string()));
    assert!(!option_labels.contains(&"LIMIT".to_string()));
    assert!(!option_labels.contains(&"NAME".to_string()));

    let result_position = position_from_byte(
        source,
        source
            .find("const OUTCOME: Result<int, string> = Ok(1)")
            .expect("Result")
            + "const OUTCOME: Result<int, string> = ".len(),
    );
    let result_labels = completion_labels(completion_items(&build, document, &result_position));
    assert!(result_labels.contains(&"Ok".to_string()));
    assert!(result_labels.contains(&"Err".to_string()));
    assert!(!result_labels.contains(&"LIMIT".to_string()));

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
    assert!(check_labels.contains(&"index".to_string()));
    assert!(check_labels.contains(&"target".to_string()));
    assert!(check_labels.contains(&"LIMIT".to_string()));
    assert!(!check_labels.contains(&"len".to_string()));

    let method_source = source.replacen("value > LIMIT", "xs.", 1);
    let method_offset = method_source.find("xs.").expect("method receiver") + "xs.".len();
    let (_method_cleanup, method_build) =
        test_lsp_build("lsp-cft-context-completion-method", &method_source);
    let method_document = first_document(&method_build);
    let method_labels = completion_labels(check_expression_completion_items(
        &method_build,
        method_document,
        method_offset,
    ));
    assert!(method_labels.contains(&"len".to_string()));
    assert!(method_labels.contains(&"contains".to_string()));
    assert!(method_labels.contains(&"startsWith".to_string()));
    assert!(method_labels.contains(&"approxEqual".to_string()));
    assert!(method_labels.contains(&"containsKey".to_string()));
    assert!(method_labels.contains(&"isSorted".to_string()));
    assert!(method_labels.contains(&"isSubsetOf".to_string()));

    let enum_member_source = source.replacen("value > LIMIT", "kind == Kind::", 1);
    let enum_member_offset = enum_member_source
        .find("kind == Kind::")
        .expect("enum member")
        + "kind == Kind::".len();
    let (_enum_member_cleanup, enum_member_build) =
        test_lsp_build("lsp-cft-enum-member-completion", &enum_member_source);
    let enum_member_document = first_document(&enum_member_build);
    assert_eq!(
        completion_labels(completion_items(
            &enum_member_build,
            enum_member_document,
            &position_from_byte(&enum_member_source, enum_member_offset),
        )),
        vec!["One".to_string(), "Two".to_string()]
    );

    let method_items = completion_items(
        &method_build,
        method_document,
        &position_from_byte(&method_source, method_offset),
    );
    let insert_text = |label: &str| {
        method_items
            .iter()
            .find(|item| item["label"] == label)
            .and_then(|item| item["insertText"].as_str())
    };
    assert_eq!(insert_text("len"), Some("len()"));
    assert_eq!(insert_text("contains"), Some("contains(${1:value})"));
    assert_eq!(
        insert_text("approxEqual"),
        Some("approxEqual(${1:value}, ${2:value})")
    );

    let filtered_method_labels = completion_labels(function_completion_items_for_type(
        &CftValueType::Array(Box::new(CftValueType::Int)),
    ));
    assert!(filtered_method_labels.contains(&"len".to_string()));
    assert!(filtered_method_labels.contains(&"isSorted".to_string()));
    assert!(
        !filtered_method_labels.contains(&"startsWith".to_string()),
        "{filtered_method_labels:?}"
    );
    assert!(!filtered_method_labels.contains(&"containsKey".to_string()));
    assert!(!filtered_method_labels.contains(&"approxEqual".to_string()));

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
fn completion_covers_const_alias_inheritance_and_annotation_arguments() {
    let source = "enum Kind {}\n\
@idAsEnum(Kind)\n\
type Entity { key: string; }\n\
sealed type Closed { value: int; }\n\
type Child : Entity { value: int; }\n\
type Alias = Entity;\n\
const LIMIT: int = 1;\n";
    let (_cleanup, build) = test_lsp_build("lsp-cft-declaration-completion", source);
    let document = first_document(&build);

    let const_type_offset = source.find("const LIMIT: int").expect("const") + "const LIMIT: ".len();
    let const_types = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, const_type_offset),
    ));
    assert!(const_types.contains(&"int".to_string()));
    assert!(const_types.contains(&"Entity".to_string()));

    let alias_offset = source.find("type Alias = Entity").expect("alias") + "type Alias = ".len();
    let alias_types = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, alias_offset),
    ));
    assert!(alias_types.contains(&"string".to_string()));
    assert!(alias_types.contains(&"Entity".to_string()));

    let parent_offset = source.find("type Child : Entity").expect("parent") + "type Child : ".len();
    let parents = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, parent_offset),
    ));
    assert!(parents.contains(&"Entity".to_string()));
    assert!(!parents.contains(&"Child".to_string()));
    assert!(!parents.contains(&"Closed".to_string()));

    let annotation_offset = source.find("@idAsEnum(Kind").expect("annotation") + "@idAsEnum(".len();
    let annotation_args = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, annotation_offset),
    ));
    assert!(annotation_args.contains(&"Kind".to_string()));
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
        Some(CftValueType::Int)
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

    assert_eq!(
        receiver_chain_before_dot("  target.child.  partial"),
        Some(vec![s("target"), s("child")])
    );
    assert_eq!(receiver_chain_before_dot("target.child.!"), None);

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
fn function_defaults_have_snippets_body_completions_and_semantic_tokens() {
    let source = "type Rule {\n\
  apply: fn(value: int) -> int = fn(input: int) -> int {\n\
    var total = input + 1;\n\
    total\n\
  };\n\
}\n";
    let (_cleanup, build) = test_lsp_build("lsp-cft-function-defaults", source);
    let document = first_document(&build);

    let default_offset = source.find("fn(input").expect("function default");
    let default_items = completion_items(
        &build,
        document,
        &position_from_byte(source, default_offset),
    );
    let function_item = default_items
        .iter()
        .find(|item| item["label"] == "fn")
        .expect("function default completion");
    assert_eq!(function_item["insertTextFormat"], 2);
    assert_eq!(
        function_item["insertText"],
        "fn(${1:value}: int) -> int {\n\t${2}\n}"
    );

    let body_offset = source.find("total\n").expect("body expression");
    let body_labels = completion_labels(completion_items(
        &build,
        document,
        &position_from_byte(source, body_offset),
    ));
    assert!(body_labels.contains(&"input".to_string()), "{body_labels:?}");
    assert!(body_labels.contains(&"total".to_string()), "{body_labels:?}");
    assert!(body_labels.contains(&"var".to_string()), "{body_labels:?}");

    let raw_tokens = semantic_raw_tokens(&build, document);
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "fn(input: int)",
        "input",
        SEM_PARAMETER,
        MOD_DECLARATION,
    ));
    assert!(has_semantic_token(
        source,
        &raw_tokens,
        "var total",
        "total",
        SEM_VARIABLE,
        MOD_DECLARATION,
    ));
}








#[test]
fn formatter_returns_independent_local_text_edits() {
    let source = "type Item {\nname:string;\n  unchanged: string;\nvalue:int;\n}\n";
    let formatted = format_cft(source);
    let edits = formatting_edits(source, &formatted);

    assert!(edits.len() >= 4, "expected local edits, got {edits:#?}");
    assert!(edits.iter().all(|edit| {
        let start = &edit["range"]["start"];
        let end = &edit["range"]["end"];
        start["line"] == end["line"]
    }));
}
