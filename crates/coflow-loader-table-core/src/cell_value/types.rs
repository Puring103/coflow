use coflow_cft::{CftFieldMeta, CftSchemaView};

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
    pub(super) fn parse(schema: &CftSchemaView, text: &str) -> Result<Self, CellValueDiagnostics> {
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
    schema: &'a CftSchemaView,
    text: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(schema: &'a CftSchemaView, text: &'a str) -> Self {
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
            if !self.schema.has_type(&name) {
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
            other if self.schema.is_schema_enum(other) => CellType::Enum(other.to_string()),
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
    schema: &CftSchemaView,
    type_name: &str,
) -> Result<Vec<FieldMeta>, CellValueDiagnostics> {
    let Some(fields) = schema.fields(type_name) else {
        return Err(CellValueDiagnostics {
            diagnostics: vec![CellValueDiagnostic {
                code: CellValueErrorCode::UnknownType,
                message: format!("unknown type `{type_name}`"),
            }],
        });
    };
    fields.map(|field| field_meta(schema, field)).collect()
}

fn field_meta(
    schema: &CftSchemaView,
    field: &CftFieldMeta,
) -> Result<FieldMeta, CellValueDiagnostics> {
    Ok(FieldMeta {
        name: field.name.clone(),
        ty: CellType::parse(schema, &field.raw_type)?,
    })
}
