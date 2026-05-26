#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::needless_raw_string_hashes,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use coflow_cfc::{CfcContainer, CfcValue, CfcValueRef, ModuleId, ResolveError};

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

    assert!(matches!(err, coflow_cfc::CfcError::Parse(_)));
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
    assert!(c.bind_import(&root, coflow_cfc::ImportId(999), &dep).is_err());
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
