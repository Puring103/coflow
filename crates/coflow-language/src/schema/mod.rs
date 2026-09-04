mod check_builtins;
mod compiler;
mod declarations;
mod dimensions;
mod names;
mod plans;
mod queries;
mod value_type;

pub use check_builtins::CftCheckBuiltin;
pub use compiler::{build_schema, build_schema_with_limits};
pub use declarations::*;
pub use dimensions::{CftDimensionInput, CftDimensionInputError, CftDimensionInputs};
pub use names::*;
use plans::CheckIndex;
pub use plans::{
    CheckStatementRef, ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan,
    ValueDependencyStep,
};
pub use queries::CftEnumValue;
pub use value_type::{CftFunctionParameter, CftValueType};

use self::compiler::SchemaDeclarations;
use crate::limits::{BudgetExceeded, StructuralBudget, StructuralLimits, StructureKind, TraversalCursor};
use crate::module::ModuleId;
use crate::{CftDiagnostic, CftDiagnostics, CftErrorCode, Span};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub(super) struct LocatedBudgetError {
    pub(super) error: BudgetExceeded,
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

/// schema 派生图只共享深度与分析步数，不继承解析阶段的节点计数。
pub(super) struct AnalysisBudget(StructuralBudget);

impl AnalysisBudget {
    pub(super) fn new(limits: StructuralLimits) -> Self {
        Self(StructuralBudget::new(limits))
    }

    pub(super) fn charge(
        &mut self,
        kind: StructureKind,
        amount: u64,
    ) -> Result<(), BudgetExceeded> {
        self.0.charge_analysis(kind, amount)
    }

    pub(super) fn check_depth(
        &self,
        parent: TraversalCursor,
        kind: StructureKind,
        additional_depth: u64,
    ) -> Result<(), BudgetExceeded> {
        self.0
            .check_additional_depth(parent, kind, additional_depth)
    }
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
    inheritance_root_by_type: BTreeMap<TypeName, TypeName>,
    ancestors_by_type: BTreeMap<TypeName, Vec<TypeName>>,
    ancestor_membership_by_type: BTreeMap<TypeName, BTreeSet<TypeName>>,
    enums: BTreeMap<EnumName, CftEnum>,
    top_level_checks: BTreeMap<CheckName, CftTopLevelCheck>,
    sources: BTreeMap<ModuleId, CftSchemaSource>,
    children_by_parent: BTreeMap<TypeName, Vec<TypeName>>,
    dimensions: BTreeMap<DimensionName, CftDimension>,
    type_by_id_as_enum: BTreeMap<EnumName, TypeName>,
    check_index: CheckIndex,
    value_dependencies: ValueDependencyPlan,
}

impl CftSchema {
    pub(in crate::schema) fn from_declarations(
        declarations: SchemaDeclarations,
        dimension_inputs: &CftDimensionInputs,
        budget: &mut AnalysisBudget,
    ) -> Result<Self, CftDiagnostics> {
        let consts = declarations.consts;
        let enums = declarations.enums;
        let top_level_checks = declarations.checks;
        let sources = declarations.sources;
        let types = declarations.types;

        let mut inheritance_root_by_type = BTreeMap::new();
        let mut ancestors_by_type = BTreeMap::new();
        let mut ancestor_membership_by_type = BTreeMap::new();
        for ty in types.values() {
            let mut ancestors = Vec::new();
            let mut current = ty.parent.as_ref();
            while let Some(parent) = current {
                ancestors.push(parent.clone());
                current = types.get(parent).and_then(|meta| meta.parent.as_ref());
            }
            inheritance_root_by_type.insert(
                ty.name.clone(),
                ancestors.last().cloned().unwrap_or_else(|| ty.name.clone()),
            );
            ancestor_membership_by_type
                .insert(ty.name.clone(), ancestors.iter().cloned().collect());
            ancestors_by_type.insert(ty.name.clone(), ancestors);
        }

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
        let check_index = CheckIndex::compile(&types, &top_level_checks, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        let value_dependencies = ValueDependencyPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        Ok(Self {
            consts,
            types,
            inheritance_root_by_type,
            ancestors_by_type,
            ancestor_membership_by_type,
            enums,
            top_level_checks,
            sources,
            children_by_parent,
            dimensions,
            type_by_id_as_enum,
            check_index,
            value_dependencies,
        })
    }
}
