use coflow_cft::{CftEnumValue, CftField, CftSchema, CftType, TypeName};

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuildSchema<'a> {
    cft: &'a CftSchema,
}

impl<'a> BuildSchema<'a> {
    pub(crate) fn new(schema: &'a CftSchema) -> Self {
        Self { cft: schema }
    }

    pub(crate) const fn cft(&self) -> &'a CftSchema {
        self.cft
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

    pub(crate) fn enum_value(&self, enum_name: &str, variant: &str) -> Option<CftEnumValue> {
        let value = self.cft.enum_variant_value(enum_name, variant)?;
        self.cft.enum_value_from_int(enum_name, value)
    }

    pub(crate) fn singleton_types(&self) -> impl Iterator<Item = &CftType> {
        self.cft.singleton_types()
    }

    pub(crate) fn inheritance_root(&self, type_name: &str) -> Option<&TypeName> {
        self.cft.inheritance_root(type_name)
    }
}
