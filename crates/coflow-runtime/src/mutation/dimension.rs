use std::collections::BTreeMap;

use coflow_api::DiagnosticSet;
use coflow_cft::{CftField, CftFieldDimension, CftSchemaTypeRef, VariantName};
use coflow_data_model::{CfdPath, CfdPathSegment, CfdRecordId, CfdValue, DimensionValueLookup};

use crate::write_rules;
use crate::{ProjectSession, RecordCoordinate};

use super::coercion::coerce_mutation_value;
use super::prepare::set_nested_value;
use super::types::{
    DimensionSourceCoordinate, DimensionValueCoordinate, DimensionValueExpectation,
    PreparedMutationOp,
};
use super::{one_mutation_error, one_path_error, schema_field, MutationValue};

struct DimensionMutationTarget<'schema> {
    record: RecordCoordinate,
    record_id: CfdRecordId,
    field: &'schema CftField,
    binding: &'schema CftFieldDimension,
    variant: VariantName,
}

pub(super) fn prepare_dimension_value(
    session: &ProjectSession,
    coordinate: DimensionValueCoordinate,
    expectation: DimensionValueExpectation,
    value: Option<MutationValue>,
    pending_records: &BTreeMap<RecordCoordinate, usize>,
) -> Result<PreparedMutationOp, DiagnosticSet> {
    let DimensionValueCoordinate {
        actual_type,
        record_key,
        field,
        dimension,
        variant,
        path,
    } = coordinate;
    let target = resolve_dimension_target(
        session,
        actual_type.as_str(),
        record_key.as_str(),
        field.as_str(),
        dimension.as_str(),
        variant.as_str(),
    )?;
    if value.is_none() && !path.is_empty() {
        return Err(one_path_error(
            "only a complete dimension variant value can be cleared",
        ));
    }
    let expected_type = dimension_path_type(session, actual_type.as_str(), field.as_str(), &path)?;
    let current_root = current_dimension_root(session, &target)?;
    validate_dimension_expectation(
        session,
        &expected_type,
        expectation,
        current_root
            .as_ref()
            .and_then(|root| value_at_nested_path(root, &path)),
        pending_records,
    )?;
    let new_value = build_dimension_value(
        session,
        &expected_type,
        value,
        current_root,
        &path,
        pending_records,
    )?;

    let entry = session
        .source_data
        .dimension_source(
            target.field.declaring_type.as_str(),
            target.field.name.as_str(),
            target.binding.dimension.as_str(),
        )
        .ok_or_else(|| {
            one_mutation_error(
                "MUTATION-DIMENSION",
                format!(
                    "dimension field `{}.{}` has no managed source",
                    target.field.declaring_type, target.field.name
                ),
            )
        })?;
    Ok(PreparedMutationOp::WriteDimensionValue {
        record: target.record,
        coordinate: DimensionSourceCoordinate {
            source_type: target.field.declaring_type.clone(),
            source_key: record_key,
            field: target.field.name.clone(),
            dimension: target.binding.dimension.clone(),
            variant: target.variant,
            path: CfdPath { segments: path },
        },
        new_value,
        write_file: entry.display_path.clone(),
    })
}

fn resolve_dimension_target<'schema>(
    session: &'schema ProjectSession,
    actual_type: &str,
    record_key: &str,
    field: &str,
    dimension: &str,
    variant: &str,
) -> Result<DimensionMutationTarget<'schema>, DiagnosticSet> {
    let record = RecordCoordinate::new(actual_type, record_key);
    let record_id = session
        .records
        .id_for_coordinate(&record.actual_type, &record.key)
        .ok_or_else(|| {
            one_mutation_error(
                "MUTATION-DIMENSION",
                format!(
                    "record `{}.{}` was not found",
                    record.actual_type, record.key
                ),
            )
        })?;
    let schema_field = schema_field(session.schema(), actual_type, field)?;
    let binding = schema_field.dimension.as_ref().ok_or_else(|| {
        one_mutation_error(
            "MUTATION-DIMENSION",
            format!("field `{}.{field}` is not dimensional", record.actual_type),
        )
    })?;
    if binding.dimension.as_str() != dimension {
        return Err(one_mutation_error(
            "MUTATION-DIMENSION",
            format!(
                "field `{}.{field}` belongs to dimension `{}`, not `{dimension}`",
                record.actual_type, binding.dimension
            ),
        ));
    }
    let schema_dimension = session
        .schema()
        .resolve_dimension(dimension)
        .ok_or_else(|| {
            one_mutation_error(
                "MUTATION-DIMENSION",
                format!("unknown dimension `{dimension}`"),
            )
        })?;
    let schema_variant = schema_dimension.variant(variant).ok_or_else(|| {
        one_mutation_error(
            "MUTATION-DIMENSION",
            format!("unknown variant `{dimension}.{variant}`"),
        )
    })?;
    Ok(DimensionMutationTarget {
        record,
        record_id,
        field: schema_field,
        binding,
        variant: schema_variant.clone(),
    })
}

fn dimension_path_type(
    session: &ProjectSession,
    actual_type: &str,
    field: &str,
    path: &[CfdPathSegment],
) -> Result<CftSchemaTypeRef, DiagnosticSet> {
    let mut full_path = vec![CfdPathSegment::Field(field.to_string())];
    full_path.extend(path.iter().cloned());
    let mut expected_type = write_rules::expected_type_for_cfd_path(
        session.schema(),
        actual_type,
        &full_path,
        "MUTATION-DIMENSION-PATH",
        "MUTATION",
    )?;
    if path.is_empty() && !matches!(expected_type, CftSchemaTypeRef::Nullable(_)) {
        expected_type = CftSchemaTypeRef::Nullable(Box::new(expected_type));
    }
    Ok(expected_type)
}

fn current_dimension_root(
    session: &ProjectSession,
    target: &DimensionMutationTarget<'_>,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    Ok(
        match session.model.dimension_field_value(
            session.schema(),
            target.record_id,
            target.field.name.as_str(),
            target.binding.dimension.as_str(),
            target.variant.as_str(),
        ) {
            Ok(DimensionValueLookup::Value { value, .. }) => Some(value.clone()),
            Ok(DimensionValueLookup::ExplicitNull { .. }) => Some(CfdValue::Null),
            Ok(DimensionValueLookup::Missing) => None,
            Err(_) => {
                return Err(one_mutation_error(
                    "MUTATION-DIMENSION",
                    "dimension value coordinate does not match the schema",
                ));
            }
        },
    )
}

fn validate_dimension_expectation(
    session: &ProjectSession,
    expected_type: &CftSchemaTypeRef,
    expectation: DimensionValueExpectation,
    current_value: Option<&CfdValue>,
    pending_records: &BTreeMap<RecordCoordinate, usize>,
) -> Result<(), DiagnosticSet> {
    match expectation {
        DimensionValueExpectation::Any => {}
        DimensionValueExpectation::Missing if current_value.is_none() => {}
        DimensionValueExpectation::Missing => {
            return Err(one_mutation_error(
                "MUTATION-DIMENSION-STALE",
                "dimension value is no longer missing",
            ));
        }
        DimensionValueExpectation::Value(expected) => {
            let expected =
                coerce_mutation_value(session, expected_type, expected, pending_records)?;
            if current_value != Some(&expected) {
                return Err(one_mutation_error(
                    "MUTATION-DIMENSION-STALE",
                    "dimension value changed since it was read",
                ));
            }
        }
    }
    Ok(())
}

fn build_dimension_value(
    session: &ProjectSession,
    expected_type: &CftSchemaTypeRef,
    value: Option<MutationValue>,
    current_root: Option<CfdValue>,
    path: &[CfdPathSegment],
    pending_records: &BTreeMap<RecordCoordinate, usize>,
) -> Result<Option<CfdValue>, DiagnosticSet> {
    if let Some(value) = value {
        let value = coerce_mutation_value(session, expected_type, value, pending_records)?;
        if path.is_empty() {
            Ok(Some(value))
        } else {
            let mut root = match current_root {
                Some(CfdValue::Null) | None => {
                    return Err(one_path_error(
                        "nested dimension writes require a materialized variant value",
                    ));
                }
                Some(value) => value,
            };
            set_nested_value(&mut root, path, value)?;
            Ok(Some(root))
        }
    } else {
        Ok(None)
    }
}

fn value_at_nested_path<'a>(
    mut current: &'a CfdValue,
    path: &[CfdPathSegment],
) -> Option<&'a CfdValue> {
    for segment in path {
        current = match (current, segment) {
            (CfdValue::Object(object), CfdPathSegment::Field(field)) => object.fields.get(field)?,
            (CfdValue::Array(items), CfdPathSegment::Index(index)) => items.get(*index)?,
            (CfdValue::Dict(entries), CfdPathSegment::DictKey(key)) => entries
                .iter()
                .find(|(entry_key, _)| crate::dict_key_path_text(entry_key) == *key)
                .map(|(_, value)| value)?,
            _ => return None,
        };
    }
    Some(current)
}
