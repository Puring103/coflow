use crate::model::{CfdDictKey, CfdDomainId, CfdDomainIndex, CfdInputValue, CfdTypeId, CfdValue};
use crate::origin::RecordOrigin;
use coflow_cft::{
    CftField, CftSchema, CftSchemaTypeRef, CftType, ValueDependencyCycle, ValueDependencyMode,
};
use std::collections::BTreeMap;

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
pub(crate) struct DataModelCompilerContext<'a> {
    cft: &'a CftSchema,
    domain_index: CfdDomainIndex,
}

impl<'a> DataModelCompilerContext<'a> {
    pub(crate) fn new(schema: &'a CftSchema) -> Self {
        let domain_index = Self::build_domain_index(schema);

        Self {
            cft: schema,
            domain_index,
        }
    }

    pub(crate) const fn cft(&self) -> &'a CftSchema {
        self.cft
    }

    fn build_domain_index(cft_view: &CftSchema) -> CfdDomainIndex {
        let type_names = cft_view
            .all_types()
            .map(|ty| ty.name.to_string())
            .collect::<Vec<_>>();
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
            let root = Self::domain_root_name(cft_view, type_name);
            let next_domain_id = CfdDomainId::new(domain_id_by_root.len());
            let domain_id = *domain_id_by_root.entry(root).or_insert(next_domain_id);
            if domain_members.len() <= domain_id.index() {
                domain_members.push(Vec::new());
            }
            let type_id = CfdTypeId::new(index);
            type_domain[index] = domain_id;
            domain_members[domain_id.index()].push(type_id);
            ancestors_by_type[index] =
                Self::ancestor_type_ids(cft_view, &type_id_by_name, type_name);
        }

        CfdDomainIndex::new(
            type_id_by_name,
            type_names,
            type_domain,
            domain_members,
            ancestors_by_type,
        )
    }

    fn domain_root_name(cft_view: &CftSchema, type_name: &str) -> String {
        let mut current = type_name;
        while let Some(parent) = cft_view
            .resolve_type(current)
            .and_then(|meta| meta.parent.as_deref())
        {
            current = parent;
        }
        current.to_string()
    }

    fn ancestor_type_ids(
        cft_view: &CftSchema,
        type_id_by_name: &BTreeMap<String, CfdTypeId>,
        type_name: &str,
    ) -> Vec<CfdTypeId> {
        let mut out = Vec::new();
        let mut current = type_name;
        while let Some(parent) = cft_view
            .resolve_type(current)
            .and_then(|meta| meta.parent.as_deref())
        {
            if let Some(type_id) = type_id_by_name.get(parent).copied() {
                out.push(type_id);
            }
            current = parent;
        }
        out
    }

    pub(crate) fn resolve_type(&self, type_name: &str) -> Option<&CftType> {
        self.cft.resolve_type(type_name)
    }

    pub(crate) fn full_fields(&self, type_name: &str) -> impl Iterator<Item = &CftField> {
        self.cft
            .resolve_type(type_name)
            .into_iter()
            .flat_map(CftType::all_fields)
    }

    pub(crate) fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        self.cft.is_assignable(actual_type, expected_type)
    }

    pub(crate) fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.cft.range_is_polymorphic(type_name)
    }

    pub(crate) fn assignable_target_names(&self, actual_type: &str) -> Vec<String> {
        self.cft
            .assignable_target_names(actual_type)
            .into_iter()
            .map(|name| name.to_string())
            .collect()
    }

    pub(crate) fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.cft.enum_variant_value(enum_name, variant)
    }

    pub(crate) fn singleton_types(&self) -> impl Iterator<Item = &CftType> {
        self.cft.singleton_types()
    }

    pub(crate) fn schema_default_cycle(&self, type_name: &str) -> Option<ValueDependencyCycle> {
        self.cft
            .value_dependencies()
            .materialization_order(type_name, ValueDependencyMode::SchemaDefaults)?
            .err()
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

pub(crate) fn type_accepts_default(expected: &CftSchemaTypeRef, actual: &CftSchemaTypeRef) -> bool {
    match expected {
        CftSchemaTypeRef::Nullable(inner) => type_accepts_default(inner, actual),
        _ => expected == actual,
    }
}

pub(crate) fn display_type_ref(ty: &CftSchemaTypeRef) -> String {
    ty.display_label()
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
