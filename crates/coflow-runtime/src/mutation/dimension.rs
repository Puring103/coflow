use std::collections::BTreeMap;

use coflow_api::DiagnosticSet;
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdPath, CfdPathSegment, CfdValue, DimensionValueLookup};

use crate::write_rules;
use crate::{ProjectSession, RecordCoordinate};

use super::coercion::coerce_mutation_value;
use super::prepare::set_nested_value;
use super::types::{
    DimensionSourceCoordinate, DimensionValueCoordinate, DimensionValueExpectation,
    PreparedMutationOp,
};
use super::{one_mutation_error, one_path_error, schema_field, MutationValue};

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
    let record = RecordCoordinate::new(actual_type.as_str(), record_key.as_str());
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
    let schema_field = schema_field(session.schema(), actual_type.as_str(), field.as_str())?;
    let binding = schema_field.dimension.as_ref().ok_or_else(|| {
        one_mutation_error(
            "MUTATION-DIMENSION",
            format!("field `{}.{field}` is not dimensional", record.actual_type),
        )
    })?;
    if binding.dimension != dimension {
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
        .resolve_dimension(&dimension)
        .ok_or_else(|| {
            one_mutation_error(
                "MUTATION-DIMENSION",
                format!("unknown dimension `{dimension}`"),
            )
        })?;
    let schema_variant = schema_dimension.variant(&variant).ok_or_else(|| {
        one_mutation_error(
            "MUTATION-DIMENSION",
            format!("unknown variant `{dimension}.{variant}`"),
        )
    })?;
    if value.is_none() && !path.is_empty() {
        return Err(one_path_error(
            "only a complete dimension variant value can be cleared",
        ));
    }

    let mut full_path = vec![CfdPathSegment::Field(field.to_string())];
    full_path.extend(path.iter().cloned());
    let mut expected_type = write_rules::expected_type_for_cfd_path(
        session.schema(),
        actual_type.as_str(),
        &full_path,
        "MUTATION-DIMENSION-PATH",
        "MUTATION",
    )?;
    if path.is_empty() && !matches!(expected_type, CftSchemaTypeRef::Nullable(_)) {
        expected_type = CftSchemaTypeRef::Nullable(Box::new(expected_type));
    }
    let current_root = match session.model.dimension_field_value(
        session.schema(),
        record_id,
        field.as_str(),
        dimension.as_str(),
        variant.as_str(),
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
    };
    let current_value = current_root
        .as_ref()
        .and_then(|root| value_at_nested_path(root, &path));
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
                coerce_mutation_value(session, &expected_type, expected, pending_records)?;
            if current_value != Some(&expected) {
                return Err(one_mutation_error(
                    "MUTATION-DIMENSION-STALE",
                    "dimension value changed since it was read",
                ));
            }
        }
    }

    let new_value = if let Some(value) = value {
        let value = coerce_mutation_value(session, &expected_type, value, pending_records)?;
        if path.is_empty() {
            Some(value)
        } else {
            let mut root = match current_root {
                Some(CfdValue::Null) | None => {
                    return Err(one_path_error(
                        "nested dimension writes require a materialized variant value",
                    ));
                }
                Some(value) => value,
            };
            set_nested_value(&mut root, &path, value)?;
            Some(root)
        }
    } else {
        None
    };

    let entry = session
        .source_data
        .dimension_source(
            schema_field.declaring_type.as_str(),
            schema_field.name.as_str(),
            binding.dimension.as_str(),
        )
        .ok_or_else(|| {
            one_mutation_error(
                "MUTATION-DIMENSION",
                format!(
                    "dimension field `{}.{}` has no managed source",
                    schema_field.declaring_type, schema_field.name
                ),
            )
        })?;
    Ok(PreparedMutationOp::WriteDimensionValue {
        record: record.clone(),
        coordinate: DimensionSourceCoordinate {
            source_type: schema_field.declaring_type.clone(),
            source_key: record_key,
            field: schema_field.name.clone(),
            dimension: binding.dimension.clone(),
            variant: schema_variant.clone(),
            path: CfdPath { segments: path },
        },
        new_value,
        write_file: entry.display_path.clone(),
    })
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
