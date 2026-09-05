use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId,
};
use coflow_language::diagnostics::CftErrorCode;

fn compile(source: &str) -> Result<coflow_language::cft::CftSchema, coflow_language::diagnostics::CftDiagnostics> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
}

#[test]
fn result_is_rejected_in_object_data_fields_but_allowed_in_function_signatures() {
    for source in [
        "type Item { value: Result<int, string>; }",
        "type Item { value: Option<Result<int, string>>; }",
        "type Item { value: [Result<int, string>]; }",
        "type Item { value: {string: Result<int, string>}; }",
        "type Outcome = Result<int, string>; type Item { value: Outcome; }",
    ] {
        let diagnostics = compile(source).expect_err("Result data field must fail");
        assert!(diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CftErrorCode::ResultDataField));
    }

    compile("type Item { run: fn(int) -> Result<int, string>; }")
        .expect("Result remains valid in function signatures");
}

#[test]
fn required_object_cycles_are_rejected() {
    for source in [
        "type Node { child: Node; }",
        "type A { b: B; } type B { a: A; }",
        "type Link = Node; type Node { child: Link; }",
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
            type Node {
                optional: Option<Node>;
                children: [Node];
                indexed: {string: Node};
                linked: Option<&Node>;
            }
        "#,
    )
    .expect("finite recursive containers must compile");
}

#[test]
fn recursive_default_materialization_is_rejected() {
    for source in [
        "type Node { child: Option<Node> = Some(Node {}); }",
        "type Node { children: [Node] = [Node {}]; }",
        "type Node { indexed: {string: Node} = { \"child\": Node {} }; }",
        concat!(
            "type A { b: Option<B> = Some(B {}); } ",
            "type B { a: Option<A> = Some(A {}); }"
        ),
    ] {
        let diagnostics = compile(source).expect_err("recursive default must fail");
        assert!(
            diagnostics.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == CftErrorCode::DefaultMaterializationCycle
            }),
            "expected default materialization cycle: {source}"
        );
    }
}

#[test]
fn terminating_recursive_defaults_are_allowed() {
    compile(
        r#"
            type Node {
                optional: Option<Node> = None;
                children: [Node] = [];
                indexed: {string: Node} = {};
            }
        "#,
    )
    .expect("empty recursive defaults terminate");

    compile(
        r#"
            type Node {
                child: Option<Node> = Some(Node { child: None });
            }
        "#,
    )
    .expect("an explicit terminating object field must not be reported as a cycle");
}
