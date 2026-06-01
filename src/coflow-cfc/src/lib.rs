// Re-export shim: coflow-cfc is superseded by coflow-cft (type layer) and coflow-cfd (data layer).
// This crate exists for backwards compatibility during migration.
pub use coflow_cfd::*;
pub use coflow_cft::{
    CftContainer, CftSchemaEnum, CftSchemaEnumVariant, CftSchemaField, CftSchemaModule,
    CftSchemaType,
};

// Legacy type aliases kept for test compatibility.
pub use coflow_cfd::{
    CfdContainer as CfcContainer, CfdModuleResult as CfcModuleResult, CfdResult as CfcResult,
    CfdValue as CfcValue, CfdValueRef as CfcValueRef,
};
pub use coflow_cfd::CfdError as CfcError;
