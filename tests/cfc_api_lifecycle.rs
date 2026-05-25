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
