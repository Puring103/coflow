use crate::schema::{CftField, CftSchema, CftValueType};

use super::diagnostics::{
    invalid_declared_type, CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CellType {
    Int,
    Float,
    Bool,
    String,
    FString,
    Type(String),
    Ref(String),
    Enum(String),
    Array(Box<CellType>),
    Dict(Box<CellType>, Box<CellType>),
    Option(Box<CellType>),
    Unsupported(String),
}

impl CellType {
    pub(super) fn from_schema_type(ty: &CftValueType) -> Self {
        match ty {
            CftValueType::Int => Self::Int,
            CftValueType::Float => Self::Float,
            CftValueType::Bool => Self::Bool,
            CftValueType::String => Self::String,
            CftValueType::FString => Self::FString,
            CftValueType::Object(name) => Self::Type(name.to_string()),
            CftValueType::Enum(name) => Self::Enum(name.to_string()),
            CftValueType::RecordRef(name) => Self::Ref(name.to_string()),
            CftValueType::Array(inner) => Self::Array(Box::new(Self::from_schema_type(inner))),
            CftValueType::Dict(key, value) => Self::Dict(
                Box::new(Self::from_schema_type(key)),
                Box::new(Self::from_schema_type(value)),
            ),
            CftValueType::Option(inner) => Self::Option(Box::new(Self::from_schema_type(inner))),
            CftValueType::Function(_, _) | CftValueType::Unit => Self::Unsupported(ty.to_string()),
        }
    }

    pub(super) fn parse(schema: &CftSchema, text: &str) -> Result<Self, CellValueDiagnostics> {
        let syntax = coflow_language::cft::syntax::parser::parse_type(text)
            .map_err(|error| invalid_declared_type(format!("{error:?}")))?;
        let value_type = schema
            .resolve_type_ref(&syntax)
            .map_err(invalid_declared_type)?;
        Ok(Self::from_schema_type(&value_type))
    }

    pub(super) fn display(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::FString => "fstring".to_string(),
            Self::Type(name) | Self::Enum(name) => name.clone(),
            Self::Ref(name) => name.clone(),
            Self::Array(inner) => format!("[{}]", inner.display()),
            Self::Dict(key, value) => format!("{{{}: {}}}", key.display(), value.display()),
            Self::Option(inner) => format!("{}?", inner.display()),
            Self::Unsupported(display) => display.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct FieldMeta {
    pub(super) name: String,
    pub(super) ty: CellType,
}

pub(super) fn full_fields(
    schema: &CftSchema,
    type_name: &str,
) -> Result<Vec<FieldMeta>, CellValueDiagnostics> {
    let Some(schema_type) = schema.resolve_type(type_name) else {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::UnknownType,
                message: format!("unknown type `{type_name}`"),
            }],
        });
    };
    Ok(schema_type.all_fields().map(field_meta).collect())
}

fn field_meta(field: &CftField) -> FieldMeta {
    FieldMeta {
        name: field.name.to_string(),
        ty: CellType::from_schema_type(&field.value_type),
    }
}
