use std::collections::BTreeSet;

use crate::{
    CftConst, CftDimension, CftEnum, CftField, CftSchema, CftSchemaTypeRef, CftType, EnumName,
    EnumVariantName, TypeName, TypedCheckSchedule, ValueDependencyPlan,
};

impl CftSchema {
    #[must_use]
    pub fn inherited_id_as_enum(&self, type_name: &str) -> Option<EnumName> {
        let mut current = Some(type_name);
        while let Some(name) = current {
            let meta = self.types.get(name)?;
            if let Some(enum_name) = &meta.id_as_enum {
                return Some(enum_name.clone());
            }
            current = meta.parent.as_deref();
        }
        None
    }

    #[must_use]
    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.type_by_id_as_enum.contains_key(enum_name)
    }

    #[must_use]
    pub fn id_as_enum_names(&self) -> BTreeSet<EnumName> {
        self.type_by_id_as_enum.keys().cloned().collect()
    }

    #[must_use]
    pub fn ref_target_names(&self) -> Vec<TypeName> {
        let mut out = BTreeSet::new();
        for ty in self.types.values() {
            let mut visited = BTreeSet::new();
            self.collect_ref_targets_for_type(ty, &mut out, &mut visited);
        }
        out.into_iter().collect()
    }

    fn collect_ref_targets_for_type(
        &self,
        ty: &CftType,
        out: &mut BTreeSet<TypeName>,
        visited: &mut BTreeSet<TypeName>,
    ) {
        if !visited.insert(ty.name.clone()) {
            return;
        }
        for field in &ty.all_fields {
            self.collect_ref_targets_in_type(&field.ty_ref, out, visited);
        }
    }

    fn collect_ref_targets_in_type(
        &self,
        ty: &CftSchemaTypeRef,
        out: &mut BTreeSet<TypeName>,
        visited: &mut BTreeSet<TypeName>,
    ) {
        match ty {
            CftSchemaTypeRef::Object(name) => {
                if let Some(meta) = self.types.get(name) {
                    self.collect_ref_targets_for_type(meta, out, visited);
                }
            }
            CftSchemaTypeRef::RecordRef(name) => {
                out.insert(name.clone());
            }
            CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
                self.collect_ref_targets_in_type(inner, out, visited);
            }
            CftSchemaTypeRef::Dict(_, value) => {
                self.collect_ref_targets_in_type(value, out, visited);
            }
            CftSchemaTypeRef::Enum(_)
            | CftSchemaTypeRef::Int
            | CftSchemaTypeRef::Float
            | CftSchemaTypeRef::Bool
            | CftSchemaTypeRef::String => {}
        }
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
            let Some(meta) = self.types.get(name) else {
                break;
            };
            out.push(meta.name.clone());
            current = meta.parent.as_deref();
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
            out.push(meta.name.clone());
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
