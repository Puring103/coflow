use crate::ast::{Item, TypeName, TypeRef};
use crate::container::{CfcContainer, ModuleId};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaModule {
    pub types: Vec<CfcSchemaType>,
    pub enums: Vec<CfcSchemaEnum>,
    pub data: Vec<CfcSchemaData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaType {
    pub module: ModuleId,
    pub name: String,
    pub fields: Vec<CfcSchemaField>,
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaField {
    pub name: String,
    pub ty: String,
    pub has_default: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaEnum {
    pub module: ModuleId,
    pub name: String,
    pub variants: Vec<CfcSchemaEnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaEnumVariant {
    pub name: String,
    pub value: Option<i64>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfcSchemaData {
    pub module: ModuleId,
    pub name: String,
    pub ty: Option<String>,
    pub span: Span,
}

impl CfcContainer {
    #[must_use]
    pub fn definitions(&self, module: &ModuleId) -> Option<CfcSchemaModule> {
        let module_data = self.modules.get(module)?;
        let mut types = Vec::new();
        let mut enums = Vec::new();
        let mut data = Vec::new();
        for item in &module_data.ast.items {
            match item {
                Item::Type(def) => types.push(CfcSchemaType {
                    module: module.clone(),
                    name: def.name.clone(),
                    fields: def
                        .fields
                        .iter()
                        .map(|field| CfcSchemaField {
                            name: field.name.clone(),
                            ty: format_type_ref(&field.ty),
                            has_default: field.default.is_some(),
                            span: field.span,
                        })
                        .collect(),
                    alias: def.alias.as_ref().map(format_type_ref),
                    span: def.span,
                }),
                Item::Enum(def) => enums.push(CfcSchemaEnum {
                    module: module.clone(),
                    name: def.name.clone(),
                    variants: def
                        .variants
                        .iter()
                        .map(|variant| CfcSchemaEnumVariant {
                            name: variant.name.clone(),
                            value: variant.value,
                            span: variant.span,
                        })
                        .collect(),
                    span: def.span,
                }),
                Item::Data(def) => data.push(CfcSchemaData {
                    module: module.clone(),
                    name: def.name.clone(),
                    ty: def.ty.as_ref().map(format_type_ref),
                    span: def.span,
                }),
                Item::Check(_) => {}
            }
        }
        Some(CfcSchemaModule { types, enums, data })
    }

    #[must_use]
    pub fn type_def(&self, module: &ModuleId, name: &str) -> Option<CfcSchemaType> {
        self.definitions(module)?
            .types
            .into_iter()
            .find(|def| def.name == name)
    }

    #[must_use]
    pub fn enum_def(&self, module: &ModuleId, name: &str) -> Option<CfcSchemaEnum> {
        self.definitions(module)?
            .enums
            .into_iter()
            .find(|def| def.name == name)
    }
}

fn format_type_ref(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Int => "int".to_string(),
        TypeRef::Float => "float".to_string(),
        TypeRef::Bool => "bool".to_string(),
        TypeRef::String => "string".to_string(),
        TypeRef::Null => "null".to_string(),
        TypeRef::StringLiteral(value) => format!("{value:?}"),
        TypeRef::IntLiteral(value) => value.to_string(),
        TypeRef::BoolLiteral(value) => value.to_string(),
        TypeRef::Any => "any".to_string(),
        TypeRef::Array(inner) => format!("[{}]", format_type_ref(inner)),
        TypeRef::Dict(key, value) => {
            format!("{{{}: {}}}", format_type_ref(key), format_type_ref(value))
        }
        TypeRef::Union(items) => match items.as_slice() {
            [inner, TypeRef::Null] => format!("{}?", format_type_ref(inner)),
            _ => items
                .iter()
                .map(format_type_ref)
                .collect::<Vec<_>>()
                .join(" | "),
        },
        TypeRef::Named(name) => format_type_name(name),
    }
}

fn format_type_name(name: &TypeName) -> String {
    match name {
        TypeName::Local(name) => name.clone(),
        TypeName::Imported { alias, name } => format!("{alias}.{name}"),
    }
}
