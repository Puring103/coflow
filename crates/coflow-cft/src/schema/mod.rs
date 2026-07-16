mod compiler;
mod declarations;
mod dimensions;
mod names;
mod plans;
mod queries;
mod value_type;

pub use compiler::build_schema;
pub use declarations::*;
pub use dimensions::{CftDimensionInput, CftDimensionInputError, CftDimensionInputs};
pub use names::*;
pub use plans::{
    ScheduledCheckBlock, TypedCheckPlan, TypedCheckSchedule, ValueDependencyCycle,
    ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};
pub use queries::CftEnumValue;
pub use value_type::CftValueType;

use self::compiler::SchemaDeclarations;
use crate::module::ModuleId;
use crate::{CftDiagnostic, CftDiagnostics, CftErrorCode, Span};
use coflow_structure::{BudgetExceeded, StructuralBudget};
use std::collections::BTreeMap;

#[derive(Debug)]
pub(super) struct LocatedBudgetError {
    pub(super) error: BudgetExceeded,
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

impl LocatedBudgetError {
    fn into_diagnostics(self) -> CftDiagnostics {
        CftDiagnostics::one(CftDiagnostic::error(
            CftErrorCode::SchemaStructureLimitExceeded,
            self.module,
            self.span,
            self.error.to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct CftSchema {
    consts: BTreeMap<ConstName, CftConst>,
    pub(crate) types: BTreeMap<TypeName, CftType>,
    enums: BTreeMap<EnumName, CftEnum>,
    children_by_parent: BTreeMap<TypeName, Vec<TypeName>>,
    dimensions: BTreeMap<DimensionName, CftDimension>,
    type_by_id_as_enum: BTreeMap<EnumName, TypeName>,
    typed_checks: TypedCheckPlan,
    value_dependencies: ValueDependencyPlan,
}

impl CftSchema {
    pub(in crate::schema) fn from_declarations(
        declarations: SchemaDeclarations,
        dimension_inputs: &CftDimensionInputs,
        budget: &mut StructuralBudget,
    ) -> Result<Self, CftDiagnostics> {
        let consts = declarations.consts;
        let enums = declarations.enums;
        let types = declarations.types;

        let children_by_parent = types.values().fold(
            BTreeMap::<TypeName, Vec<TypeName>>::new(),
            |mut children, ty| {
                if let Some(parent) = &ty.parent {
                    children
                        .entry(parent.clone())
                        .or_default()
                        .push(ty.name.clone());
                }
                children
            },
        );

        let dimensions = dimensions::build_dimensions(&types, dimension_inputs)?;
        let type_by_id_as_enum = types
            .values()
            .filter_map(|ty| {
                ty.id_as_enum
                    .as_ref()
                    .map(|enum_name| (enum_name.clone(), ty.name.clone()))
            })
            .collect();
        let typed_checks = TypedCheckPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        let value_dependencies = ValueDependencyPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        Ok(Self {
            consts,
            types,
            enums,
            children_by_parent,
            dimensions,
            type_by_id_as_enum,
            typed_checks,
            value_dependencies,
        })
    }
}
