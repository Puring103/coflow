#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow_cfc::{CfcContainer, CfcValue, ModuleId};

#[test]
fn same_type_name_from_different_modules_is_not_compatible() {
    let root_source = r#"
use "other.cfc" as other;

type Item {
  id: string;
}

local: Item = { id: "x" };
remote: other.Item = local;
"#;
    let other_source = r#"
type Item {
  id: string;
}
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let other = ModuleId::from("other");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(other.clone(), other_source).unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &other).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `other.Item`")));
}

#[test]
fn imported_enum_value_must_match_expected_enum() {
    let root_source = r#"
use "other.cfc" as other;

enum Local {
  common,
}

value: other.Rarity = other.Local.common;
"#;
    let other_source = r#"
enum Rarity {
  common,
}

enum Local {
  common,
}
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let other = ModuleId::from("other");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(other.clone(), other_source).unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &other).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `Rarity`")));
}

#[test]
fn empty_array_requires_context_type() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), "items = [];").unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err.errors.iter().any(|e| e.message.contains("empty array")));
}

#[test]
fn untyped_array_elements_must_have_same_type() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), r#"items = [1, "a"];"#).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("array elements")));
}

#[test]
fn untyped_dict_keys_and_values_must_have_same_type() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), r#"scores = dict{ "a": 1, 2: "b" };"#)
        .unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| { e.message.contains("dict keys") || e.message.contains("dict values") }));
}

#[test]
fn dict_key_type_must_be_string_int_or_enum() {
    let source = r#"
type Bad {
  values: {float: int};
}

bad: Bad = {
  values: dict{ 1.0: 1 },
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("dict key type")));
}

#[test]
fn field_default_must_be_constant() {
    let source = r#"
type Bad {
  value: int = shared;
}

shared = 1;
bad: Bad = {};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("must be a constant")));
}

#[test]
fn data_path_defaults_are_not_constants() {
    let source = r#"
type Bad {
  value: int = shared.value;
}

shared = {
  value: 1,
};

bad: Bad = {};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("must be a constant")));
}

#[test]
fn imported_data_paths_are_not_field_default_constants() {
    let source = r#"
use "common" as common;

type Bad {
  value: int = common.shared.value;
}

bad: Bad = {};
"#;
    let common = r#"
shared = {
  value: 1,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let common_id = ModuleId::from("common");
    c.add_module(root.clone(), source).unwrap();
    c.add_module(common_id.clone(), common).unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &common_id).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("must be a constant")));
}

#[test]
fn data_before_type_is_parse_error() {
    let mut c = CfcContainer::new();
    let err = c
        .add_module(
            ModuleId::from("root"),
            r#"
value = 1;
type Late {
  id: string;
}
"#,
        )
        .unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("before data definitions")));
}

#[test]
fn imported_type_fields_resolve_nested_types_from_defining_module() {
    let lib_source = r#"
type Stats {
  hp: int;
}

type Monster {
  stats: Stats;
}
"#;
    let game_source = r#"
use "lib" as lib;

slime: lib.Monster = {
  stats: {
    hp: 7,
  },
};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), lib_source).unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let result = c.build(&game).unwrap();
    let slime = result.root().unwrap().get("slime").unwrap();
    let borrowed = slime.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };
    let stats = fields.get("stats").unwrap();
    let stats_borrowed = stats.borrow();
    let CfcValue::Object { type_name, fields } = &*stats_borrowed else {
        panic!("expected stats object");
    };

    assert_eq!(type_name.as_ref().unwrap().module, lib);
    assert_eq!(type_name.as_ref().unwrap().name, "Stats");
    assert!(matches!(
        &*fields.get("hp").unwrap().borrow(),
        CfcValue::Int(7)
    ));
}

#[test]
fn imported_type_defaults_resolve_nested_types_from_defining_module() {
    let lib_source = r#"
type Stats {
  hp: int = 3;
}

type Monster {
  stats: Stats = {
    hp: 5,
  };
}
"#;
    let game_source = r#"
use "lib" as lib;

slime: lib.Monster = {};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), lib_source).unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let result = c.build(&game).unwrap();
    let slime = result.root().unwrap().get("slime").unwrap();
    let borrowed = slime.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };
    let stats = fields.get("stats").unwrap();
    let stats_borrowed = stats.borrow();
    let CfcValue::Object { type_name, fields } = &*stats_borrowed else {
        panic!("expected stats object");
    };

    assert_eq!(type_name.as_ref().unwrap().module, lib);
    assert_eq!(type_name.as_ref().unwrap().name, "Stats");
    assert!(matches!(
        &*fields.get("hp").unwrap().borrow(),
        CfcValue::Int(5)
    ));
}

#[test]
fn fields_can_reference_imported_data_deep_paths() {
    let lib_source = r#"
base = {
  stats: {
    hp: 12,
  },
};
"#;
    let game_source = r#"
use "lib" as lib;

type Monster {
  hp: int;
}

slime: Monster = {
  hp: lib.base.stats.hp,
};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), lib_source).unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let result = c.build(&game).unwrap();
    let slime = result.root().unwrap().get("slime").unwrap();
    let borrowed = slime.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };

    assert!(matches!(
        &*fields.get("hp").unwrap().borrow(),
        CfcValue::Int(12)
    ));
}

#[test]
fn fields_can_reference_imported_data_index_paths() {
    let lib_source = r#"
drops = [
  {
    amount: 4,
  },
];
"#;
    let game_source = r#"
use "lib" as lib;

type Loot {
  amount: int;
}

loot: Loot = {
  amount: lib.drops[0].amount,
};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), lib_source).unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let result = c.build(&game).unwrap();
    let loot = result.root().unwrap().get("loot").unwrap();
    let borrowed = loot.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };

    assert!(matches!(
        &*fields.get("amount").unwrap().borrow(),
        CfcValue::Int(4)
    ));
}

#[test]
fn local_field_paths_take_priority_over_import_aliases() {
    let lib_source = "value = 99;";
    let game_source = r#"
use "lib" as lib;

type Boxed {
  lib: {string: int};
  copied: int;
}

boxed: Boxed = {
  lib: dict{ "value": 3 },
  copied: lib[0],
};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), lib_source).unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let result = c.build(&game).unwrap();
    let boxed = result.root().unwrap().get("boxed").unwrap();
    let borrowed = boxed.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };

    assert!(matches!(
        &*fields.get("copied").unwrap().borrow(),
        CfcValue::Int(3)
    ));
}

#[test]
fn missing_imported_path_reports_remote_data_name() {
    let game_source = r#"
use "lib" as lib;

type Monster {
  hp: int;
}

slime: Monster = {
  hp: lib.missing.hp,
};
"#;

    let mut c = CfcContainer::new();
    let lib = ModuleId::from("lib");
    let game = ModuleId::from("game");
    c.add_module(lib.clone(), "").unwrap();
    c.add_module(game.clone(), game_source).unwrap();
    let import = c.imports(&game).unwrap()[0].id;
    c.bind_import(&game, import, &lib).unwrap();

    let err = c.build(&game).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("unknown data node `lib.missing`")));
}

#[test]
fn complex_references_inside_typed_arrays_and_dicts_resolve() {
    let source = r#"
type Drop {
  id: string;
  amount: int;
}

type LootTable {
  first_id: string;
  drops: [Drop];
  by_name: {string: Drop};
  copied_amount: int;
}

base: Drop = {
  id: "coin",
  amount: 2,
};

table: LootTable = {
  first_id: base.id,
  drops: [
    base,
    {
      id: first_id,
      amount: base.amount,
    },
  ],
  by_name: dict{
    first_id: drops[0],
    "second": drops[1],
  },
  copied_amount: by_name[1].amount,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let table = result.root().unwrap().get("table").unwrap();
    let borrowed = table.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected table object");
    };

    assert!(matches!(
        &*fields.get("copied_amount").unwrap().borrow(),
        CfcValue::Int(2)
    ));
}

#[test]
fn nominal_union_alias_selects_branch_by_literal_kind() {
    let source = r#"
type ItemReward {
  kind: "item" = "item";
  item: string;
}

type CurrencyReward {
  kind: "currency" = "currency";
  amount: int;
}

type Reward = ItemReward | CurrencyReward;

type Chest {
  reward: Reward;
}

chest: Chest = {
  reward: {
    kind: "currency",
    amount: 100,
  },
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let chest = result.root().unwrap().get("chest").unwrap();
    let borrowed = chest.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected chest object");
    };
    let reward = fields.get("reward").unwrap();
    let reward_borrowed = reward.borrow();
    let CfcValue::Object { type_name, fields } = &*reward_borrowed else {
        panic!("expected reward object");
    };

    assert_eq!(type_name.as_ref().unwrap().name, "CurrencyReward");
    assert!(matches!(
        &*fields.get("amount").unwrap().borrow(),
        CfcValue::Int(100)
    ));
}

#[test]
fn union_alias_accepts_named_branch_values() {
    let source = r#"
type ItemReward {
  kind: "item" = "item";
  item: string;
}

type CurrencyReward {
  kind: "currency" = "currency";
  amount: int;
}

type Reward = ItemReward | CurrencyReward;

coin: CurrencyReward = {
  amount: 10,
};

reward: Reward = coin;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let reward = result.root().unwrap().get("reward").unwrap();
    let borrowed = reward.borrow();
    let CfcValue::Object { type_name, .. } = &*borrowed else {
        panic!("expected reward object");
    };

    assert_eq!(type_name.as_ref().unwrap().name, "CurrencyReward");
}

#[test]
fn union_alias_rejects_unknown_kind() {
    let source = r#"
type ItemReward {
  kind: "item" = "item";
  item: string;
}

type CurrencyReward {
  kind: "currency" = "currency";
  amount: int;
}

type Reward = ItemReward | CurrencyReward;

reward: Reward = {
  kind: "xp",
  amount: 100,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("no union branch matches kind `xp`")));
}

#[test]
fn string_literal_fields_validate_values() {
    let source = r#"
type CurrencyReward {
  kind: "currency" = "currency";
  amount: int;
}

bad: CurrencyReward = {
  kind: "item",
  amount: 1,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `\"currency\"`")));
}
