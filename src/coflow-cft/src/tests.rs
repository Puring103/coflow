use crate::{
    CftConstValue, CftContainer, CftDiagnostics, CftErrorCode, CftSeverity, CftStage, ModuleId,
};

fn add_source(source: &str) -> Result<CftContainer, CftDiagnostics> {
    let mut container = CftContainer::new();
    container.add_module(ModuleId::from("main"), source)?;
    Ok(container)
}

fn compile_one(source: &str) -> Result<CftContainer, CftDiagnostics> {
    let mut container = add_source(source)?;
    container.compile()?;
    Ok(container)
}

fn assert_has_code(diags: &CftDiagnostics, code: CftErrorCode) {
    assert!(
        diags.diagnostics.iter().any(|diag| diag.code == code),
        "expected {code}, got {:?}",
        diags
            .diagnostics
            .iter()
            .map(|diag| diag.code)
            .collect::<Vec<_>>()
    );
}

fn assert_primary_stage(diags: &CftDiagnostics, code: CftErrorCode, stage: CftStage) {
    let diag = diags
        .diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .expect("diagnostic code");
    assert_eq!(diag.stage, stage);
    assert_eq!(diag.severity, CftSeverity::Error);
    assert!(diag.primary.is_some());
}

#[test]
fn lexer_reports_invalid_character() {
    let err = add_source("type A { id: string; } #").unwrap_err();
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
fn parser_rejects_invalid_top_level_item() {
    let err = add_source("let x = 1;").unwrap_err();
    assert_primary_stage(&err, CftErrorCode::InvalidTopLevelItem, CftStage::Syn);
}

#[test]
fn parser_rejects_invalid_chain_comparison() {
    let err = add_source("type A { value: int; check { 0 < value > 10; } }").unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidChainComparison);
}

#[test]
fn parser_requires_check_to_be_last_and_unique() {
    let check_last = add_source("type A { check { true; } value: int; }").unwrap_err();
    assert_has_code(&check_last, CftErrorCode::CheckBlockMustBeLast);

    let duplicate = add_source("type A { check { true; } check { true; } }").unwrap_err();
    assert_has_code(&duplicate, CftErrorCode::DuplicateCheckBlock);
}

#[test]
fn schema_reports_cross_module_duplicate_with_related_label() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("a"), "type Item { id: string; }")
        .unwrap();
    container
        .add_module(ModuleId::from("b"), "enum Item { A, }")
        .unwrap();
    let err = container.compile().unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CftErrorCode::DuplicateGlobalName)
        .unwrap();
    assert!(!diag.related.is_empty());
}

#[test]
fn schema_reports_duplicate_field_enum_value_and_unknown_type() {
    let source = r#"
        enum E { A = 1, B = 1, }
        type A { x: Missing; x: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DuplicateEnumValue);
    assert_has_code(&err, CftErrorCode::DuplicateFieldName);
    assert_has_code(&err, CftErrorCode::UnknownNamedType);
}

#[test]
fn schema_reports_inheritance_and_modifier_errors() {
    let source = r#"
        sealed type Parent { id: string; }
        abstract sealed type Bad { x: int; }
        type Child : Parent { id: string; }
        type A : B { x: int; }
        type B : A { y: int; }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InheritSealedType);
    assert_has_code(&err, CftErrorCode::DuplicateInheritedField);
    assert_has_code(&err, CftErrorCode::ConflictingTypeModifiers);
    assert_has_code(&err, CftErrorCode::InheritanceCycle);
}

#[test]
fn schema_reports_id_annotation_and_flag_errors() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 3, }

        type Base { @id id: string; }
        type Child : Base { @id other: int; }

        @struct
        type NotSealed { x: int; }

        type BadRef {
            @ref(Flags)
            flag_id: string;
            @index
            xs: [int];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidFlagEnumValue);
    assert_has_code(&err, CftErrorCode::MultipleIdFieldsInTree);
    assert_has_code(&err, CftErrorCode::StructRequiresSealedType);
    assert_has_code(&err, CftErrorCode::RefTargetMustBeType);
    assert_has_code(&err, CftErrorCode::InvalidAnnotatedFieldType);
}

#[test]
fn schema_reports_default_errors() {
    let source = r#"
        const NAME = "x";
        enum Rarity { Common, }
        type Item {
            id: int = NAME;
            bad: int = Missing;
            field_ref: int = id;
            rarity: Rarity = Rarity.Missing;
            xs: [int] = [1];
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::DefaultTypeMismatch);
    assert_has_code(&err, CftErrorCode::UnknownConst);
    assert_has_code(&err, CftErrorCode::DefaultReferencesField);
    assert_has_code(&err, CftErrorCode::UnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::InvalidDefaultExpression);
}

#[test]
fn type_checker_reports_name_field_enum_function_quantifier_index_and_regex_errors() {
    let source = r#"
        const PAT = "^[a";
        enum Rarity { Common, Rare, }
        type Item {
            id: string;
            rarity: Rarity;
            tags: [string];
            scores: {string: int};
            check {
                missing != "";
                id.missing != "";
                Rarity.Missing == rarity;
                rarity > 5;
                len(id);
                all ch in id { ch != ""; }
                tags["x"] != "";
                matches(id, PAT);
                matches(id, "[");
            }
        }
    "#;
    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownValueName);
    assert_has_code(&err, CftErrorCode::FieldAccessOnNonObject);
    assert_has_code(&err, CftErrorCode::TypeUnknownEnumVariant);
    assert_has_code(&err, CftErrorCode::ComparisonTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::QuantifierRequiresCollection);
    assert_has_code(&err, CftErrorCode::IndexTypeMismatch);
    assert_has_code(&err, CftErrorCode::RegexPatternMustBeLiteral);
    assert_has_code(&err, CftErrorCode::InvalidRegexPattern);
}

#[test]
fn type_checker_accepts_nullable_guarded_access_and_ref_object_view() {
    let source = r#"
        type Item {
            @id
            id: string;
            rarity: int;
        }

        type Holder {
            maybe: Item? = null;

            @ref(Item)
            item_id: string;

            check {
                maybe != null && maybe.id != "";
                item_id.id != "";
                item_id.rarity >= 0;
            }
        }
    "#;

    compile_one(source).unwrap();
}

#[test]
fn type_checker_reports_is_predicate_and_condition_edges() {
    let source = r#"
        enum E { A, }
        type Item {
            id: string;
            check {
                id is E;
                id;
                when id { true; }
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::InvalidIsPredicate);
    assert_has_code(&err, CftErrorCode::ConditionMustBeBool);
}

#[test]
fn type_checker_reports_bitwise_shift_and_function_edges() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 2, }
        enum Color { Red, Blue, }

        type Item {
            flags: Flags;
            color: Color;
            numbers: [int];
            texts: [string];
            floats: [float];
            objects: [Item];

            check {
                flags | Flags.A != Flags.B;
                color | Color.Red != Color.Blue;
                flags & 1 != Flags.A;
                color << 1 == color;
                unique(floats);
                unique(objects);
                min(texts) != "";
                sum(texts) == 0;
                contains(numbers, "x");
                len(numbers, texts) == 0;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::BitwiseRequiresIntOrFlagEnum);
    assert_has_code(&err, CftErrorCode::ShiftRequiresInt);
    assert_has_code(&err, CftErrorCode::UniqueUnsupportedElementType);
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArityMismatch);
}

#[test]
fn type_checker_accepts_dict_entry_keys_values_and_enum_constructor() {
    let source = r#"
        @flag
        enum Flags { A = 1, B = 2, }
        enum Damage { Fire, Ice, }

        type Item {
            flags: Flags;
            resistances: {Damage: float};
            names: {string: int};

            check {
                (flags & Flags.A) != Flags(0);
                contains(resistances, Damage.Fire);
                len(keys(resistances)) >= 0;
                sum(values(names)) >= 0;
                all entry in resistances {
                    entry.key >= Damage.Fire;
                    0.0 <= entry.value <= 1.0;
                }
            }
        }
    "#;

    compile_one(source).unwrap();
}

#[test]
fn type_checker_reports_enum_constructor_and_dict_index_edges() {
    let source = r#"
        enum Damage { Fire, Ice, }
        type Item {
            resistances: {Damage: float};
            check {
                Damage("x") == Damage.Fire;
                Damage(0, 1) == Damage.Fire;
                resistances["Fire"] >= 0.0;
            }
        }
    "#;

    let err = compile_one(source).unwrap_err();
    assert_has_code(&err, CftErrorCode::FunctionArgTypeMismatch);
    assert_has_code(&err, CftErrorCode::FunctionArityMismatch);
    assert_has_code(&err, CftErrorCode::IndexTypeMismatch);
}

#[test]
fn api_exposes_schema_only_after_successful_compile() {
    let mut container = CftContainer::new();
    container
        .add_module(
            ModuleId::from("b"),
            r#"
                type B { value: int = LIMIT; }
            "#,
        )
        .unwrap();
    container
        .add_module(
            ModuleId::from("a"),
            r#"
                const LIMIT = 7;
                enum E { A, B, }
                type A { b: B; e: E = E.A; }
            "#,
        )
        .unwrap();

    assert!(container.resolve_type("A").is_none());
    container.compile().unwrap();

    assert_eq!(
        container.resolve_const("LIMIT").unwrap().value,
        CftConstValue::Int(7)
    );
    assert!(container.resolve_type("B").is_some());
    assert!(container.resolve_enum("E").is_some());
    assert_eq!(container.all_types().count(), 2);
    assert_eq!(container.all_enums().count(), 1);
    assert!(container.schema(&ModuleId::from("a")).is_some());
}

#[test]
fn failed_compile_clears_published_schema() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("ok"), "type A { id: string; }")
        .unwrap();
    container.compile().unwrap();
    assert!(container.has_type("A"));
    container
        .add_module(ModuleId::from("bad"), "type B { missing: Missing; }")
        .unwrap();
    let err = container.compile().unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownNamedType);
    assert!(!container.has_type("A"));
}

#[test]
fn spec_comprehensive_example_compiles() {
    let source = r#"
        const MAX_LEVEL  = 100;
        const MAX_ATTACK = 999;
        const MIN_SPEED  = 0.1;

        @flag
        enum Permission {
          Read    = 1,
          Write   = 2,
          Execute = 4,
        }

        enum Rarity {
          Common = 0,
          Rare   = 10,
          Epic   = 20,
        }

        enum DamageType {
          Physical,
          Fire,
          Ice,
        }

        @struct
        sealed type Vector2 {
          x: float;
          y: float;
        }

        type Stats {
          hp:     int;
          attack: int;
          speed:  float = 1.0;

          check {
            hp > 0;
            0 <= attack <= MAX_ATTACK;
            speed >= MIN_SPEED;
          }
        }

        @display("物品")
        type Item {
          @id
          id: string;

          @display("名称")
          name: string;

          rarity: Rarity = Rarity.Common;
          tags:   [string] = [];

          check {
            id != "";
            name != "";
            matches(id, "^[a-z][a-z0-9_]*$");
            none tag in tags { tag == ""; }
          }
        }

        abstract type Reward {
          @id
          id: string;

          check { id != ""; }
        }

        type ItemReward : Reward {
          @ref(Item)
          item_id: string;

          count: int = 1;

          check { count > 0; }
        }

        type CurrencyReward : Reward {
          amount: int;

          check { amount > 0; }
        }

        type DropTable {
          rewards: [Reward];
          weights: [int];

          check {
            len(rewards) == len(weights);
            len(rewards) > 0;
            sum(weights) == 100;
            min(weights) >= 0;
            any reward in rewards { reward is CurrencyReward; }
          }
        }

        @display("怪物")
        type Monster {
          @id
          id: string;

          @display("名称")
          name: string;

          @index
          rarity: Rarity;

          level:       int;
          stats:       Stats;
          drops:       DropTable;
          boss_drop:   Item? = null;
          resistances: {DamageType: float};
          skill:       Skill? = null;

          check {
            id != "";
            name != "";
            1 <= level <= MAX_LEVEL;
            stats.hp > 0;
            rarity >= Rarity.Common;
            contains(resistances, DamageType.Fire);

            when boss_drop != null {
              boss_drop.rarity >= Rarity.Rare;
            }

            all entry in resistances {
              0.0 <= entry.value <= 1.0;
            }
          }
        }

        type Skill {
          id:         string;
          is_passive: bool;
          cooldown:   float? = null;
          range:      float? = null;

          check {
            id != "";
            when !is_passive {
              cooldown != null;
              cooldown > 0.0;
            }
            when is_passive {
              range != null;
              range > 0.0;
            }
          }
        }
    "#;

    let container = compile_one(source).unwrap();
    assert!(container.has_type("Monster"));
    assert_eq!(container.all_enums().count(), 3);
}
