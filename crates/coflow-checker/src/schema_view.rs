use coflow_cft::{
    CftConstValue, CftContainer, CftSchemaCheckBlock, CftSchemaEnum, CftSchemaType,
    CftSchemaTypeRef,
};
use coflow_data_model::CfdEnumValue;
use std::collections::BTreeMap;

/// Cached reflection used by check evaluation. The shape mirrors the data
/// needed to resolve names, walk inheritance and look up enum variants;
/// shared semantic helpers (`is_assignable`, `enum_variant_value`) live on
/// [`CftContainer`] itself so this view and the data-model's cannot drift.
#[derive(Debug, Clone)]
pub(crate) struct SchemaView {
    pub(crate) consts: BTreeMap<String, CftConstValue>,
    pub(crate) types: BTreeMap<String, TypeMeta>,
    pub(crate) enums: BTreeMap<String, EnumMeta>,
    /// Snapshot of (type name, parent) for inheritance walks. Avoids holding
    /// a `&CftContainer` borrow on the runner; shaped to match what
    /// [`CftContainer::is_assignable`] expects so we can delegate to it.
    inheritance: BTreeMap<String, Option<String>>,
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

        let mut inheritance = BTreeMap::new();
        let types = schema
            .all_types()
            .map(|schema_type| {
                inheritance.insert(schema_type.name.clone(), schema_type.parent.clone());
                let meta = TypeMeta::from_schema(schema_type);
                (meta.name.clone(), meta)
            })
            .collect::<BTreeMap<_, _>>();

        Self {
            consts,
            types,
            enums,
            inheritance,
        }
    }

    /// Walks inheritance the same way [`CftContainer::is_assignable`] does,
    /// but against the cached snapshot so the runtime evaluator doesn't need
    /// to keep a `&CftContainer` borrow alive for the lifetime of the run.
    pub(crate) fn is_assignable(&self, actual_type: &str, expected_type: &str) -> bool {
        let mut current = Some(actual_type);
        while let Some(name) = current {
            if name == expected_type {
                return true;
            }
            current = self
                .inheritance
                .get(name)
                .and_then(|parent| parent.as_deref());
        }
        false
    }

    pub(crate) fn enum_variant_value(&self, enum_name: &str, variant: &str) -> Option<i64> {
        self.enums
            .get(enum_name)
            .and_then(|meta| meta.variants.get(variant))
            .copied()
    }

    /// Returns a fully-resolved enum value when `value` matches a single declared
    /// variant. Returns `None` when no variant has exactly this value (typical
    /// for `@flag` bitwise composites). Callers should fall back to a
    /// variantless `CfdEnumValue` in that case.
    pub(crate) fn enum_value_from_int(&self, enum_name: &str, value: i64) -> Option<CfdEnumValue> {
        let meta = self.enums.get(enum_name)?;
        meta.variants
            .iter()
            .find(|(_, variant_value)| **variant_value == value)
            .map(|(variant, variant_value)| CfdEnumValue {
                enum_name: enum_name.to_string(),
                variant: Some(variant.clone()),
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

    pub(crate) fn field_type(
        &self,
        actual_type: &str,
        field_name: &str,
    ) -> Option<&CftSchemaTypeRef> {
        self.types
            .get(actual_type)
            .and_then(|meta| meta.fields.get(field_name))
    }

    /// Returns whether a field is `@localized`.
    pub(crate) fn field_is_localized(&self, actual_type: &str, field_name: &str) -> bool {
        self.types
            .get(actual_type)
            .is_some_and(|meta| meta.localized_fields.contains(field_name))
    }

    /// Returns whether a type is `@singleton`.
    pub(crate) fn type_is_singleton(&self, type_name: &str) -> bool {
        self.types
            .get(type_name)
            .is_some_and(|meta| meta.is_singleton)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TypeMeta {
    name: String,
    parent: Option<String>,
    check: Option<CftSchemaCheckBlock>,
    fields: BTreeMap<String, CftSchemaTypeRef>,
    is_singleton: bool,
    /// Names of fields that carry `@localized`.
    localized_fields: std::collections::BTreeSet<String>,
}

impl TypeMeta {
    fn from_schema(schema_type: &CftSchemaType) -> Self {
        let localized_fields = schema_type
            .all_fields
            .iter()
            .filter(|field| field.is_localized)
            .map(|field| field.name.clone())
            .collect::<std::collections::BTreeSet<_>>();
        Self {
            name: schema_type.name.clone(),
            parent: schema_type.parent.clone(),
            check: schema_type.check.clone(),
            fields: schema_type
                .all_fields
                .iter()
                .map(|field| (field.name.clone(), field.ty_ref.clone()))
                .collect(),
            is_singleton: schema_type.is_singleton,
            localized_fields,
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
