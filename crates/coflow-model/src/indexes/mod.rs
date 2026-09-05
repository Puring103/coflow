mod records;
mod relations;

pub(crate) use records::{build_indexes, validate_singletons};
pub(crate) use relations::{build_ref_indexes, first_ref_cycle, RefIndexes};
