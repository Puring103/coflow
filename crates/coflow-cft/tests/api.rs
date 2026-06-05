#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::needless_raw_string_hashes
)]

mod common;
use common::*;

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
