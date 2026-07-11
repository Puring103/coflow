mod lexer;
mod schema;
mod value;

use coflow_cft::CompiledSchema;
use coflow_data_model::{CfdInputRecord, CfdInputValue};

use crate::{CfdTextDiagnostic, CfdTextDiagnostics, CfdTextErrorCode, CfdTextSpan};
use lexer::{NameTokenKind, ScalarToken};
use schema::{
    validate_actual_type, validate_group_type, validate_record_key, validate_record_type,
    ParsedObjectFields,
};

pub(super) struct Parser<'a> {
    schema: &'a CompiledSchema,
    source: &'a str,
    pos: usize,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedCfdInputRecord {
    pub(super) record: CfdInputRecord,
    pub(super) span: CfdTextSpan,
}

impl<'a> Parser<'a> {
    pub(super) fn new(schema: &'a CompiledSchema, source: &'a str) -> Self {
        Self {
            schema,
            source,
            pos: 0,
        }
    }

    pub(super) fn parse_records_with_spans(
        &mut self,
    ) -> Result<Vec<ParsedCfdInputRecord>, CfdTextDiagnostics> {
        let mut records = Vec::new();
        self.skip_ws_and_comments();
        while !self.is_eof() {
            let record_start = self.pos;
            let first = self.parse_key("record key or group type")?;
            self.skip_ws_and_comments();
            if self.eat_char(':') {
                self.validate_record_key(&first)?;
                let actual_type = self.parse_name("record type")?;
                self.validate_record_type(&actual_type)?;
                let parsed = self.parse_record_fields(&actual_type)?;
                records.push(ParsedCfdInputRecord {
                    record: CfdInputRecord::with_spreads(
                        first,
                        actual_type,
                        parsed.spreads,
                        parsed.fields,
                    ),
                    span: CfdTextSpan {
                        start: record_start,
                        end: self.pos,
                    },
                });
            } else if self.peek_char() == Some('{') {
                self.validate_group_type(&first)?;
                records.extend(self.parse_group_records(&first)?);
            } else {
                return Err(self.error(
                    CfdTextErrorCode::Syntax,
                    "expected record type separator `:` or group body `{`",
                ));
            }
            self.skip_ws_and_comments();
        }
        Ok(records)
    }

    fn validate_group_type(&self, type_name: &str) -> Result<(), CfdTextDiagnostics> {
        validate_group_type(self.schema, type_name, self.pos)
    }

    fn validate_record_type(&self, actual_type: &str) -> Result<(), CfdTextDiagnostics> {
        validate_record_type(self.schema, actual_type, self.pos)
    }

    fn parse_group_records(
        &mut self,
        group_type: &str,
    ) -> Result<Vec<ParsedCfdInputRecord>, CfdTextDiagnostics> {
        self.expect_char('{', "group start `{`")?;
        let mut records = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.eat_char('}') {
                break;
            }
            let record_start = self.pos;
            let key = self.parse_key("record key")?;
            self.validate_record_key(&key)?;
            self.skip_ws_and_comments();
            let actual_type = if self.eat_char(':') {
                let actual_type = self.parse_name("record type")?;
                validate_actual_type(self.schema, group_type, &actual_type, self.pos)?;
                actual_type
            } else {
                self.validate_record_type(group_type)?;
                group_type.to_string()
            };
            let parsed = self.parse_record_fields(&actual_type)?;
            records.push(ParsedCfdInputRecord {
                record: CfdInputRecord::with_spreads(
                    key,
                    actual_type,
                    parsed.spreads,
                    parsed.fields,
                ),
                span: CfdTextSpan {
                    start: record_start,
                    end: self.pos,
                },
            });
            self.skip_ws_and_comments();
            let _ = self.eat_char(',');
        }
        Ok(records)
    }

    fn parse_record_fields(
        &mut self,
        actual_type: &str,
    ) -> Result<ParsedObjectFields, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        let value = self.parse_object_value(actual_type)?;
        match value {
            CfdInputValue::Object { fields, .. } => Ok(ParsedObjectFields {
                spreads: Vec::new(),
                fields,
            }),
            CfdInputValue::ObjectSpread {
                spreads, fields, ..
            } => Ok(ParsedObjectFields { spreads, fields }),
            _ => Err(self.error(
                CfdTextErrorCode::TypeMismatch,
                "top-level record value must be an object",
            )),
        }
    }

    fn parse_key(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.peek_char() == Some('"') {
            return self.parse_quoted_string();
        }
        self.parse_name(label)
    }

    fn parse_name(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        lexer::parse_name(self.source, &mut self.pos, label, NameTokenKind::General)
    }

    fn parse_ref_name(&mut self, label: &str) -> Result<String, CfdTextDiagnostics> {
        lexer::parse_name(self.source, &mut self.pos, label, NameTokenKind::Reference)
    }

    fn parse_scalar_token(
        &mut self,
        label: &str,
    ) -> Result<(String, CfdTextSpan), CfdTextDiagnostics> {
        let ScalarToken { text, span } =
            lexer::parse_scalar_token(self.source, &mut self.pos, label)?;
        Ok((text, span))
    }

    fn parse_quoted_string(&mut self) -> Result<String, CfdTextDiagnostics> {
        lexer::parse_quoted_string(self.source, &mut self.pos)
    }

    fn skip_ws_and_comments(&mut self) {
        lexer::skip_ws_and_comments(self.source, &mut self.pos);
    }

    fn expect_char(&mut self, expected: char, label: &str) -> Result<(), CfdTextDiagnostics> {
        self.skip_ws_and_comments();
        if self.eat_char(expected) {
            Ok(())
        } else {
            Err(self.error(CfdTextErrorCode::Syntax, format!("expected {label}")))
        }
    }

    fn eat_char(&mut self, expected: char) -> bool {
        lexer::eat_char(self.source, &mut self.pos, expected)
    }

    fn eat_spread(&mut self) -> bool {
        lexer::eat_spread(self.source, &mut self.pos)
    }

    fn eat_keyword(&mut self, expected: &str) -> bool {
        lexer::eat_keyword(self.source, &mut self.pos, expected)
    }

    fn peek_keyword(&self, expected: &str) -> bool {
        lexer::peek_keyword(self.source, self.pos, expected)
    }

    fn peek_char(&self) -> Option<char> {
        lexer::peek_char(self.source, self.pos)
    }

    fn previous_char(&self) -> Option<char> {
        lexer::previous_char(self.source, self.pos)
    }

    fn is_eof(&self) -> bool {
        lexer::is_eof(self.source, self.pos)
    }

    fn error(&self, code: CfdTextErrorCode, message: impl Into<String>) -> CfdTextDiagnostics {
        CfdTextDiagnostics::one(CfdTextDiagnostic::error(
            code,
            message,
            CfdTextSpan {
                start: self.pos,
                end: self.pos,
            },
        ))
    }

    fn reference_needs_marker(&self, key: &str) -> CfdTextDiagnostics {
        self.error(
            CfdTextErrorCode::ReferenceNeedsMarker,
            format!("object reference `{key}` must be written as `&{key}`"),
        )
    }

    fn validate_record_key(&self, key: &str) -> Result<(), CfdTextDiagnostics> {
        validate_record_key(key, self.pos)
    }
}
