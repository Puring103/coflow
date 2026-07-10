use super::{schema::full_fields, validate_actual_type, Parser};
use crate::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode};
use coflow_cft::CftSchemaTypeRef;
use coflow_data_model::{CfdInputDictKey, CfdInputValue};
use std::collections::{BTreeMap, BTreeSet};

impl Parser<'_> {
    pub(super) fn parse_value(
        &mut self,
        ty: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if let CftSchemaTypeRef::Nullable(inner) = ty {
            if self.eat_keyword("null") {
                return Ok(CfdInputValue::Null);
            }
            return self.parse_value(inner);
        }
        if self.peek_keyword("null") {
            return Err(self.error(CfdTextErrorCode::TypeMismatch, "unexpected null value"));
        }
        if self.peek_char() == Some('@') {
            return Err(self.error(
                CfdTextErrorCode::Syntax,
                "typed and path references are no longer supported; use `&key`",
            ));
        }

        match ty {
            CftSchemaTypeRef::Int => self.parse_int(),
            CftSchemaTypeRef::Float => self.parse_float(),
            CftSchemaTypeRef::Bool => self.parse_bool(),
            CftSchemaTypeRef::String => self.parse_string_value(),
            CftSchemaTypeRef::Named(name) if self.schema.is_schema_enum(name) => {
                self.parse_enum(name)
            }
            CftSchemaTypeRef::Named(name) => self.parse_object_value(name),
            CftSchemaTypeRef::Ref(name) => self.parse_ref_value(name),
            CftSchemaTypeRef::Array(inner) => self.parse_array(inner),
            CftSchemaTypeRef::Dict(key, value) => self.parse_dict(key, value),
            CftSchemaTypeRef::Nullable(inner) => self.parse_value(inner),
        }
    }

    fn parse_int(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("int")?;
        let value = text.parse::<i64>().map_err(|_| {
            CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected int",
                span,
            ))
        })?;
        Ok(CfdInputValue::Int(value))
    }

    fn parse_float(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("float")?;
        let value = text.parse::<f64>().map_err(|_| {
            CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected float",
                span,
            ))
        })?;
        if !value.is_finite() {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "float value must be finite",
                span,
            )));
        }
        Ok(CfdInputValue::Float(value))
    }

    fn parse_bool(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (text, span) = self.parse_scalar_token("bool")?;
        match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(CfdInputValue::Bool(true)),
            "false" | "0" | "no" | "n" => Ok(CfdInputValue::Bool(false)),
            _ => Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                "expected bool",
                span,
            ))),
        }
    }

    fn parse_string_value(&mut self) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let value = if self.peek_char() == Some('"') {
            self.parse_quoted_string()?
        } else {
            let (text, _) = self.parse_scalar_token("string")?;
            text
        };
        Ok(CfdInputValue::String(value))
    }

    fn parse_enum(&mut self, enum_name: &str) -> Result<CfdInputValue, CfdTextDiagnostics> {
        let (raw, span) = self.parse_scalar_token("enum value")?;
        let variant = raw
            .strip_prefix(enum_name)
            .and_then(|rest| rest.strip_prefix('.'))
            .map_or(raw.as_str(), |variant| variant);
        let Some(schema_enum) = self.schema.enum_meta(enum_name) else {
            return Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::TypeMismatch,
                format!("expected enum `{enum_name}`"),
                span,
            )));
        };
        if schema_enum
            .all_variants
            .iter()
            .any(|schema_variant| schema_variant.name == variant)
        {
            Ok(CfdInputValue::enum_variant(enum_name, variant))
        } else {
            Err(CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                CfdTextErrorCode::InvalidEnumVariant,
                format!("unknown enum variant `{enum_name}.{variant}`"),
                span,
            )))
        }
    }

    fn parse_array(
        &mut self,
        inner: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.expect_char('[', "array start `[`")?;
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char(']') {
                break;
            }
            out.push(self.parse_value(inner)?);
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char(']');
                if self.previous_char() == Some(']') {
                    break;
                }
                continue;
            }
            self.expect_char(']', "array separator or closing `]`")?;
            break;
        }
        Ok(CfdInputValue::Array(out))
    }

    fn parse_dict(
        &mut self,
        key: &CftSchemaTypeRef,
        value: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.expect_char('{', "dict start `{`")?;
        let mut spreads = Vec::new();
        let mut out = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.eat_spread() {
                spreads.push(self.parse_spread_value(&CftSchemaTypeRef::Dict(
                    Box::new(key.clone()),
                    Box::new(value.clone()),
                ))?);
            } else {
                let entry_key = self.parse_dict_key(key)?;
                self.skip_ws_and_comments();
                self.expect_char(':', "dict key separator `:`")?;
                let entry_value = self.parse_value(value)?;
                out.push((entry_key, entry_value));
            }
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char('}');
                if self.previous_char() == Some('}') {
                    break;
                }
                continue;
            }
            self.expect_char('}', "dict separator or closing `}`")?;
            break;
        }
        if spreads.is_empty() {
            Ok(CfdInputValue::dict(out))
        } else {
            Ok(CfdInputValue::dict_spread(spreads, out))
        }
    }

    fn parse_dict_key(
        &mut self,
        ty: &CftSchemaTypeRef,
    ) -> Result<CfdInputDictKey, CfdTextDiagnostics> {
        match ty {
            CftSchemaTypeRef::String => {
                if self.peek_char() == Some('"') {
                    self.parse_quoted_string().map(CfdInputDictKey::String)
                } else {
                    let (text, _) = self.parse_scalar_token("dict string key")?;
                    Ok(CfdInputDictKey::String(text))
                }
            }
            CftSchemaTypeRef::Int => {
                let (text, span) = self.parse_scalar_token("dict int key")?;
                let value = text.parse::<i64>().map_err(|_| {
                    CfdTextDiagnostics::one(CfdTextDiagnostic::error(
                        CfdTextErrorCode::TypeMismatch,
                        "expected int dict key",
                        span,
                    ))
                })?;
                Ok(CfdInputDictKey::Int(value))
            }
            CftSchemaTypeRef::Named(enum_name) if self.schema.is_schema_enum(enum_name) => {
                let CfdInputValue::EnumVariant { variant, .. } = self.parse_enum(enum_name)? else {
                    return Err(self.error(CfdTextErrorCode::TypeMismatch, "expected enum key"));
                };
                Ok(CfdInputDictKey::enum_variant(enum_name, variant))
            }
            CftSchemaTypeRef::Nullable(inner) => self.parse_dict_key(inner),
            _ => Err(self.error(CfdTextErrorCode::TypeMismatch, "invalid dict key type")),
        }
    }

    pub(super) fn parse_object_value(
        &mut self,
        expected_type: &str,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if matches!(self.peek_char(), Some('@' | '&')) {
            return if self.peek_char() == Some('@') {
                Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "typed and path references are no longer supported; use `&key`",
                ))
            } else {
                Err(self.error(
                    CfdTextErrorCode::TypeMismatch,
                    "inline object fields do not accept record references",
                ))
            };
        }

        let actual_type = if self.peek_char() == Some('{') {
            None
        } else {
            let saved = self.pos;
            let marker = self.parse_name("object type or reference key")?;
            self.skip_ws_and_comments();
            if self.peek_char() == Some('{') {
                validate_actual_type(self.schema, expected_type, &marker, self.pos)?;
                Some(marker)
            } else {
                self.pos = saved;
                let key = self.parse_name("object reference")?;
                return Err(self.reference_needs_marker(&key));
            }
        };

        let value_type = actual_type.as_deref().unwrap_or(expected_type);
        let parsed = self.parse_object_fields(value_type)?;
        Ok(if let Some(actual_type) = actual_type {
            if parsed.spreads.is_empty() {
                CfdInputValue::object(actual_type, parsed.fields)
            } else {
                CfdInputValue::object_spread_with_actual_type(
                    actual_type,
                    parsed.spreads,
                    parsed.fields,
                )
            }
        } else if parsed.spreads.is_empty() {
            CfdInputValue::object_with_declared_type(parsed.fields)
        } else {
            CfdInputValue::object_spread(parsed.spreads, parsed.fields)
        })
    }

    fn parse_object_fields(
        &mut self,
        type_name: &str,
    ) -> Result<super::ParsedObjectFields, CfdTextDiagnostics> {
        let fields = full_fields(self.schema, type_name)?;
        let fields_by_name = fields
            .iter()
            .map(|field| (field.name.as_str(), field))
            .collect::<BTreeMap<_, _>>();
        self.expect_char('{', "object start `{`")?;
        let mut spreads = Vec::new();
        let mut out = BTreeMap::new();
        let mut seen = BTreeSet::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            if self.eat_spread() {
                spreads.push(
                    self.parse_spread_value(&CftSchemaTypeRef::Named(type_name.to_string()))?,
                );
                self.skip_ws_and_comments();
                if self.eat_char(',') {
                    self.skip_ws_and_comments();
                    self.eat_char('}');
                    if self.previous_char() == Some('}') {
                        break;
                    }
                    continue;
                }
                self.expect_char('}', "field separator or closing `}`")?;
                break;
            }
            let field_name = self.parse_key("field name")?;
            self.skip_ws_and_comments();
            if field_name == "check" && self.peek_char() == Some('{') {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "check blocks are not valid CFD data syntax",
                ));
            }
            if field_name == "id" {
                return Err(self.error(
                    CfdTextErrorCode::ReservedIdField,
                    "`id` is reserved for the record key",
                ));
            }
            if !seen.insert(field_name.clone()) {
                return Err(self.error(
                    CfdTextErrorCode::DuplicateField,
                    format!("duplicate field `{field_name}`"),
                ));
            }
            let Some(field) = fields_by_name.get(field_name.as_str()) else {
                return Err(self.error(
                    CfdTextErrorCode::UnknownField,
                    format!("unknown field `{field_name}` on type `{type_name}`"),
                ));
            };
            if !self.eat_char(':') {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "field value separator must be `:`",
                ));
            }
            let value = self.parse_value(&field.ty)?;
            out.insert(field_name, value);
            self.skip_ws_and_comments();
            if self.eat_char(',') {
                self.skip_ws_and_comments();
                self.eat_char('}');
                if self.previous_char() == Some('}') {
                    break;
                }
                continue;
            }
            self.expect_char('}', "field separator or closing `}`")?;
            break;
        }
        Ok(super::ParsedObjectFields {
            spreads,
            fields: out,
        })
    }

    fn parse_ref_value(
        &mut self,
        _expected_type: &str,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.eat_char('&') {
            let key = self.parse_ref_name("reference key")?;
            if self.peek_char().is_some_and(|ch| matches!(ch, '.' | '[')) {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "record references do not support paths",
                ));
            }
            self.validate_record_key(&key)?;
            return Ok(CfdInputValue::record_ref(key));
        }

        Err(self.error(
            CfdTextErrorCode::Syntax,
            "typed and path references are no longer supported; use `&key`",
        ))
    }

    fn parse_spread_value(
        &mut self,
        ty: &CftSchemaTypeRef,
    ) -> Result<CfdInputValue, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('&') {
            return self.parse_ref_value("");
        }
        self.parse_value(ty)
    }
}
