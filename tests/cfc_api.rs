#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow::{CfcContainer, CfcValue, CfcValueRef, ModuleId, ResolveError};

#[test]
fn load_graph_builds_root_closure() {
    let root = r#"
use "common.cfc" as common;

type Monster {
  id: string;
  stats: common.Stats;
  rarity: common.Rarity;
}

slime: Monster = {
  id: "slime",
  stats: common.base_stats,
  rarity: common.Rarity.common,
};
"#;
    let common = r#"
type Stats {
  hp: int;
  speed: float = 1.0;
}

enum Rarity {
  common,
  rare = 10,
}

base_stats: Stats = {
  hp: 30,
};
"#;

    let mut c = CfcContainer::new();
    let result = c
        .load_graph(ModuleId::from("root"), root, |_, import| {
            if import.path == "common.cfc" {
                Ok((ModuleId::from("common"), common.to_string()))
            } else {
                Err(ResolveError::new("missing fixture"))
            }
        })
        .unwrap();

    assert!(result.root().unwrap().get("slime").is_some());
    assert!(result.module(&ModuleId::from("common")).is_some());
}

#[test]
fn low_level_api_preserves_named_identity() {
    let source = r#"
shared = {
  hp: 100,
};

slime = {
  stats: shared,
};

goblin = {
  stats: shared,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    assert_eq!(c.source(&root).unwrap(), source);
    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();

    let slime = module.get("slime").unwrap();
    let goblin = module.get("goblin").unwrap();
    let CfcValue::Object {
        fields: slime_fields,
        ..
    } = &*slime.borrow()
    else {
        panic!("expected object");
    };
    let CfcValue::Object {
        fields: goblin_fields,
        ..
    } = &*goblin.borrow()
    else {
        panic!("expected object");
    };

    assert!(CfcValueRef::ptr_eq(
        slime_fields.get("stats").unwrap(),
        goblin_fields.get("stats").unwrap()
    ));
}

#[test]
fn named_nodes_can_form_cycles() {
    let source = r#"
node_a = {
  value: 1,
  next: node_b,
};

node_b = {
  value: 2,
  next: node_a,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();

    let node_a = module.get("node_a").unwrap();
    let node_b = module.get("node_b").unwrap();

    {
        let borrowed = node_a.borrow();
        let CfcValue::Object {
            fields: a_fields, ..
        } = &*borrowed
        else {
            panic!("expected object");
        };
        assert!(CfcValueRef::ptr_eq(a_fields.get("next").unwrap(), &node_b));
    }

    {
        let borrowed = node_b.borrow();
        let CfcValue::Object {
            fields: b_fields, ..
        } = &*borrowed
        else {
            panic!("expected object");
        };
        assert!(CfcValueRef::ptr_eq(b_fields.get("next").unwrap(), &node_a));
    }
}

#[test]
fn typed_named_nodes_can_form_cycles() {
    let source = r#"
type Node {
  value: int;
  next: Node;
}

node_a: Node = {
  value: 1,
  next: node_b,
};

node_b: Node = {
  value: 2,
  next: node_a,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();

    let node_a = module.get("node_a").unwrap();
    let node_b = module.get("node_b").unwrap();

    {
        let borrowed = node_a.borrow();
        let CfcValue::Object {
            fields: a_fields, ..
        } = &*borrowed
        else {
            panic!("expected node_a object");
        };
        assert!(CfcValueRef::ptr_eq(a_fields.get("next").unwrap(), &node_b));
    }

    {
        let borrowed = node_b.borrow();
        let CfcValue::Object {
            fields: b_fields, ..
        } = &*borrowed
        else {
            panic!("expected node_b object");
        };
        assert!(CfcValueRef::ptr_eq(b_fields.get("next").unwrap(), &node_a));
    }
}

#[test]
fn bind_import_rejects_two_aliases_for_same_dependency() {
    let root_source = r#"
use "./item.cfc" as item;
use "item.cfc" as item2;
"#;
    let dep_source = "";

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("item");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(dep.clone(), dep_source).unwrap();

    let imports: Vec<_> = c.imports(&root).unwrap().iter().map(|i| i.id).collect();
    c.bind_import(&root, imports[0], &dep).unwrap();
    let err = c.bind_import(&root, imports[1], &dep).unwrap_err();

    assert!(err.message.contains("more than once"));
}

#[test]
fn replace_module_is_atomic_on_parse_error() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), "value = 1;").unwrap();

    assert!(c.replace_module(root.clone(), "value = ;").is_err());
    let result = c.build(&root).unwrap();
    let value = result.root().unwrap().get("value").unwrap();

    assert!(matches!(&*value.borrow(), CfcValue::Int(1)));
}

#[test]
fn build_reports_structure_errors() {
    let source = r#"
type Stats {
  hp: int;
  speed: float;
}

stats: Stats = {
  hp: "bad",
  extra: 1,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();
    let err = c.build(&root).unwrap_err();

    assert!(err.errors.len() >= 3);
}

#[test]
fn failed_nodes_do_not_produce_result_values() {
    let source = r#"
type Stats {
  hp: int;
}

stats: Stats = {
  hp: missing_value,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    assert!(c.build(&root).is_err());
}

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

#[test]
fn object_array_and_dict_defaults_are_not_shared_between_instances() {
    let source = r#"
type Defaults {
  object: any = {};
  array: [string] = [];
  mapping: {string: int} = dict{};
}

a: Defaults = {};
b: Defaults = {};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();
    let a = module.get("a").unwrap();
    let b = module.get("b").unwrap();
    let a_borrowed = a.borrow();
    let b_borrowed = b.borrow();
    let CfcValue::Object {
        fields: a_fields, ..
    } = &*a_borrowed
    else {
        panic!("expected a object");
    };
    let CfcValue::Object {
        fields: b_fields, ..
    } = &*b_borrowed
    else {
        panic!("expected b object");
    };

    assert!(!CfcValueRef::ptr_eq(
        a_fields.get("object").unwrap(),
        b_fields.get("object").unwrap()
    ));
    assert!(!CfcValueRef::ptr_eq(
        a_fields.get("array").unwrap(),
        b_fields.get("array").unwrap()
    ));
    assert!(!CfcValueRef::ptr_eq(
        a_fields.get("mapping").unwrap(),
        b_fields.get("mapping").unwrap()
    ));
}

#[test]
fn enum_auto_numbering_continues_after_explicit_value() {
    let source = r#"
enum Status {
  none = 0,
  active = 10,
  dead = 20,
  ghost,
}

ghost: Status = Status.ghost;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let ghost = result.root().unwrap().get("ghost").unwrap();

    assert!(matches!(
        &*ghost.borrow(),
        CfcValue::Enum { variant, value, .. } if variant == "ghost" && *value == 21
    ));
}

#[test]
fn duplicate_enum_values_are_rejected() {
    let source = r#"
enum Bad {
  a = 1,
  b = 1,
}

value: Bad = Bad.a;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("duplicate enum value `1`")));
}

#[test]
fn duplicate_enum_variants_are_rejected() {
    let source = r#"
enum Bad {
  a,
  a,
}

value: Bad = Bad.a;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let err = c.build(&root).unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("duplicate enum variant `a`")));
}

#[test]
fn use_cycles_are_allowed_when_imports_are_bound() {
    let a_source = r#"
use "b" as b;

value = b.value;
"#;
    let b_source = r#"
use "a" as a;

value = 2;
back = a.value;
"#;

    let mut c = CfcContainer::new();
    let a = ModuleId::from("a");
    let b = ModuleId::from("b");
    c.add_module(a.clone(), a_source).unwrap();
    c.add_module(b.clone(), b_source).unwrap();
    let a_import = c.imports(&a).unwrap()[0].id;
    let b_import = c.imports(&b).unwrap()[0].id;
    c.bind_import(&a, a_import, &b).unwrap();
    c.bind_import(&b, b_import, &a).unwrap();

    let result = c.build(&a).unwrap();

    assert!(result.module(&a).unwrap().get("value").is_some());
    assert!(result.module(&b).unwrap().get("back").is_some());
}

#[test]
fn build_all_has_no_root_and_includes_disconnected_modules() {
    let mut c = CfcContainer::new();
    let a = ModuleId::from("a");
    let b = ModuleId::from("b");
    c.add_module(a.clone(), "a = 1;").unwrap();
    c.add_module(b.clone(), "b = 2;").unwrap();

    let result = c.build_all().unwrap();

    assert!(result.root_id().is_none());
    assert!(result.root().is_none());
    assert!(result.module(&a).unwrap().get("a").is_some());
    assert!(result.module(&b).unwrap().get("b").is_some());
}

#[test]
fn build_all_reports_unbound_imports() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root, "use \"dep\" as dep;").unwrap();

    let err = c.build_all().unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("unbound import `dep`")));
}

#[test]
fn typed_array_and_dict_fields_can_reference_named_values() {
    let source = r#"
type Bag {
  names: [string];
  scores: {string: int};
}

shared_names: [string] = ["slime"];
shared_scores: {string: int} = dict{ "slime": 1 };

bag: Bag = {
  names: shared_names,
  scores: shared_scores,
};
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), source).unwrap();

    let result = c.build(&root).unwrap();
    let module = result.root().unwrap();
    let bag = module.get("bag").unwrap();
    let names = module.get("shared_names").unwrap();
    let scores = module.get("shared_scores").unwrap();
    let borrowed = bag.borrow();
    let CfcValue::Object { fields, .. } = &*borrowed else {
        panic!("expected bag object");
    };

    assert!(CfcValueRef::ptr_eq(fields.get("names").unwrap(), &names));
    assert!(CfcValueRef::ptr_eq(fields.get("scores").unwrap(), &scores));
}

#[test]
fn build_root_result_only_contains_import_closure() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let other = ModuleId::from("other");
    c.add_module(root.clone(), "root = 1;").unwrap();
    c.add_module(other.clone(), "other = 2;").unwrap();

    let result = c.build(&root).unwrap();

    assert!(result.module(&root).is_some());
    assert!(result.module(&other).is_none());
}

#[test]
fn replace_module_clears_outgoing_import_bindings_but_preserves_incoming_bindings() {
    let root_source = r#"
use "dep" as dep;

value = dep.value;
"#;
    let other_source = r#"
use "root" as root;

value = root.value;
"#;

    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("dep");
    let other = ModuleId::from("other");
    c.add_module(root.clone(), root_source).unwrap();
    c.add_module(dep.clone(), "value = 1;").unwrap();
    c.add_module(other.clone(), other_source).unwrap();
    let root_import = c.imports(&root).unwrap()[0].id;
    let other_import = c.imports(&other).unwrap()[0].id;
    c.bind_import(&root, root_import, &dep).unwrap();
    c.bind_import(&other, other_import, &root).unwrap();

    c.replace_module(root.clone(), "value = 2;").unwrap();

    assert!(c.imports(&root).unwrap().is_empty());
    assert!(c.build(&other).is_ok());
}

#[test]
fn load_graph_rejects_existing_root_module() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    c.add_module(root.clone(), "value = 1;").unwrap();

    let err = c
        .load_graph(root, "value = 2;", |_, _| {
            Err(ResolveError::new("should not resolve"))
        })
        .unwrap_err();

    assert!(matches!(err, coflow::CfcError::Parse(_)));
}

#[test]
fn low_level_api_reports_basic_phase_errors() {
    let mut c = CfcContainer::new();
    let root = ModuleId::from("root");
    let dep = ModuleId::from("dep");
    c.add_module(root.clone(), "use \"dep\" as dep;").unwrap();

    assert!(c.add_module(root.clone(), "value = 1;").is_err());
    assert!(c
        .replace_module(ModuleId::from("missing"), "value = 1;")
        .is_err());
    assert!(c.imports(&ModuleId::from("missing")).is_err());

    let import = c.imports(&root).unwrap()[0].id;
    assert!(c.bind_import(&root, import, &dep).is_err());
    c.add_module(dep.clone(), "value = 1;").unwrap();
    assert!(c
        .bind_import(&ModuleId::from("missing"), import, &dep)
        .is_err());
    assert!(c.bind_import(&root, coflow::ImportId(999), &dep).is_err());
}

#[test]
fn missing_semicolon_is_a_parse_error() {
    let mut c = CfcContainer::new();
    let err = c
        .add_module(ModuleId::from("root"), "value = { id: \"slime\" }")
        .unwrap_err();

    assert!(err
        .errors
        .iter()
        .any(|e| e.message.contains("expected `;`")));
}

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
