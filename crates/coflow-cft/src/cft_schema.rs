mod dimension_checks;
mod queries;
mod typed_checks;
mod value_dependencies;

pub use typed_checks::{TypedCheckPlan, TypedCheckSchedule};
pub use value_dependencies::{
    ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};

use crate::module_id::ModuleId;
use crate::schema::CompiledSchema;
use crate::{
    CftConst, CftConstValue, CftDiagnostic, CftDiagnostics, CftEnum, CftErrorCode, CftField,
    CftFieldDimension, CftSchemaCheckBlock, CftSchemaTypeRef, CftType, ConstName, DimensionName,
    EnumName, EnumVariantName, FieldName, Span, TypeName,
};
use coflow_structure::{BudgetExceeded, StructuralBudget, StructuralLimits};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

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
    sources: BTreeMap<ModuleId, String>,
    consts: BTreeMap<ConstName, CftConst>,
    pub(crate) types: BTreeMap<TypeName, CftType>,
    enums: BTreeMap<EnumName, CftEnum>,
    children_by_parent: BTreeMap<TypeName, BTreeSet<TypeName>>,
    dimension_storage_types:
        BTreeMap<DimensionName, BTreeMap<TypeName, BTreeMap<FieldName, TypeName>>>,
    typed_checks: TypedCheckPlan,
    value_dependencies: ValueDependencyPlan,
    structural_limits: StructuralLimits,
}

impl CftSchema {
    pub(crate) fn with_extension_types(
        self,
        types: impl IntoIterator<Item = CftType>,
    ) -> Result<Self, CftDiagnostics> {
        let mut compiled = CompiledSchema {
            consts: self.consts.clone(),
            types: self.types.clone(),
            enums: self.enums.clone(),
        };
        let sources = self.sources.clone();
        let structural_limits = self.structural_limits;
        for ty in types {
            if compiled.types.contains_key(&ty.name)
                || compiled.enums.contains_key(ty.name.as_str())
                || compiled.consts.contains_key(ty.name.as_str())
            {
                return Err(CftDiagnostics::one(CftDiagnostic::error(
                    CftErrorCode::DuplicateGlobalName,
                    ty.module.clone(),
                    ty.span,
                    format!("duplicate global name `{}`", ty.name),
                )));
            }
            compiled.types.insert(ty.name.clone(), ty);
        }
        let mut budget = StructuralBudget::new(structural_limits);
        Self::from_compiled(compiled, sources, structural_limits, &mut budget)
    }

    pub(crate) fn from_compiled(
        compiled: CompiledSchema,
        sources: BTreeMap<ModuleId, String>,
        structural_limits: StructuralLimits,
        budget: &mut StructuralBudget,
    ) -> Result<Self, CftDiagnostics> {
        let consts = compiled.consts;
        let enums = compiled.enums;
        let types = compiled.types;

        let children_by_parent = types.values().fold(
            BTreeMap::<TypeName, BTreeSet<TypeName>>::new(),
            |mut children, ty| {
                if let Some(parent) = &ty.parent {
                    children
                        .entry(parent.clone())
                        .or_default()
                        .insert(ty.name.clone());
                }
                children
            },
        );

        let dimension_storage_types = Self::build_dimension_storage_index(&types);
        let typed_checks = TypedCheckPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        let value_dependencies = ValueDependencyPlan::compile(&types, budget)
            .map_err(LocatedBudgetError::into_diagnostics)?;
        let mut view = Self {
            sources,
            consts,
            types,
            enums,
            children_by_parent,
            dimension_storage_types,
            typed_checks,
            value_dependencies,
            structural_limits,
        };
        view.populate_dimension_checks();
        Ok(view)
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            sources: BTreeMap::new(),
            consts: BTreeMap::new(),
            types: BTreeMap::new(),
            enums: BTreeMap::new(),
            children_by_parent: BTreeMap::new(),
            dimension_storage_types: BTreeMap::new(),
            typed_checks: TypedCheckPlan::default(),
            value_dependencies: ValueDependencyPlan::default(),
            structural_limits: StructuralLimits::default(),
        }
    }

    fn build_dimension_storage_index(
        types: &BTreeMap<TypeName, CftType>,
    ) -> BTreeMap<DimensionName, BTreeMap<TypeName, BTreeMap<FieldName, TypeName>>> {
        let mut out: BTreeMap<
            DimensionName,
            BTreeMap<TypeName, BTreeMap<FieldName, TypeName>>,
        > = BTreeMap::new();
        for schema_type in types.values() {
            for annotation in &schema_type.annotations {
                if annotation.name != "__coflow_dimension_storage" {
                    continue;
                }
                if let [crate::CftAnnotationValue::String(dimension), crate::CftAnnotationValue::String(source_type), crate::CftAnnotationValue::String(source_field)] =
                    annotation.args.as_slice()
                {
                    out.entry(DimensionName::from_validated(dimension.clone()))
                        .or_default()
                        .entry(TypeName::from_validated(source_type.clone()))
                        .or_default()
                        .insert(
                            FieldName::from_validated(source_field.clone()),
                            schema_type.name.clone(),
                        );
                }
            }
        }
        out
    }

    #[must_use]
    pub const fn value_dependencies(&self) -> &ValueDependencyPlan {
        &self.value_dependencies
    }

    fn populate_dimension_checks(&mut self) {
        let names = self.types.keys().cloned().collect::<Vec<_>>();
        for name in &names {
            let checks = self.dimension_checks_for_type(name);
            if let Some(meta) = self.types.get_mut(name) {
                meta.dimension_checks = checks;
            }
        }
    }

    fn dimension_checks_for_type(
        &self,
        type_name: &str,
    ) -> BTreeMap<DimensionName, CftSchemaCheckBlock> {
        dimension_checks::dimension_checks_for_type(self, type_name)
    }

    pub fn type_names(&self) -> impl Iterator<Item = &TypeName> {
        self.types.keys()
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

    pub fn module_ids(&self) -> impl Iterator<Item = &ModuleId> {
        self.sources.keys()
    }

    #[must_use]
    pub fn source(&self, module: &ModuleId) -> Option<&str> {
        self.sources.get(module).map(String::as_str)
    }

    pub fn const_names(&self) -> impl Iterator<Item = &ConstName> {
        self.consts.keys()
    }

    pub fn const_values(&self) -> impl Iterator<Item = &CftConstValue> {
        self.consts.values().map(|value| &value.value)
    }

    #[must_use]
    pub fn const_value(&self, const_name: &str) -> Option<&CftConstValue> {
        self.consts.get(const_name).map(|value| &value.value)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &EnumName> {
        self.enums.keys()
    }

    #[must_use]
    pub fn has_type(&self, type_name: &str) -> bool {
        self.types.contains_key(type_name)
    }

    #[must_use]
    pub fn is_schema_enum(&self, name: &str) -> bool {
        self.enums.contains_key(name)
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
    pub fn dimension_storage_type(
        &self,
        dimension: &str,
        source_type: &str,
        source_field: &str,
    ) -> Option<&str> {
        let by_source_type = self.dimension_storage_types.get(dimension)?;
        let mut current = Some(source_type);
        while let Some(type_name) = current {
            if let Some(storage_type) = by_source_type
                .get(type_name)
                .and_then(|by_field| by_field.get(source_field))
            {
                return Some(storage_type.as_str());
            }
            current = self
                .types
                .get(type_name)
                .and_then(|meta| meta.parent.as_deref());
        }
        None
    }

    #[must_use]
    pub fn is_dimension_storage_type(&self, type_name: &str) -> bool {
        self.types.get(type_name).is_some_and(|meta| {
            meta.annotations
                .iter()
                .any(|annotation| annotation.name == "__coflow_dimension_storage")
        })
    }

    #[must_use]
    pub fn field_type(&self, actual_type: &str, field_name: &str) -> Option<&CftSchemaTypeRef> {
        self.types
            .get(actual_type)
            .and_then(|ty| ty.field(field_name))
            .map(|field| &field.ty_ref)
    }

    #[must_use]
    pub fn field(&self, actual_type: &str, field_name: &str) -> Option<&CftField> {
        self.types.get(actual_type)?.field(field_name)
    }

    #[must_use]
    pub fn full_fields(&self, type_name: &str) -> Option<&[Arc<CftField>]> {
        self.fields_slice(type_name)
    }

    #[must_use]
    pub fn fields_slice(&self, type_name: &str) -> Option<&[Arc<CftField>]> {
        self.types
            .get(type_name)
            .map(|meta| meta.all_fields.as_slice())
    }

    #[must_use]
    pub fn fields(&self, type_name: &str) -> Option<impl Iterator<Item = &CftField>> {
        self.fields_slice(type_name)
            .map(|fields| fields.iter().map(AsRef::as_ref))
    }

    #[must_use]
    pub fn field_count(&self, type_name: &str) -> Option<usize> {
        self.fields_slice(type_name).map(<[_]>::len)
    }

    #[must_use]
    pub fn has_dimension_fields(&self) -> bool {
        self.all_types()
            .any(|ty| ty.all_fields.iter().any(|field| field.dimension.is_some()))
    }

    #[must_use]
    pub fn has_descendants(&self, type_name: &str) -> bool {
        self.children_by_parent
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub fn descendant_names(&self, type_name: &str) -> impl Iterator<Item = &TypeName> {
        self.children_by_parent.get(type_name).into_iter().flatten()
    }

    #[must_use]
    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
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
        for child in self.descendant_names(type_name) {
            let Some(child_meta) = self.types.get(child) else {
                continue;
            };
            if !child_meta.is_abstract {
                out.push(child.clone());
            }
            self.collect_concrete_descendants(child, out);
        }
    }

    #[must_use]
    pub fn dimension_field(
        &self,
        actual_type: &str,
        field_name: &str,
    ) -> Option<&CftFieldDimension> {
        self.types.get(actual_type)?.field(field_name)?.dimension.as_ref()
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

    pub fn fields(&self) -> impl Iterator<Item = &CftField> {
        self.all_fields.iter().map(AsRef::as_ref)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumValue {
    pub enum_name: EnumName,
    pub variant: Option<EnumVariantName>,
    pub value: i64,
}
