#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow::{CfcContainer, CfcValue, ModuleId, ResolveError};

#[test]
fn imported_enum_values_work_as_typed_dict_keys() {
    let common = r#"
enum Rarity {
  common,
  rare = 10,
}

common_weight = 3;
"#;
    let game = r#"
use "common" as common;

type Weights {
  values: {common.Rarity: int};
  copied: int;
}

weights: Weights = {
  values: dict{
    common.Rarity.common: common.common_weight,
    common.Rarity.rare: 7,
  },
  copied: values[0],
};
"#;

    let mut c = CfcContainer::new();
    let common_id = ModuleId::from("common");
    let game_id = ModuleId::from("game");
    c.add_module(common_id.clone(), common).unwrap();
    c.add_module(game_id.clone(), game).unwrap();
    let import = c.imports(&game_id).unwrap()[0].id;
    c.bind_import(&game_id, import, &common_id).unwrap();

    let result = c.build(&game_id).unwrap();
    let weights = result.root().unwrap().get("weights").unwrap();
    let borrowed = weights.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected weights object");
    };

    assert!(matches!(
        &*fields.get("copied").unwrap().borrow(),
        CfcValue::Int(3)
    ));
}

#[test]
fn imported_enum_values_work_without_context_type() {
    let common = r#"
enum Rarity {
  common,
  rare,
}
"#;
    let game = r#"
use "common" as common;

rarity = common.Rarity.common;
"#;

    let mut c = CfcContainer::new();
    let common_id = ModuleId::from("common");
    let game_id = ModuleId::from("game");
    c.add_module(common_id.clone(), common).unwrap();
    c.add_module(game_id.clone(), game).unwrap();
    let import = c.imports(&game_id).unwrap()[0].id;
    c.bind_import(&game_id, import, &common_id).unwrap();

    let result = c.build(&game_id).unwrap();
    let rarity = result.root().unwrap().get("rarity").unwrap();

    assert!(matches!(
        &*rarity.borrow(),
        CfcValue::Enum { variant, value, .. } if variant == "common" && *value == 0
    ));
}

#[test]
fn load_graph_resolves_multi_module_complex_references() {
    let root = r#"
use "monsters" as monsters;

copy = {
  hp: monsters.slime.stats.hp,
  first_drop: monsters.slime.drops[0].id,
};
"#;
    let monsters = r#"
use "common" as common;

type Monster {
  stats: common.Stats;
  drops: [common.Drop];
}

slime: Monster = {
  stats: common.base_stats,
  drops: [
    common.coin,
  ],
};
"#;
    let common = r#"
type Stats {
  hp: int;
}

type Drop {
  id: string;
}

base_stats: Stats = {
  hp: 30,
};

coin: Drop = {
  id: "coin",
};
"#;

    let mut c = CfcContainer::new();
    let result = c
        .load_graph(ModuleId::from("root"), root, |_, import| {
            match import.path.as_str() {
                "monsters" => Ok((ModuleId::from("monsters"), monsters.to_string())),
                "common" => Ok((ModuleId::from("common"), common.to_string())),
                _ => Err(ResolveError::new("unknown import")),
            }
        })
        .unwrap();

    let copy = result.root().unwrap().get("copy").unwrap();
    let borrowed = copy.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected copy object");
    };

    assert!(matches!(
        &*fields.get("hp").unwrap().borrow(),
        CfcValue::Int(30)
    ));
    assert!(matches!(
        &*fields.get("first_drop").unwrap().borrow(),
        CfcValue::String(value) if value == "coin"
    ));
}

#[test]
fn complex_errors_are_aggregated_across_nodes() {
    let source = r#"
type Stats {
  hp: int;
}

bad_stats: Stats = {
  hp: "wrong",
};

missing: Stats = {
};

ok_stats: Stats = {
  hp: 1,
};

bad_path: Stats = {
  hp: ok_stats.nope,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err.errors.len() >= 3);
    assert!(err.errors.iter().any(|e| e.message == "expected `int`"));
    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("missing required field `hp`")));
    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("missing field `nope`")));
}

#[test]
fn comments_are_ignored_in_all_supported_positions() {
    let source = r#"
// file header
type Stats { // type comment
  hp: int; // field comment
}

// data comment
stats: Stats = {
  hp: 10, // object field comment
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    assert!(result.root().unwrap().get("stats").is_some());
}

#[test]
fn unicode_identifiers_and_strings_are_preserved() {
    let source = r#"
type 本地化 {
  名称: string;
}

玩家生命 = 100;
名前: 本地化 = {
  名称: "史莱姆 Ω",
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();
    assert!(module.get("玩家生命").is_some());

    let named = module.get("名前").unwrap();
    let borrowed = named.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };

    assert!(matches!(
        &*fields.get("名称").unwrap().borrow(),
        CfcValue::String(value) if value == "史莱姆 Ω"
    ));
}

#[test]
fn build_accepts_type_and_top_level_check_blocks() {
    let source = r#"
type Range {
  min: int;
  max: int;

  check {
    min <= max;
    max >= min;
  }
}

low: Range = {
  min: 1,
  max: 2,
};

check {
  low.min < low.max;
}

high: Range = {
  min: 3,
  max: 4,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    assert!(result.root().unwrap().get("low").is_some());
    assert!(result.root().unwrap().get("high").is_some());
    assert!(c.check(&result).is_empty());
}

#[test]
fn fields_after_type_check_block_are_parse_errors() {
    let source = r#"
type Bad {
  value: int;
  check {
    true;
  }
  later: int;
}
"#;

    let mut c = CfcContainer::new();
    let err = c.add_module(ModuleId::from("root"), source).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `}`")));
}

#[test]
fn any_fields_accept_empty_and_mixed_complex_values() {
    let source = r#"
type Bag {
  empty_array: any;
  empty_dict: any;
  mixed_array: any;
  mixed_dict: any;
}

bag: Bag = {
  empty_array: [],
  empty_dict: dict{},
  mixed_array: [1, "two", { ok: true }],
  mixed_dict: dict{ "a": 1, "b": "two", "c": { ok: true } },
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let bag = result.root().unwrap().get("bag").unwrap();
    let borrowed = bag.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected bag object");
    };

    assert!(matches!(
        &*fields.get("empty_array").unwrap().borrow(),
        CfcValue::Array(items) if items.is_empty()
    ));
    assert!(matches!(
        &*fields.get("empty_dict").unwrap().borrow(),
        CfcValue::Dict(entries) if entries.is_empty()
    ));
}

#[test]
fn any_still_requires_references_to_resolve() {
    let source = r#"
type Bag {
  value: any;
}

bag: Bag = {
  value: missing,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("unknown data node `missing`")));
}

#[test]
fn typed_empty_array_and_dict_values_are_valid() {
    let source = r#"
type Empty {
  names: [string];
  scores: {string: int};
}

empty: Empty = {
  names: [],
  scores: dict{},
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let empty = result.root().unwrap().get("empty").unwrap();
    let borrowed = empty.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected empty object");
    };

    assert!(matches!(
        &*fields.get("names").unwrap().borrow(),
        CfcValue::Array(items) if items.is_empty()
    ));
    assert!(matches!(
        &*fields.get("scores").unwrap().borrow(),
        CfcValue::Dict(entries) if entries.is_empty()
    ));
}
