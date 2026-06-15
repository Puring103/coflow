use crate::model::{CfdDictKey, CfdInputValue, CfdRefPathSegment, CfdValue};
use coflow_cft::{
    CftContainer, CftSchemaDefaultValue, CftSchemaEnum, CftSchemaField, CftSchemaType,
    CftSchemaTypeRef,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordDraft {
    pub(crate) key: String,
    pub(crate) actual_type: String,
    pub(crate) fields: BTreeMap<String, CfdValueDraft>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CfdValueDraft {
    Value(CfdValue),
    Object(Box<RecordDraft>),
    PendingRef {
        target_type: String,
        key: String,
    },
    PathRef {
        expected_type: CfdType,
        target_type: String,
        key: String,
        segments: Vec<CfdRefPathSegment>,
    },
    Array(Vec<CfdValueDraft>),
    Dict(Vec<(CfdDictKey, CfdValueDraft)>),
}

#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl SchemaView {
    pub(crate) fn new(schema: &CftContainer) -> Self {
        let enums = schema
            .all_enums()
            .map(|schema_enum| (schema_enum.name.clone(), EnumMeta::from_schema(schema_enum)))
            .collect::<BTreeMap<_, _>>();

        let mut types = BTreeMap::new();
        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for schema_type in schema.all_types() {
            let meta = TypeMeta::from_schema(schema, schema_type);
            if let Some(parent) = &meta.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert(meta.name.clone());
            }
            types.insert(meta.name.clone(), meta);
        }

        Self {
            types,
            enums,
            children,
        }
    }

    pub(crate) fn full_fields(&self, type_name: &str) -> &[FieldMeta] {
        self.types.get(type_name).map_or(&[], |meta| &meta.fields)
    }

    pub(crate) fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        false
    }

    pub(crate) fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_abstract || self.has_descendants(type_name))
    }

    fn has_descendants(&self, type_name: &str) -> bool {
        self.children
            .get(type_name)
            .is_some_and(|children| !children.is_empty())
    }

    pub(crate) fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            out.push(name.to_string());
            current = self.types.get(name).and_then(|meta| meta.parent.as_deref());
        }
        out
    }

    pub(crate) fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) is_abstract: bool,
    fields: Vec<FieldMeta>,
}

impl TypeMeta {
    fn from_schema(schema: &CftContainer, schema_type: &CftSchemaType) -> Self {
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            fields: schema_type
                .all_fields
                .iter()
                .map(|field| FieldMeta::from_schema(schema, field))
                .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct FieldMeta {
    pub(crate) name: String,
    pub(crate) ty: CfdType,
    pub(crate) default: Option<CftSchemaDefaultValue>,
}

impl FieldMeta {
    fn from_schema(schema: &CftContainer, field: &CftSchemaField) -> Self {
        Self {
            name: field.name.clone(),
            ty: CfdType::from_schema(&field.ty_ref, schema),
            default: field.default.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EnumMeta {
    pub(crate) variants: BTreeMap<String, i64>,
}

impl EnumMeta {
    fn from_schema(schema_enum: &CftSchemaEnum) -> Self {
        Self {
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| (variant.name.clone(), variant.value))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CfdType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Enum(String),
    Array(Box<CfdType>),
    Dict(Box<CfdType>, Box<CfdType>),
    Nullable(Box<CfdType>),
}

impl CfdType {
    fn from_schema(ty: &CftSchemaTypeRef, schema: &CftContainer) -> Self {
        match ty {
            CftSchemaTypeRef::Int => Self::Int,
            CftSchemaTypeRef::Float => Self::Float,
            CftSchemaTypeRef::Bool => Self::Bool,
            CftSchemaTypeRef::String => Self::String,
            CftSchemaTypeRef::Named(name) if schema.has_enum(name) => Self::Enum(name.clone()),
            CftSchemaTypeRef::Named(name) => Self::Type(name.clone()),
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

    pub(crate) fn is_nullable(&self) -> bool {
        matches!(self, Self::Nullable(_))
    }

    pub(crate) fn display(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Type(name) | Self::Enum(name) => name.clone(),
            Self::Array(inner) => format!("[{}]", inner.display()),
            Self::Dict(key, value) => format!("{{{}: {}}}", key.display(), value.display()),
            Self::Nullable(inner) => format!("{}?", inner.display()),
        }
    }
}

pub(crate) fn type_accepts_default(expected: &CfdType, actual: &CfdType) -> bool {
    match expected {
        CfdType::Nullable(inner) => type_accepts_default(inner, actual),
        _ => expected == actual,
    }
}

pub(crate) fn input_value_kind(value: &CfdInputValue) -> &'static str {
    match value {
        CfdInputValue::Null => "null",
        CfdInputValue::Bool(_) => "bool",
        CfdInputValue::Int(_) => "int",
        CfdInputValue::Float(_) => "float",
        CfdInputValue::String(_) => "string",
        CfdInputValue::EnumVariant { .. } => "enum",
        CfdInputValue::Object { .. } => "object",
        CfdInputValue::RecordRef { .. } => "record ref",
        CfdInputValue::PathRef { .. } => "path ref",
        CfdInputValue::Array(_) => "array",
        CfdInputValue::Dict(_) => "dict",
    }
}
