#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow::{CfcContainer, CfcValue, CfcValueRef, ModuleId};

#[test]
fn type_fields_support_forward_references() {
    let source = r#"
type A {
  b: B;
  rarity: Rarity;
}

type B {
  value: int;
}

enum Rarity {
  common,
}

a: A = {
  b: { value: 1 },
  rarity: Rarity.common,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn object_field_values_can_reference_previous_fields() {
    let source = r#"
type Range {
  min: int;
  max: int;
}

range: Range = {
  min: 1,
  max: min,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let result = c.build(&root).unwrap();
    let range = result.root().unwrap().get("range").unwrap();
    let borrowed = range.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected object");
    };

    assert!(CfcValueRef::ptr_eq(
        fields.get("min").unwrap(),
        fields.get("max").unwrap()
    ));
}

#[test]
fn object_field_values_can_reference_fields_on_other_data_nodes() {
    let source = r#"
type Stats {
  hp: int;
  speed: float;
}

type Monster {
  hp: int;
  speed: float;
}

base_stats: Stats = {
  hp: 30,
  speed: 1.5,
};

slime: Monster = {
  hp: base_stats.hp,
  speed: base_stats.speed,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn object_field_values_can_use_index_selection() {
    let source = r#"
type Drop {
  id: string;
  amount: int;
}

type Monster {
  first_drop: Drop;
  first_amount: int;
}

drops: [Drop] = [
  { id: "coin", amount: 3 },
  { id: "gel", amount: 1 },
];

slime: Monster = {
  first_drop: drops[0],
  first_amount: drops[0].amount,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn object_field_values_can_use_deep_path_selection() {
    let source = r#"
type Inner {
  value: int;
}

type Outer {
  inner: Inner;
}

type UseValue {
  copied: int;
}

outer: Outer = {
  inner: { value: 7 },
};

use_value: UseValue = {
  copied: outer.inner.value,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn object_field_values_can_reference_later_fields() {
    let source = r#"
type Range {
  min: int;
  max: int;
}

range: Range = {
  min: max,
  max: 2,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn nested_object_fields_can_reference_outer_and_inner_fields() {
    let source = r#"
type Stats {
  hp: int;
  max_hp: int;
}

type Monster {
  base_hp: int;
  stats: Stats;
}

slime: Monster = {
  base_hp: 30,
  stats: {
    hp: base_hp,
    max_hp: hp,
  },
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn object_field_cycles_are_rejected() {
    let source = r#"
type Pair {
  a: int;
  b: int;
}

pair: Pair = {
  a: b,
  b: a,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("cyclic field reference")));
}

#[test]
fn duplicate_object_fields_are_rejected() {
    let source = r#"
type Stats {
  hp: int;
}

stats: Stats = {
  hp: 1,
  hp: 2,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("duplicate field `hp`")));
}

#[test]
fn complex_nested_typed_configuration_builds() {
    let source = r#"
type Stats {
  hp: int;
  speed: float = 1.0;
}

type Drop {
  id: string;
  amount: int;
}

type Monster {
  id: string;
  stats: Stats;
  drops: [Drop];
  resists: {string: float};
}

slime: Monster = {
  id: "slime",
  stats: { hp: 30 },
  drops: [
    { id: "coin", amount: 3 },
    { id: "gel", amount: 1 },
  ],
  resists: dict{ "fire": 0.5, "ice": 1.2 },
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_ok());
}

#[test]
fn nominal_types_are_not_structurally_compatible() {
    let source = r##"
type A {
  id: string;
}

type B {
  id: string;
}

a: A = { id: "x" };
b: B = a;
"##;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `root.B`")));
}
