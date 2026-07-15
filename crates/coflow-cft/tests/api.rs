#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::needless_raw_string_hashes
)]

mod common;
use coflow_cft::{
    build_schema, is_cft_identifier, parse_modules, record_key_ident_error, CftDimensionInputs, CftFile,
    CftSchemaTypeRef, ModuleId, ValueDependencyMode,
};
use common::*;
use std::path::PathBuf;

#[test]
fn parsed_module_set_preserves_module_source_and_ast() {
    let modules = parse_modules([
        CftFile::new(
            ModuleId::new("schema/items.cft"),
            PathBuf::from("schema/items.cft"),
            "type Item { id: int; }",
        ),
        CftFile::new(
            ModuleId::new("schema/rewards.cft"),
            PathBuf::from("schema/rewards.cft"),
            "type Reward { value: int; }",
        ),
    ]);

    assert!(modules.diagnostics().is_empty());
    let item = modules
        .module(&ModuleId::new("schema/items.cft"))
        .expect("collected item module");
    assert_eq!(item.source(), "type Item { id: int; }");
    assert_eq!(item.path(), PathBuf::from("schema/items.cft").as_path());
    assert_eq!(item.ast().expect("parsed AST").items.len(), 1);
}

#[test]
fn build_schema_compiles_a_parsed_module_set() {
    let modules = parse_modules([CftFile::new(
        ModuleId::new("schema/items.cft"),
        PathBuf::from("schema/items.cft"),
        "type Item { value: int; }",
    )]);

    let schema = build_schema(&modules, &CftDimensionInputs::default())
        .expect("parsed module set compiles");

    assert!(schema.resolve_type("Item").is_some());
    assert_eq!(
        schema.field("Item", "value").map(|field| &field.ty_ref),
        Some(&CftSchemaTypeRef::Int)
    );
}

#[test]
fn build_schema_models_configured_dimension_directly() {
    let modules = parse_modules([CftFile::new(
        ModuleId::new("schema/items.cft"),
        PathBuf::from("schema/items.cft"),
        "type Item { @localized name: string; }",
    )]);
    let dimensions = CftDimensionInputs::new([("language", vec!["zh".to_string()])]);

    let schema = build_schema(&modules, &dimensions).expect("dimension schema compiles");

    let dimension = schema
        .resolve_dimension("language")
        .expect("localized dimension");
    assert_eq!(
        dimension
            .variants
            .iter()
            .map(|variant| variant.as_str())
            .collect::<Vec<_>>(),
        ["zh"]
    );
    assert_eq!(dimension.fields.len(), 1);
    assert_eq!(dimension.fields[0].declaring_type.as_str(), "Item");
    assert_eq!(schema.all_types().count(), 1);
}

#[test]
fn dimension_schema_does_not_reserve_generated_type_names() {
    let modules = parse_modules([CftFile::new(
        ModuleId::new("schema/items.cft"),
        PathBuf::from("schema/items.cft"),
        r#"
            enum __coflow_dimension_Item_name { Value }
            type Item { @localized name: string; }
        "#,
    )]);
    let dimensions = CftDimensionInputs::new([("language", vec!["zh".to_string()])]);

    let schema = build_schema(&modules, &dimensions).expect("no generated type can collide");
    assert!(schema.resolve_enum("__coflow_dimension_Item_name").is_some());
    assert_eq!(schema.all_types().count(), 1);
}

#[test]
fn record_key_identifier_helper_accepts_only_cft_identifiers() {
    for key in ["fireball", "Gene_孢子", "_private"] {
        assert!(
            is_cft_identifier(key),
            "expected `{key}` to be a valid identifier key"
        );
        assert_eq!(record_key_ident_error(key), None);
    }

    for key in [
        "",
        "id",
        "Id",
        "ID",
        "type",
        "fire-ball",
        "fire.ball",
        "123abc",
        "\"fireball\"",
    ] {
        assert!(
            !is_cft_identifier(key),
            "expected `{key}` to be rejected as an identifier key"
        );
        assert!(
            record_key_ident_error(key).is_some(),
            "expected an error for `{key}`"
        );
    }
}

#[test]
fn value_dependency_plan_reports_direct_default_cycle() {
    let schema = compile_one("type Node { child: Node = {}; }").expect("schema compiles");
    let compiled = &schema;
    let cycle = compiled
        .value_dependencies()
        .materialization_order("Node", ValueDependencyMode::SchemaDefaults)
        .expect("known type")
        .expect_err("self default must be cyclic");

    assert_eq!(cycle.to_string(), "Node.child -> Node");
}

#[test]
fn value_dependency_plan_reports_indirect_default_cycle_stably() {
    let schema = compile_one(
        r#"
            type A { b: B = {}; }
            type B { c: C = {}; }
            type C { a: A = {}; }
        "#,
    )
    .expect("schema compiles");
    let compiled = &schema;
    let cycle = compiled
        .value_dependencies()
        .materialization_order("A", ValueDependencyMode::SchemaDefaults)
        .expect("known type")
        .expect_err("indirect default must be cyclic");

    assert_eq!(cycle.to_string(), "A.b -> B.c -> C.a -> A");
}

#[test]
fn value_dependency_plan_memoizes_shared_subgraphs_in_topological_order() {
    let schema = compile_one(
        r#"
            type Leaf { value: int = 1; }
            type Branch { leaf: Leaf = {}; }
            type Root { left: Branch = {}; right: Branch = {}; }
        "#,
    )
    .expect("schema compiles");
    let compiled = &schema;
    let order = compiled
        .value_dependencies()
        .materialization_order("Root", ValueDependencyMode::SchemaDefaults)
        .expect("known type")
        .expect("graph is acyclic");

    assert_eq!(order, ["Leaf", "Branch", "Root"]);
}

#[test]
fn typed_check_schedule_borrows_inherited_blocks_in_parent_first_order() {
    let schema = compile_one(
        r#"
            abstract type Base {
                base_value: int;
                check { base_value > 0; }
            }
            type Child : Base {
                child_value: int;
                check { child_value > 0; }
            }
        "#,
    )
    .expect("schema compiles");
    let compiled = &schema;
    let checks = compiled.check_schedule("Child", None).collect::<Vec<_>>();

    assert_eq!(checks.len(), 2);
    assert!(std::ptr::eq(
        checks[0],
        compiled
            .resolve_type("Base")
            .and_then(|meta| meta.check.as_ref())
            .expect("base check")
    ));
    assert!(std::ptr::eq(
        checks[1],
        compiled
            .resolve_type("Child")
            .and_then(|meta| meta.check.as_ref())
            .expect("child check")
    ));
}

#[test]
fn dimension_check_schedule_includes_inherited_dimension_checks() {
    let schema = compile_one_with_dimensions(
        r#"
            abstract type Base {
                @localized
                base_name: string;
                check { base_name != ""; }
            }
            type Child : Base {
                @localized
                child_name: string;
                check { child_name != ""; }
            }
        "#,
        CftDimensionInputs::new([("language", vec!["zh".to_string()])]),
    )
    .expect("schema compiles");
    let compiled = &schema;

    assert_eq!(
        compiled.check_schedule("Child", Some("language")).count(),
        2
    );
}

#[test]
fn dimension_check_analysis_respects_quantifier_binding_shadowing() {
    let schema = compile_one_with_dimensions(
        r#"
            type Item {
                @localized
                item: string;
                items: [string];
                check {
                    all item in items { item != ""; }
                }
            }
        "#,
        CftDimensionInputs::new([("language", vec!["zh".to_string()])]),
    )
    .expect("schema compiles");
    let compiled = &schema;

    assert_eq!(compiled.check_schedule("Item", Some("language")).count(), 0);
}

#[test]
fn schema_indexes_dimension_fields() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main"),
        r#"
            type Item { @localized name: string; }
            type Weapon : Item { damage: int; }
        "#,
    )]);
    let schema = build_schema(
        &modules,
        &CftDimensionInputs::new([("language", vec!["zh".to_string()])]),
    )
    .expect("schema compiles");
    let dimension = schema.resolve_dimension("language").expect("dimension");
    assert_eq!(dimension.fields.len(), 1);
    assert_eq!(dimension.fields[0].declaring_type.as_str(), "Item");
    assert_eq!(
        schema.field("Weapon", "name"),
        Some(dimension.fields[0].as_ref())
    );
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
        @idAsEnum(ItemKey)
        type Item {
          key: string;
          name: string;

          rarity: Rarity = Rarity.Common;
          tags:   [string] = [];

          check {
            id != "";
            key != "";
            name != "";
            key.matches("^[a-z][a-z0-9_]*$");
            none tag in tags { tag == ""; }
          }
        }
        enum ItemKey {}

        abstract type Reward {
          key: string;

          check { id != ""; key != ""; }
        }

        type ItemReward : Reward {
          item: Item;

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
            rewards.len() == weights.len();
            rewards.len() > 0;
            weights.sum() == 100;
            weights.min() >= 0;
            any reward in rewards { reward is CurrencyReward; }
          }
        }
        @idAsEnum(MonsterKey)
        type Monster {
          key: string;
          name: string;

          rarity: Rarity;

          level:       int;
          stats:       Stats;
          drops:       DropTable;
          boss_drop:   Item? = null;
          resistances: {DamageType: float};
          skill:       Skill? = null;

          check {
            id != "";
            key != "";
            name != "";
            1 <= level <= MAX_LEVEL;
            stats.hp > 0;
            rarity >= Rarity.Common;
            resistances.contains(DamageType.Fire);

            when boss_drop != null {
              boss_drop.rarity >= Rarity.Rare;
            }

            all entry in resistances {
              0.0 <= entry.value <= 1.0;
            }
          }
        }
        enum MonsterKey {}

        type Skill {
          key:        string;
          is_passive: bool;
          cooldown:   float? = null;
          range:      float? = null;

          check {
            id != "";
            key != "";
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
    assert!(container.resolve_type("Monster").is_some());
    assert_eq!(container.all_enums().count(), 5);
}

#[test]
fn typed_check_plan_marks_only_fields_that_can_reach_nested_checks() {
    let container = compile_one(
        r#"
            abstract type Reward {}
            type CheckedReward : Reward { value: int; check { value > 0; } }
            type Wrapper { reward: Reward; }
            type Recursive { child: Recursive? = null; check { true; } }
            type Holder {
                primitive: [int];
                wrapped: [Wrapper];
                recursive: Recursive;
                reference: &CheckedReward;
            }
        "#,
    )
    .expect("schema compiles");
    let schema = container;

    assert!(!schema.field_has_nested_checks("Holder", "primitive"));
    assert!(schema.field_has_nested_checks("Holder", "wrapped"));
    assert!(schema.field_has_nested_checks("Holder", "recursive"));
    assert!(!schema.field_has_nested_checks("Holder", "reference"));
    assert!(schema.field_has_nested_checks("Wrapper", "reward"));
    assert!(schema.field_has_nested_checks("Recursive", "child"));
}
