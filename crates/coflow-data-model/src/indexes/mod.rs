mod records;
mod relations;

pub(crate) use records::{build_indexes, validate_singletons};
pub(crate) use relations::{build_ref_indexes, build_spread_indexes};
