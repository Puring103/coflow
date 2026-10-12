#![allow(clippy::expect_used, clippy::needless_raw_string_hashes)]

use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};
use coflow_language::diagnostics::CftErrorCode;

fn compile(
    source: &str,
) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules)
}

#[test]
fn removed_result_and_unit_types_are_rejected_in_all_declaration_positions() {
    for source in [
        "table Item { value: Result<int, string>; }",
        "@struct sealed data Item { value: Result<int, string>; }",
        "table Item { value: Option<Result<int, string>>; }",
        "table Item { value: [Result<int, string>]; }",
        "table Item { value: {string: Result<int, string>}; }",
        "type Outcome = Result<int, string>; table Item { value: Outcome; }",
        "const OUTCOME: Result<int, string> = Ok(1);",
        "table Item { value: (); }",
        "type Empty = ();",
        "const EMPTY: () = ();",
    ] {
        assert!(compile(source).is_err(), "removed type must fail: {source}");
    }
}

#[test]
fn removed_result_constructors_are_rejected_in_defaults_and_constants() {
    for source in [
        "table Item { value: int = Ok(1); }",
        "table Item { value: string = Err(\"bad\"); }",
        "table Item { value: Option<int> = Some(Ok(1)); }",
        "const OUTCOME = Ok(1);",
        "const OUTCOME = Err(\"bad\");",
    ] {
        assert!(
            compile(source).is_err(),
            "removed constructor must fail: {source}"
        );
    }
}

#[test]
fn former_result_names_are_ordinary_user_type_names() {
    compile("data Result { value: int; } table Ok {} table Err {} table Item { value: Result; }")
        .expect("removed built-in names can name business types");
}

#[test]
fn required_object_cycles_are_rejected() {
    for source in [
        "data Node { child: Node; }",
        "data A { b: B; } data B { a: A; }",
        "type Link = Node; data Node { child: Link; }",
    ] {
        let diagnostics = compile(source).expect_err("required object cycle must fail");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::RequiredObjectCycle));
    }
}

#[test]
fn optional_collection_and_reference_recursion_are_finite() {
    compile(
        r#"
            table Node {
                optional: Option<&Node>;
                children: [&Node];
                indexed: {string: &Node};
                linked: Option<&Node>;
            }
        "#,
    )
    .expect("finite recursive containers must compile");
}

#[test]
fn recursive_default_materialization_is_rejected() {
    for source in [
        "data Node { child: Option<Node> = Some(Node {}); }",
        "data Node { children: [Node] = [Node {}]; }",
        "data Node { indexed: {string: Node} = { \"child\": Node {} }; }",
        concat!(
            "data A { b: Option<B> = Some(B {}); } ",
            "data B { a: Option<A> = Some(A {}); }"
        ),
    ] {
        let diagnostics = compile(source).expect_err("recursive default must fail");
        assert!(
            diagnostics
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == CftErrorCode::DefaultMaterializationCycle }),
            "expected default materialization cycle: {source}"
        );
    }
}

#[test]
fn terminating_recursive_defaults_are_allowed() {
    compile(
        r#"
            data Node {
                optional: Option<Node> = None;
                children: [Node] = [];
                indexed: {string: Node} = {};
            }
        "#,
    )
    .expect("empty recursive defaults terminate");

    compile(
        r#"
            data Node {
                child: Option<Node> = Some(Node { child: None });
            }
        "#,
    )
    .expect("an explicit terminating object field must not be reported as a cycle");
}
