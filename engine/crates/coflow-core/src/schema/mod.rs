mod check_builtins;
pub use coflow_language::cft::{
    parse_modules, parse_modules_with_options, syntax, tokenize_cft, CftFile, CftModule,
    CftModuleSet, ModuleId,
};
#[cfg(feature = "cft-compiler")]
mod compiler;
mod declarations;
pub(crate) mod dimensions;
mod names;
mod plans;
mod queries;
mod signature;
mod value_type;

pub use check_builtins::CftCheckBuiltin;
#[cfg(feature = "cft-compiler")]
pub use compiler::{build_schema, build_schema_with_limits};
pub use declarations::*;
pub use names::*;
pub use plans::{
    ValueDependencyCycle, ValueDependencyPlan, ValueDependencyStep,
};
pub use queries::CftEnumValue;
pub use value_type::{CftFunctionParameter, CftValueType};

use crate::limits::{
    BudgetExceeded, StructuralBudget, StructuralLimits, StructureKind, TraversalCursor,
};
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct CftSchema {
    aliases: BTreeMap<String, CftValueType>,
    consts: BTreeMap<ConstName, CftConst>,
    pub(crate) types: BTreeMap<TypeName, CftType>,
    #[serde(skip)]
    inheritance_root_by_type: BTreeMap<TypeName, TypeName>,
    #[serde(skip)]
    ancestors_by_type: BTreeMap<TypeName, Vec<TypeName>>,
    #[serde(skip)]
    ancestor_membership_by_type: BTreeMap<TypeName, BTreeSet<TypeName>>,
    enums: BTreeMap<EnumName, CftEnum>,
    top_level_checks: BTreeMap<CheckName, CftTopLevelCheck>,
    sources: BTreeMap<ModuleId, CftSchemaSource>,
    #[serde(skip)]
    children_by_parent: BTreeMap<TypeName, Vec<TypeName>>,
    #[serde(skip)]
    dimensions: BTreeMap<DimensionName, CftDimension>,
    #[serde(skip)]
    type_by_id_as_enum: BTreeMap<EnumName, TypeName>,
    #[serde(skip)]
    value_dependencies: ValueDependencyPlan,
}

impl CftSchema {
    pub(in crate::schema) fn from_declarations(
        declarations: SchemaDeclarations,
        budget: &mut AnalysisBudget,
    ) -> Result<Self, CftDiagnostics> {
        let aliases = declarations.aliases;
        let consts = declarations.consts;
        let mut enums = declarations.enums;
        let top_level_checks = declarations.checks;
        let sources = declarations.sources;
        let mut types = declarations.types;
        let dimensions = dimensions::build_dimensions(&types);

        let mut inheritance_root_by_type = BTreeMap::new();
        let mut ancestors_by_type = BTreeMap::new();
        let mut ancestor_membership_by_type = BTreeMap::new();
        for (key, ty) in &types {
            if key != &ty.name {
                return Err(CftDiagnostics::one(CftDiagnostic::error(
                    CftErrorCode::InvalidTypeReference, ty.module.clone(), ty.span,
                    format!("type key `{key}` differs from declaration `{}`", ty.name))));
            }
            let mut ancestors = Vec::new();
            let mut current = ty.parent.as_ref();
            let mut seen = BTreeSet::from([ty.name.clone()]);
            while let Some(parent) = current {
                let invalid = |code, message: String| CftDiagnostics::one(CftDiagnostic::error(
                    code, ty.module.clone(), ty.span, message));
                budget.charge(StructureKind::SchemaDependency, 1)
                    .map_err(|error| invalid(CftErrorCode::SchemaStructureLimitExceeded, error.to_string()))?;
                budget.check_depth(TraversalCursor::root(), StructureKind::SchemaDependency, ancestors.len() as u64 + 1)
                    .map_err(|error| invalid(CftErrorCode::SchemaStructureLimitExceeded, error.to_string()))?;
                if !seen.insert(parent.clone()) {
                    return Err(invalid(CftErrorCode::InheritanceCycle, format!("cyclic inheritance at `{parent}`")));
                }
                let meta = types.get(parent).ok_or_else(|| invalid(CftErrorCode::UnknownNamedType, format!("unknown parent `{parent}`")))?;
                ancestors.push(parent.clone());
                current = meta.parent.as_ref();
            }
            inheritance_root_by_type.insert(
                ty.name.clone(),
                ancestors.last().cloned().unwrap_or_else(|| ty.name.clone()),
            );
            ancestor_membership_by_type
                .insert(ty.name.clone(), ancestors.iter().cloned().collect());
            ancestors_by_type.insert(ty.name.clone(), ancestors);
        }

        // 所有入口共用一次索引构造；契约不保存可以从声明恢复的缓存。
        let fields: BTreeMap<_, Vec<_>> = types.values().map(|ty| {
            let fields = ancestors_by_type[&ty.name].iter().rev()
                .flat_map(|name| types[name].own_fields.iter().cloned())
                .chain(ty.own_fields.iter().cloned()).collect();
            (ty.name.clone(), fields)
        }).collect();
        for (name, ty) in &mut types {
            ty.all_fields = fields[name].clone();
            ty.field_by_name = ty.all_fields.iter().enumerate()
                .map(|(index, field)| (field.name.clone(), index)).collect();
        }
        for enumeration in enums.values_mut() {
            enumeration.variant_by_name = enumeration.variants.iter().enumerate()
                .map(|(index, variant)| (variant.name.clone(), index)).collect();
            enumeration.variant_by_value = enumeration.variants.iter().enumerate()
                .map(|(index, variant)| (variant.value, index)).collect();
            enumeration.flag_mask = enumeration.variants.iter().fold(0, |mask, variant| mask | variant.value as u32);
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

        let default_diagnostics = plans::validate_default_materialization(&types);
        if !default_diagnostics.is_empty() {
            return Err(CftDiagnostics::new(default_diagnostics));
        }

        let type_by_id_as_enum = types
            .values()
            .filter_map(|ty| {
                ty.id_as_enum
                    .as_ref()
                    .map(|enum_name| (enum_name.clone(), ty.name.clone()))
            })
            .collect();
        let value_dependencies = ValueDependencyPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        Ok(Self {
            aliases,
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
            value_dependencies,
        })
    }
}

#[derive(serde::Deserialize)]
struct SchemaDeclarations {
    aliases: BTreeMap<String, CftValueType>,
    consts: BTreeMap<ConstName, CftConst>,
    types: BTreeMap<TypeName, CftType>,
    enums: BTreeMap<EnumName, CftEnum>,
    #[serde(rename = "top_level_checks")]
    checks: BTreeMap<CheckName, CftTopLevelCheck>,
    sources: BTreeMap<ModuleId, CftSchemaSource>,
}

impl<'de> serde::Deserialize<'de> for CftSchema {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let declarations = SchemaDeclarations::deserialize(deserializer)?;
        Self::from_declarations(declarations, &mut AnalysisBudget::new(StructuralLimits::default()))
            .map_err(|error| serde::de::Error::custom(format!("invalid schema: {error:?}")))
    }
}
