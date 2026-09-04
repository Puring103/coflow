use coflow_language::cfd::{parse_cfd, CfdValue};
use std::fs;
use std::path::PathBuf;

#[test]
fn shared_cfd_parser_corpus_has_the_expected_outcomes() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cfd-parser-parity");
    let mut fixtures = fs::read_dir(&fixture_dir)
        .expect("shared CFD parser fixture directory")
        .map(|entry| entry.expect("shared CFD parser fixture entry").path())
        .collect::<Vec<_>>();
    fixtures.sort();
    assert!(
        !fixtures.is_empty(),
        "shared CFD parser corpus must not be empty"
    );

    for path in fixtures {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("UTF-8 fixture name");
        let expected_valid = name.ends_with(".valid.cfd");
        assert!(
            expected_valid || name.ends_with(".invalid.cfd"),
            "fixture name must declare its expected outcome: {name}"
        );
        let source = fs::read_to_string(&path).expect("read shared CFD parser fixture");
        let (_, diagnostics) = parse_cfd(&source);
        assert_eq!(
            diagnostics.is_empty(),
            expected_valid,
            "unexpected Rust parser outcome for {name}: {diagnostics:?}"
        );
    }
}

#[test]
fn numeric_scalars_keep_their_schema_free_source_text() {
    let source = "item: Sample { values: [12.5e-2f, 1., 1e+, 1..2, 42] }";
    let (ast, diagnostics) = parse_cfd(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let CfdValue::Array(values, _) = &ast.records[0].fields[0].value else {
        panic!("values must be an array");
    };
    let scalars = values
        .iter()
        .map(|value| match value {
            CfdValue::Scalar(value, _) => value.as_str(),
            _ => panic!("array entry must remain a scalar"),
        })
        .collect::<Vec<_>>();
    assert_eq!(scalars, ["12.5e-2f", "1.", "1e+", "1..2", "42"]);
}

#[test]
fn recovery_ignores_delimiters_inside_strings_and_comments() {
    let source = concat!(
        "broken: Sample { values: [1 \"# } ] )\", 2] }\n",
        "# { [ ( ignored\n",
        "next: Sample { value: 3 }\n",
    );
    let (ast, diagnostics) = parse_cfd(source);
    assert!(!diagnostics.is_empty());
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.records[0].key, "next");
}
