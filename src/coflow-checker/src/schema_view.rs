use coflow_cft::{CftConstValue, CftContainer, CftSchemaCheckBlock, CftSchemaEnum, CftSchemaType};
use coflow_data_model::CfdEnumValue;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) consts: BTreeMap<String, CftConstValue>,
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
}

impl SchemaView {
    pub(crate) fn new(schema: &CftContainer) -> Self {
        let consts = schema
            .module_ids()
            .filter_map(|id| schema.schema(id))
            .flat_map(|module| module.consts.iter())
            .map(|schema_const| (schema_const.name.clone(), schema_const.value.clone()))
            .collect::<BTreeMap<_, _>>();

        let enums = schema
            .all_enums()
            .map(|schema_enum| (schema_enum.name.clone(), EnumMeta::from_schema(schema_enum)))
            .collect::<BTreeMap<_, _>>();

        let types = schema
            .all_types()
            .map(|schema_type| {
                let meta = TypeMeta::from_schema(schema_type);
                (meta.name.clone(), meta)
            })
            .collect::<BTreeMap<_, _>>();

        Self {
            consts,
            types,
            enums,
        }
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

    pub(crate) fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    pub(crate) fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CfdEnumValue> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CfdEnumValue {
                enum_name: enum_name.to_string(),
                variant: variant.clone(),
                value: *variant_value,
            })
    }

    pub(crate) fn checks_for_actual(&self, actual_type: &str) -> Vec<CftSchemaCheckBlock> {
        let mut chain = Vec::new();
        let mut current = Some(actual_type);
        while let Some(name) = current {
            let Some(meta) = self.types.get(name) else {
                break;
            };
            chain.push(meta);
            current = meta.parent.as_deref();
        }
        chain.reverse();
        chain
            .into_iter()
            .filter_map(|meta| meta.check.clone())
            .collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    name: String,
    parent: Option<String>,
    check: Option<CftSchemaCheckBlock>,
}

impl TypeMeta {
    fn from_schema(schema_type: &CftSchemaType) -> Self {
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            check: schema_type.check.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct EnumMeta {
    variants: BTreeMap<String, i64>,
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
