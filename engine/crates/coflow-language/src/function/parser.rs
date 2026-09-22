mod body;
mod types;
use super::*;
use crate::{
    lexical::{
        decode_string, is_cft_identifier, tokenize_lossless, validate_number_literal,
        LosslessToken, LosslessTokenKind,
    },
    limits::StructuralLimits,
    syntax::ast::{FunctionParameterRef, NameRef, TypeRefKind},
};

enum TemplateInput<'a> {
    Text(String),
    Expression(&'a str, usize),
}

type Parsed<T> = Result<T, SyntaxError>;

pub fn parse_function(source: &str) -> Parsed<Function> {
    parse_function_with_limits(source, StructuralLimits::default())
}
pub fn parse_function_with_limits(source: &str, limits: StructuralLimits) -> Parsed<Function> {
    let mut parser = Parser::new(source, 0, limits)?;
    let function = parser.function()?;
    parser.finish()?;
    Ok(function)
}
pub fn parse_expression(source: &str) -> Parsed<Expr> {
    let mut parser = Parser::new(source, 0, StructuralLimits::default())?;
    let expression = parser.expression(0, true)?;
    parser.finish()?;
    Ok(expression)
}

pub fn parse_checks(source: &str) -> Parsed<Vec<Check>> {
    let mut parser = Parser::new(source, 0, StructuralLimits::default())?;
    let mut checks = Vec::new();
    while !parser.peek().is_empty() {
        let start = parser.span().start;
        parser.expect("check")?;
        let name = if parser.peek() == "{" {
            None
        } else {
            Some(parser.identifier(false)?)
        };
        let body = parser.block()?;
        checks.push(Check {
            name,
            body,
            span: Span::new(start, parser.end()),
        });
    }
    Ok(checks)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<LosslessToken>,
    pos: usize,
    offset: usize,
    depth: u64,
    nodes: u64,
    limits: StructuralLimits,
}
impl<'a> Parser<'a> {
    fn new(source: &'a str, offset: usize, limits: StructuralLimits) -> Parsed<Self> {
        let tokens: Vec<_> = tokenize_lossless(source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .collect();
        if tokens.len() as u64 > limits.max_analysis_steps {
            return Err(SyntaxError {
                span: Span::new(offset, offset + source.len()),
                message: "函数分析工作量超限".into(),
            });
        }
        Ok(Self {
            source,
            tokens,
            pos: 0,
            offset,
            depth: 0,
            nodes: 0,
            limits,
        })
    }
    fn peek(&self) -> &'a str {
        self.tokens
            .get(self.pos)
            .map_or("", |t| t.text(self.source))
    }
    fn nth(&self, delta: usize) -> &'a str {
        self.tokens
            .get(self.pos + delta)
            .map_or("", |t| t.text(self.source))
    }
    fn span(&self) -> Span {
        self.tokens.get(self.pos).map_or(
            Span::new(
                self.offset + self.source.len(),
                self.offset + self.source.len(),
            ),
            |t| Span::new(self.offset + t.span.start, self.offset + t.span.end),
        )
    }
    fn end(&self) -> usize {
        self.pos
            .checked_sub(1)
            .and_then(|i| self.tokens.get(i))
            .map_or(self.offset, |t| self.offset + t.span.end)
    }
    fn error(&self, message: impl Into<String>) -> SyntaxError {
        SyntaxError {
            span: self.span(),
            message: message.into(),
        }
    }
    fn take(&mut self, text: &str) -> bool {
        if self.peek() == text && !text.is_empty() {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, text: &str) -> Parsed<()> {
        if self.take(text) {
            Ok(())
        } else {
            Err(self.error(format!("需要 `{text}`，实际为 `{}`", self.peek())))
        }
    }
    fn finish(&self) -> Parsed<()> {
        if self.pos == self.tokens.len() {
            Ok(())
        } else {
            Err(self.error("表达式后存在多余输入"))
        }
    }
    fn enter_structure(&mut self) -> Parsed<()> {
        if self.depth >= self.limits.max_depth || self.nodes >= self.limits.max_nodes {
            return Err(self.error("函数源码结构超限"));
        }
        self.depth += 1;
        self.nodes += 1;
        Ok(())
    }
    fn nested<T>(&mut self, run: impl FnOnce(&mut Self) -> Parsed<T>) -> Parsed<T> {
        self.enter_structure()?;
        let result = run(self);
        self.depth -= 1;
        result
    }
    fn identifier(&mut self, member: bool) -> Parsed<String> {
        let name = self.peek();
        let valid = self
            .tokens
            .get(self.pos)
            .is_some_and(|t| t.kind == LosslessTokenKind::Identifier)
            && if member {
                crate::lexical::is_identifier(name)
            } else {
                is_cft_identifier(name)
            };
        if !valid {
            return Err(self.error("需要标识符"));
        }
        self.pos += 1;
        Ok(name.into())
    }
    fn name(&mut self) -> Parsed<String> {
        let mut name = self.identifier(true)?;
        while self.take("::") {
            name.push_str("::");
            name.push_str(&self.identifier(true)?);
        }
        Ok(name)
    }
    fn function(&mut self) -> Parsed<Function> {
        self.nested(|p| {
            let (start, parameters, result) = p.function_header()?;
            let body = p.block()?;
            Ok(Function {
                parameters,
                result,
                body,
                span: Span::new(start, p.end()),
            })
        })
    }
    fn function_header(&mut self) -> Parsed<(usize, Vec<(String, TypeRef)>, TypeRef)> {
        let start = self.span().start;
        self.expect("fn")?;
        self.expect("(")?;
        let mut parameters = Vec::new();
        while !self.take(")") {
            let name = self.identifier(false)?;
            self.expect(":")?;
            let ty = self.ty()?;
            parameters.push((name, ty));
            if !self.take(",") {
                self.expect(")")?;
                break;
            }
        }
        self.expect("->")?;
        let result = self.ty()?;
        self.take("=>");
        Ok((start, parameters, result))
    }
    fn atom(&mut self) -> Parsed<Expr> {
        let start = self.span().start;
        let token = self.peek();
        let kind = match token {
            "true" | "false" => {
                self.pos += 1;
                ExprKind::Bool(token == "true")
            }
            "None" => {
                self.pos += 1;
                ExprKind::None
            }
            "inf" => {
                self.pos += 1;
                ExprKind::Number(token.into())
            }
            "&" => {
                self.pos += 1;
                let name = self.name()?;
                let (type_name, key) = name
                    .rsplit_once("::")
                    .map_or((None, name.clone()), |(ty, key)| {
                        (Some(ty.into()), key.into())
                    });
                ExprKind::Reference { type_name, key }
            }
            _ => {
                let Some(current) = self.tokens.get(self.pos).copied() else {
                    return Err(self.error("需要表达式"));
                };
                if current.kind == LosslessTokenKind::Number {
                    validate_number_literal(token).map_err(|error| SyntaxError {
                        span: self.span(),
                        message: error.message,
                    })?;
                    self.pos += 1;
                    ExprKind::Number(token.into())
                } else if current.kind == LosslessTokenKind::String {
                    self.pos += 1;
                    ExprKind::String(decode_string(token).map_err(|error| SyntaxError {
                        span: Span::new(start, self.end()),
                        message: error.message,
                    })?)
                } else if current.kind == LosslessTokenKind::Identifier {
                    let name = self.name()?;
                    ExprKind::Name(name)
                } else {
                    return Err(self.error("需要表达式"));
                }
            }
        };
        Ok(Expr {
            kind,
            span: Span::new(start, self.end()),
        })
    }
    fn template(&mut self, source: &'a str, base: usize) -> Parsed<Vec<TemplateInput<'a>>> {
        crate::lexical::validate_formatted_string_literal(source).map_err(|e| SyntaxError {
            span: Span::new(base + e.offset, base + e.offset),
            message: e.message,
        })?;
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut pos = 2;
        while pos < source.len() - 1 {
            if source[pos..].starts_with("{{") || source[pos..].starts_with("}}") {
                text.push(source.as_bytes()[pos] as char);
                pos += 2;
            } else if source.as_bytes()[pos] == b'{' {
                if !text.is_empty() {
                    parts.push(TemplateInput::Text(std::mem::take(&mut text)));
                }
                let end = crate::lexical::scan_balanced_delimiter(source, pos, '{', '}')
                    .ok_or_else(|| self.error("插值缺少结束花括号"))?;
                parts.push(TemplateInput::Expression(
                    &source[pos + 1..end - 1],
                    base + pos + 1,
                ));
                pos = end;
            } else if source.as_bytes()[pos] == b'\\' {
                let begin = pos;
                pos += 1;
                if source.as_bytes()[pos] == b'u' {
                    while source.as_bytes()[pos] != b'}' {
                        pos += 1;
                    }
                }
                pos += 1;
                let quoted = format!("\"{}\"", &source[begin..pos]);
                text.push_str(&decode_string(&quoted).map_err(|e| SyntaxError {
                    span: Span::new(base + begin, base + pos),
                    message: e.message,
                })?);
            } else {
                let Some(ch) = source[pos..].chars().next() else {
                    return Err(self.error("无效模板"));
                };
                text.push(ch);
                pos += ch.len_utf8();
            }
        }
        if !text.is_empty() {
            parts.push(TemplateInput::Text(text));
        }
        Ok(parts)
    }
}
fn precedence_of_comparison(operator: &str) -> bool {
    matches!(operator, "==" | "!=" | "<" | "<=" | ">" | ">=")
}
fn precedence(operator: &str) -> Option<(u8, bool)> {
    Some((
        match operator {
            "||" => 1,
            "&&" => 2,
            "==" | "!=" | "<" | "<=" | ">" | ">=" => 4,
            ".." | "..=" => 5,
            "|" => 6,
            "^" => 7,
            "&" => 8,
            "<<" | ">>" => 9,
            "+" | "-" => 10,
            "*" | "/" | "//" | "%" => 11,
            "**" => 13,
            _ => return None,
        },
        operator == "**",
    ))
}
