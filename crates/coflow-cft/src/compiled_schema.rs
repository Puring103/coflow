mod dimension_checks;
mod queries;
mod typed_checks;
mod value_dependencies;

pub use typed_checks::{TypedCheckPlan, TypedCheckSchedule};
pub use value_dependencies::{
    ValueDependencyCycle, ValueDependencyMode, ValueDependencyPlan, ValueDependencyStep,
};

use crate::container::ModuleId;
use crate::schema::SchemaReflection;
use crate::{
    CftAnnotation, CftConstValue, CftDiagnostic, CftDiagnostics, CftErrorCode, CftSchemaCheckBlock,
    CftSchemaEnum, CftSchemaType, CftSchemaTypeRef, Span,
};
use coflow_structure::{BudgetExceeded, StructuralBudget, StructuralLimits};
use std::collections::{BTreeMap, BTreeSet};

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
pub struct CompiledSchema {
    reflection: SchemaReflection,
    sources: BTreeMap<ModuleId, String>,
    consts: BTreeMap<String, CftConstValue>,
    types: BTreeMap<String, CftTypeMeta>,
    enums: BTreeMap<String, CftEnumMeta>,
    children_by_parent: BTreeMap<String, BTreeSet<String>>,
    dimension_storage_types: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>,
    typed_checks: TypedCheckPlan,
    value_dependencies: ValueDependencyPlan,
    structural_limits: StructuralLimits,
}

impl CompiledSchema {
    pub(crate) fn from_reflection(
        reflection: SchemaReflection,
        sources: BTreeMap<ModuleId, String>,
        structural_limits: StructuralLimits,
        budget: &mut StructuralBudget,
    ) -> Result<Self, CftDiagnostics> {
        let consts = reflection
            .modules
            .values()
            .flat_map(|module| module.consts.iter())
            .map(|schema_const| (schema_const.name.clone(), schema_const.value.clone()))
            .collect::<BTreeMap<_, _>>();

        let enums = reflection
            .enums
            .values()
            .map(|schema_enum| {
                (
                    schema_enum.name.clone(),
                    CftEnumMeta::from_schema(schema_enum),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = reflection
            .types
            .values()
            .map(|schema_type| {
                let meta = CftTypeMeta::from_schema(schema_type);
                (meta.name.clone(), meta)
            })
            .collect::<BTreeMap<_, _>>();

        let children_by_parent = types.values().fold(
            BTreeMap::<String, BTreeSet<String>>::new(),
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
            reflection,
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

    pub(crate) fn empty() -> Self {
        Self {
            reflection: SchemaReflection::default(),
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

    pub(crate) const fn reflection(&self) -> &SchemaReflection {
        &self.reflection
    }

    pub(crate) const fn sources(&self) -> &BTreeMap<ModuleId, String> {
        &self.sources
    }

    pub(crate) const fn structural_limits(&self) -> StructuralLimits {
        self.structural_limits
    }

    fn build_dimension_storage_index(
        types: &BTreeMap<String, CftTypeMeta>,
    ) -> BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>> {
        let mut out = BTreeMap::<String, BTreeMap<String, BTreeMap<String, String>>>::new();
        for schema_type in types.values() {
            for annotation in &schema_type.annotations {
                if annotation.name != "__coflow_dimension_storage" {
                    continue;
                }
                if let [crate::CftAnnotationValue::String(dimension), crate::CftAnnotationValue::String(source_type), crate::CftAnnotationValue::String(source_field)] =
                    annotation.args.as_slice()
                {
                    out.entry(dimension.clone())
                        .or_default()
                        .entry(source_type.clone())
                        .or_default()
                        .insert(source_field.clone(), schema_type.name.clone());
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

    fn dimension_checks_for_type(&self, type_name: &str) -> BTreeMap<String, CftSchemaCheckBlock> {
        dimension_checks::dimension_checks_for_type(self, type_name)
    }

    #[must_use]
    pub fn type_meta(&self, type_name: &str) -> Option<&CftTypeMeta> {
        self.types.get(type_name)
    }

    pub fn type_names(&self) -> impl Iterator<Item = &String> {
        self.types.keys()
    }

    pub fn type_metas(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.values()
    }

    pub fn const_names(&self) -> impl Iterator<Item = &String> {
        self.consts.keys()
    }

    pub fn const_values(&self) -> impl Iterator<Item = &CftConstValue> {
        self.consts.values()
    }

    #[must_use]
    pub fn const_value(&self, const_name: &str) -> Option<&CftConstValue> {
        self.consts.get(const_name)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enums.keys()
    }

    pub fn enum_metas(&self) -> impl Iterator<Item = &CftEnumMeta> {
        self.enums.values()
    }

    #[must_use]
    pub fn enum_meta(&self, enum_name: &str) -> Option<&CftEnumMeta> {
        self.enums.get(enum_name)
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
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    #[must_use]
    pub fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CftEnumValueMeta> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CftEnumValueMeta {
                enum_name: enum_name.to_string(),
                variant: Some(variant.clone()),
                value: *variant_value,
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
                return Some(storage_type);
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
            .and_then(|meta| meta.fields.get(field_name))
    }

    #[must_use]
    pub fn field_meta(&self, actual_type: &str, field_name: &str) -> Option<&CftFieldMeta> {
        self.fields(actual_type)?
            .find(|field| field.name == field_name)
    }

    #[must_use]
    pub fn full_fields(&self, type_name: &str) -> Option<&[CftFieldMeta]> {
        self.fields_slice(type_name)
    }

    #[must_use]
    pub fn fields_slice(&self, type_name: &str) -> Option<&[CftFieldMeta]> {
        self.types
            .get(type_name)
            .map(|meta| meta.all_fields.as_slice())
    }

    #[must_use]
    pub fn fields(&self, type_name: &str) -> Option<impl Iterator<Item = &CftFieldMeta>> {
        self.fields_slice(type_name).map(|fields| fields.iter())
    }

    #[must_use]
    pub fn field_count(&self, type_name: &str) -> Option<usize> {
        self.fields_slice(type_name).map(<[_]>::len)
    }

    #[must_use]
    pub fn has_dimension_fields(&self) -> bool {
        self.type_metas()
            .any(|ty| ty.all_fields.iter().any(|field| field.dimension.is_some()))
    }

    #[must_use]
    pub fn has_descendants(&self, type_name: &str) -> bool {
        self.children_by_parent
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub fn descendant_names(&self, type_name: &str) -> impl Iterator<Item = &String> {
        self.children_by_parent.get(type_name).into_iter().flatten()
    }

    #[must_use]
    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
    }

    #[must_use]
    pub fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(name.to_string());
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    pub fn singleton_types(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.values().filter(|meta| meta.is_singleton)
    }

    #[must_use]
    pub fn concrete_assignable_types(&self, type_name: &str) -> Option<Vec<String>> {
        let mut out = Vec::new();
        let meta = self.types.get(type_name)?;
        if !meta.is_abstract {
            out.push(type_name.to_string());
        }
        self.collect_concrete_descendants(type_name, &mut out);
        Some(out)
    }

    fn collect_concrete_descendants(&self, type_name: &str, out: &mut Vec<String>) {
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
    ) -> Option<&CftDimensionFieldMeta> {
        self.types
            .get(actual_type)
            .and_then(|meta| meta.dimension_fields.get(field_name))
    }
}

#[derive(Debug, Clone)]
pub struct CftTypeMeta {
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub span: Span,
    pub annotations: Vec<CftAnnotation>,
    pub check: Option<CftSchemaCheckBlock>,
    pub dimension_checks: BTreeMap<String, CftSchemaCheckBlock>,
    pub own_fields: Vec<CftFieldMeta>,
    pub all_fields: Vec<CftFieldMeta>,
    fields: BTreeMap<String, CftSchemaTypeRef>,
    dimension_fields: BTreeMap<String, CftDimensionFieldMeta>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CftFieldMeta {
    pub module: String,
    pub name: String,
    pub raw_type: String,
    pub ty_ref: CftSchemaTypeRef,
    pub has_default: bool,
    pub default: Option<crate::CftSchemaDefaultValue>,
    pub annotations: Vec<CftAnnotation>,
    pub dimension: Option<CftDimensionFieldMeta>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftDimensionFieldMeta {
    pub dimension: String,
    pub bucket: Option<String>,
}

impl CftTypeMeta {
    fn from_schema(schema_type: &CftSchemaType) -> Self {
        let dimension_fields = schema_type
            .all_fields
            .iter()
            .filter_map(|field| {
                let dimension = field.dimension.as_ref().map(|d| d.kind.name())?;
                Some((
                    field.name.clone(),
                    CftDimensionFieldMeta {
                        dimension: dimension.to_string(),
                        bucket: field.dimension.as_ref().and_then(|d| d.bucket.clone()),
                    },
                ))
            })
            .collect();
        Self {
            module: schema_type.module.to_string(),
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            is_sealed: schema_type.is_sealed,
            is_singleton: schema_type.is_singleton,
            span: schema_type.span,
            annotations: schema_type.annotations.clone(),
            check: schema_type.check.clone(),
            dimension_checks: BTreeMap::new(),
            own_fields: schema_type
                .fields
                .iter()
                .map(|field| CftFieldMeta::from_schema(field, &schema_type.module))
                .collect(),
            all_fields: schema_type
                .all_fields
                .iter()
                .map(|field| CftFieldMeta::from_schema(field, &schema_type.module))
                .collect(),
            fields: schema_type
                .all_fields
                .iter()
                .map(|field| (field.name.clone(), field.ty_ref.clone()))
                .collect(),
            dimension_fields,
        }
    }
}

impl CftFieldMeta {
    fn from_schema(field: &crate::CftSchemaField, module: &ModuleId) -> Self {
        Self {
            module: module.to_string(),
            name: field.name.clone(),
            raw_type: field.ty.clone(),
            ty_ref: field.ty_ref.clone(),
            has_default: field.has_default,
            default: field.default.clone(),
            annotations: field.annotations.clone(),
            dimension: field
                .dimension
                .as_ref()
                .map(|dimension| CftDimensionFieldMeta {
                    dimension: dimension.kind.name().to_string(),
                    bucket: dimension.bucket.clone(),
                }),
            span: field.span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftEnumValueMeta {
    pub enum_name: String,
    pub variant: Option<String>,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct CftEnumMeta {
    pub module: String,
    pub name: String,
    pub annotations: Vec<CftAnnotation>,
    pub all_variants: Vec<CftEnumVariantMeta>,
    variants: BTreeMap<String, i64>,
}

#[derive(Debug, Clone)]
pub struct CftEnumVariantMeta {
    pub name: String,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
}

impl CftEnumMeta {
    fn from_schema(schema_enum: &CftSchemaEnum) -> Self {
        Self {
            module: schema_enum.module.to_string(),
            name: schema_enum.name.clone(),
            annotations: schema_enum.annotations.clone(),
            all_variants: schema_enum
                .variants
                .iter()
                .map(|variant| CftEnumVariantMeta {
                    name: variant.name.clone(),
                    value: variant.value,
                    annotations: variant.annotations.clone(),
                })
                .collect(),
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.value))
                .collect(),
        }
    }
}
