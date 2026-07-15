use coflow_api::FlatDiagnostic;
use coflow_cft::{CftConstValue, CftSchema, CftSchemaDefaultValue, CftSchemaTypeRef};
use serde::Serialize;

use crate::ProjectSchemaSession;

#[derive(Debug, Clone, Serialize)]
pub struct SchemaInspectReport {
    pub types: Vec<SchemaTypeInfo>,
    pub enums: Vec<SchemaEnumInfo>,
    pub consts: Vec<SchemaConstInfo>,
    pub dimensions: Vec<SchemaDimensionInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFilesReport {
    pub files: Vec<SchemaFileInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct SchemaTypeInfo {
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub id_as_enum: Option<String>,
    pub fields: Vec<SchemaFieldInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldInfo {
    pub name: String,
    pub ty: SchemaTypeRefInfo,
    pub has_default: bool,
    pub default: Option<SchemaDefaultValueInfo>,
    pub is_expand: bool,
    pub dimension: Option<SchemaFieldDimensionInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldDimensionInfo {
    pub name: String,
    pub bucket: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumInfo {
    pub module: String,
    pub name: String,
    pub is_flag: bool,
    pub variants: Vec<SchemaEnumVariantInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumVariantInfo {
    pub name: String,
    #[serde(with = "coflow_data_model::serde_i64")]
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaDimensionInfo {
    pub name: String,
    pub variants: Vec<String>,
    pub fields: Vec<SchemaDimensionFieldInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaDimensionFieldInfo {
    pub declaring_type: String,
    pub field: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaTypeRefInfo {
    Int,
    Float,
    Bool,
    String,
    Named { name: String, target_kind: String },
    Ref { target: String },
    Array { item: Box<Self> },
    Dict { key: Box<Self>, value: Box<Self> },
    Nullable { inner: Box<Self> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaDefaultValueInfo {
    Null,
    Int(#[serde(with = "coflow_data_model::serde_i64")] i64),
    Float(f64),
    Bool(bool),
    String(String),
    Enum {
        enum_name: String,
        variant: String,
        #[serde(with = "coflow_data_model::serde_i64")]
        value: i64,
    },
    EmptyArray,
    EmptyObject,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaConstInfo {
    pub module: String,
    pub name: String,
    pub value: SchemaConstValueInfo,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaConstValueInfo {
    Int(#[serde(with = "coflow_data_model::serde_i64")] i64),
    Float(f64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFileInfo {
    pub module: String,
    pub source: String,
}

#[must_use]
pub fn inspect_schema(
    session: &ProjectSchemaSession,
    type_filter: Option<&str>,
    include_derived: bool,
) -> SchemaInspectReport {
    let Some(view) = session.schema() else {
        return SchemaInspectReport {
            types: Vec::new(),
            enums: Vec::new(),
            consts: Vec::new(),
            dimensions: Vec::new(),
            diagnostics: session.diagnostics.flat_diagnostics(),
        };
    };
    let mut type_names = view
        .all_types()
        .map(|ty| ty.name.clone())
        .collect::<Vec<_>>();
    type_names.sort();
    if let Some(filter) = type_filter {
        type_names.retain(|name| {
            name.as_str() == filter || (include_derived && view.is_assignable(name, filter))
        });
    }

    let types = type_names
        .into_iter()
        .filter_map(|name| view.resolve_type(&name))
        .map(|ty| SchemaTypeInfo {
            module: ty.module.to_string(),
            name: ty.name.to_string(),
            parent: ty.parent.as_ref().map(ToString::to_string),
            is_abstract: ty.is_abstract,
            is_sealed: ty.is_sealed,
            is_struct: ty.is_struct,
            is_singleton: ty.is_singleton,
            id_as_enum: ty.id_as_enum.as_ref().map(ToString::to_string),
            fields: view
                .resolve_type(&ty.name)
                .into_iter()
                .flat_map(coflow_cft::CftType::all_fields)
                .map(|field| SchemaFieldInfo {
                    name: field.name.to_string(),
                    ty: type_ref_info(&field.ty_ref),
                    has_default: field.default.is_some(),
                    default: field.default.as_ref().map(default_value_info),
                    is_expand: field.is_expand,
                    dimension: field
                        .dimension
                        .as_ref()
                        .map(|dimension| SchemaFieldDimensionInfo {
                            name: dimension.dimension.to_string(),
                            bucket: dimension.bucket.as_ref().map(ToString::to_string),
                        }),
                })
                .collect(),
        })
        .collect();

    let mut enums = view
        .all_enums()
        .map(|schema_enum| SchemaEnumInfo {
            module: schema_enum.module.to_string(),
            name: schema_enum.name.to_string(),
            is_flag: schema_enum.is_flag,
            variants: schema_enum
                .variants
                .iter()
                .map(|variant| SchemaEnumVariantInfo {
                    name: variant.name.to_string(),
                    value: variant.value,
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    enums.sort_by(|left, right| left.name.cmp(&right.name));

    SchemaInspectReport {
        types,
        enums,
        consts: consts(view),
        dimensions: view
            .all_dimensions()
            .map(|dimension| SchemaDimensionInfo {
                name: dimension.name.to_string(),
                variants: dimension.variants.iter().map(ToString::to_string).collect(),
                fields: dimension
                    .fields
                    .iter()
                    .map(|field| SchemaDimensionFieldInfo {
                        declaring_type: field.declaring_type.to_string(),
                        field: field.name.to_string(),
                    })
                    .collect(),
            })
            .collect(),
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

#[must_use]
pub fn schema_files(session: &ProjectSchemaSession) -> SchemaFilesReport {
    let files = session
        .modules()
        .modules()
        .map(|(module_id, module)| SchemaFileInfo {
            module: module_id.to_string(),
            source: module.source().to_string(),
        })
        .collect();
    SchemaFilesReport {
        files,
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

fn consts(schema: &CftSchema) -> Vec<SchemaConstInfo> {
    let mut consts = schema
        .all_consts()
        .map(|schema_const| SchemaConstInfo {
            module: schema_const.module.to_string(),
            name: schema_const.name.to_string(),
            value: const_value_info(&schema_const.value),
        })
        .collect::<Vec<_>>();
    consts.sort_by(|left, right| left.name.cmp(&right.name));
    consts
}

fn type_ref_info(ty: &CftSchemaTypeRef) -> SchemaTypeRefInfo {
    match ty {
        CftSchemaTypeRef::Int => SchemaTypeRefInfo::Int,
        CftSchemaTypeRef::Float => SchemaTypeRefInfo::Float,
        CftSchemaTypeRef::Bool => SchemaTypeRefInfo::Bool,
        CftSchemaTypeRef::String => SchemaTypeRefInfo::String,
        CftSchemaTypeRef::Object(name) => SchemaTypeRefInfo::Named {
            name: name.to_string(),
            target_kind: "type".to_string(),
        },
        CftSchemaTypeRef::Enum(name) => SchemaTypeRefInfo::Named {
            name: name.to_string(),
            target_kind: "enum".to_string(),
        },
        CftSchemaTypeRef::RecordRef(target) => SchemaTypeRefInfo::Ref {
            target: target.to_string(),
        },
        CftSchemaTypeRef::Array(inner) => SchemaTypeRefInfo::Array {
            item: Box::new(type_ref_info(inner)),
        },
        CftSchemaTypeRef::Dict(key, value) => SchemaTypeRefInfo::Dict {
            key: Box::new(type_ref_info(key)),
            value: Box::new(type_ref_info(value)),
        },
        CftSchemaTypeRef::Nullable(inner) => SchemaTypeRefInfo::Nullable {
            inner: Box::new(type_ref_info(inner)),
        },
    }
}

fn default_value_info(value: &CftSchemaDefaultValue) -> SchemaDefaultValueInfo {
    match value {
        CftSchemaDefaultValue::Null => SchemaDefaultValueInfo::Null,
        CftSchemaDefaultValue::Int(value) => SchemaDefaultValueInfo::Int(*value),
        CftSchemaDefaultValue::Float(value) => SchemaDefaultValueInfo::Float(*value),
        CftSchemaDefaultValue::Bool(value) => SchemaDefaultValueInfo::Bool(*value),
        CftSchemaDefaultValue::String(value) => SchemaDefaultValueInfo::String(value.clone()),
        CftSchemaDefaultValue::Enum {
            enum_name,
            variant,
            value,
        } => SchemaDefaultValueInfo::Enum {
            enum_name: enum_name.to_string(),
            variant: variant.to_string(),
            value: *value,
        },
        CftSchemaDefaultValue::EmptyArray => SchemaDefaultValueInfo::EmptyArray,
        CftSchemaDefaultValue::EmptyObject => SchemaDefaultValueInfo::EmptyObject,
    }
}

fn const_value_info(value: &CftConstValue) -> SchemaConstValueInfo {
    match value {
        CftConstValue::Int(value) => SchemaConstValueInfo::Int(*value),
        CftConstValue::Float(value) => SchemaConstValueInfo::Float(*value),
        CftConstValue::Bool(value) => SchemaConstValueInfo::Bool(*value),
        CftConstValue::String(value) => SchemaConstValueInfo::String(value.clone()),
    }
}
