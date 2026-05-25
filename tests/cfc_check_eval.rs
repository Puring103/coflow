#![allow(clippy::panic, clippy::unwrap_used)]

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
