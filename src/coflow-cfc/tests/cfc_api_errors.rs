#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow_cfc::{CfcContainer, CfcValue, CfcValueRef, ModuleId};

#[test]
fn duplicate_top_level_names_are_rejected_across_kinds() {
    let source = r#"
type Item {
  id: string;
}

enum Item {
  common,
}

Item = 1;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err.errors.len() >= 2);
    assert!(err
        .errors
        .iter()
        .all(|e| e.message.contains("duplicate name `Item`")));
}

#[test]
fn import_alias_conflicting_with_local_name_is_rejected() {
    let root_source = r#"
use "dep" as item;

type item {
  id: string;
}
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("dep");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(dep.clone(), "").unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &dep).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("duplicate name `item`")));
}

#[test]
fn bind_import_rejects_duplicate_aliases() {
    let root_source = r#"
use "a" as dep;
use "b" as dep;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let a = ModuleId::from("a");
    let b = ModuleId::from("b");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(a.clone(), "").unwrap();
    c.add_module(b.clone(), "").unwrap();

    let imports: Vec<_> = c.imports(&root).unwrap().iter().map(|i| i.id).collect();
    let err = c.bind_import(&root, imports[0], &a).unwrap_err();

    assert!(err.message.contains("duplicate import alias `dep`"));
}

#[test]
fn unknown_import_alias_in_type_is_reported() {
    let source = r#"
type Bad {
  item: missing.Item;
}

bad: Bad = {
  item: {},
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("unknown import alias `missing`")));
}

#[test]
fn missing_required_field_reports_object_span() {
    let source = r#"
type Stats {
  hp: int;
}

stats: Stats = {};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();
    let error = err
        .errors
        .iter()
        .find(|error| error.message.contains("missing required field `hp`"))
        .unwrap();
    let object_pos = source.find("{}").unwrap();
    let field_def_pos = source.find("hp: int").unwrap();

    assert_eq!(error.span.unwrap().start, object_pos);
    assert_ne!(error.span.unwrap().start, field_def_pos);
}

#[test]
fn ambiguous_imported_enum_variant_shortcut_is_rejected() {
    let root_source = r#"
use "dep" as dep;

value = dep.same;
"#;
    let dep_source = r#"
enum A {
  same,
}

enum B {
  same,
}
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("dep");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(dep.clone(), dep_source).unwrap();
    let import = c.imports(&root).unwrap()[0].id;
    c.bind_import(&root, import, &dep).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("ambiguous enum variant `same`")));
}

#[test]
fn imported_type_refs_reject_multi_level_paths_at_parse_time() {
    let source = r#"
type Bad {
  value: dep.other.Item;
}
"#;

    let mut c = CfcContainer::new();
    let err = c.add_module(ModuleId::from("root"), source).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `;`")));
}

#[test]
fn build_paths_report_array_and_dict_index_bounds() {
    let source = r#"
type UseValues {
  array_value: int;
  dict_value: int;
}

values: [int] = [1];
scores: {string: int} = dict{ "a": 1 };

bad: UseValues = {
  array_value: values[2],
  dict_value: scores[2],
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("array index `2` is out of bounds")));
    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("dict index `2` is out of bounds")));
}

#[test]
fn build_paths_report_selecting_or_indexing_non_containers() {
    let source = r#"
type UseValues {
  selected: int;
  indexed: int;
}

value = 1;

bad: UseValues = {
  selected: value.missing,
  indexed: value[0],
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("cannot select field `missing`")));
    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("cannot index value at `0`")));
}

#[test]
fn untyped_empty_dict_requires_context_type() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), "items = dict{};").unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("cannot infer type of empty dict")));
}

#[test]
fn unknown_enum_variant_is_reported() {
    let source = r#"
enum Rarity {
  common,
}

rarity: Rarity = Rarity.missing;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("unknown enum variant `Rarity.missing`")));
}

#[test]
fn complex_nested_game_data_preserves_references_and_defaults() {
    let source = r#"
enum Element {
  fire,
  ice,
}

type Resist {
  element: Element;
  value: float = 1.0;
}

type Skill {
  id: string;
  tags: [string] = [];
}

type Drop {
  id: string;
  amount: int;
}

type Monster {
  id: string;
  skills: [Skill];
  drops: [Drop];
  resists: {Element: Resist};
  primary_skill: Skill;
  first_drop_amount: int;
}

shared_skill: Skill = {
  id: "burn",
};

coin: Drop = {
  id: "coin",
  amount: 3,
};

slime: Monster = {
  id: "slime",
  skills: [
    shared_skill,
    { id: "jump", tags: ["movement"] },
  ],
  drops: [
    coin,
    { id: "gel", amount: 1 },
  ],
  resists: dict{
    Element.fire: { element: Element.fire, value: 0.5 },
    Element.ice: { element: Element.ice },
  },
  primary_skill: skills[0],
  first_drop_amount: drops[0].amount,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();
    let shared_skill = module.get("shared_skill").unwrap();
    let slime = module.get("slime").unwrap();
    let borrowed = slime.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected slime object");
    };

    assert!(CfcValueRef::ptr_eq(
        fields.get("primary_skill").unwrap(),
        &shared_skill
    ));
    assert!(matches!(
        &*fields.get("first_drop_amount").unwrap().borrow(),
        CfcValue::Int(3)
    ));

    let resists = fields.get("resists").unwrap();
    let resists_borrowed = resists.borrow();
    let CfcValue::Dict(entries) = &*resists_borrowed else {
        panic!("expected resists dict");
    };
    let (_, ice) = entries
        .iter()
        .find(|(key, _)| {
            matches!(
                &*key.borrow(),
                CfcValue::Enum { variant, .. } if variant == "ice"
            )
        })
        .unwrap();
    let ice_borrowed = ice.borrow();
    let CfcValue::Object {
        fields: ice_fields, ..
    } = &*ice_borrowed
    else {
        panic!("expected ice resist object");
    };

    let value = ice_fields.get("value").unwrap();
    let borrowed = value.borrow();
    let CfcValue::Float(value) = &*borrowed else {
        panic!("expected default resist value");
    };
    assert!((*value - 1.0).abs() < f64::EPSILON);
}
