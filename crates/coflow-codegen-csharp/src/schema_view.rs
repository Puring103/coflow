use crate::names::{annotation_name_arg, csharp_type_name, has_annotation};
use crate::CsharpCodegenError;
use coflow_cft::{
    CftContainer, CftEnumMeta, CftFieldMeta, CftSchemaTypeRef, CftSchemaView, CftTypeMeta,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct SchemaView {
    pub int_32: bool,
    pub float_32: bool,
    pub loadable_tables: BTreeSet<String>,
    children: BTreeMap<String, BTreeSet<String>>,
    pub cft: CftSchemaView,
    csharp_types: BTreeMap<String, String>,
    csharp_enums: BTreeMap<String, String>,
}

impl SchemaView {
    pub fn new(schema: &CftContainer) -> Self {
        let cft = CftSchemaView::new(schema);

        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for schema_type in cft.types.values() {
            if let Some(parent) = &schema_type.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert(schema_type.name.clone());
            }
        }

        let csharp_types = cft
            .types
            .keys()
            .map(|name| (name.clone(), csharp_type_name(name)))
            .collect::<BTreeMap<_, _>>();
        let csharp_enums = cft
            .enums
            .keys()
            .map(|name| (name.clone(), csharp_type_name(name)))
            .collect::<BTreeMap<_, _>>();

        Self {
            int_32: false,
            float_32: false,
            loadable_tables: BTreeSet::new(),
            children,
            cft,
            csharp_types,
            csharp_enums,
        }
    }

    pub fn cft_enum_meta(&self, name: &str) -> Option<&CftEnumMeta> {
        self.cft.enums.get(name)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.cft.enums.keys()
    }

    pub fn is_schema_enum(&self, name: &str) -> bool {
        self.cft.enums.contains_key(name)
    }

    pub fn uses_localization(&self) -> bool {
        self.cft
            .types
            .values()
            .any(|ty| ty.all_fields.iter().any(|field| field.dimension.is_some()))
    }

    pub fn id_as_enum_names(&self) -> BTreeSet<String> {
        self.cft
            .types
            .values()
            .filter_map(|ty| self.type_id_as_enum(ty))
            .collect()
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

    pub fn type_meta(&self, name: &str) -> Result<&TypeMeta, CsharpCodegenError> {
        self.cft
            .types
            .get(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> Vec<String> {
        self.cft.types.keys().cloned().collect()
    }

    pub fn table_names(&self) -> Vec<String> {
        self.cft
            .types
            .values()
            .filter(|ty| !ty.is_abstract && !ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    /// Names of `@singleton` types, in declaration order.
    pub fn singleton_type_names(&self) -> Vec<String> {
        self.cft
            .types
            .values()
            .filter(|ty| ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn polymorphic_type_names(&self) -> Vec<String> {
        self.cft
            .types
            .values()
            .filter(|ty| self.range_is_polymorphic(&ty.name))
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.cft
            .types
            .get(type_name)
            .is_some_and(|ty| ty.is_abstract || self.has_descendants(type_name))
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
        let mut current = Some(type_name);
        while let Some(name) = current {
            let meta = self.cft.types.get(name)?;
            if let Some(enum_name) = self.type_id_as_enum(meta) {
                return Some(enum_name);
            }
            current = meta.parent.as_deref();
        }
        None
    }

    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.cft
            .types
            .values()
            .any(|ty| self.type_id_as_enum(ty).as_deref() == Some(enum_name))
    }

    pub fn key_field_type(&self, type_name: &str) -> CftSchemaTypeRef {
        self.id_as_enum(type_name)
            .map_or_else(|| CftSchemaTypeRef::String, CftSchemaTypeRef::Named)
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub fn type_has_descendants(&self, type_name: &str) -> bool {
        self.has_descendants(type_name)
    }

    pub fn concrete_assignable_types(
        &self,
        type_name: &str,
    ) -> Result<Vec<String>, CsharpCodegenError> {
        let mut out = Vec::new();
        let ty = self.type_meta(type_name)?;
        if !ty.is_abstract {
            out.push(type_name.to_string());
        }
        self.fill_concrete_descendants(type_name, &mut out)?;
        Ok(out)
    }

    fn fill_concrete_descendants(
        &self,
        type_name: &str,
        out: &mut Vec<String>,
    ) -> Result<(), CsharpCodegenError> {
        let Some(children) = self.children.get(type_name) else {
            return Ok(());
        };
        for child in children {
            let child_meta = self.type_meta(child)?;
            if !child_meta.is_abstract {
                out.push(child.clone());
            }
            self.fill_concrete_descendants(child, out)?;
        }
        Ok(())
    }

    pub fn ref_target_names(&self) -> Vec<String> {
        let mut out = BTreeSet::new();
        for ty in self.cft.types.values() {
            let mut visited = BTreeSet::new();
            self.collect_ref_targets_for_type(ty, &mut out, &mut visited);
        }
        out.into_iter().collect()
    }

    pub fn type_is_struct(&self, ty: &TypeMeta) -> bool {
        has_annotation(&ty.annotations, "struct")
    }

    fn type_id_as_enum(&self, ty: &TypeMeta) -> Option<String> {
        annotation_name_arg(&ty.annotations, "idAsEnum")
    }

    fn collect_ref_targets_for_type(
        &self,
        ty: &TypeMeta,
        out: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) {
        if !visited.insert(ty.name.clone()) {
            return;
        }
        for field in &ty.all_fields {
            self.collect_ref_targets_in_field(field, out, visited);
        }
    }

    fn collect_ref_targets_in_field(
        &self,
        field: &FieldMeta,
        out: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) {
        self.collect_ref_targets_in_type(&field.ty_ref, out, visited);
    }

    fn collect_ref_targets_in_type(
        &self,
        ty: &CftSchemaTypeRef,
        out: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) {
        match ty {
            CftSchemaTypeRef::Named(name) if self.is_schema_enum(name) => {}
            CftSchemaTypeRef::Named(name) => {
                if let Some(meta) = self.cft.types.get(name) {
                    self.collect_ref_targets_for_type(meta, out, visited);
                }
            }
            CftSchemaTypeRef::Ref(name) => {
                out.insert(name.clone());
            }
            CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
                self.collect_ref_targets_in_type(inner, out, visited);
            }
            CftSchemaTypeRef::Dict(_, value) => {
                self.collect_ref_targets_in_type(value, out, visited);
            }
            CftSchemaTypeRef::Int
            | CftSchemaTypeRef::Float
            | CftSchemaTypeRef::Bool
            | CftSchemaTypeRef::String => {}
        }
    }
}

pub type TypeMeta = CftTypeMeta;
pub type FieldMeta = CftFieldMeta;
