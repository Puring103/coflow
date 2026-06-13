use crate::names::{annotation_name_arg, annotation_string_arg, csharp_type_name, has_annotation};
use crate::CsharpCodegenError;
use coflow_cft::{
    CftAnnotation, CftContainer, CftSchemaDefaultValue, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct SchemaView {
    pub types: BTreeMap<String, TypeMeta>,
    pub enums: BTreeSet<String>,
    children: BTreeMap<String, BTreeSet<String>>,
    csharp_types: BTreeMap<String, String>,
    csharp_enums: BTreeMap<String, String>,
}

impl SchemaView {
    pub fn new(schema: &CftContainer) -> Self {
        let enums = schema
            .all_enums()
            .map(|schema_enum| schema_enum.name.clone())
            .collect::<BTreeSet<_>>();

        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for schema_type in schema.all_types() {
            if let Some(parent) = &schema_type.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert(schema_type.name.clone());
            }
        }

        let types = schema
            .all_types()
            .map(|schema_type| {
                (
                    schema_type.name.clone(),
                    TypeMeta::from_schema(schema_type, &enums),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let csharp_types = schema
            .all_types()
            .map(|schema_type| {
                (
                    schema_type.name.clone(),
                    csharp_type_name(&schema_type.name),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let csharp_enums = schema
            .all_enums()
            .map(|schema_enum| {
                (
                    schema_enum.name.clone(),
                    csharp_type_name(&schema_enum.name),
                )
            })
            .collect::<BTreeMap<_, _>>();

        Self {
            types,
            enums,
            children,
            csharp_types,
            csharp_enums,
        }
    }

    pub fn type_meta(&self, name: &str) -> Result<&TypeMeta, CsharpCodegenError> {
        self.types
            .get(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> Vec<String> {
        self.types.keys().cloned().collect()
    }

    pub fn non_abstract_type_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| !ty.is_abstract)
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub fn table_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| !ty.is_abstract && ty.id_field().is_ok())
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

    pub fn type_is_struct(&self, type_name: &str) -> bool {
        self.types.get(type_name).is_some_and(|ty| ty.is_struct)
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
        if self.enums.contains(name) {
            self.csharp_enum_name(name)
        } else {
            self.csharp_type_name(name)
        }
    }

    pub fn ref_target_id_csharp_enum_override(&self, target: &str) -> Option<String> {
        self.types
            .get(target)?
            .id_field()
            .ok()?
            .csharp_enum_override
            .clone()
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
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

    pub fn range_contains_ref(&self, type_name: &str) -> bool {
        let mut visited = BTreeSet::new();
        self.range_contains_ref_inner(type_name, &mut visited)
    }

    fn range_contains_ref_inner(&self, type_name: &str, visited: &mut BTreeSet<String>) -> bool {
        if !visited.insert(type_name.to_string()) {
            return false;
        }
        let Some(meta) = self.types.get(type_name) else {
            return false;
        };
        if meta.contains_ref(self) {
            return true;
        }
        self.children
            .get(type_name)
            .into_iter()
            .flatten()
            .any(|child| self.range_contains_ref_inner(child, visited))
    }
}

#[derive(Debug, Clone)]
pub struct TypeMeta {
    pub name: String,
    pub is_abstract: bool,
    pub is_struct: bool,
    pub all_fields: Vec<FieldMeta>,
}

impl TypeMeta {
    fn from_schema(schema_type: &CftSchemaType, enums: &BTreeSet<String>) -> Self {
        Self {
            name: schema_type.name.clone(),
            is_abstract: schema_type.is_abstract,
            is_struct: has_annotation(&schema_type.annotations, "struct"),
            all_fields: schema_type
                .all_fields
                .iter()
                .map(|field| FieldMeta::from_schema(field, enums))
                .collect(),
        }
    }

    pub fn id_field(&self) -> Result<&FieldMeta, CsharpCodegenError> {
        self.all_fields
            .iter()
            .find(|field| has_annotation(&field.annotations, "id"))
            .ok_or_else(|| {
                CsharpCodegenError::new(format!("type `{}` has no @id field", self.name))
            })
    }

    pub fn index_fields(&self) -> impl Iterator<Item = &FieldMeta> {
        self.all_fields
            .iter()
            .filter(|field| has_annotation(&field.annotations, "index"))
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

    fn contains_ref(&self, view: &SchemaView) -> bool {
        let mut refs = BTreeSet::new();
        let mut visited = BTreeSet::new();
        self.collect_ref_targets(view, &mut refs, &mut visited);
        !refs.is_empty()
    }
}

fn collect_ref_targets_in_field(
    view: &SchemaView,
    field: &FieldMeta,
    out: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) {
    if let Some(target) = annotation_name_arg(&field.annotations, "ref") {
        out.insert(target);
        return;
    }
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
    pub has_default: bool,
    pub default: Option<CftSchemaDefaultValue>,
    pub annotations: Vec<CftAnnotation>,
    pub csharp_enum_override: Option<String>,
}

impl FieldMeta {
    pub fn from_schema(field: &CftSchemaField, enums: &BTreeSet<String>) -> Self {
        Self {
            name: field.name.clone(),
            ty: FieldType::from_schema(&field.ty_ref, enums),
            has_default: field.has_default,
            default: field.default.clone(),
            annotations: field.annotations.clone(),
            csharp_enum_override: annotation_string_arg(&field.annotations, "IdAsEnum")
                .or_else(|| annotation_string_arg(&field.annotations, "GenAsEnum")),
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
    Enum(String),
    Array(Box<Self>),
    Dict(Box<Self>, Box<Self>),
    Nullable(Box<Self>),
}

impl FieldType {
    pub fn from_schema(ty: &CftSchemaTypeRef, enums: &BTreeSet<String>) -> Self {
        match ty {
            CftSchemaTypeRef::Int => Self::Int,
            CftSchemaTypeRef::Float => Self::Float,
            CftSchemaTypeRef::Bool => Self::Bool,
            CftSchemaTypeRef::String => Self::String,
            CftSchemaTypeRef::Named(name) if enums.contains(name) => Self::Enum(name.clone()),
            CftSchemaTypeRef::Named(name) => Self::Type(name.clone()),
            CftSchemaTypeRef::Array(inner) => {
                Self::Array(Box::new(Self::from_schema(inner, enums)))
            }
            CftSchemaTypeRef::Dict(key, value) => Self::Dict(
                Box::new(Self::from_schema(key, enums)),
                Box::new(Self::from_schema(value, enums)),
            ),
            CftSchemaTypeRef::Nullable(inner) => {
                Self::Nullable(Box::new(Self::from_schema(inner, enums)))
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
