#![allow(clippy::expect_used, clippy::panic)]

use coflow_language::cfd::{parse_cfd, CfdValue};
use std::fmt::Write as _;
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
    assert_ne!(
        diagnostics,
        [] as [coflow_language::cfd::CfdSyntaxDiagnostic; 0]
    );
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.records[0].id, "next");
}

#[test]
fn repeated_recovery_preserves_diagnostics_and_following_unicode_source_spans() {
    let mut source = String::new();
    for index in 0..1_000 {
        write!(source,
            "bad{index}: Sample {{ value: , text: \"括号 }} ] 不改变层级\" }}\r\n# {{ [ ignored\r\n"
        )
        .expect("write to String");
    }
    let expected_start = source.len();
    source.push_str("survivor: Sample { value: 3 }\r\n");
    let (ast, diagnostics) = parse_cfd(&source);
    assert_eq!(diagnostics.len(), 1_000);
    assert!(diagnostics
        .windows(2)
        .all(|pair| pair[0].span.start < pair[1].span.start));
    assert_eq!(ast.records.len(), 1);
    assert_eq!(ast.records[0].id, "survivor");
    assert_eq!(ast.records[0].span.start, expected_start);
}

#[test]
fn option_constructors_share_the_structural_depth_budget() {
    use coflow_language::cfd::{parse_cfd_with_options, CfdParseOptions};
    use coflow_language::limits::StructuralLimits;
    let options = CfdParseOptions {
        structural_limits: StructuralLimits::new(4, 1000, 1000),
    };
    for (depth, accepted) in [(3, true), (4, false), (12, false)] {
        let source = format!(
            "item: Sample {{ value: {}None{} }}",
            "Some(".repeat(depth),
            ")".repeat(depth)
        );
        let (ast, diagnostics) = parse_cfd_with_options(&source, options);
        assert_eq!(
            diagnostics.is_empty(),
            accepted,
            "depth {depth}: {diagnostics:?}"
        );
        assert_eq!(ast.records.len(), usize::from(accepted));
        if !accepted {
            assert!(diagnostics[0].message.contains("depth limit 4"));
        }
    }
}

#[test]
fn unparenthesized_flag_expression_chains_use_the_ast_depth_limit() {
    use coflow_language::cfd::{parse_cfd_with_options, CfdParseOptions};
    use coflow_language::limits::StructuralLimits;
    let options = CfdParseOptions {
        structural_limits: StructuralLimits::new(8, 1000, 1000),
    };
    let source = format!(
        "item: Sample {{ value: {} }}",
        std::iter::repeat_n("Read", 16)
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let (_, diagnostics) = parse_cfd_with_options(&source, options);
    assert!(diagnostics
        .iter()
        .any(|error| error.message.contains("depth limit 8")));
}
