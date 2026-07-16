use crate::model::{CfdDomainId, CfdDomainIndex, CfdTypeId};
use coflow_cft::{CftEnumValue, CftField, CftSchema, CftType, TypeName};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(crate) struct BuildSchema<'a> {
    cft: &'a CftSchema,
    domain_index: CfdDomainIndex,
}

impl<'a> BuildSchema<'a> {
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
            .map(|ty| ty.name.clone())
            .collect::<Vec<_>>();
        let type_id_by_name = type_names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), CfdTypeId::new(index)))
            .collect::<BTreeMap<_, _>>();

        let mut domain_id_by_root = BTreeMap::<TypeName, CfdDomainId>::new();
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

    fn domain_root_name(cft_view: &CftSchema, type_name: &TypeName) -> TypeName {
        let mut current = type_name;
        while let Some(parent) = cft_view
            .resolve_type(current.as_str())
            .and_then(|meta| meta.parent.as_ref())
        {
            current = parent;
        }
        current.clone()
    }

    fn ancestor_type_ids(
        cft_view: &CftSchema,
        type_id_by_name: &BTreeMap<TypeName, CfdTypeId>,
        type_name: &TypeName,
    ) -> Vec<CfdTypeId> {
        let mut out = Vec::new();
        let mut current = type_name;
        while let Some(parent) = cft_view
            .resolve_type(current.as_str())
            .and_then(|meta| meta.parent.as_ref())
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

    pub(crate) fn assignable_target_names(&self, actual_type: &str) -> Vec<TypeName> {
        self.cft.assignable_target_names(actual_type)
    }

    pub(crate) fn enum_value(&self, enum_name: &str, variant: &str) -> Option<CftEnumValue> {
        let value = self.cft.enum_variant_value(enum_name, variant)?;
        self.cft.enum_value_from_int(enum_name, value)
    }

    pub(crate) fn singleton_types(&self) -> impl Iterator<Item = &CftType> {
        self.cft.singleton_types()
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
