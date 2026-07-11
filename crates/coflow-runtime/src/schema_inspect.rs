use coflow_api::FlatDiagnostic;
use coflow_cft::{
    CftAnnotation, CftAnnotationValue, CftConstValue, CftContainer, CftSchemaDefaultValue,
    CftSchemaTypeRef, CompiledSchema, ModuleId,
};
use serde::Serialize;

use crate::ProjectSchemaSession;

#[derive(Debug, Clone, Serialize)]
pub struct SchemaInspectReport {
    pub types: Vec<SchemaTypeInfo>,
    pub enums: Vec<SchemaEnumInfo>,
    pub consts: Vec<SchemaConstInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFilesReport {
    pub files: Vec<SchemaFileInfo>,
    pub diagnostics: Vec<FlatDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaTypeInfo {
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_singleton: bool,
    pub annotations: Vec<SchemaAnnotation>,
    pub fields: Vec<SchemaFieldInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaFieldInfo {
    pub name: String,
    pub ty: SchemaTypeRefInfo,
    pub raw_type: String,
    pub has_default: bool,
    pub default: Option<SchemaDefaultValueInfo>,
    pub annotations: Vec<SchemaAnnotation>,
    pub dimension: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumInfo {
    pub module: String,
    pub name: String,
    pub annotations: Vec<SchemaAnnotation>,
    pub variants: Vec<SchemaEnumVariantInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaEnumVariantInfo {
    pub name: String,
    #[serde(with = "coflow_data_model::serde_i64")]
    pub value: i64,
    pub annotations: Vec<SchemaAnnotation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaAnnotation {
    pub name: String,
    pub args: Vec<SchemaAnnotationValueInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SchemaAnnotationValueInfo {
    Name(String),
    String(String),
    Int(#[serde(with = "coflow_data_model::serde_i64")] i64),
    Float(f64),
    Bool(bool),
    Null,
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
    let view = session.compiled_schema();
    let mut type_names = view.type_names().cloned().collect::<Vec<_>>();
    type_names.sort();
    if let Some(filter) = type_filter {
        type_names
            .retain(|name| name == filter || (include_derived && view.is_assignable(name, filter)));
    }

    let types = type_names
        .into_iter()
        .filter_map(|name| view.type_meta(&name))
        .map(|ty| SchemaTypeInfo {
            module: ty.module.clone(),
            name: ty.name.clone(),
            parent: ty.parent.clone(),
            is_abstract: ty.is_abstract,
            is_sealed: ty.is_sealed,
            is_singleton: ty.is_singleton,
            annotations: annotations(&ty.annotations),
            fields: view
                .fields(&ty.name)
                .into_iter()
                .flatten()
                .map(|field| SchemaFieldInfo {
                    name: field.name.clone(),
                    ty: type_ref_info(&view, &field.ty_ref),
                    raw_type: field.raw_type.clone(),
                    has_default: field.has_default,
                    default: field.default.as_ref().map(default_value_info),
                    annotations: annotations(&field.annotations),
                    dimension: field
                        .dimension
                        .as_ref()
                        .map(|dimension| dimension.dimension.clone()),
                })
                .collect(),
        })
        .collect();

    let mut enums = view
        .enum_metas()
        .map(|schema_enum| SchemaEnumInfo {
            module: schema_enum.module.clone(),
            name: schema_enum.name.clone(),
            annotations: annotations(&schema_enum.annotations),
            variants: schema_enum
                .all_variants
                .iter()
                .map(|variant| SchemaEnumVariantInfo {
                    name: variant.name.clone(),
                    value: variant.value,
                    annotations: annotations(&variant.annotations),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    enums.sort_by(|left, right| left.name.cmp(&right.name));

    SchemaInspectReport {
        types,
        enums,
        consts: consts(&session.schema),
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

#[must_use]
pub fn schema_files(session: &ProjectSchemaSession) -> SchemaFilesReport {
    let files = session
        .schema
        .module_ids()
        .filter_map(|module| {
            session.schema.source(module).map(|source| SchemaFileInfo {
                module: module.to_string(),
                source: source.to_string(),
            })
        })
        .collect();
    SchemaFilesReport {
        files,
        diagnostics: session.diagnostics.flat_diagnostics(),
    }
}

fn consts(schema: &CftContainer) -> Vec<SchemaConstInfo> {
    let mut consts = Vec::new();
    for module_id in schema.module_ids() {
        append_module_consts(schema, module_id, &mut consts);
    }
    consts.sort_by(|left, right| left.name.cmp(&right.name));
    consts
}

fn append_module_consts(
    schema: &CftContainer,
    module_id: &ModuleId,
    out: &mut Vec<SchemaConstInfo>,
) {
    let Some(module) = schema.schema(module_id) else {
        return;
    };
    out.extend(module.consts.iter().map(|schema_const| SchemaConstInfo {
        module: schema_const.module.to_string(),
        name: schema_const.name.clone(),
        value: const_value_info(&schema_const.value),
    }));
}

fn type_ref_info(schema: &CompiledSchema, ty: &CftSchemaTypeRef) -> SchemaTypeRefInfo {
    match ty {
        CftSchemaTypeRef::Int => SchemaTypeRefInfo::Int,
        CftSchemaTypeRef::Float => SchemaTypeRefInfo::Float,
        CftSchemaTypeRef::Bool => SchemaTypeRefInfo::Bool,
        CftSchemaTypeRef::String => SchemaTypeRefInfo::String,
        CftSchemaTypeRef::Named(name) => {
            let target_kind = if schema.is_schema_enum(name) {
                "enum"
            } else if schema.has_type(name) {
                "type"
            } else {
                "unknown"
            };
            SchemaTypeRefInfo::Named {
                name: name.clone(),
                target_kind: target_kind.to_string(),
            }
        }
        CftSchemaTypeRef::Ref(target) => SchemaTypeRefInfo::Ref {
            target: target.clone(),
        },
        CftSchemaTypeRef::Array(inner) => SchemaTypeRefInfo::Array {
            item: Box::new(type_ref_info(schema, inner)),
        },
        CftSchemaTypeRef::Dict(key, value) => SchemaTypeRefInfo::Dict {
            key: Box::new(type_ref_info(schema, key)),
            value: Box::new(type_ref_info(schema, value)),
        },
        CftSchemaTypeRef::Nullable(inner) => SchemaTypeRefInfo::Nullable {
            inner: Box::new(type_ref_info(schema, inner)),
        },
    }
}

fn annotations(items: &[CftAnnotation]) -> Vec<SchemaAnnotation> {
    items
        .iter()
        .map(|annotation| SchemaAnnotation {
            name: annotation.name.clone(),
            args: annotation.args.iter().map(annotation_value_info).collect(),
        })
        .collect()
}

fn annotation_value_info(value: &CftAnnotationValue) -> SchemaAnnotationValueInfo {
    match value {
        CftAnnotationValue::Name(value) => SchemaAnnotationValueInfo::Name(value.clone()),
        CftAnnotationValue::String(value) => SchemaAnnotationValueInfo::String(value.clone()),
        CftAnnotationValue::Int(value) => SchemaAnnotationValueInfo::Int(*value),
        CftAnnotationValue::Float(value) => SchemaAnnotationValueInfo::Float(*value),
        CftAnnotationValue::Bool(value) => SchemaAnnotationValueInfo::Bool(*value),
        CftAnnotationValue::Null => SchemaAnnotationValueInfo::Null,
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
            enum_name: enum_name.clone(),
            variant: variant.clone(),
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

