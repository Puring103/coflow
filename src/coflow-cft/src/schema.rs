use crate::ast::{Item, TypeName, TypeRef};
use crate::container::{CftContainer, ModuleId};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftSchemaModule {
    pub types: Vec<CftSchemaType>,
    pub enums: Vec<CftSchemaEnum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftSchemaType {
    pub module: ModuleId,
    pub name: String,
    pub fields: Vec<CftSchemaField>,
    pub alias: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftSchemaField {
    pub name: String,
    pub ty: String,
    pub has_default: bool,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftSchemaEnum {
    pub module: ModuleId,
    pub name: String,
    pub variants: Vec<CftSchemaEnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftSchemaEnumVariant {
    pub name: String,
    pub value: Option<i64>,
    pub span: Span,
}

impl CftContainer {
    #[must_use]
    pub fn schema(&self, module: &ModuleId) -> Option<CftSchemaModule> {
        let module_data = self.modules.get(module)?;
        let mut types = Vec::new();
        let mut enums = Vec::new();
        for item in &module_data.ast.items {
            match item {
                Item::Type(def) => types.push(CftSchemaType {
                    module: module.clone(),
                    name: def.name.clone(),
                    fields: def
                        .fields
                        .iter()
                        .map(|field| CftSchemaField {
                            name: field.name.clone(),
                            ty: format_type_ref(&field.ty),
                            has_default: field.default.is_some(),
                            span: field.span,
                        })
                        .collect(),
                    alias: def.alias.as_ref().map(format_type_ref),
                    span: def.span,
                }),
                Item::Enum(def) => enums.push(CftSchemaEnum {
                    module: module.clone(),
                    name: def.name.clone(),
                    variants: def
                        .variants
                        .iter()
                        .map(|variant| CftSchemaEnumVariant {
                            name: variant.name.clone(),
                            value: variant.value,
                            span: variant.span,
                        })
                        .collect(),
                    span: def.span,
                }),
            }
        }
        Some(CftSchemaModule { types, enums })
    }

    #[must_use]
    pub fn type_def(&self, module: &ModuleId, name: &str) -> Option<CftSchemaType> {
        self.schema(module)?
            .types
            .into_iter()
            .find(|def| def.name == name)
    }

    #[must_use]
    pub fn enum_def(&self, module: &ModuleId, name: &str) -> Option<CftSchemaEnum> {
        self.schema(module)?
            .enums
            .into_iter()
            .find(|def| def.name == name)
    }

    #[must_use]
    pub fn resolve_type(&self, name: &str) -> Option<CftSchemaType> {
        let module = self.type_names.get(name)?;
        self.type_def(module, name)
    }

    #[must_use]
    pub fn resolve_enum(&self, name: &str) -> Option<CftSchemaEnum> {
        let module = self.enum_names.get(name)?;
        self.enum_def(module, name)
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
    }
}
