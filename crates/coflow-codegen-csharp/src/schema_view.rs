use crate::names::{annotation_name_arg, has_annotation};
use crate::CsharpCodegenError;
use coflow_cft::{CftAnnotation, CftContainer, CftSchemaField, CftSchemaType, CftSchemaTypeRef};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeSet<String>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl SchemaView {
    pub(crate) fn new(schema: &CftContainer) -> Self {
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

        Self {
            types,
            enums,
            children,
        }
    }

    pub(crate) fn type_meta(&self, name: &str) -> Result<&TypeMeta, CsharpCodegenError> {
        self.types
            .get(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub(crate) fn all_type_names(&self) -> Vec<String> {
        self.types.keys().cloned().collect()
    }

    pub(crate) fn non_abstract_type_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| !ty.is_abstract)
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub(crate) fn table_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| !ty.is_abstract && ty.id_field().is_ok())
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub(crate) fn polymorphic_type_names(&self) -> Vec<String> {
        self.types
            .values()
            .filter(|ty| self.range_is_polymorphic(&ty.name))
            .map(|ty| ty.name.clone())
            .collect()
    }

    pub(crate) fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|ty| ty.is_abstract || self.has_descendants(type_name))
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub(crate) fn concrete_assignable_types(
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

    pub(crate) fn ref_target_names(&self) -> Vec<String> {
        let mut out = BTreeSet::new();
        for ty in self.types.values() {
            ty.collect_ref_targets(self, &mut out);
        }
        out.into_iter().collect()
    }

    pub(crate) fn range_contains_ref(&self, type_name: &str) -> bool {
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
            .any(|child| self.range_contains_ref(child))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    pub(crate) name: String,
    pub(crate) is_abstract: bool,
    pub(crate) all_fields: Vec<FieldMeta>,
}

impl TypeMeta {
    fn from_schema(schema_type: &CftSchemaType, enums: &BTreeSet<String>) -> Self {
        Self {
            name: schema_type.name.clone(),
            is_abstract: schema_type.is_abstract,
            all_fields: schema_type
                .all_fields
                .iter()
                .map(|field| FieldMeta::from_schema(field, enums))
                .collect(),
        }
    }

    pub(crate) fn id_field(&self) -> Result<&FieldMeta, CsharpCodegenError> {
        self.all_fields
            .iter()
            .find(|field| has_annotation(&field.annotations, "id"))
            .ok_or_else(|| {
                CsharpCodegenError::new(format!("type `{}` has no @id field", self.name))
            })
    }

    pub(crate) fn index_fields(&self) -> impl Iterator<Item = &FieldMeta> {
        self.all_fields
            .iter()
            .filter(|field| has_annotation(&field.annotations, "index"))
    }

    fn collect_ref_targets(&self, view: &SchemaView, out: &mut BTreeSet<String>) {
        for field in &self.all_fields {
            collect_ref_targets_in_field(view, field, out);
        }
    }

    fn contains_ref(&self, view: &SchemaView) -> bool {
        let mut refs = BTreeSet::new();
        self.collect_ref_targets(view, &mut refs);
        !refs.is_empty()
    }
}

fn collect_ref_targets_in_field(view: &SchemaView, field: &FieldMeta, out: &mut BTreeSet<String>) {
    if let Some(target) = annotation_name_arg(&field.annotations, "ref") {
        out.insert(target);
        return;
    }
    collect_ref_targets_in_type(view, &field.ty, out);
}

fn collect_ref_targets_in_type(view: &SchemaView, ty: &FieldType, out: &mut BTreeSet<String>) {
    match ty {
        FieldType::Type(name) => {
            if let Some(meta) = view.types.get(name) {
                meta.collect_ref_targets(view, out);
            }
        }
        FieldType::Array(inner) | FieldType::Nullable(inner) => {
            collect_ref_targets_in_type(view, inner, out);
        }
        FieldType::Dict(_, value) => collect_ref_targets_in_type(view, value, out),
        FieldType::Int
        | FieldType::Float
        | FieldType::Bool
        | FieldType::String
        | FieldType::Enum(_) => {}
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FieldMeta {
    pub(crate) name: String,
    pub(crate) ty: FieldType,
    pub(crate) annotations: Vec<CftAnnotation>,
}

impl FieldMeta {
    fn from_schema(field: &CftSchemaField, enums: &BTreeSet<String>) -> Self {
        Self {
            name: field.name.clone(),
            ty: FieldType::from_schema(&field.ty_ref, enums),
            annotations: field.annotations.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Enum(String),
    Array(Box<FieldType>),
    Dict(Box<FieldType>, Box<FieldType>),
    Nullable(Box<FieldType>),
}

impl FieldType {
    pub(crate) fn from_schema(ty: &CftSchemaTypeRef, enums: &BTreeSet<String>) -> Self {
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

    pub(crate) fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    pub(crate) fn non_nullable(&self) -> &Self {
        match self {
            Self::Nullable(inner) => inner.non_nullable(),
            other => other,
        }
    }
}
