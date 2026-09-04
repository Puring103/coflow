//! Internal adapters used by the runtime check benchmarks.

use std::collections::{BTreeMap, BTreeSet};

use coflow_checker::CheckTask;
use crate::data_model::CfdDataModel;
use coflow_language::{DimensionName, FieldName, RecordKey, TypeName, VariantName};

use crate::checks::impact::{ChangedField, ChangedProjection, ChangedRecordFields, CheckImpact};
use crate::checks::{plan_full_checks, plan_full_checks_bounded, plan_incremental_checks};
use crate::RecordCoordinate;

#[derive(Debug, Clone)]
pub struct BenchmarkFieldChange {
    pub actual_type: String,
    pub key: String,
    pub field: String,
    pub projection: BenchmarkProjection,
}

#[derive(Debug, Clone)]
pub enum BenchmarkProjection {
    Base,
    Dimension { dimension: String, variant: String },
}

pub fn plan_full(schema: &coflow_language::CftSchema, model: &CfdDataModel) -> Vec<CheckTask> {
    plan_full_checks(schema, model)
}

/// Plans a full check through the production bounded planner.
///
/// # Errors
///
/// Returns the configured task limit when planning exceeds it.
pub fn plan_full_with_limit(
    schema: &coflow_language::CftSchema,
    model: &CfdDataModel,
    max_tasks: usize,
) -> Result<Vec<CheckTask>, usize> {
    plan_full_checks_bounded(schema, model, max_tasks)
}

/// Plans field and record-set changes through the production runtime planner.
///
/// # Errors
///
/// Returns a validation message when benchmark input contains an invalid name.
pub fn plan_incremental(
    schema: &coflow_language::CftSchema,
    model: &CfdDataModel,
    fields: impl IntoIterator<Item = BenchmarkFieldChange>,
    record_sets: impl IntoIterator<Item = String>,
) -> Result<Vec<CheckTask>, String> {
    let mut records = BTreeMap::new();
    for change in fields {
        let coordinate = RecordCoordinate::new(
            TypeName::new(&change.actual_type).map_err(|error| error.to_string())?,
            RecordKey::new(&change.key).map_err(|error| error.to_string())?,
        );
        let projection = match change.projection {
            BenchmarkProjection::Base => ChangedProjection::Base,
            BenchmarkProjection::Dimension { dimension, variant } => ChangedProjection::Dimension {
                dimension: DimensionName::new(dimension).map_err(|error| error.to_string())?,
                variant: VariantName::new(variant).map_err(|error| error.to_string())?,
            },
        };
        let changed = ChangedField {
            field: FieldName::new(change.field).map_err(|error| error.to_string())?,
            projection,
        };
        match records.entry(coordinate) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ChangedRecordFields::Fields(BTreeSet::from([changed])));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if let ChangedRecordFields::Fields(fields) = entry.get_mut() {
                    fields.insert(changed);
                }
            }
        }
    }
    let record_sets = record_sets
        .into_iter()
        .map(|name| TypeName::new(name).map_err(|error| error.to_string()))
        .collect::<Result<_, _>>()?;
    Ok(plan_incremental_checks(
        schema,
        model,
        &CheckImpact {
            records,
            record_sets,
        },
    ))
}
