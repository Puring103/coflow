#![allow(clippy::panic, clippy::unwrap_used, clippy::needless_raw_string_hashes)]

use coflow::{CfcContainer, ModuleId};

#[test]
fn parses_type_check_conditions_and_all_blocks() {
    let source = r#"
enum Rarity {
  common,
  rare,
}

type Drop {
  value: int;
  rarity: Rarity;
}

type Loot {
  drops: [Drop];
  scores: {string: int};

  check {
    all drop in drops {
      drop.value > 0;
      drop.rarity != Rarity.common;
    }
    all entry in scores {
      entry.value >= 0;
    }
  }
}

loot: Loot = {
  drops: [],
  scores: dict{},
};
"#;

    let mut c = CfcContainer::new();
    c.add_module(ModuleId::from("root"), source).unwrap();
}

#[test]
fn parses_top_level_check_expression_operators() {
    let source = r#"
type Flags {
  flags: int;
  mask: int;
  hp: int;
}

flags: Flags = {
  flags: 3,
  mask: 1,
  hp: 25,
};

check {
  flags.flags & flags.mask != 0;
  flags.hp // 10 > 0;
  flags.hp ** 2 <= 1000;
  !(flags.hp < 0) && ~flags.mask < 0;
}
"#;

    let mut c = CfcContainer::new();
    c.add_module(ModuleId::from("root"), source).unwrap();
}

#[test]
fn rejects_mixed_direction_chain_comparisons() {
    let source = r#"
type Bad {
  value: int;

  check {
    0 < value > 10;
  }
}
"#;

    let mut c = CfcContainer::new();
    let err = c.add_module(ModuleId::from("root"), source).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("consistent direction")));
}

#[test]
fn rejects_not_equal_chain_comparisons() {
    let source = r#"
type Bad {
  value: int;

  check {
    0 != value != 10;
  }
}
"#;

    let mut c = CfcContainer::new();
    let err = c.add_module(ModuleId::from("root"), source).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("cannot be used in chain")));
}
