use crate::build::BuildSchema;
use coflow_language::cft::{CftField, CftSchema};

/// 借用 schema 代次计划，局部重建不重复计算可达性。
pub(crate) struct ReferenceFieldPlan<'a> {
    schema: &'a CftSchema,
}
impl<'a> ReferenceFieldPlan<'a> {
    pub(crate) fn new(schema: BuildSchema<'a>) -> Self {
        Self {
            schema: schema.cft(),
        }
    }
    pub(crate) fn fields<'b>(
        &'b self,
        actual_type: &'b str,
        references_only: bool,
    ) -> impl Iterator<Item = &'b CftField> {
        self.schema.reference_fields(actual_type, references_only)
    }
}
