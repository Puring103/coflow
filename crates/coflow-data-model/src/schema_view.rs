use crate::model::{CfdDictKey, CfdDomainId, CfdDomainIndex, CfdInputValue, CfdTypeId, CfdValue};
use crate::origin::RecordOrigin;
use coflow_cft::{
    CftAnnotationValue, CftContainer, CftSchemaDefaultValue, CftSchemaTypeRef, CftSchemaView,
    CftTypeMeta,
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
    pub(crate) types: BTreeMap<String, CftTypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
    fields: BTreeMap<String, Vec<FieldMeta>>,
    children: BTreeMap<String, BTreeSet<String>>,
    dimension_storage_types: BTreeMap<DimensionStorageKey, String>,
    domain_index: CfdDomainIndex,
}

impl SchemaView {
    pub(crate) fn new(schema: &CftContainer) -> Self {
        let cft_view = CftSchemaView::new(schema);
        let enums = cft_view
            .enums
            .iter()
            .map(|(name, schema_enum)| {
                (
                    name.clone(),
                    EnumMeta {
                        variants: schema_enum.variants.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = cft_view.types.clone();
        let mut fields = BTreeMap::new();
        let mut children = BTreeMap::<String, BTreeSet<String>>::new();
        for schema_type in cft_view.types.values() {
            if let Some(parent) = &schema_type.parent {
                children
                    .entry(parent.clone())
                    .or_default()
                    .insert(schema_type.name.clone());
            }
            fields.insert(
                schema_type.name.clone(),
                schema_type
                    .all_fields
                    .iter()
                    .map(|field| FieldMeta::from_schema_view(&cft_view, field))
                    .collect(),
            );
        }

        let domain_index = Self::build_domain_index(&types);
        let dimension_storage_types = Self::build_dimension_storage_index(&cft_view);

        Self {
            types,
            enums,
            fields,
            children,
            dimension_storage_types,
            domain_index,
        }
    }

    fn build_dimension_storage_index(
        cft_view: &CftSchemaView,
    ) -> BTreeMap<DimensionStorageKey, String> {
        let mut out = BTreeMap::new();
        for schema_type in cft_view.types.values() {
            for annotation in &schema_type.annotations {
                if annotation.name != "__coflow_dimension_storage" {
                    continue;
                }
                if let [CftAnnotationValue::String(dimension), CftAnnotationValue::String(source_type), CftAnnotationValue::String(source_field)] =
                    annotation.args.as_slice()
                {
                    out.insert(
                        DimensionStorageKey {
                            dimension: dimension.clone(),
                            source_type: source_type.clone(),
                            source_field: source_field.clone(),
                        },
                        schema_type.name.clone(),
                    );
                }
            }
        }
        out
    }

    pub(crate) fn dimension_storage_type(
        &self,
        dimension: &str,
        source_type: &str,
        source_field: &str,
    ) -> Option<&str> {
        self.dimension_storage_types
            .get(&DimensionStorageKey {
                dimension: dimension.to_string(),
                source_type: source_type.to_string(),
                source_field: source_field.to_string(),
            })
            .map(String::as_str)
    }

    pub(crate) fn field_meta(&self, type_name: &str, field_name: &str) -> Option<&FieldMeta> {
        self.fields
            .get(type_name)?
            .iter()
            .find(|field| field.name == field_name)
    }

    fn build_domain_index(types: &BTreeMap<String, CftTypeMeta>) -> CfdDomainIndex {
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

    fn domain_root_name(types: &BTreeMap<String, CftTypeMeta>, type_name: &str) -> String {
        let mut current = type_name;
        while let Some(parent) = types.get(current).and_then(|meta| meta.parent.as_deref()) {
            current = parent;
        }
        current.to_string()
    }

    fn ancestor_type_ids(
        types: &BTreeMap<String, CftTypeMeta>,
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
        self.fields.get(type_name).map_or(&[], Vec::as_slice)
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

    pub(crate) fn singleton_types(&self) -> impl Iterator<Item = &CftTypeMeta> {
        self.types.values().filter(|meta| meta.is_singleton)
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DimensionStorageKey {
    dimension: String,
    source_type: String,
    source_field: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldMeta {
    pub(crate) name: String,
    pub(crate) ty: CfdType,
    pub(crate) ty_ref: CftSchemaTypeRef,
    pub(crate) default: Option<CftSchemaDefaultValue>,
    pub(crate) dimension: Option<String>,
}

impl FieldMeta {
    fn from_schema_view(schema: &CftSchemaView, field: &coflow_cft::CftFieldMeta) -> Self {
        Self {
            name: field.name.clone(),
            ty: CfdType::from_schema(&field.ty_ref, schema),
            ty_ref: field.ty_ref.clone(),
            default: field.default.clone(),
            dimension: field.dimension.as_ref().map(|dimension| dimension.dimension.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EnumMeta {
    pub(crate) variants: BTreeMap<String, i64>,
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
    fn from_schema(ty: &CftSchemaTypeRef, schema: &CftSchemaView) -> Self {
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
