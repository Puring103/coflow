#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;
use common::*;

use coflow_cft::{CftValueType, TypeName};
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

struct ModelContext<'a>(&'a CfdDataModel);

impl CfdValueSemanticContext for ModelContext<'_> {
    fn type_domain_id(&self, type_name: &str) -> Option<CfdDomainId> {
        self.0.type_domain_id(type_name)
    }

    fn record_by_domain_key(&self, domain_id: CfdDomainId, key: &str) -> Option<CfdRecordId> {
        self.0.record_by_domain_key(domain_id, key)
    }

    fn record_actual_type(&self, id: CfdRecordId) -> Option<&str> {
        self.0.record(id).map(CfdRecord::actual_type)
    }
}

#[test]
fn complete_validation_rejects_missing_nested_required_fields() {
    let schema = compile_schema(
        r"
            type Child { required: int; defaulted: int = 1; }
            type Parent { child: Child; }
        ",
    );
    let compiled = &schema;
    let value = CfdValue::Object(Box::new(
        CfdObject::try_new(
            "Parent",
            BTreeMap::from([(
                "child".to_string(),
                CfdValue::Object(Box::new(
                    CfdObject::try_new("Child", BTreeMap::new()).unwrap(),
                )),
            )]),
        )
        .unwrap(),
    ));

    let err = validate_value_for_schema(
        compiled,
        &EmptyContext,
        ValueValidationRequest::new(
            &CftValueType::Object(TypeName::new("Parent").unwrap()),
            &value,
            ValueValidationMode::Complete,
        ),
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
    let compiled = &schema;
    let expected = CftValueType::Object(TypeName::new("Child").unwrap());
    let empty = CfdValue::Object(Box::new(
        CfdObject::try_new("Child", BTreeMap::new()).unwrap(),
    ));

    validate_value_for_schema(
        compiled,
        &EmptyContext,
        ValueValidationRequest::new(&expected, &empty, ValueValidationMode::SourceFragment),
    )
    .expect("object fragment may omit required fields");

    let invalid = CfdValue::Object(Box::new(
        CfdObject::try_new(
            "Child",
            BTreeMap::from([("required".to_string(), CfdValue::String("bad".to_string()))]),
        )
        .unwrap(),
    ));
    let err = validate_value_for_schema(
        compiled,
        &EmptyContext,
        ValueValidationRequest::new(&expected, &invalid, ValueValidationMode::SourceFragment),
    )
    .expect_err("provided fragment fields still require the right type");
    assert_eq!(err.message(), "expected int, got string");
}

#[test]
fn complete_validation_allows_omitted_schema_defaults() {
    let schema = compile_schema("type Child { defaulted: int = 1; }");
    let compiled = &schema;
    let value = CfdValue::Object(Box::new(
        CfdObject::try_new("Child", BTreeMap::new()).unwrap(),
    ));

    validate_value_for_schema(
        compiled,
        &EmptyContext,
        ValueValidationRequest::new(
            &CftValueType::Object(TypeName::new("Child").unwrap()),
            &value,
            ValueValidationMode::Complete,
        ),
    )
    .expect("schema defaults may be materialized by the data-model compiler");
}

#[test]
fn source_build_and_mutation_validation_share_semantic_rule_matrix() {
    struct Case {
        name: &'static str,
        schema: &'static str,
        source_value: LoadedValueDraft,
        mutation_value: CfdValue,
        context_records: Vec<LoadedRecordDraft>,
        valid: bool,
    }

    let object = |actual_type: &str, fields: BTreeMap<String, CfdValue>| {
        CfdValue::Object(Box::new(CfdObject::try_new(actual_type, fields).unwrap()))
    };
    let input_record = |key: &str, actual_type: &str, fields: Vec<(&str, LoadedValueDraft)>| {
        LoadedRecordDraft::new(key, actual_type, fields)
    };

    let cases = vec![
        Case {
            name: "nullable null",
            schema: "type Root { value: int?; }",
            source_value: LoadedValueDraft::Null,
            mutation_value: CfdValue::Null,
            context_records: Vec::new(),
            valid: true,
        },
        Case {
            name: "primitive mismatch",
            schema: "type Root { value: int; }",
            source_value: LoadedValueDraft::String("bad".to_string()),
            mutation_value: CfdValue::String("bad".to_string()),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "non-finite float",
            schema: "type Root { value: float; }",
            source_value: LoadedValueDraft::Float(f64::NAN),
            mutation_value: CfdValue::Float(f64::NAN),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "array item mismatch",
            schema: "type Root { value: [int]; }",
            source_value: LoadedValueDraft::Array(vec![LoadedValueDraft::String("bad".to_string())]),
            mutation_value: CfdValue::Array(vec![CfdValue::String("bad".to_string())]),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "dict key mismatch",
            schema: "type Root { value: {int: string}; }",
            source_value: LoadedValueDraft::dict([(
                LoadedDictKeyDraft::String("bad".to_string()),
                LoadedValueDraft::String("value".to_string()),
            )]),
            mutation_value: CfdValue::Dict(vec![(
                CfdDictKey::String("bad".to_string()),
                CfdValue::String("value".to_string()),
            )]),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "unknown enum variant",
            schema: "enum Rarity { Common } type Root { value: Rarity; }",
            source_value: LoadedValueDraft::enum_variant("Rarity", "Missing"),
            mutation_value: CfdValue::Enum(
                CfdEnumValue::try_new("Rarity", Some("Missing"), 0).unwrap(),
            ),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "missing nested required field",
            schema: "type Child { required: int; } type Root { value: Child; }",
            source_value: LoadedValueDraft::object(
                "Child",
                std::iter::empty::<(&str, LoadedValueDraft)>(),
            ),
            mutation_value: object("Child", BTreeMap::new()),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "abstract object instantiation",
            schema: "abstract type Base { n: int; } type Root { value: Base; }",
            source_value: LoadedValueDraft::object("Base", [("n", LoadedValueDraft::Int(1))]),
            mutation_value: object(
                "Base",
                BTreeMap::from([("n".to_string(), CfdValue::Int(1))]),
            ),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "valid concrete object",
            schema: "abstract type Base {} type Child : Base { n: int; } type Root { value: Base; }",
            source_value: LoadedValueDraft::object("Child", [("n", LoadedValueDraft::Int(1))]),
            mutation_value: object(
                "Child",
                BTreeMap::from([("n".to_string(), CfdValue::Int(1))]),
            ),
            context_records: Vec::new(),
            valid: true,
        },
        Case {
            name: "valid record ref",
            schema: "type Target { name: string; } type Root { value: &Target; }",
            source_value: LoadedValueDraft::record_ref("target"),
            mutation_value: CfdValue::record_ref("target").unwrap(),
            context_records: vec![input_record(
                "target",
                "Target",
                vec![("name", LoadedValueDraft::String("Target".to_string()))],
            )],
            valid: true,
        },
        Case {
            name: "missing record ref",
            schema: "type Target {} type Root { value: &Target; }",
            source_value: LoadedValueDraft::record_ref("missing"),
            mutation_value: CfdValue::record_ref("missing").unwrap(),
            context_records: Vec::new(),
            valid: false,
        },
        Case {
            name: "record ref actual type mismatch",
            schema: "abstract type Reward {} type ItemReward : Reward {} type CurrencyReward : Reward {} type Root { value: &ItemReward; }",
            source_value: LoadedValueDraft::record_ref("reward"),
            mutation_value: CfdValue::record_ref("reward").unwrap(),
            context_records: vec![input_record("reward", "CurrencyReward", Vec::new())],
            valid: false,
        },
    ];

    for case in cases {
        let schema = compile_schema(case.schema);

        let mut source_builder = CfdDataModel::builder(&schema);
        for record in case.context_records.iter().cloned() {
            source_builder.add_loaded_record(record);
        }
        source_builder.add_record("root", "Root", [("value", case.source_value)]);
        let source_valid = source_builder.build().is_ok();

        let mut context_builder = CfdDataModel::builder(&schema);
        for record in case.context_records {
            context_builder.add_loaded_record(record);
        }
        let context_model = context_builder.build().expect("valid context records");
        let expected = &schema
            .field("Root", "value")
            .expect("value field")
            .value_type;
        let mutation_valid = validate_value_for_schema(
            &schema,
            &ModelContext(&context_model),
            ValueValidationRequest::new(
                expected,
                &case.mutation_value,
                ValueValidationMode::Mutation,
            ),
        )
        .is_ok();

        assert_eq!(source_valid, case.valid, "source build case: {}", case.name);
        assert_eq!(
            mutation_valid, case.valid,
            "mutation validation case: {}",
            case.name
        );
        assert_eq!(
            source_valid, mutation_valid,
            "conformance case: {}",
            case.name
        );
    }
}
