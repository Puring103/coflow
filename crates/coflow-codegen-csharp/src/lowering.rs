use crate::names::csharp_type_name;
use crate::CsharpCodegenError;
use coflow_cft::{CftEnum, CftField, CftSchema, CftSchemaTypeRef, CftType, FieldName, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub struct CsharpLoweringPlan<'a> {
    pub int_32: bool,
    pub float_32: bool,
    schema: &'a CftSchema,
    types: Vec<&'a CftType>,
    dimension_tables: Vec<CsharpDimensionTable>,
    enums: Vec<&'a CftEnum>,
    csharp_types: BTreeMap<String, String>,
    csharp_enums: BTreeMap<String, String>,
    declared_tables: Vec<String>,
    loadable_tables: Vec<String>,
    loadable_table_set: BTreeSet<String>,
    singleton_types: Vec<String>,
    polymorphic_types: Vec<String>,
    polymorphic_type_set: BTreeSet<String>,
    ref_targets: Vec<String>,
    id_as_enum_names: BTreeSet<String>,
    type_id_as_enum: BTreeMap<String, String>,
    assignable_types: BTreeMap<String, Vec<String>>,
    types_with_descendants: BTreeSet<String>,
    uses_localization: bool,
}

#[derive(Debug)]
pub struct CsharpDimensionTable {
    pub source_name: String,
    pub source_type: String,
    pub fields: Vec<CftField>,
}

impl<'a> CsharpLoweringPlan<'a> {
    #[allow(clippy::too_many_lines)]
    pub fn lower(
        schema: &'a CftSchema,
        int_32: bool,
        float_32: bool,
        non_empty_tables: Option<&BTreeSet<String>>,
    ) -> Result<Self, CsharpCodegenError> {
        let enums = schema.all_enums().collect::<Vec<_>>();
        let csharp_enums = enums
            .iter()
            .map(|schema_enum| {
                (
                    schema_enum.name.to_string(),
                    csharp_type_name(&schema_enum.name),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let types = schema.all_types().collect::<Vec<_>>();
        let mut csharp_types = BTreeMap::new();
        let mut declared_tables = Vec::new();
        let mut singleton_types = Vec::new();
        let mut polymorphic_types = Vec::new();
        let mut polymorphic_type_set = BTreeSet::new();
        let mut ref_targets = BTreeSet::new();
        let mut id_as_enum_names = BTreeSet::new();
        let mut type_id_as_enum = BTreeMap::new();
        let mut assignable_types: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut types_with_descendants = BTreeSet::new();
        let mut uses_localization = false;

        for ty in &types {
            csharp_types.insert(ty.name.to_string(), csharp_type_name(&ty.name));
            if !ty.is_abstract && !ty.is_singleton {
                declared_tables.push(ty.name.to_string());
            }
            if ty.is_singleton {
                singleton_types.push(ty.name.to_string());
            }
            if schema.range_is_polymorphic(&ty.name) {
                polymorphic_types.push(ty.name.to_string());
                polymorphic_type_set.insert(ty.name.to_string());
            }
            if let Some(parent) = &ty.parent {
                types_with_descendants.insert(parent.to_string());
            }
            if let Some(enum_name) = schema.inherited_id_as_enum(&ty.name) {
                id_as_enum_names.insert(enum_name.to_string());
                type_id_as_enum.insert(ty.name.to_string(), enum_name.to_string());
            }
            let assignable = schema.concrete_assignable_types(&ty.name).ok_or_else(|| {
                CsharpCodegenError::new(format!("unknown CFT type `{}`", ty.name))
            })?;
            assignable_types.insert(
                ty.name.to_string(),
                assignable
                    .into_iter()
                    .map(|name| name.to_string())
                    .collect(),
            );
            for field in ty.own_fields() {
                uses_localization |= field.dimension.is_some();
                collect_ref_targets(&field.ty_ref, &mut ref_targets);
            }
        }

        let mut dimension_tables = BTreeMap::new();
        for dimension in schema.all_dimensions() {
            for source_field in &dimension.fields {
                let source_name = format!(
                    "{}_{}Variants",
                    source_field.declaring_type, source_field.name
                );
                let declaring_type = TypeName::new(source_name.clone()).map_err(|err| {
                    CsharpCodegenError::new(format!(
                        "invalid generated dimension table name `{source_name}`: {err}"
                    ))
                })?;
                let field_type = CftSchemaTypeRef::Nullable(Box::new(
                    source_field.ty_ref.non_nullable().clone(),
                ));
                let mut fields = Vec::with_capacity(dimension.variants.len() + 1);
                for name in
                    std::iter::once("default").chain(dimension.variants.iter().map(AsRef::as_ref))
                {
                    fields.push(CftField {
                        declaring_type: declaring_type.clone(),
                        name: FieldName::new(name).map_err(|err| {
                            CsharpCodegenError::new(format!(
                                "invalid generated dimension field name `{name}`: {err}"
                            ))
                        })?,
                        ty_ref: field_type.clone(),
                        default: None,
                        is_expand: false,
                        dimension: None,
                        span: source_field.span,
                    });
                }
                csharp_types.insert(source_name.clone(), csharp_type_name(&source_name));
                declared_tables.push(source_name.clone());
                dimension_tables.insert(
                    source_name.clone(),
                    CsharpDimensionTable {
                        source_name,
                        source_type: source_field.declaring_type.to_string(),
                        fields,
                    },
                );
            }
        }
        declared_tables.sort();
        let dimension_tables = dimension_tables.into_values().collect::<Vec<_>>();
        let dimension_source_types = dimension_tables
            .iter()
            .map(|table| (table.source_name.as_str(), table.source_type.as_str()))
            .collect::<BTreeMap<_, _>>();

        let loadable_tables = declared_tables
            .iter()
            .filter(|name| {
                non_empty_tables.is_none_or(|tables| {
                    dimension_source_types.get(name.as_str()).map_or_else(
                        || tables.contains(*name),
                        |source_type| {
                            assignable_types.get(*source_type).is_some_and(|types| {
                                types.iter().any(|type_name| tables.contains(type_name))
                            })
                        },
                    )
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let loadable_table_set = loadable_tables.iter().cloned().collect();
        let ref_targets = ref_targets.into_iter().collect::<Vec<_>>();
        Ok(Self {
            int_32,
            float_32,
            schema,
            types,
            dimension_tables,
            enums,
            csharp_types,
            csharp_enums,
            declared_tables,
            loadable_tables,
            loadable_table_set,
            singleton_types,
            polymorphic_types,
            polymorphic_type_set,
            ref_targets,
            id_as_enum_names,
            type_id_as_enum,
            assignable_types,
            types_with_descendants,
            uses_localization,
        })
    }

    pub fn cft_enum_meta(&self, name: &str) -> Option<&CftEnum> {
        self.schema.resolve_enum(name)
    }

    pub fn enum_names(&self) -> impl Iterator<Item = &str> {
        self.enums
            .iter()
            .map(|schema_enum| schema_enum.name.as_str())
    }

    pub fn cft_enum_metas(&self) -> impl Iterator<Item = &CftEnum> {
        self.enums.iter().copied()
    }

    pub fn all_types(&self) -> impl Iterator<Item = &CftType> {
        self.types.iter().copied()
    }

    pub fn dimension_tables(&self) -> &[CsharpDimensionTable] {
        &self.dimension_tables
    }

    pub const fn uses_localization(&self) -> bool {
        self.uses_localization
    }

    pub const fn id_as_enum_names(&self) -> &BTreeSet<String> {
        &self.id_as_enum_names
    }

    pub fn is_ref_target_loadable(&self, name: &str) -> bool {
        self.loadable_table_set.contains(name)
            || self
                .assignable_types
                .get(name)
                .is_some_and(|types| types.iter().any(|ty| self.loadable_table_set.contains(ty)))
    }

    pub fn resolve_type(&self, name: &str) -> Result<&CftType, CsharpCodegenError> {
        self.schema
            .resolve_type(name)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn fields(
        &self,
        name: &str,
    ) -> Result<impl Iterator<Item = &CftField>, CsharpCodegenError> {
        self.schema
            .resolve_type(name)
            .map(CftType::all_fields)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{name}`")))
    }

    pub fn all_type_names(&self) -> impl Iterator<Item = &str> {
        self.types.iter().map(|ty| ty.name.as_str()).chain(
            self.dimension_tables
                .iter()
                .map(|table| table.source_name.as_str()),
        )
    }

    pub fn declared_table_names(&self) -> &[String] {
        &self.declared_tables
    }

    pub fn table_names(&self) -> &[String] {
        &self.loadable_tables
    }

    pub fn singleton_type_names(&self) -> &[String] {
        &self.singleton_types
    }

    pub fn polymorphic_type_names(&self) -> &[String] {
        &self.polymorphic_types
    }

    pub fn range_is_polymorphic(&self, type_name: &str) -> bool {
        self.polymorphic_type_set.contains(type_name)
    }

    pub fn csharp_type_name(&self, type_name: &str) -> String {
        self.csharp_types
            .get(type_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(type_name))
    }

    pub fn csharp_enum_name(&self, enum_name: &str) -> String {
        self.csharp_enums
            .get(enum_name)
            .cloned()
            .unwrap_or_else(|| csharp_type_name(enum_name))
    }

    pub fn id_as_enum(&self, type_name: &str) -> Option<String> {
        self.type_id_as_enum.get(type_name).cloned()
    }

    pub fn is_id_as_enum(&self, enum_name: &str) -> bool {
        self.id_as_enum_names.contains(enum_name)
    }

    pub fn key_field_type(&self, type_name: &str) -> CftSchemaTypeRef {
        self.id_as_enum(type_name)
            .and_then(|name| self.schema.resolve_enum(&name))
            .map_or_else(
                || CftSchemaTypeRef::String,
                |schema_enum| CftSchemaTypeRef::Enum(schema_enum.name.clone()),
            )
    }

    pub fn type_has_descendants(&self, type_name: &str) -> bool {
        self.types_with_descendants.contains(type_name)
    }

    pub fn concrete_assignable_types(
        &self,
        type_name: &str,
    ) -> Result<&[String], CsharpCodegenError> {
        self.assignable_types
            .get(type_name)
            .map(Vec::as_slice)
            .ok_or_else(|| CsharpCodegenError::new(format!("unknown CFT type `{type_name}`")))
    }

    pub fn ref_target_names(&self) -> &[String] {
        &self.ref_targets
    }
}

fn collect_ref_targets(ty: &CftSchemaTypeRef, out: &mut BTreeSet<String>) {
    match ty {
        CftSchemaTypeRef::RecordRef(name) => {
            out.insert(name.to_string());
        }
        CftSchemaTypeRef::Array(inner) | CftSchemaTypeRef::Nullable(inner) => {
            collect_ref_targets(inner, out);
        }
        CftSchemaTypeRef::Dict(key, value) => {
            collect_ref_targets(key, out);
            collect_ref_targets(value, out);
        }
        CftSchemaTypeRef::Int
        | CftSchemaTypeRef::Float
        | CftSchemaTypeRef::Bool
        | CftSchemaTypeRef::String
        | CftSchemaTypeRef::Object(_)
        | CftSchemaTypeRef::Enum(_) => {}
    }
}
