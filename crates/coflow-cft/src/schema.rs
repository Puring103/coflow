mod dimension_checks;
mod queries;
mod typed_checks;
mod value_dependencies;

pub use typed_checks::{TypedCheckPlan, TypedCheckSchedule};
pub use value_dependencies::{
    ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};

use crate::module_id::ModuleId;
use crate::compiled::CompiledSchema;
use crate::{
    CftConst, CftDiagnostic, CftDiagnostics, CftDimension, CftDimensionInputs, CftEnum,
    CftErrorCode, CftField, CftType, ConstName, DimensionName, EnumName, EnumVariantName, Span,
    TypeName,
};
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
    pub(crate) fn from_compiled(
        compiled: CompiledSchema,
        dimension_inputs: &CftDimensionInputs,
        budget: &mut StructuralBudget,
    ) -> Result<Self, CftDiagnostics> {
        let consts = compiled.consts;
        let enums = compiled.enums;
        let types = compiled.types;

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

        let dimensions = crate::dimensions::build_dimensions(&types, dimension_inputs)?;
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

    #[must_use]
    pub const fn value_dependencies(&self) -> &ValueDependencyPlan {
        &self.value_dependencies
    }

    /// Returns the semantic declaration retained for language tooling.
    #[must_use]
    pub fn resolve_type(&self, name: &str) -> Option<&CftType> {
        self.types.get(name)
    }

    /// Returns the semantic enum declaration retained for language tooling.
    #[must_use]
    pub fn resolve_enum(&self, name: &str) -> Option<&CftEnum> {
        self.enums.get(name)
    }

    /// Returns the semantic const declaration retained for language tooling.
    #[must_use]
    pub fn resolve_const(&self, name: &str) -> Option<&CftConst> {
        self.consts.get(name)
    }

    pub fn all_types(&self) -> impl Iterator<Item = &CftType> {
        self.types.values()
    }

    pub fn all_enums(&self) -> impl Iterator<Item = &CftEnum> {
        self.enums.values()
    }

    pub fn all_consts(&self) -> impl Iterator<Item = &CftConst> {
        self.consts.values()
    }

    #[must_use]
    pub fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    #[must_use]
    pub fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        let meta = self.enums.get(enum_name)?;
        let index = *meta.variant_by_name.get(variant)?;
        meta.variants.get(index).map(|variant| variant.value)
    }

    #[must_use]
    pub fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CftEnumValue> {
        let meta = self.enums.get(enum_name)?;
        let index = *meta.variant_by_value.get(&value)?;
        let variant = meta.variants.get(index)?;
        Some(CftEnumValue {
            enum_name: meta.name.clone(),
            variant: Some(variant.name.clone()),
            value,
        })
    }

    #[must_use]
    pub fn check_schedule<'schema, 'dimension>(
        &'schema self,
        actual_type: &str,
        dimension: Option<&'dimension str>,
    ) -> TypedCheckSchedule<'schema, 'dimension> {
        TypedCheckSchedule::new(self, actual_type, dimension)
    }

    #[must_use]
    pub fn field_has_nested_checks(&self, actual_type: &str, field_name: &str) -> bool {
        self.typed_checks
            .field_has_nested_checks(actual_type, field_name)
    }

    #[must_use]
    pub fn resolve_dimension(&self, name: &str) -> Option<&CftDimension> {
        self.dimensions.get(name)
    }

    pub fn all_dimensions(&self) -> impl Iterator<Item = &CftDimension> {
        self.dimensions.values()
    }

    #[must_use]
    pub fn type_for_id_as_enum(&self, enum_name: &str) -> Option<&CftType> {
        self.types.get(self.type_by_id_as_enum.get(enum_name)?)
    }

    #[must_use]
    pub fn field(&self, actual_type: &str, field_name: &str) -> Option<&CftField> {
        self.types.get(actual_type)?.field(field_name)
    }

    pub fn children(&self, type_name: &TypeName) -> &[TypeName] {
        self.children_by_parent
            .get(type_name)
            .map_or(&[], Vec::as_slice)
    }

    #[must_use]
    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || !self.children(&meta.name).is_empty())
    }

    #[must_use]
    pub fn assignable_target_names(&self, actual_type: &str) -> Vec<TypeName> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(TypeName::from_validated(name.to_string()));
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    pub fn singleton_types(&self) -> impl Iterator<Item = &CftType> {
        self.types.values().filter(|meta| meta.is_singleton)
    }

    #[must_use]
    pub fn concrete_assignable_types(&self, type_name: &str) -> Option<Vec<TypeName>> {
        let mut out = Vec::new();
        let meta = self.types.get(type_name)?;
        if !meta.is_abstract {
            out.push(TypeName::from_validated(type_name.to_string()));
        }
        self.collect_concrete_descendants(type_name, &mut out);
        Some(out)
    }

    fn collect_concrete_descendants(&self, type_name: &str, out: &mut Vec<TypeName>) {
        let Some(parent) = self.types.get(type_name) else {
            return;
        };
        for child in self.children(&parent.name) {
            let Some(child_meta) = self.types.get(child) else {
                continue;
            };
            if !child_meta.is_abstract {
                out.push(child.clone());
            }
            self.collect_concrete_descendants(child, out);
        }
    }

}

impl CftType {
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&CftField> {
        let index = *self.field_by_name.get(name)?;
        self.all_fields.get(index).map(AsRef::as_ref)
    }

    pub fn own_fields(&self) -> impl Iterator<Item = &CftField> {
        self.own_fields.iter().map(AsRef::as_ref)
    }

    pub fn all_fields(&self) -> impl Iterator<Item = &CftField> {
        self.all_fields.iter().map(AsRef::as_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumValue {
    pub enum_name: EnumName,
    pub variant: Option<EnumVariantName>,
    pub value: i64,
}
