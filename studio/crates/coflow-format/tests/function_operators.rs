use coflow_format::{format_cfd, format_cft};
use coflow_language::lexical::tokenize_lossless;

fn significant(source: &str) -> Vec<&str> {
    tokenize_lossless(source)
        .into_iter()
        .filter(|token| !token.is_trivia())
        .map(|token| token.text(source))
        .collect()
}

#[test]
fn cft_and_cfd_keep_compound_assignment_intact_and_unary_rhs_tight() {
    for operator in [
        "+=", "-=", "*=", "/=", "%=", "//=", "**=", "<<=", ">>=", "&=", "|=", "^=",
    ] {
        for rhs in ["-2", "~2", "2"] {
            let statement = format!("value{operator}{rhs};");
            let expected = format!("value {operator} {rhs};");
            let sources = [
                format!("table Rule {{\nrun: fn() -> int => {{\nvar value: int = 4;\n{statement}\nvalue\n}};\n}}"),
                format!("rule: Rule {{\nrun: fn() -> int {{\nvar value: int = 4;\n{statement}\nvalue\n}},\n}}"),
            ];
            for (source, formatter) in sources
                .iter()
                .zip([format_cft as fn(&str) -> String, format_cfd])
            {
                let formatted = formatter(source);
                assert!(formatted.contains(&expected), "{expected}: {formatted}");
                assert_eq!(significant(source), significant(&formatted));
                assert_eq!(formatter(&formatted), formatted);
            }
        }
    }
}

#[test]
fn builder_scopes_format_without_changing_dictionary_types_or_tokens() {
    for body in ["build [int] as b { b.append(1); b[0]=2; }", "build {string:int} as b { b[\"a\"]=1; b.remove(\"a\"); }", "build (values) as b { b.append(3); }"] {
        let sources = [format!("table Rule {{ run: fn(values: [int]) -> [int] => {{ {body} }}; }}"), format!("r: Rule {{ run: fn(values: [int]) -> [int] {{ {body} }} }}")];
        for (source, formatter) in sources.iter().zip([format_cft as fn(&str) -> String, format_cfd]) {
            let formatted = formatter(source);
            assert_eq!(significant(source), significant(&formatted));
            assert_eq!(formatter(&formatted), formatted);
            assert!(formatted.contains("as b {\n"), "{formatted}");
        }
    }
}
