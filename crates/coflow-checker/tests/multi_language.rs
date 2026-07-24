#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;
use coflow_cft::{CheckOwner, DimensionName, FieldName, RecordKey, TypeName, VariantName};
use coflow_checker::{execute_checks, CheckLimits, CheckProjection, CheckTarget, CheckTask};
use common::*;

fn schema() -> CftSchema {
    compile_schema(
        r#"
            type Item {
                @localized
                name: string;
                check { name != "": f"empty {id}"; }
            }
        "#,
    )
}

fn model(schema: &CftSchema) -> CfdDataModel {
    let mut builder = CfdDataModel::builder(schema);
    builder.add_record("item", "Item", [("name", LoadedValueDraft::from("base"))]);
    builder.add_dimension_value_draft(DimensionValueDraft {
        source_type: TypeName::new("Item").unwrap(),
        source_key: RecordKey::new("item").unwrap(),
        field: FieldName::new("name").unwrap(),
        dimension: DimensionName::new("language").unwrap(),
        variant: VariantName::new("zh").unwrap(),
        value: LoadedValueDraft::from(""),
        origin: RecordOrigin::None,
    });
    builder.build().expect("model")
}

#[test]
fn dimension_projection_reads_the_requested_variant_and_attaches_context() {
    let schema = schema();
    let model = model(&schema);
    let statement =
        schema.check_statements_for_owner(&CheckOwner::Type(TypeName::new("Item").unwrap()))[0];
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement,
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Dimension {
                dimension: DimensionName::new("language").unwrap(),
                variant: VariantName::new("zh").unwrap(),
            },
        }],
        CheckLimits::default(),
    );

    assert_eq!(output.results[0].diagnostics.len(), 1);
    assert_eq!(
        output.results[0].diagnostics[0].diagnostic.message,
        "empty item"
    );
    assert!(matches!(
        output.results[0].diagnostics[0].contexts.first(),
        Some(coflow_checker::CheckDiagnosticContext::Dimension { dimension, variant })
            if dimension == "language" && variant == "zh"
    ));
}

#[test]
fn base_projection_does_not_read_dimension_overlay() {
    let schema = schema();
    let model = model(&schema);
    let statement =
        schema.check_statements_for_owner(&CheckOwner::Type(TypeName::new("Item").unwrap()))[0];
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement,
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Base,
        }],
        CheckLimits::default(),
    );
    assert!(output.is_success());
}

#[test]
fn unrelated_dimension_projection_is_rejected() {
    let schema = schema();
    let model = model(&schema);
    let statement =
        schema.check_statements_for_owner(&CheckOwner::Type(TypeName::new("Item").unwrap()))[0];
    let output = execute_checks(
        &schema,
        &model,
        [CheckTask {
            statement,
            target: CheckTarget::Record(record_id_at(&model, 0)),
            projection: CheckProjection::Dimension {
                dimension: DimensionName::new("platform").unwrap(),
                variant: VariantName::new("pc").unwrap(),
            },
        }],
        CheckLimits::default(),
    );
    assert!(output.results[0].diagnostics[0]
        .diagnostic
        .message
        .contains("does not match"));
}
