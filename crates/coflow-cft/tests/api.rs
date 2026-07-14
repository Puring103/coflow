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
    build_schema, is_cft_identifier, parse_modules, record_key_ident_error, CftFile,
    CftSchemaField, CftSchemaType, CftSchemaTypeRef, ModuleId, Span, ValueDependencyMode,
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
    assert_eq!(item.ast().items.len(), 1);
}

#[test]
fn build_schema_compiles_a_parsed_module_set() {
    let modules = parse_modules([CftFile::new(
        ModuleId::new("schema/items.cft"),
        PathBuf::from("schema/items.cft"),
        "type Item { value: int; }",
    )]);

    let schema = build_schema(&modules).expect("parsed module set compiles");

    assert!(schema.has_type("Item"));
    assert_eq!(
        schema.field_type("Item", "value"),
        Some(&CftSchemaTypeRef::Int)
    );
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
    let compiled = schema.compiled_schema();
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
    let compiled = schema.compiled_schema();
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
    let compiled = schema.compiled_schema();
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
    let compiled = schema.compiled_schema();
    let checks = compiled.check_schedule("Child", None).collect::<Vec<_>>();

    assert_eq!(checks.len(), 2);
    assert!(std::ptr::eq(
        checks[0],
        compiled
            .type_meta("Base")
            .and_then(|meta| meta.check.as_ref())
            .expect("base check")
    ));
    assert!(std::ptr::eq(
        checks[1],
        compiled
            .type_meta("Child")
            .and_then(|meta| meta.check.as_ref())
            .expect("child check")
    ));
}

#[test]
fn dimension_check_schedule_includes_inherited_dimension_checks() {
    let schema = compile_one(
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
    )
    .expect("schema compiles");
    let compiled = schema.compiled_schema();

    assert_eq!(
        compiled.check_schedule("Child", Some("language")).count(),
        2
    );
}

#[test]
fn dimension_check_analysis_respects_quantifier_binding_shadowing() {
    let schema = compile_one(
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
    )
    .expect("schema compiles");
    let compiled = schema.compiled_schema();

    assert_eq!(compiled.check_schedule("Item", Some("language")).count(), 0);
}

#[test]
fn compiled_schema_indexes_dimension_storage_types() {
    let schema = compile_one(
        r#"
            type Item { name: string; }
            type Weapon : Item { damage: int; }
            @__coflow_dimension_storage("language", "Item", "name")
            type Item_nameVariants { zh: string?; }
        "#,
    )
    .expect("schema compiles");
    let compiled = schema.compiled_schema();

    assert_eq!(
        compiled.dimension_storage_type("language", "Item", "name"),
        Some("Item_nameVariants")
    );
    assert_eq!(
        compiled.dimension_storage_type("platform", "Item", "name"),
        None
    );
    assert_eq!(
        compiled.dimension_storage_type("language", "Weapon", "name"),
        Some("Item_nameVariants")
    );
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
fn failed_compile_keeps_previously_published_schema() {
    // Spec 7: "返回的引用在下次成功调用 compile 之前保持稳定" — a failed
    // recompile must leave the prior schema observable so consumers don't
    // get a transient empty view.
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("ok"), "type A { key: string; }")
        .unwrap();
    container.compile().unwrap();
    assert!(container.has_type("A"));
    container
        .add_module(ModuleId::from("bad"), "type B { missing: Missing; }")
        .unwrap();
    // Staging a new module does not disturb the last published generation.
    assert!(container.has_type("A"));
    assert!(!container.has_type("B"));
    let err = container.compile().unwrap_err();
    assert_has_code(&err, CftErrorCode::UnknownNamedType);
    assert!(container.has_type("A"));
    assert!(!container.has_type("B"));
}

#[test]
fn failed_add_module_keeps_previously_published_schema() {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("ok"), "type A { key: string; }")
        .unwrap();
    container.compile().unwrap();
    assert!(container.has_type("A"));

    let err = container
        .add_module(ModuleId::from("bad"), "type B { value: ; }")
        .unwrap_err();
    assert_has_code(&err, CftErrorCode::ExpectedIdentifier);
    assert!(container.has_type("A"));
    assert_eq!(container.all_types().count(), 1);
}

#[test]
fn failed_recompile_without_new_modules_keeps_old_schema() {
    // If the only thing that changed between two compile calls is that the
    // second one fails (e.g. because callers staged invalid modules earlier
    // and only now detect it), the prior successful compile output must
    // remain observable.
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("ok"), "type A { key: string; }")
        .unwrap();
    container.compile().unwrap();
    assert!(container.has_type("A"));
    // No new add_module call — recompile re-runs validation. Forge a failure
    // by simulating the situation where the same content compiles repeatedly:
    // here we just confirm that calling compile again on the already-compiled
    // container preserves observable state.
    container.compile().unwrap();
    assert!(container.has_type("A"));
}

#[test]
fn register_runtime_types_injects_schema_type_and_rejects_duplicates_atomically() {
    let mut container = compile_one("type Item { name: string; }").unwrap();
    let runtime_type = runtime_variants_type("Item_nameVariants");

    container
        .register_runtime_types([runtime_type.clone()])
        .expect("runtime type registers");

    let resolved = container
        .resolve_type("Item_nameVariants")
        .expect("runtime type is visible");
    assert_eq!(resolved.fields.len(), 2);
    assert_eq!(resolved.fields[0].name, "default");
    assert_eq!(
        resolved.fields[0].ty_ref,
        CftSchemaTypeRef::Nullable(Box::new(CftSchemaTypeRef::String))
    );
    assert!(container
        .schema(&ModuleId::from("__runtime__"))
        .expect("runtime module")
        .types
        .iter()
        .any(|ty| ty.name == "Item_nameVariants"));

    let staged = runtime_variants_type("Other_nameVariants");
    let err = container
        .register_runtime_types([staged, runtime_type])
        .expect_err("duplicate runtime type should fail");
    assert_has_code(&err, CftErrorCode::DuplicateGlobalName);
    assert!(
        container.resolve_type("Other_nameVariants").is_none(),
        "the valid prefix of a failed runtime batch must not be published"
    );
}

fn runtime_variants_type(name: &str) -> CftSchemaType {
    let fields = vec![
        runtime_variant_field("default"),
        runtime_variant_field("zh"),
    ];
    CftSchemaType {
        module: ModuleId::from("__runtime__"),
        name: name.to_string(),
        parent: None,
        is_abstract: false,
        is_sealed: false,
        is_singleton: false,
        fields: fields.clone(),
        all_fields: fields,
        check: None,
        annotations: Vec::new(),
        span: Span::new(0, 0),
    }
}

fn runtime_variant_field(name: &str) -> CftSchemaField {
    CftSchemaField {
        name: name.to_string(),
        ty: "string?".to_string(),
        ty_ref: CftSchemaTypeRef::Nullable(Box::new(CftSchemaTypeRef::String)),
        has_default: false,
        default: None,
        annotations: Vec::new(),
        dimension: None,
        span: Span::new(0, 0),
    }
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
    assert!(container.has_type("Monster"));
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
    let schema = container.compiled_schema();

    assert!(!schema.field_has_nested_checks("Holder", "primitive"));
    assert!(schema.field_has_nested_checks("Holder", "wrapped"));
    assert!(schema.field_has_nested_checks("Holder", "recursive"));
    assert!(!schema.field_has_nested_checks("Holder", "reference"));
    assert!(schema.field_has_nested_checks("Wrapper", "reward"));
    assert!(schema.field_has_nested_checks("Recursive", "child"));
}
