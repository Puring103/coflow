mod common;
use common::*;

#[test]
fn lexer_reports_invalid_character() {
    let err = add_source("type A { id: string; } $").unwrap_err();
    assert_primary_stage(&err, CftErrorCode::UnexpectedCharacter, CftStage::Lex);
}

#[test]
fn lexer_reports_invalid_escape() {
    let err = add_source("const NAME = \"bad\\q\";").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidStringEscape);
}

#[test]
fn lexer_reports_unterminated_string() {
    let err = add_source("const NAME = \"bad;").unwrap_err();
    assert_has_code(&err, CftErrorCode::UnterminatedString);
}

#[test]
fn lexer_reports_invalid_int_and_float_literals() {
    let int_err = add_source("const N = 999999999999999999999999999999;").unwrap_err();
    assert_has_code(&int_err, CftErrorCode::InvalidIntLiteral);

    let float_err = add_source("const N = 1.;").unwrap_err();
    assert_has_code(&float_err, CftErrorCode::InvalidFloatLiteral);
}

#[test]
fn parser_accepts_core_syntax() {
    let source = r#"
        const MAX = 10;
        @flag
        enum Permission { Read = 1, Write = 2, }
        enum Rarity { Common, Rare = 10, Epic, }

        @display("Item")
        abstract type Base {
            @id
            id: string;
            check { id != ""; }
        }

        sealed type Item : Base {
            @index
            rarity: Rarity = Rarity.Common;
            tags: [string] = [];
            attrs: {string: int} = {};
            check {
                0 < MAX <= 20;
                when rarity >= Rarity.Common { id != ""; }
                all tag in tags { tag != ""; }
            }
        }
    "#;

    let mut container = add_source(source).unwrap();
    container.compile().unwrap();
    assert!(container.has_type("Item"));
    assert!(container.has_enum("Permission"));
}

#[test]
fn parser_accepts_unicode_identifiers_whitespace_and_int_division() {
    let source = r#"
        const 上限 = 10;
        enum 稀有度 { 普通, 传说 = 2, }

        type　道具 {
            名称: string = "长剑🙂";
            数量: int = 9;
            每组数量: int = 3;
            稀有: 稀有度 = 稀有度.普通;
            check {
                数量 // 每组数量 >= 1;
                数量 <= 上限;
                名称 != "";
            }
        }
    "#;

    let mut container = add_source(source).unwrap();
    container.compile().unwrap();
    assert!(container.has_type("道具"));
    assert!(container.has_enum("稀有度"));
}

#[test]
fn parser_rejects_invalid_top_level_item() {
    let err = add_source("let x = 1;").unwrap_err();
    assert_primary_stage(&err, CftErrorCode::InvalidTopLevelItem, CftStage::Syn);
}

#[test]
fn parser_rejects_invalid_chain_comparison() {
    let err = add_source("type A { value: int; check { 0 < value > 10; } }").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidChainComparison);

    let eq_chain = add_source("type A { value: int; check { 0 == value == 10; } }").unwrap_err();
    assert_has_code(&eq_chain, CftErrorCode::InvalidChainComparison);
}

#[test]
fn parser_requires_check_to_be_last_and_unique() {
    let check_last = add_source("type A { check { true; } value: int; }").unwrap_err();
    assert_has_code(&check_last, CftErrorCode::CheckBlockMustBeLast);

    let duplicate = add_source("type A { check { true; } check { true; } }").unwrap_err();
    assert_has_code(&duplicate, CftErrorCode::DuplicateCheckBlock);
}
