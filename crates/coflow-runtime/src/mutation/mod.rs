use coflow_api::{Diagnostic, DiagnosticSet, Severity};
use coflow_cft::{CftFieldMeta, CftSchemaTypeRef, CompiledSchema};
use coflow_data_model::CfdEnumValue;

use crate::ProjectSession;

mod apply;
mod coercion;
mod defaults;
mod plan;
mod prepare;
mod types;

pub(crate) use types::PreparedMutationOp;
pub use types::{
    CreateFieldSource, CreateRecordDraft, CreateRecordFieldDraft, CreateRequiredInput,
    DefaultMaterialization, MutationAppliedOp, MutationFailedOp, MutationFields, MutationOp,
    MutationReport, MutationRequest, MutationValue,
};

pub(super) fn schema_field<'a>(
    schema: &'a CompiledSchema,
    actual_type: &str,
    field_name: &str,
) -> Result<&'a CftFieldMeta, DiagnosticSet> {
    if !schema.has_type(actual_type) {
        return Err(one_mutation_error(
            "MUTATION-TYPE",
            format!("unknown type `{actual_type}`"),
        ));
    }
    schema.field_meta(actual_type, field_name).ok_or_else(|| {
        one_path_error(format!(
            "unknown field `{field_name}` on type `{actual_type}`"
        ))
    })
}

pub(super) fn enum_value(
    session: &ProjectSession,
    enum_name: &str,
    raw_variant: &str,
) -> Result<CfdEnumValue, DiagnosticSet> {
    let variant = raw_variant
        .strip_prefix(enum_name)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(raw_variant);
    let schema = session.compiled_schema();
    let int_value = schema
        .enum_variant_value(enum_name, variant)
        .ok_or_else(|| one_value_error(format!("unknown enum variant `{enum_name}.{variant}`")))?;
    Ok(CfdEnumValue {
        enum_name: enum_name.to_string(),
        variant: Some(variant.to_string()),
        value: int_value,
    })
}

pub(super) fn is_schema_enum(session: &ProjectSession, name: &str) -> bool {
    session.compiled_schema().is_schema_enum(name)
}

fn non_nullable(ty: &CftSchemaTypeRef) -> &CftSchemaTypeRef {
    match ty {
        CftSchemaTypeRef::Nullable(inner) => non_nullable(inner),
        other => other,
    }
}

fn one_path_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-PATH", message)
}

pub(super) fn one_value_error(message: impl Into<String>) -> DiagnosticSet {
    one_mutation_error("MUTATION-VALUE", message)
}

fn one_mutation_error(code: &'static str, message: impl Into<String>) -> DiagnosticSet {
    DiagnosticSet::one(Diagnostic {
        code: code.to_string(),
        stage: "MUTATION".to_string(),
        severity: Severity::Error,
        message: message.into(),
        primary: None,
        related: Vec::new(),
    })
}
