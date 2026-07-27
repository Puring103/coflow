//! Language-neutral projection used by Coflow code generators.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_api::{Diagnostic, DiagnosticSet};
use coflow_cft::{CftSchema, CftSchemaDefaultValue, CftValueType};
use coflow_data_model::CfdDataModel;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IdAsEnumVariant {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct CodegenModel {
    pub types: Vec<CodegenType>,
    pub enums: Vec<CodegenEnum>,
    pub dimensions: Vec<CodegenDimension>,
    pub table_names: Vec<String>,
    pub singleton_names: Vec<String>,
    pub non_empty_tables: Option<BTreeSet<String>>,
}

#[derive(Debug, Clone)]
pub struct CodegenType {
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub id_as_enum: Option<String>,
    pub own_fields: Vec<CodegenField>,
    pub all_fields: Vec<CodegenField>,
    pub concrete_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CodegenField {
    pub declaring_type: String,
    pub name: String,
    pub value_type: CftValueType,
    pub default: Option<CftSchemaDefaultValue>,
    pub is_localized: bool,
}

#[derive(Debug, Clone)]
pub struct CodegenEnum {
    pub name: String,
    pub is_flags: bool,
    pub is_id_as_enum: bool,
    pub variants: Vec<CodegenEnumVariant>,
}

#[derive(Debug, Clone)]
pub struct CodegenEnumVariant {
    pub name: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct CodegenDimension {
    pub name: String,
    pub variants: Vec<String>,
    pub fields: Vec<CodegenDimensionField>,
}

#[derive(Debug, Clone)]
pub struct CodegenDimensionField {
    pub declaring_type: String,
    pub field: String,
    pub value_type: CftValueType,
}

impl CodegenModel {
    /// Builds the common model consumed by target-language lowering.
    ///
    /// # Errors
    ///
    /// Returns diagnostics when generated id-as-enum metadata is malformed.
    pub fn build(
        schema: &CftSchema,
        model: Option<&CfdDataModel>,
        id_as_enum_variants: &serde_json::Value,
    ) -> Result<Self, DiagnosticSet> {
        let generated_variants = decode_id_as_enum_variants(id_as_enum_variants)?;
        let id_as_enums = schema
            .all_types()
            .filter_map(|ty| ty.id_as_enum.as_ref().map(ToString::to_string))
            .collect::<BTreeSet<_>>();
        let mut enums = schema
            .all_enums()
            .map(|meta| {
                let variants = generated_variants.get(meta.name.as_str()).map_or_else(
                    || {
                        meta.variants
                            .iter()
                            .map(|variant| CodegenEnumVariant {
                                name: variant.name.to_string(),
                                value: variant.value,
                            })
                            .collect()
                    },
                    |variants| {
                        variants
                            .iter()
                            .map(|variant| CodegenEnumVariant {
                                name: variant.name.clone(),
                                value: variant.value,
                            })
                            .collect()
                    },
                );
                CodegenEnum {
                    name: meta.name.to_string(),
                    is_flags: meta.is_flag,
                    is_id_as_enum: id_as_enums.contains(meta.name.as_str()),
                    variants,
                }
            })
            .collect::<Vec<_>>();
        enums.sort_by(|left, right| left.name.cmp(&right.name));

        let mut types = schema
            .all_types()
            .map(|meta| CodegenType {
                name: meta.name.to_string(),
                parent: meta.parent.as_ref().map(ToString::to_string),
                is_abstract: meta.is_abstract,
                is_sealed: meta.is_sealed,
                is_struct: meta.is_struct,
                is_singleton: meta.is_singleton,
                id_as_enum: schema
                    .inherited_id_as_enum(meta.name.as_str())
                    .map(|name| name.to_string()),
                own_fields: meta.own_fields().map(project_field).collect(),
                all_fields: meta.all_fields().map(project_field).collect(),
                concrete_types: schema
                    .concrete_assignable_types(meta.name.as_str())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|name| name.to_string())
                    .collect(),
            })
            .collect::<Vec<_>>();
        types.sort_by(|left, right| left.name.cmp(&right.name));

        let table_names = types
            .iter()
            .filter(|ty| !ty.is_abstract && !ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect();
        let singleton_names = types
            .iter()
            .filter(|ty| ty.is_singleton)
            .map(|ty| ty.name.clone())
            .collect();
        let dimensions = schema
            .all_dimensions()
            .map(|dimension| CodegenDimension {
                name: dimension.name.to_string(),
                variants: dimension.variants.iter().map(ToString::to_string).collect(),
                fields: dimension
                    .fields
                    .iter()
                    .map(|field| CodegenDimensionField {
                        declaring_type: field.declaring_type.to_string(),
                        field: field.name.to_string(),
                        value_type: field.value_type.clone(),
                    })
                    .collect(),
            })
            .collect();
        let non_empty_tables = model.map(|model| {
            model
                .tables()
                .filter(|(_, table)| !table.records.is_empty())
                .map(|(name, _)| name.to_string())
                .collect()
        });
        Ok(Self {
            types,
            enums,
            dimensions,
            table_names,
            singleton_names,
            non_empty_tables,
        })
    }

    #[must_use]
    pub fn type_by_name(&self, name: &str) -> Option<&CodegenType> {
        self.types.iter().find(|ty| ty.name == name)
    }

    #[must_use]
    pub fn enum_by_name(&self, name: &str) -> Option<&CodegenEnum> {
        self.enums.iter().find(|item| item.name == name)
    }
}

fn project_field(field: &coflow_cft::CftField) -> CodegenField {
    CodegenField {
        declaring_type: field.declaring_type.to_string(),
        name: field.name.to_string(),
        value_type: field.value_type.clone(),
        default: field.default.clone(),
        is_localized: field.dimension.is_some(),
    }
}

fn decode_id_as_enum_variants(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<IdAsEnumVariant>>, DiagnosticSet> {
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_value(value.clone()).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "CODEGEN-MODEL",
            "CODEGEN",
            format!("invalid generated id-as-enum metadata: {error}"),
        ))
    })
}
