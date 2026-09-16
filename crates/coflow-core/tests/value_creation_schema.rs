#![allow(clippy::expect_used, clippy::needless_raw_string_hashes)]

use coflow_core::schema::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
use coflow_language::diagnostics::CftErrorCode;

fn compile(
    source: &str,
) -> Result<coflow_core::schema::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
}

#[test]
fn removed_result_type_is_rejected_in_all_declaration_positions() {
    for source in [
        "table Item { value: Result<int, string>; }",
        "@struct sealed data Item { value: Result<int, string>; }",
        "table Item { value: Option<Result<int, string>>; }",
        "table Item { value: [Result<int, string>]; }",
        "table Item { value: {string: Result<int, string>}; }",
        "type Outcome = Result<int, string>; table Item { value: Outcome; }",
        "table Item { run: fn(int) -> Result<int, string>; }",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
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
            data Node {
                optional: Node?;
                children: [Node];
                indexed: {string: Node};
                linked: Node?;
            }
        "#,
    )
    .expect("finite recursive containers must compile");
}

#[test]
fn recursive_default_materialization_is_rejected() {
    for source in [
        "data Node { child: Node? = Node {}; }",
        "data Node { children: [Node] = [Node {}]; }",
        "data Node { indexed: {string: Node} = { \"child\": Node {} }; }",
        concat!("data A { b: B? = B {}; } ", "data B { a: A? = A {}; }"),
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
                optional: Node? = None;
                children: [Node] = [];
                indexed: {string: Node} = {};
            }
        "#,
    )
    .expect("empty recursive defaults terminate");

    compile(
        r#"
            data Node {
                child: Node? = Node { child: None };
            }
        "#,
    )
    .expect("an explicit terminating object field must not be reported as a cycle");
}
