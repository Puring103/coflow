#![allow(clippy::expect_used)]

mod common;
use common::*;

use coflow_cft::{CftSchemaTypeRef, CompiledSchema};
use std::collections::BTreeMap;

struct EmptyContext;

impl CfdValueSemanticContext for EmptyContext {
    fn type_domain_id(&self, _type_name: &str) -> Option<CfdDomainId> {
        None
    }

    fn record_by_domain_key(&self, _domain_id: CfdDomainId, _key: &str) -> Option<CfdRecordId> {
        None
    }

    fn record_actual_type(&self, _id: CfdRecordId) -> Option<&str> {
        None
    }
}

#[test]
fn complete_validation_rejects_missing_nested_required_fields() {
    let schema = compile_schema(
        r#"
            type Child { required: int; defaulted: int = 1; }
            type Parent { child: Child; }
        "#,
    );
    let compiled = CompiledSchema::new(&schema);
    let value = CfdValue::Object(Box::new(CfdObject::new(
        "Parent",
        BTreeMap::from([(
            "child".to_string(),
            CfdValue::Object(Box::new(CfdObject::new("Child", BTreeMap::new()))),
        )]),
    )));

    let err = validate_complete_value_for_schema(
        &compiled,
        &EmptyContext,
        &CftSchemaTypeRef::Named("Parent".to_string()),
        &value,
        None,
    )
    .expect_err("complete object must contain required nested fields");

    assert_eq!(
        err.message(),
        "missing required field `required` on object type `Child`"
    );
}

#[test]
fn fragment_validation_allows_missing_fields_but_checks_provided_values() {
    let schema = compile_schema("type Child { required: int; }");
    let compiled = CompiledSchema::new(&schema);
    let expected = CftSchemaTypeRef::Named("Child".to_string());
    let empty = CfdValue::Object(Box::new(CfdObject::new("Child", BTreeMap::new())));

    validate_fragment_value_for_schema(&compiled, &EmptyContext, &expected, &empty, None)
        .expect("object fragment may omit required fields");

    let invalid = CfdValue::Object(Box::new(CfdObject::new(
        "Child",
        BTreeMap::from([("required".to_string(), CfdValue::String("bad".to_string()))]),
    )));
    let err =
        validate_fragment_value_for_schema(&compiled, &EmptyContext, &expected, &invalid, None)
            .expect_err("provided fragment fields still require the right type");
    assert_eq!(err.message(), "expected int, got string");
}

#[test]
fn complete_validation_allows_omitted_schema_defaults() {
    let schema = compile_schema("type Child { defaulted: int = 1; }");
    let compiled = CompiledSchema::new(&schema);
    let value = CfdValue::Object(Box::new(CfdObject::new("Child", BTreeMap::new())));

    validate_complete_value_for_schema(
        &compiled,
        &EmptyContext,
        &CftSchemaTypeRef::Named("Child".to_string()),
        &value,
        None,
    )
    .expect("schema defaults may be materialized by the data-model compiler");
}
