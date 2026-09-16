use super::*;
use crate::{
    lexical::{
        decode_string, is_cft_identifier, tokenize_lossless, validate_number_literal,
        LosslessToken, LosslessTokenKind,
    },
    limits::StructuralLimits,
    syntax::ast::{FunctionParameterRef, NameRef, TypeRefKind},
};

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
    fn nested<T>(&mut self, run: impl FnOnce(&mut Self) -> Parsed<T>) -> Parsed<T> {
        if self.depth >= self.limits.max_depth || self.nodes >= self.limits.max_nodes {
            return Err(self.error("函数源码结构超限"));
        }
        self.depth += 1;
        self.nodes += 1;
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
    fn ty(&mut self) -> Parsed<TypeRef> {
        self.nested(|p| {
            let start = p.span().start;
            let mut kind = if p.take("[") {
                let inner = p.ty()?;
                p.expect("]")?;
                TypeRefKind::Array(Box::new(inner))
            } else if p.take("{") {
                let key = p.ty()?;
                p.expect(":")?;
                let value = p.ty()?;
                p.expect("}")?;
                TypeRefKind::Dict(Box::new(key), Box::new(value))
            } else if p.take("(") {
                if p.take(")") {
                    TypeRefKind::Unit
                } else {
                    let inner = p.ty()?;
                    p.expect(")")?;
                    inner.kind
                }
            } else if p.take("fn") {
                p.expect("(")?;
                let mut parameters = Vec::new();
                while !p.take(")") {
                    let name = if p.nth(1) == ":" {
                        let span = p.span();
                        let name = p.identifier(false)?;
                        p.expect(":")?;
                        Some(NameRef { name, span })
                    } else {
                        None
                    };
                    parameters.push(FunctionParameterRef {
                        name,
                        value_type: p.ty()?,
                    });
                    if !p.take(",") {
                        p.expect(")")?;
                        break;
                    }
                }
                p.expect("->")?;
                TypeRefKind::Function(parameters, Box::new(p.ty()?))
            } else {
                match p.name()?.as_str() {
                    "int" => TypeRefKind::Int,
                    "float" => TypeRefKind::Float,
                    "bool" => TypeRefKind::Bool,
                    "string" => TypeRefKind::String,
                    "fstring" => TypeRefKind::FString,
                    name => TypeRefKind::Named(name.into()),
                }
            };
            if p.take("?") {
                kind = TypeRefKind::Option(Box::new(TypeRef {
                    kind,
                    span: Span::new(start, p.end() - 1),
                }));
            }
            Ok(TypeRef {
                kind,
                span: Span::new(start, p.end()),
            })
        })
    }
    fn function(&mut self) -> Parsed<Function> {
        self.nested(|p| {
            let start = p.span().start;
            p.expect("fn")?;
            p.expect("(")?;
            let mut parameters = Vec::new();
            while !p.take(")") {
                let name = p.identifier(false)?;
                p.expect(":")?;
                let ty = p.ty()?;
                parameters.push((name, ty));
                if !p.take(",") {
                    p.expect(")")?;
                    break;
                }
            }
            p.expect("->")?;
            let result = p.ty()?;
            p.take("=>");
            let body = p.block()?;
            Ok(Function {
                parameters,
                result,
                body,
                span: Span::new(start, p.end()),
            })
        })
    }
    fn block(&mut self) -> Parsed<Block> {
        self.nested(|p| {
            let start = p.span().start;
            p.expect("{")?;
            let mut statements = Vec::new();
            let mut tail = None;
            while !p.take("}") {
                let position = p.span().start;
                let kind = if p.take("var") {
                    let name = p.identifier(false)?;
                    p.expect(":")?;
                    let ty = p.ty()?;
                    p.expect("=")?;
                    let value = p.expression(0, true)?;
                    p.expect(";")?;
                    StatementKind::Variable { name, ty, value }
                } else if p.take("while") {
                    let condition = p.expression(0, false)?;
                    StatementKind::While {
                        condition,
                        body: p.block()?,
                    }
                } else if p.take("for") {
                    let mut bindings = vec![p.identifier(false)?];
                    if p.take(",") {
                        bindings.push(p.identifier(false)?);
                    }
                    p.expect("in")?;
                    let iterable = p.expression(0, false)?;
                    StatementKind::For {
                        bindings,
                        iterable,
                        body: p.block()?,
                    }
                } else if p.take("break") {
                    p.expect(";")?;
                    StatementKind::Break
                } else if p.take("continue") {
                    p.expect(";")?;
                    StatementKind::Continue
                } else if matches!(
                    p.nth(1),
                    "=" | "+="
                        | "-="
                        | "*="
                        | "/="
                        | "%="
                        | "//="
                        | "**="
                        | "<<="
                        | ">>="
                        | "&="
                        | "|="
                        | "^="
                ) {
                    let name = p.identifier(false)?;
                    let operator = p.peek().to_string();
                    p.pos += 1;
                    let value = p.expression(0, true)?;
                    p.expect(";")?;
                    StatementKind::Assign {
                        name,
                        operator,
                        value,
                    }
                } else {
                    let expression = p.expression(0, true)?;
                    if p.take(";") {
                        StatementKind::Expression(expression)
                    } else if p.take("}") {
                        tail = Some(Box::new(expression));
                        break;
                    } else if matches!(expression.kind, ExprKind::If { .. }) {
                        StatementKind::Expression(expression)
                    } else {
                        return Err(p.error("表达式语句需要分号"));
                    }
                };
                statements.push(Statement {
                    kind,
                    span: Span::new(position, p.end()),
                });
                if statements.len() as u64 >= p.limits.max_nodes {
                    return Err(p.error("函数语句数超限"));
                }
            }
            Ok(Block {
                statements,
                tail,
                span: Span::new(start, p.end()),
            })
        })
    }
    fn expression(&mut self, minimum: u8, object_allowed: bool) -> Parsed<Expr> {
        self.nested(|p| {
            let mut left = p.prefix(object_allowed)?;
            let mut chain = 0;
            loop {
                // Pratt 左链也限制深度，避免合法解析结果在遍历或释放时耗尽宿主栈。
                chain += 1;
                if chain + p.depth > p.limits.max_depth { return Err(p.error("表达式嵌套超限")); }
                let start = left.span.start;
                let kind = if minimum <= 15 && p.take(".") {
                    ExprKind::Field { value: Box::new(left), name: p.identifier(true)? }
                } else if minimum <= 15 && p.take("[") {
                    let index = p.expression(0, true)?; p.expect("]")?;
                    ExprKind::Index { value: Box::new(left), index: Box::new(index) }
                } else if minimum <= 15 && p.take("(") {
                    let arguments = p.arguments(")")?;
                    ExprKind::Call { function: Box::new(left), arguments }
                } else if minimum <= 15 && p.take("?") {
                    if matches!(left.kind, ExprKind::Propagate(_)) { return Err(p.error("不能连续使用可选传播")); }
                    ExprKind::Propagate(Box::new(left))
                } else if minimum <= 3 && p.take("is") {
                    if p.take("Some") {
                        p.expect("(")?; let binding = p.identifier(false)?; p.expect(")")?;
                        ExprKind::IsSome { value: Box::new(left), binding }
                    } else { ExprKind::IsType { value: Box::new(left), name: p.name()? } }
                } else {
                    let operator = p.peek();
                    let Some((precedence, right_associative)) = precedence(operator) else { break; };
                    if precedence < minimum { break; }
                    if precedence == 4 && matches!(&left.kind, ExprKind::Binary { operator, .. } if precedence_of_comparison(operator)) {
                        return Err(p.error("不支持链式比较"));
                    }
                    p.pos += 1;
                    let right = p.expression(precedence + u8::from(!right_associative), object_allowed)?;
                    ExprKind::Binary { operator: operator.into(), left: Box::new(left), right: Box::new(right) }
                };
                left = Expr { kind, span: Span::new(start, p.end()) };
                p.nodes += 1;
                if p.nodes > p.limits.max_nodes { return Err(p.error("表达式节点数超限")); }
            }
            Ok(left)
        })
    }
    fn arguments(&mut self, closing: &str) -> Parsed<Vec<Expr>> {
        let mut arguments = Vec::new();
        while !self.take(closing) {
            arguments.push(self.expression(0, true)?);
            if !self.take(",") {
                self.expect(closing)?;
                break;
            }
        }
        Ok(arguments)
    }
    fn prefix(&mut self, object_allowed: bool) -> Parsed<Expr> {
        let start = self.span().start;
        let token = self.peek();
        let kind = match token {
            "fn" => ExprKind::Function(self.function()?),
            "if" => {
                self.pos += 1;
                let condition = self.expression(0, false)?;
                let then = self.block()?;
                let otherwise = if self.take("else") {
                    Some(Box::new(if self.peek() == "if" {
                        self.nested(|p| p.prefix(false))?
                    } else {
                        let block = self.block()?;
                        Expr {
                            span: block.span,
                            kind: ExprKind::Block(block),
                        }
                    }))
                } else {
                    None
                };
                ExprKind::If {
                    condition: Box::new(condition),
                    then,
                    otherwise,
                }
            }
            "return" => {
                self.pos += 1;
                let value = if matches!(self.peek(), "}" | ";" | "," | ")" | "") {
                    None
                } else {
                    Some(Box::new(self.expression(0, object_allowed)?))
                };
                ExprKind::Return(value)
            }
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
            "-" | "!" | "~" => {
                self.pos += 1;
                ExprKind::Unary {
                    operator: token.into(),
                    value: Box::new(self.expression(14, object_allowed)?),
                }
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
            "(" => {
                self.pos += 1;
                if self.take(")") {
                    ExprKind::Unit
                } else {
                    let inner = self.expression(0, true)?;
                    self.expect(")")?;
                    inner.kind
                }
            }
            "[" => {
                self.pos += 1;
                ExprKind::Array(self.arguments("]")?)
            }
            "{" => {
                self.pos += 1;
                let mut entries = Vec::new();
                while !self.take("}") {
                    let key = self.expression(0, true)?;
                    self.expect(":")?;
                    let value = self.expression(0, true)?;
                    entries.push((key, value));
                    if !self.take(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                ExprKind::Dictionary(entries)
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
                    if token.starts_with("f\"") {
                        ExprKind::Template(self.template(token, start)?)
                    } else {
                        ExprKind::String(decode_string(token).map_err(|error| SyntaxError {
                            span: Span::new(start, self.end()),
                            message: error.message,
                        })?)
                    }
                } else if current.kind == LosslessTokenKind::Identifier {
                    let name = self.name()?;
                    if object_allowed && self.take("{") {
                        let mut fields = Vec::new();
                        while !self.take("}") {
                            let field = self.identifier(true)?;
                            self.expect(":")?;
                            fields.push((field, self.expression(0, true)?));
                            if !self.take(",") {
                                self.expect("}")?;
                                break;
                            }
                        }
                        ExprKind::Object {
                            type_name: name,
                            fields,
                        }
                    } else {
                        ExprKind::Name(name)
                    }
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
    fn template(&mut self, source: &'a str, base: usize) -> Parsed<Vec<TemplatePart>> {
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
                    parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                }
                let end = crate::lexical::scan_balanced_delimiter(source, pos, '{', '}')
                    .ok_or_else(|| self.error("插值缺少结束花括号"))?;
                // 子解析器继承剩余结构预算，来源偏移仍指向原始模板中的表达式。
                let limits = StructuralLimits {
                    max_depth: self.limits.max_depth.saturating_sub(self.depth),
                    max_nodes: self.limits.max_nodes.saturating_sub(self.nodes),
                    ..self.limits
                };
                let mut parser = Parser::new(&source[pos + 1..end - 1], base + pos + 1, limits)?;
                let expression = parser.expression(0, true)?;
                parser.finish()?;
                self.nodes += parser.nodes;
                parts.push(TemplatePart::Expression(expression));
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
            parts.push(TemplatePart::Text(text));
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
