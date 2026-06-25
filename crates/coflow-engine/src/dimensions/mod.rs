mod regenerate;
mod synthesize;

pub use regenerate::regenerate_dimension_sources;
pub use synthesize::{
    inject_language_dimension_types, language_dimension_fields, language_dimension_sources,
    DimensionField,
};
