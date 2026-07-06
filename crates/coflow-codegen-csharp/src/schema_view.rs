use crate::names::{annotation_name_arg, csharp_type_name, has_annotation};
use crate::CsharpCodegenError;
use coflow_cft::{
    CftContainer, CftEnumMeta, CftSchemaDefaultValue, CftSchemaTypeRef, CftSchemaView,
    CftTypeMeta as CftTypeMeta,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct SchemaView {
    pub types: BTreeMap<String, TypeMeta>,
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

        let types = cft
            .types
            .values()
            .map(|schema_type| {
                (
                    schema_type.name.clone(),
                    TypeMeta::from_schema_view(schema_type, &cft),
                )
            })
            .collect::<BTreeMap<_, _>>();
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
            types,
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
        self.types
            .values()
            .any(|ty| ty.all_fields.iter().any(|field| field.is_dimensional))
    }

    pub fn id_as_enum_names(&self) -> BTreeSet<String> {
        self.types
            .values()
            .filter_map(|ty| ty.id_as_enum.clone())
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
        self.types
            .get(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> Vec<String> {
        self.types.keys().cloned().collect()
    }

    pub fn table_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| !ty.is_abstract && !ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    /// Names of `@singleton` types, in declaration order.
    pub fn singleton_type_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn polymorphic_type_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| self.range_is_polymorphic(&ty.name))
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
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
            let meta = self.types.get(name)?;
            if let Some(enum_name) = &meta.id_as_enum {
                return Some(enum_name.clone());
            }
            current = meta.parent.as_deref();
        }
        None
    }

    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.types
            .values()
            .any(|ty| ty.id_as_enum.as_deref() == Some(enum_name))
    }

    pub fn key_field_type(&self, type_name: &str) -> FieldType {
        self.id_as_enum(type_name)
            .map_or(FieldType::String, FieldType::Enum)
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
        for ty in self.types.values() {
            let mut visited = BTreeSet::new();
            ty.collect_ref_targets(self, &mut out, &mut visited);
        }
        out.into_iter().collect()
    }
}

#[derive(Debug, Clone)]
pub struct TypeMeta {
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub is_struct: bool,
    pub id_as_enum: Option<String>,
    pub own_fields: Vec<FieldMeta>,
    pub all_fields: Vec<FieldMeta>,
}

impl TypeMeta {
    fn from_schema_view(schema_type: &CftTypeMeta, schema: &CftSchemaView) -> Self {
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            is_sealed: schema_type.is_sealed,
            is_singleton: schema_type.is_singleton,
            is_struct: has_annotation(&schema_type.annotations, "struct"),
            id_as_enum: annotation_name_arg(&schema_type.annotations, "idAsEnum"),
            own_fields: schema_type
                .own_fields
                .iter()
                .map(|field| FieldMeta::from_schema_view(field, schema))
                .collect(),
            all_fields: schema_type
                .all_fields
                .iter()
                .map(|field| FieldMeta::from_schema_view(field, schema))
                .collect(),
        }
    }

    fn collect_ref_targets(
        &self,
        view: &SchemaView,
        out: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
    ) {
        if !visited.insert(self.name.clone()) {
            return;
        }
        for field in &self.all_fields {
            collect_ref_targets_in_field(view, field, out, visited);
        }
    }
}

fn collect_ref_targets_in_field(
    view: &SchemaView,
    field: &FieldMeta,
    out: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) {
    collect_ref_targets_in_type(view, &field.ty, out, visited);
}

fn collect_ref_targets_in_type(
    view: &SchemaView,
    ty: &FieldType,
    out: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) {
    match ty {
        FieldType::Type(name) => {
            if let Some(meta) = view.types.get(name) {
                meta.collect_ref_targets(view, out, visited);
            }
        }
        FieldType::Ref(name) => {
            out.insert(name.clone());
        }
        FieldType::Array(inner) | FieldType::Nullable(inner) => {
            collect_ref_targets_in_type(view, inner, out, visited);
        }
        FieldType::Dict(_, value) => collect_ref_targets_in_type(view, value, out, visited),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => {}
    }
}

#[derive(Debug, Clone)]
pub struct FieldMeta {
    pub name: String,
    pub ty: FieldType,
    pub default: Option<CftSchemaDefaultValue>,
    pub is_dimensional: bool,
}

impl FieldMeta {
    pub fn from_schema_view(field: &coflow_cft::CftFieldMeta, schema: &CftSchemaView) -> Self {
        Self {
            name: field.name.clone(),
            ty: FieldType::from_schema(&field.ty_ref, schema),
            default: field.default.clone(),
            is_dimensional: field.dimension.is_some(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Ref(String),
    Enum(String),
    Array(Box<Self>),
    Dict(Box<Self>, Box<Self>),
    Nullable(Box<Self>),
}

impl FieldType {
    pub fn from_schema(ty: &CftSchemaTypeRef, schema: &CftSchemaView) -> Self {
        match ty {
            CftSchemaTypeRef::Int => Self::Int,
            CftSchemaTypeRef::Float => Self::Float,
            CftSchemaTypeRef::Bool => Self::Bool,
            CftSchemaTypeRef::String => Self::String,
            CftSchemaTypeRef::Named(name) if schema.enums.contains_key(name) => {
                Self::Enum(name.clone())
            }
            CftSchemaTypeRef::Named(name) => Self::Type(name.clone()),
            CftSchemaTypeRef::Ref(name) => Self::Ref(name.clone()),
            CftSchemaTypeRef::Array(inner) => {
                Self::Array(Box::new(Self::from_schema(inner, schema)))
            }
            CftSchemaTypeRef::Dict(key, value) => Self::Dict(
                Box::new(Self::from_schema(key, schema)),
                Box::new(Self::from_schema(value, schema)),
            ),
            CftSchemaTypeRef::Nullable(inner) => {
                Self::Nullable(Box::new(Self::from_schema(inner, schema)))
            }
        }
    }

    pub const fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    pub fn non_nullable(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner.non_nullable(),
            other => other,
        }
    }
}
