use crate::names::csharp_type_name;
use crate::CsharpCodegenError;
use coflow_cft::{CftContainer, CftEnumMeta, CftSchemaTypeRef, CftSchemaView, CftTypeMeta};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CsharpSchemaContext {
    pub int_32: bool,
    pub float_32: bool,
    pub loadable_tables: BTreeSet<String>,
    pub cft: CftSchemaView,
    csharp_types: BTreeMap<String, String>,
    csharp_enums: BTreeMap<String, String>,
}

impl CsharpSchemaContext {
    pub fn new(schema: &CftContainer) -> Self {
        let cft = CftSchemaView::new(schema);

        let csharp_types = cft
            .type_names()
            .map(|name| (name.clone(), csharp_type_name(name)))
            .collect::<BTreeMap<_, _>>();
        let csharp_enums = cft
            .enum_names()
            .map(|name| (name.clone(), csharp_type_name(name)))
            .collect::<BTreeMap<_, _>>();

        Self {
            int_32: false,
            float_32: false,
            loadable_tables: BTreeSet::new(),
            cft,
            csharp_types,
            csharp_enums,
        }
    }

    pub fn cft_enum_meta(&self, name: &str) -> Option<&CftEnumMeta> {
        self.cft.enum_meta(name)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.cft.enum_names()
    }

    pub fn cft_enum_metas(&self) -> impl Iterator<Item = &CftEnumMeta> {
        self.cft.enum_metas()
    }

    pub fn type_metas(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.cft.type_metas()
    }

    pub fn is_schema_enum(&self, name: &str) -> bool {
        self.cft.is_schema_enum(name)
    }

    pub fn uses_localization(&self) -> bool {
        self.cft
            .type_metas()
            .any(|ty| ty.all_fields.iter().any(|field| field.dimension.is_some()))
    }

    pub fn id_as_enum_names(&self) -> BTreeSet<String> {
        self.cft.id_as_enum_names()
    }

    #[must_use]
    pub const fn with_int_32(mut self, value: bool) -> Self {
        self.int_32 = value;
        self
    }

    #[must_use]
    pub const fn with_float_32(mut self, value: bool) -> Self {
        self.float_32 = value;
        self
    }

    #[must_use]
    pub fn with_loadable_tables(mut self, tables: BTreeSet<String>) -> Self {
        self.loadable_tables = tables;
        self
    }

    /// Returns true if `name` resolves (directly or via descendants) to at
    /// least one type that has a generated table loader. This determines
    /// whether `context.GetX(...)` is callable for fields of that type.
    pub fn is_ref_target_loadable(&self, name: &str) -> bool {
        if self.loadable_tables.contains(name) {
            return true;
        }
        self.concrete_assignable_types(name)
            .is_ok_and(|assignable| assignable.iter().any(|t| self.loadable_tables.contains(t)))
    }

    pub fn type_meta(&self, name: &str) -> Result<&CftTypeMeta, CsharpCodegenError> {
        self.cft
            .type_meta(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> Vec<String> {
        self.cft.type_names().cloned().collect()
    }

    pub fn table_names(&self) -> Vec<String> {
        self.cft
            .type_metas()
            .filter(|ty| !ty.is_abstract && !ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    /// Names of `@singleton` types, in declaration order.
    pub fn singleton_type_names(&self) -> Vec<String> {
        self.cft
            .type_metas()
            .filter(|ty| ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn polymorphic_type_names(&self) -> Vec<String> {
        self.cft
            .type_metas()
            .filter(|ty| self.range_is_polymorphic(&ty.name))
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.cft.range_is_polymorphic(type_name)
    }

    pub fn csharp_type_name(&self, type_name: &str) -> String {
        self.csharp_types
            .get(type_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(type_name))
    }

    pub fn csharp_enum_name(&self, enum_name: &str) -> String {
        self.csharp_enums
            .get(enum_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(enum_name))
    }

    pub fn csharp_named_type(&self, name: &str) -> String {
        if self.is_schema_enum(name) {
            self.csharp_enum_name(name)
        } else {
            self.csharp_type_name(name)
        }
    }

    pub fn id_as_enum(&self, type_name: &str) -> Option<String> {
        self.cft.inherited_id_as_enum(type_name)
    }

    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.cft.is_id_as_enum(enum_name)
    }

    pub fn key_field_type(&self, type_name: &str) -> CftSchemaTypeRef {
        self.id_as_enum(type_name)
            .map_or_else(|| CftSchemaTypeRef::String, CftSchemaTypeRef::Named)
    }

    pub fn type_has_descendants(&self, type_name: &str) -> bool {
        self.cft.has_descendants(type_name)
    }

    pub fn concrete_assignable_types(
        &self,
        type_name: &str,
    ) -> Result<Vec<String>, CsharpCodegenError> {
        self.cft
            .concrete_assignable_types(type_name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{type_name}`")))
    }

    pub fn ref_target_names(&self) -> Vec<String> {
        self.cft.ref_target_names()
    }

    pub fn type_is_struct(&self, ty: &CftTypeMeta) -> bool {
        self.cft.type_is_struct(&ty.name)
    }
}
