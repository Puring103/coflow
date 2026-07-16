use coflow_cft::{CftField, CftSchema, CftValueType};

use super::diagnostics::{
    invalid_declared_type, CellValueDiagnostic, CellValueDiagnostics, CellValueErrorCode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CellType {
    Int,
    Float,
    Bool,
    String,
    Type(String),
    Ref(String),
    Enum(String),
    Array(Box<CellType>),
    Dict(Box<CellType>, Box<CellType>),
    Nullable(Box<CellType>),
}

impl CellType {
    pub(super) fn from_schema_type(ty: &CftValueType) -> Self {
        match ty {
            CftValueType::Int => Self::Int,
            CftValueType::Float => Self::Float,
            CftValueType::Bool => Self::Bool,
            CftValueType::String => Self::String,
            CftValueType::Object(name) => Self::Type(name.to_string()),
            CftValueType::Enum(name) => Self::Enum(name.to_string()),
            CftValueType::RecordRef(name) => Self::Ref(name.to_string()),
            CftValueType::Array(inner) => Self::Array(Box::new(Self::from_schema_type(inner))),
            CftValueType::Dict(key, value) => Self::Dict(
                Box::new(Self::from_schema_type(key)),
                Box::new(Self::from_schema_type(value)),
            ),
            CftValueType::Nullable(inner) => {
                Self::Nullable(Box::new(Self::from_schema_type(inner)))
            }
        }
    }

    pub(super) fn parse(schema: &CftSchema, text: &str) -> Result<Self, CellValueDiagnostics> {
        let mut parser = TypeParser::new(schema, text);
        let ty = parser.parse_type()?;
        parser.skip_ws();
        if parser.is_eof() {
            Ok(ty)
        } else {
            Err(invalid_declared_type("unexpected text after type"))
        }
    }

    pub(super) fn display(&self) -> String {
        match self {
            Self::Int => "int".to_string(),
            Self::Float => "float".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::Type(name) | Self::Enum(name) => name.clone(),
            Self::Ref(name) => format!("&{name}"),
            Self::Array(inner) => format!("[{}]", inner.display()),
            Self::Dict(key, value) => format!("{{{}: {}}}", key.display(), value.display()),
            Self::Nullable(inner) => format!("{}?", inner.display()),
        }
    }
}

struct TypeParser<'a> {
    schema: &'a CftSchema,
    text: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(schema: &'a CftSchema, text: &'a str) -> Self {
        Self {
            schema,
            text,
            pos: 0,
        }
    }

    fn parse_type(&mut self) -> Result<CellType, CellValueDiagnostics> {
        self.skip_ws();
        let mut ty = self.parse_primary()?;
        self.skip_ws();
        while self.eat('?') {
            ty = CellType::Nullable(Box::new(ty));
            self.skip_ws();
        }
        Ok(ty)
    }

    fn parse_primary(&mut self) -> Result<CellType, CellValueDiagnostics> {
        self.skip_ws();
        if self.eat('&') {
            let name = self.parse_name();
            if name.is_empty() {
                return Err(invalid_declared_type(
                    "reference type is missing target type",
                ));
            }
            if self.schema.resolve_type(&name).is_none() {
                return Err(invalid_declared_type(format!(
                    "reference target `{name}` is not an object type"
                )));
            }
            return Ok(CellType::Ref(name));
        }
        if self.eat('[') {
            let inner = self.parse_type()?;
            self.skip_ws();
            if !self.eat(']') {
                return Err(invalid_declared_type("array type is missing `]`"));
            }
            return Ok(CellType::Array(Box::new(inner)));
        }
        if self.eat('{') {
            let key = self.parse_type()?;
            self.skip_ws();
            if !self.eat(':') {
                return Err(invalid_declared_type("dict type is missing `:`"));
            }
            let value = self.parse_type()?;
            self.skip_ws();
            if !self.eat('}') {
                return Err(invalid_declared_type("dict type is missing `}`"));
            }
            return Ok(CellType::Dict(Box::new(key), Box::new(value)));
        }

        let name = self.parse_name();
        if name.is_empty() {
            return Err(invalid_declared_type("expected type name"));
        }
        Ok(match name.as_str() {
            "int" => CellType::Int,
            "float" => CellType::Float,
            "bool" => CellType::Bool,
            "string" => CellType::String,
            other if self.schema.resolve_enum(other).is_some() => CellType::Enum(other.to_string()),
            other => CellType::Type(other.to_string()),
        })
    }

    fn parse_name(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while let Some(ch) = self.peek() {
            if matches!(
                ch,
                '[' | ']' | '{' | '}' | ':' | '?' | ' ' | '\t' | '\r' | '\n'
            ) {
                break;
            }
            self.pos += ch.len_utf8();
        }
        self.text[start..self.pos].to_string()
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.pos == self.text.len()
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
