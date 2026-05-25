#![allow(clippy::panic, clippy::unwrap_used, clippy::needless_raw_string_hashes)]

use coflow::{CfcContainer, CheckError, CheckErrorKind, ModuleId};

fn check_errors(source: &str) -> Vec<String> {
    check_results(source)
        .into_iter()
        .map(|error| error.message)
        .collect()
}

fn check_results(source: &str) -> Vec<CheckError> {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let result = c.build(&root).unwrap();
    c.check(&result)
}

fn check_errors_with_import(root_source: &str, dep_source: &str) -> Vec<String> {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("dep");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(dep.clone(), dep_source).unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &dep).unwrap();
    let result = c.build(&root).unwrap();
    c.check(&result)
        .into_iter()
        .map(|error| error.message)
        .collect()
}

#[test]
fn type_check_conditions_pass() {
    let errors = check_errors(
        r#"
type Range {
  min: int;
  max: int;

  check {
    min <= max;
    0 <= min <= 10;
  }
}

range: Range = {
  min: 1,
  max: 3,
};
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn failing_conditions_are_collected() {
    let errors = check_results(
        r#"
type Range {
  min: int;
  max: int;

  check {
    min <= max;
    min >= 0;
  }
}

range: Range = {
  min: -1,
  max: -2,
};
"#,
    );

    assert_eq!(errors.len(), 2);
    assert!(errors
        .iter()
        .any(|error| matches!(&error.kind, CheckErrorKind::CondFailed { source, .. } if source == "min <= max")));
    assert!(errors
        .iter()
        .any(|error| matches!(&error.kind, CheckErrorKind::CondFailed { source, .. } if source == "min >= 0")));
}

#[test]
fn eval_error_stops_current_object_check() {
    let errors = check_results(
        r#"
type Bad {
  name: string;

  check {
    name > 0;
    missing_field > 0;
  }
}

bad: Bad = {
  name: "slime",
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. }
            if message.contains("cannot order compare string and int")
    ));
}

#[test]
fn all_over_arrays_reports_failed_items() {
    let errors = check_results(
        r#"
type Drop {
  value: int;
}

type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.value > 0;
    }
  }
}

loot: Loot = {
  drops: [
    { value: 1 },
    { value: 0 },
    { value: -1 },
  ],
};
"#,
    );

    assert_eq!(errors.len(), 1);
    let CheckErrorKind::AllFailed { total, failed, .. } = &errors[0].kind else {
        panic!("expected all failure");
    };
    assert_eq!(*total, 3);
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0].key, "drop[1]");
    assert_eq!(failed[1].key, "drop[2]");
    assert!(errors[0].message.contains("all drop in drops"));
}

#[test]
fn all_over_dict_entries_exposes_key_and_value() {
    let errors = check_results(
        r#"
type ScoreTable {
  scores: {string: int};

  check {
    all entry in scores {
      entry.value >= 0;
    }
  }
}

table: ScoreTable = {
  scores: dict{
    "alice": 1,
    "bob": -2,
  },
};
"#,
    );

    assert_eq!(errors.len(), 1);
    let CheckErrorKind::AllFailed { total, failed, .. } = &errors[0].kind else {
        panic!("expected all failure");
    };
    assert_eq!(*total, 2);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].key, "entry[1]");
    assert!(errors[0].message.contains("all entry in scores"));
}

#[test]
fn all_eval_error_is_reported_as_failed_item() {
    let errors = check_results(
        r#"
type Drop {
  value: int;
}

type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.missing > 0;
    }
  }
}

loot: Loot = {
  drops: [
    { value: 1 },
  ],
};
"#,
    );

    assert_eq!(errors.len(), 1);
    let CheckErrorKind::AllFailed { failed, .. } = &errors[0].kind else {
        panic!("expected all failure");
    };
    assert_eq!(failed.len(), 1);
    assert!(matches!(
        &failed[0].errors[0].kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("missing field `missing`")
    ));
}

#[test]
fn all_eval_error_stops_current_object_check() {
    let errors = check_results(
        r#"
type Drop {
  value: int;
}

type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.missing > 0;
    }
    false;
  }
}

loot: Loot = {
  drops: [
    { value: 1 },
  ],
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(&errors[0].kind, CheckErrorKind::AllFailed { .. }));
}

#[test]
fn top_level_checks_can_compare_nodes() {
    let errors = check_errors(
        r#"
type Stats {
  hp: int;
}

slime: Stats = {
  hp: 5,
};

goblin: Stats = {
  hp: 3,
};

check {
  slime.hp < goblin.hp;
}
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("slime.hp < goblin.hp"));
}

#[test]
fn enum_comparisons_use_enum_values() {
    let errors = check_errors(
        r#"
enum Rarity {
  common,
  rare,
  epic,
}

type Item {
  rarity: Rarity;

  check {
    rarity >= Rarity.rare;
  }
}

item: Item = {
  rarity: Rarity.common,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("rarity >= Rarity.rare"));
}

#[test]
fn shared_objects_are_checked_once() {
    let errors = check_errors(
        r#"
type Stats {
  hp: int;

  check {
    hp > 0;
  }
}

type Monster {
  stats: Stats;
}

shared: Stats = {
  hp: 0,
};

slime: Monster = {
  stats: shared,
};

goblin: Monster = {
  stats: shared,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("hp > 0"));
}

#[test]
fn arithmetic_bitwise_and_short_circuit_operators_work() {
    let errors = check_errors(
        r#"
type Stats {
  hp: int;
  flags: int;

  check {
    hp // 10 > 0;
    hp ** 2 <= 400;
    (flags & 1) != 0;
    (false && missing.value > 0) == false;
    true || missing.value > 0;
  }
}

stats: Stats = {
  hp: 20,
  flags: 3,
};
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn unparenthesized_bitwise_compare_uses_bitwise_first() {
    let errors = check_errors(
        r#"
type Stats {
  flags: int;
  mask: int;

  check {
    flags & mask != 0;
  }
}

stats: Stats = {
  flags: 3,
  mask: 1,
};
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn string_fields_can_be_concatenated() {
    let errors = check_errors(
        r#"
type Named {
  a: string;
  b: string;

  check {
    a + b == "ab";
  }
}

named: Named = {
  a: "a",
  b: "b",
};
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn user_magic_field_names_are_plain_fields() {
    let errors = check_errors(
        r#"
type Boxed {
  __module: string;
  __allow_data: bool;
  value: int;
}

value = 99;

boxed: Boxed = {
  __module: "root",
  __allow_data: true,
  value: 1,
};

check {
  boxed.value == 1;
}
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn integer_arithmetic_errors_do_not_panic() {
    let errors = check_results(
        r#"
type Limits {
  value: int;

  check {
    value + 1 > 0;
    value // -1 > 0;
    1 << 100 > 0;
  }
}

limits: Limits = {
  value: 9223372036854775807,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("integer addition overflow")
    ));
}

#[test]
fn top_level_checks_can_use_imported_data_and_enums() {
    let root = r#"
use "dep" as dep;

check {
  dep.value > 0;
  dep.rarity == dep.Rarity.rare;
}
"#;
    let dep = r#"
enum Rarity {
  common,
  rare,
}

value = 1;
rarity: Rarity = Rarity.rare;
"#;

    let errors = check_errors_with_import(root, dep);

    assert!(errors.is_empty());
}

#[test]
fn type_checks_cannot_access_top_level_data() {
    let errors = check_errors(
        r#"
type Item {
  value: int;

  check {
    value < limit;
  }
}

limit = 10;
item: Item = {
  value: 1,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("unknown name `limit`"));
}

#[test]
fn empty_all_blocks_pass() {
    let errors = check_errors(
        r#"
type Drop {
  value: int;
}

type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.value > 0;
    }
  }
}

loot: Loot = {
  drops: [],
};
"#,
    );

    assert!(errors.is_empty());
}

#[test]
fn nested_all_reports_inner_failures() {
    let errors = check_results(
        r#"
type Drop {
  value: int;
}

type Monster {
  drops: [Drop];
}

type Zone {
  monsters: [Monster];

  check {
    all monster in monsters {
      all drop in monster.drops {
        drop.value > 0;
      }
    }
  }
}

zone: Zone = {
  monsters: [
    { drops: [{ value: 1 }] },
    { drops: [{ value: 0 }, { value: 2 }] },
  ],
};
"#,
    );

    assert_eq!(errors.len(), 1);
    let CheckErrorKind::AllFailed { total, failed, .. } = &errors[0].kind else {
        panic!("expected outer all failure");
    };
    assert_eq!(*total, 2);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].key, "monster[1]");
    assert_eq!(failed[0].errors.len(), 1);
    let CheckErrorKind::AllFailed {
        total: inner_total,
        failed: inner_failed,
        ..
    } = &failed[0].errors[0].kind
    else {
        panic!("expected inner all failure");
    };
    assert_eq!(*inner_total, 2);
    assert_eq!(inner_failed.len(), 1);
    assert_eq!(inner_failed[0].key, "drop[0]");
}

#[test]
fn top_level_all_blocks_continue_across_multiple_check_blocks() {
    let errors = check_results(
        r#"
values: [int] = [1, 0, 2];

check {
  all value in values {
    value > 0;
  }
}

check {
  values[2] == 2;
}
"#,
    );

    assert_eq!(errors.len(), 1);
    let CheckErrorKind::AllFailed { total, failed, .. } = &errors[0].kind else {
        panic!("expected all failure");
    };
    assert_eq!(*total, 3);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].key, "value[1]");
}

#[test]
fn condition_must_evaluate_to_bool() {
    let errors = check_results(
        r#"
type Bad {
  value: int;

  check {
    value + 1;
    false;
  }
}

bad: Bad = {
  value: 1,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("condition must be bool")
    ));
}

#[test]
fn check_array_index_out_of_bounds_is_eval_error() {
    let errors = check_results(
        r#"
type Bag {
  values: [int];

  check {
    values[2] > 0;
    false;
  }
}

bag: Bag = {
  values: [1],
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. }
            if message.contains("array index `2` is out of bounds")
    ));
}

#[test]
fn check_dict_missing_key_is_eval_error() {
    let errors = check_results(
        r#"
type Scores {
  values: {string: int};

  check {
    values["missing"] > 0;
    false;
  }
}

scores: Scores = {
  values: dict{ "alice": 1 },
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("dict key not found")
    ));
}

#[test]
fn all_collection_must_be_array_or_dict() {
    let errors = check_results(
        r#"
type Bad {
  value: int;

  check {
    all item in value {
      true;
    }
    false;
  }
}

bad: Bad = {
  value: 1,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. }
            if message.contains("all collection must be array or dict")
    ));
}

#[test]
fn division_and_modulo_by_zero_are_eval_errors() {
    let errors = check_results(
        r#"
type Div {
  value: int;

  check {
    value / 0 > 0;
  }
}

type Mod {
  value: int;

  check {
    value % 0 == 0;
  }
}

div: Div = {
  value: 1,
};

modulo: Mod = {
  value: 1,
};
"#,
    );

    assert_eq!(errors.len(), 2);
    assert!(errors.iter().any(|error| matches!(
        &error.kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("division by zero")
    )));
    assert!(errors.iter().any(|error| matches!(
        &error.kind,
        CheckErrorKind::EvalError { message, .. } if message.contains("modulo by zero")
    )));
}

#[test]
fn ordered_comparison_between_different_enums_is_eval_error() {
    let errors = check_results(
        r#"
enum Element {
  fire,
}

enum Rarity {
  common,
}

type Pair {
  element: Element;
  rarity: Rarity;

  check {
    element < rarity;
  }
}

pair: Pair = {
  element: Element.fire,
  rarity: Rarity.common,
};
"#,
    );

    assert_eq!(errors.len(), 1);
    assert!(matches!(
        &errors[0].kind,
        CheckErrorKind::EvalError { message, .. }
            if message.contains("cannot compare enum `Element` with enum `Rarity`")
    ));
}

#[test]
fn check_power_operator_is_right_associative() {
    let errors = check_errors(
        r#"
check {
  2 ** 3 ** 2 == 512;
}
"#,
    );

    assert!(errors.is_empty());
}
