use coflow_language::cfd::parse_cfd;
use std::fs;
use std::path::PathBuf;

#[test]
fn shared_cfd_parser_corpus_has_the_expected_outcomes() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/cfd-parser-parity");
    let mut fixtures = fs::read_dir(&fixture_dir)
        .expect("shared CFD parser fixture directory")
        .map(|entry| entry.expect("shared CFD parser fixture entry").path())
        .collect::<Vec<_>>();
    fixtures.sort();
    assert!(!fixtures.is_empty(), "shared CFD parser corpus must not be empty");

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
