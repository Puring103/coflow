use crate::model::{CfdDictKey, CfdDomainId, CfdDomainIndex, CfdInputValue, CfdTypeId, CfdValue};
use crate::origin::RecordOrigin;
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
    /// Origin moved from `CfdInputRecord`. For nested object drafts (created
    /// inside fields), defaults to `RecordOrigin::None`.
    pub(crate) origin: RecordOrigin,
    /// Object-level spread occurrences at this record/object site. Kept even
    /// when local fields override every imported field so source rewrites can
    /// still target the spread token.
    pub(crate) spread_sources: Vec<SpreadFieldSource>,
    /// Top-level only: which fields came from `...spread` references and
    /// where they came from (target type + key, since record id resolution
    /// happens later). Empty for nested objects.
    pub(crate) spread_field_sources: BTreeMap<String, SpreadFieldSource>,
}

/// A spread origin captured during validation. The compiler resolves these to
/// concrete `CfdRecordId` values once all drafts have been indexed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SpreadFieldSource {
    pub(crate) expected_type: String,
    pub(crate) key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CfdValueDraft {
    Value(CfdValue),
    Object(Box<RecordDraft>),
    PendingRef {
        expected_type: String,
        key: String,
    },
    PendingSpreadField {
        source_type: String,
        key: String,
        field: String,
    },
    Array(Vec<CfdValueDraft>),
    Dict(Vec<(CfdDictKey, CfdValueDraft)>),
    DictSpread {
        spreads: Vec<CfdValueDraft>,
        entries: Vec<(CfdDictKey, CfdValueDraft)>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
    children: BTreeMap<String, BTreeSet<String>>,
    domain_index: CfdDomainIndex,
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

        let domain_index = Self::build_domain_index(&types);

        Self {
            types,
            enums,
            children,
            domain_index,
        }
    }

    fn build_domain_index(types: &BTreeMap<String, TypeMeta>) -> CfdDomainIndex {
        let type_names = types.keys().cloned().collect::<Vec<_>>();
        let type_id_by_name = type_names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), CfdTypeId::new(index)))
            .collect::<BTreeMap<_, _>>();

        let mut domain_id_by_root = BTreeMap::<String, CfdDomainId>::new();
        let mut type_domain = vec![CfdDomainId::new(0); type_names.len()];
        let mut domain_members = Vec::<Vec<CfdTypeId>>::new();
        let mut ancestors_by_type = vec![Vec::new(); type_names.len()];

        for (index, type_name) in type_names.iter().enumerate() {
            let root = Self::domain_root_name(types, type_name);
            let next_domain_id = CfdDomainId::new(domain_id_by_root.len());
            let domain_id = *domain_id_by_root.entry(root).or_insert(next_domain_id);
            if domain_members.len() <= domain_id.index() {
                domain_members.push(Vec::new());
            }
            let type_id = CfdTypeId::new(index);
            type_domain[index] = domain_id;
            domain_members[domain_id.index()].push(type_id);
            ancestors_by_type[index] = Self::ancestor_type_ids(types, &type_id_by_name, type_name);
        }

        CfdDomainIndex::new(
            type_id_by_name,
            type_names,
            type_domain,
            domain_members,
            ancestors_by_type,
        )
    }

    fn domain_root_name(types: &BTreeMap<String, TypeMeta>, type_name: &str) -> String {
        let mut current = type_name;
        while let Some(parent) = types.get(current).and_then(|meta| meta.parent.as_deref()) {
            current = parent;
        }
        current.to_string()
    }

    fn ancestor_type_ids(
        types: &BTreeMap<String, TypeMeta>,
        type_id_by_name: &BTreeMap<String, CfdTypeId>,
        type_name: &str,
    ) -> Vec<CfdTypeId> {
        let mut out = Vec::new();
        let mut current = type_name;
        while let Some(parent) = types.get(current).and_then(|meta| meta.parent.as_deref()) {
            if let Some(type_id) = type_id_by_name.get(parent).copied() {
                out.push(type_id);
            }
            current = parent;
        }
        out
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

    pub(crate) fn singleton_types(&self) -> impl Iterator<Item = &TypeMeta> {
        self.types.values().filter(|meta| meta.is_singleton)
    }

    pub(crate) fn type_contains_singleton(&self, ty: &CfdType) -> bool {
        match ty {
            CfdType::Type(name) | CfdType::Ref(name) => {
                self.types.get(name).is_some_and(|meta| meta.is_singleton)
            }
            CfdType::Array(inner) | CfdType::Nullable(inner) => self.type_contains_singleton(inner),
            CfdType::Dict(key, value) => {
                self.type_contains_singleton(key) || self.type_contains_singleton(value)
            }
            CfdType::Int | CfdType::Float | CfdType::Bool | CfdType::String | CfdType::Enum(_) => {
                false
            }
        }
    }

    pub(crate) fn type_name_is_singleton(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_singleton)
    }

    pub(crate) fn domain_index(&self) -> &CfdDomainIndex {
        &self.domain_index
    }

    pub(crate) fn type_id(&self, type_name: &str) -> Option<CfdTypeId> {
        self.domain_index.type_id(type_name)
    }

    pub(crate) fn type_domain_id(&self, type_name: &str) -> Option<CfdDomainId> {
        self.domain_index.type_domain_by_name(type_name)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    pub(crate) name: String,
    pub(crate) parent: Option<String>,
    pub(crate) is_abstract: bool,
    pub(crate) is_singleton: bool,
    fields: Vec<FieldMeta>,
}

impl TypeMeta {
    fn from_schema(schema: &CftContainer, schema_type: &CftSchemaType) -> Self {
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            is_abstract: schema_type.is_abstract,
            is_singleton: schema_type.is_singleton,
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
    Ref(String),
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
            Self::Ref(name) => format!("&{name}"),
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
        CfdInputValue::ObjectSpread { .. } => "object spread",
        CfdInputValue::RecordRef(_) => "record ref",
        CfdInputValue::Array(_) => "array",
        CfdInputValue::Dict(_) => "dict",
        CfdInputValue::DictSpread { .. } => "dict spread",
    }
}
