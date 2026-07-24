#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;
use coflow_cft::{
    CheckDependency, CheckField, CheckOwner, CheckStatementInfo, DimensionName, FieldName, TypeName,
};
use common::*;
use std::collections::BTreeSet;

fn type_statement<'a>(schema: &'a CftSchema, owner: &str, index: usize) -> &'a CheckStatementInfo {
    schema
        .all_check_statements()
        .find(|info| {
            info.owner == CheckOwner::Type(TypeName::new(owner).unwrap())
                && info.root_index == index
        })
        .expect("statement")
}

fn field(owner: &str, name: &str) -> CheckDependency {
    CheckDependency::Field(CheckField {
        owner: TypeName::new(owner).unwrap(),
        field: FieldName::new(name).unwrap(),
    })
}

#[test]
fn root_statements_have_independent_field_and_message_dependencies() {
    let schema = compile_one(
        r#"
            type Item {
                price: int;
                name: string;
                enabled: bool;
                check {
                    price > 0: f"bad {name}";
                    enabled;
                }
            }
        "#,
    )
    .expect("schema");
    assert_eq!(
        type_statement(&schema, "Item", 0).dependencies,
        BTreeSet::from([field("Item", "name"), field("Item", "price")])
    );
    assert_eq!(
        type_statement(&schema, "Item", 1).dependencies,
        BTreeSet::from([field("Item", "enabled")])
    );
}

#[test]
fn conditions_quantifiers_and_short_circuit_branches_form_one_static_union() {
    let schema = compile_one(
        r#"
            type Item {
                enabled: bool;
                nums: [int];
                fallback: bool;
                check {
                    when enabled || fallback {
                        all value in nums { value > 0; }
                    }
                }
            }
        "#,
    )
    .expect("schema");
    assert_eq!(
        type_statement(&schema, "Item", 0).dependencies,
        BTreeSet::from([
            field("Item", "enabled"),
            field("Item", "fallback"),
            field("Item", "nums"),
        ])
    );
}

#[test]
fn nullable_safe_access_coalesce_and_pure_schema_values_collect_the_static_union() {
    let schema = compile_one(
        r#"
            const LIMIT = 0;
            enum Mode { Enabled }
            type Target { left: int; right: int; }
            type Item {
                target: &Target? = null;
                fallback: int;
                enabled: bool;
                check {
                    enabled && ((target?.left ?? fallback) > LIMIT || target?.right == null);
                    Mode.Enabled == Mode.Enabled;
                }
            }
        "#,
    )
    .expect("schema");
    assert_eq!(
        type_statement(&schema, "Item", 0).dependencies,
        BTreeSet::from([
            field("Item", "enabled"),
            field("Item", "fallback"),
            field("Item", "target"),
            field("Target", "left"),
            field("Target", "right"),
        ])
    );
    assert!(type_statement(&schema, "Item", 1).dependencies.is_empty());
}

#[test]
fn record_reference_chains_collect_each_cross_record_field() {
    let schema = compile_one(
        r#"
            type Guild { rank: int; }
            type Character { guild: &Guild; }
            type Item { owner: &Character; check { owner.guild.rank > 0; } }
        "#,
    )
    .expect("schema");
    assert_eq!(
        type_statement(&schema, "Item", 0).dependencies,
        BTreeSet::from([
            field("Character", "guild"),
            field("Guild", "rank"),
            field("Item", "owner"),
        ])
    );
}

#[test]
fn same_type_record_reference_dependencies_retain_cross_record_locality() {
    let schema = compile_one(
        r#"
            type Item {
                value: int;
                target: &Item? = null;
                check {
                    value > 0;
                    target == null || target.value > 0;
                }
            }
        "#,
    )
    .expect("schema");
    let direct = type_statement(&schema, "Item", 0);
    let referenced = type_statement(&schema, "Item", 1);
    let dependency = field("Item", "value");

    assert!(!schema.check_dependency_is_cross_record(direct.id, &dependency));
    assert!(schema.check_dependency_is_cross_record(referenced.id, &dependency));
}

#[test]
fn record_sets_and_binding_fields_are_indexed_for_project_checks() {
    let schema = compile_one(
        "type Item { price: int; } check Prices { all item in records(Item) { item.price > 0; } }",
    )
    .expect("schema");
    let info = schema
        .all_check_statements()
        .find(|info| matches!(info.owner, CheckOwner::Project(_)))
        .expect("project statement");
    assert_eq!(
        info.dependencies,
        BTreeSet::from([
            CheckDependency::RecordSet(TypeName::new("Item").unwrap()),
            field("Item", "price"),
        ])
    );
    assert_eq!(
        schema
            .check_statements_for_dependency(&field("Item", "price"))
            .collect::<Vec<_>>(),
        vec![info.id]
    );
}

#[test]
fn nested_fields_normalize_to_the_top_level_storage_field_and_dimensions_are_retained() {
    let schema = compile_one_with_dimensions(
        r#"
            type Stats { level: int; }
            type Item {
                stats: Stats;
                rewards: [Stats];
                stats_by_name: {string: Stats};
                @localized
                name: string;
                check {
                    stats.level > 0
                        && rewards[0].level > 0
                        && stats_by_name["main"].level > 0
                        && name != "";
                }
            }
        "#,
        valid_dimensions([("language", vec!["zh".to_string(), "en".to_string()])]),
    )
    .expect("schema");
    let info = type_statement(&schema, "Item", 0);
    assert_eq!(
        info.dependencies,
        BTreeSet::from([
            field("Item", "name"),
            field("Item", "rewards"),
            field("Item", "stats"),
            field("Item", "stats_by_name"),
        ])
    );
    assert_eq!(
        info.dimensions,
        BTreeSet::from([DimensionName::new("language").unwrap()])
    );
}

#[test]
fn inheritance_and_nested_hosts_expand_actual_type_statement_queries() {
    let schema = compile_one(
        r#"
            type Part { value: int; check { value > 0; } }
            abstract type Base { enabled: bool; check { enabled; } }
            type Item : Base { part: Part; }
        "#,
    )
    .expect("schema");
    let actual = schema
        .check_statements_for_actual_type("Item")
        .collect::<BTreeSet<_>>();
    assert!(actual.contains(&type_statement(&schema, "Base", 0).id));
    assert!(actual.contains(&type_statement(&schema, "Part", 0).id));
    assert_eq!(
        schema
            .check_hosts_for_nested_type("Part")
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([TypeName::new("Item").unwrap()])
    );
    assert_eq!(
        schema
            .check_statements_for_nested_field(
                &TypeName::new("Item").unwrap(),
                &FieldName::new("part").unwrap(),
            )
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([type_statement(&schema, "Part", 0).id])
    );
}
