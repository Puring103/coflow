use super::ids::{CfdDomainId, CfdTypeId};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct CfdDomainIndex {
    pub(crate) type_id_by_name: BTreeMap<String, CfdTypeId>,
    pub(crate) type_names: Vec<String>,
    pub(crate) type_domain: Vec<CfdDomainId>,
    pub(crate) domain_members: Vec<Vec<CfdTypeId>>,
    pub(crate) ancestors_by_type: Vec<Vec<CfdTypeId>>,
}

impl CfdDomainIndex {
    #[must_use]
    pub(crate) fn new(
        type_id_by_name: BTreeMap<String, CfdTypeId>,
        type_names: Vec<String>,
        type_domain: Vec<CfdDomainId>,
        domain_members: Vec<Vec<CfdTypeId>>,
        ancestors_by_type: Vec<Vec<CfdTypeId>>,
    ) -> Self {
        Self {
            type_id_by_name,
            type_names,
            type_domain,
            domain_members,
            ancestors_by_type,
        }
    }

    #[must_use]
    pub(crate) fn type_id(&self, type_name: &str) -> Option<CfdTypeId> {
        self.type_id_by_name.get(type_name).copied()
    }

    #[must_use]
    pub(crate) fn type_name(&self, type_id: CfdTypeId) -> Option<&str> {
        self.type_names.get(type_id.index()).map(String::as_str)
    }

    #[must_use]
    pub(crate) fn type_domain(&self, type_id: CfdTypeId) -> Option<CfdDomainId> {
        self.type_domain.get(type_id.index()).copied()
    }

    #[must_use]
    pub(crate) fn type_domain_by_name(&self, type_name: &str) -> Option<CfdDomainId> {
        self.type_domain(self.type_id(type_name)?)
    }

    #[must_use]
    pub(crate) fn domain_members(&self, domain_id: CfdDomainId) -> Option<&[CfdTypeId]> {
        self.domain_members
            .get(domain_id.index())
            .map(Vec::as_slice)
    }

    #[must_use]
    pub(crate) fn type_ancestors(&self, type_id: CfdTypeId) -> Option<&[CfdTypeId]> {
        self.ancestors_by_type
            .get(type_id.index())
            .map(Vec::as_slice)
    }
}
