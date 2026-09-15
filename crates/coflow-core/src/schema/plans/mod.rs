#[cfg(feature = "cft-compiler")]
mod default_materialization;
mod value_dependencies;

#[cfg(feature = "cft-compiler")]
pub(crate) use default_materialization::validate_default_materialization;
pub use value_dependencies::{
    ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};
