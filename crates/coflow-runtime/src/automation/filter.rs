use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::{
    query::{compare_values, resolved_path},
    Failure, Result,
};
use crate::{CfdPath, CfdRecord, CfdValue, ReadOnlyProjectSession};
use coflow_language::cft::CftValueType;
use coflow_language::lexical::{tokenize_lossless, LosslessToken};

#[derive(Debug)]
enum Expression {
    Constant(bool),
    Boolean(String),
    Not(Box<Self>),
    All(Vec<Self>),
    Any(Vec<Self>),
    Compare(String, usize, Comparison, String),
    Contains(String, String),
    State(String, State),
}

#[derive(Debug, Clone, Copy)]
enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
#[derive(Debug, Clone, Copy)]
enum State {
    Some,
    None,
    Reachable,
}

/// 过滤条件先绑定 Schema 与字段路径；遍历记录时不再解析字面量或检查类型。
#[derive(Debug)]
pub(crate) struct Filter {
    predicate: Predicate,
}

#[derive(Debug)]
enum Predicate {
    Constant(bool),
    Boolean(CfdPath),
    Not(Box<Filter>),
    All(Vec<Filter>),
    Any(Vec<Filter>),
    Compare(CfdPath, bool, Comparison, CfdValue),
    Contains(CfdPath, CfdValue),
    ContainsKey(CfdPath, crate::CfdDictKey),
    State(CfdPath, State),
}

pub(crate) fn compile(
    session: &ReadOnlyProjectSession,
    types: &[String],
    records: &[CfdRecord],
    source: &str,
) -> Result<BTreeMap<(String, String), Filter>> {
    if source.len() > 65536 {
        return Err(Failure::request("filter structural budget exceeded"));
    }
    let mut parser = Parser {
        source,
        tokens: tokenize_lossless(source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .collect(),
        offset: 0,
    };
    let expression = parser.any(0)?;
    if parser.offset != parser.tokens.len() {
        return Err(Failure::request("unexpected token after filter expression"));
    }
    if types.is_empty() && expression.needs_type() {
        return Err(Failure::request(
            "field filter requires a selected record type",
        ));
    }
    // 所有路径与字面量在遍历前完成绑定；零样本类型也必须通过声明检查。
    for ty in types.iter().filter(|ty| {
        !records
            .iter()
            .any(|record| record.actual_type() == ty.as_str())
    }) {
        expression.bind(session, ty, None)?;
    }
    records
        .iter()
        .map(|record| {
            let filter = expression
                .bind(session, record.actual_type(), Some(record))
                .map_err(|mut failure| {
                    failure.status = super::Status::Error;
                    failure.error_category = Some(super::ErrorCategory::Request);
                    failure
                })?;
            Ok((
                (record.actual_type().to_string(), record.id().to_string()),
                filter,
            ))
        })
        .collect()
}

impl Expression {
    fn needs_type(&self) -> bool {
        match self {
            Self::Constant(_) => false,
            Self::Not(inner) => inner.needs_type(),
            Self::All(items) | Self::Any(items) => items.iter().any(Self::needs_type),
            _ => true,
        }
    }
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    fn bind(
        &self,
        session: &ReadOnlyProjectSession,
        ty: &str,
        record: Option<&CfdRecord>,
    ) -> Result<Filter> {
        let field = |name: &str| -> Result<(CfdPath, CftValueType)> {
            let segments = match record {
                Some(record) => super::query::resolved_record_path(session, record, name)?,
                None => resolved_path(session, ty, name)?,
            };
            let expected = record
                .map_or_else(
                    || {
                        crate::write_rules::expected_type_for_cfd_path(
                            session.schema(),
                            ty,
                            &segments,
                            "CLI-FILTER",
                            "CLI",
                        )
                    },
                    |record| {
                        crate::write_rules::expected_type_for_record_path(
                            session.schema(),
                            record,
                            &segments,
                            "CLI-FILTER",
                            "CLI",
                        )
                    },
                )
                .map_err(|diagnostics| Failure {
                    status: super::Status::Error,
                    error_category: Some(super::ErrorCategory::Request),
                    diagnostics,
                    result: None,
                })?;
            Ok((CfdPath { segments }, expected))
        };
        Ok(Filter {
            predicate: match self {
                Self::Constant(value) => Predicate::Constant(*value),
                Self::Not(inner) => Predicate::Not(Box::new(inner.bind(session, ty, record)?)),
                Self::All(items) => Predicate::All(
                    items
                        .iter()
                        .map(|item| item.bind(session, ty, record))
                        .collect::<Result<_>>()?,
                ),
                Self::Any(items) => Predicate::Any(
                    items
                        .iter()
                        .map(|item| item.bind(session, ty, record))
                        .collect::<Result<_>>()?,
                ),
                Self::Boolean(name) => {
                    let (path, expected) = field(name)?;
                    if expected != CftValueType::Bool {
                        return Err(Failure::request("filter requires a boolean field"));
                    }
                    Predicate::Boolean(path)
                }
                Self::State(name, state) => {
                    let (path, expected) = field(name)?;
                    if !matches!(state, State::Reachable)
                        && !matches!(expected, CftValueType::Option(_))
                    {
                        return Err(Failure::request(
                            "is_some and is_none require an Option field",
                        ));
                    }
                    Predicate::State(path, *state)
                }
                Self::Compare(name, negations, comparison, literal) => {
                    let (path, expected) = field(name)?;
                    if *negations > 0 && expected != CftValueType::Bool {
                        return Err(Failure::request(
                            "boolean negation requires a boolean field",
                        ));
                    }
                    let value = super::values::expression(session.schema(), &expected, literal)?;
                    if !matches!(comparison, Comparison::Equal | Comparison::NotEqual)
                        && !matches!(
                            expected,
                            CftValueType::Int
                                | CftValueType::Long
                                | CftValueType::Float
                                | CftValueType::Double
                                | CftValueType::String
                                | CftValueType::Enum(_)
                        )
                    {
                        return Err(Failure::request(
                            "ordered comparison requires a number, enum or string",
                        ));
                    }
                    Predicate::Compare(path, negations % 2 == 1, *comparison, value)
                }
                Self::Contains(name, literal) => {
                    let (path, expected) = field(name)?;
                    let element = match &expected {
                        CftValueType::String => &CftValueType::String,
                        CftValueType::Array(inner) | CftValueType::Dict(inner, _) => inner.as_ref(),
                        _ => {
                            return Err(Failure::request(
                                "contains requires a string, array or dictionary field",
                            ))
                        }
                    };
                    let value = super::values::expression(session.schema(), element, literal)?;
                    if matches!(expected, CftValueType::Dict(_, _)) {
                        Predicate::ContainsKey(path, super::model_value::dict_key(value)?)
                    } else {
                        Predicate::Contains(path, value)
                    }
                }
            },
        })
    }
}

impl Filter {
    pub(crate) fn matches(&self, record: &CfdRecord) -> bool {
        match &self.predicate {
            Predicate::Constant(value) => *value,
            Predicate::Boolean(path) => {
                matches!(record.value_at_path(path), Some(CfdValue::Bool(true)))
            }
            Predicate::Not(inner) => !inner.matches(record),
            Predicate::All(items) => items.iter().all(|item| item.matches(record)),
            Predicate::Any(items) => items.iter().any(|item| item.matches(record)),
            Predicate::State(path, state) => {
                let value = record.value_at_path(path);
                match state {
                    State::Some => matches!(value, Some(CfdValue::OptionSome(_))),
                    State::None => matches!(value, Some(CfdValue::OptionNone)),
                    State::Reachable => value.is_some(),
                }
            }
            Predicate::Compare(path, negated, comparison, other) => {
                // 不可达不等同于 None；包括 != 在内的比较均返回假。
                let Some(value) = record.value_at_path(path) else {
                    return false;
                };
                let negation;
                let value = if *negated {
                    let CfdValue::Bool(value) = value else {
                        return false;
                    };
                    negation = CfdValue::Bool(!value);
                    &negation
                } else {
                    value
                };
                let order = compare_values(Some(value), Some(other));
                match comparison {
                    Comparison::Equal => equal(value, other),
                    Comparison::NotEqual => !equal(value, other),
                    Comparison::Less => order == Ordering::Less,
                    Comparison::LessEqual => order != Ordering::Greater,
                    Comparison::Greater => order == Ordering::Greater,
                    Comparison::GreaterEqual => order != Ordering::Less,
                }
            }
            Predicate::ContainsKey(path, other) => {
                matches!(record.value_at_path(path), Some(CfdValue::Dict(entries)) if entries.iter().any(|(key, _)| key == other))
            }
            Predicate::Contains(path, other) => match record.value_at_path(path) {
                Some(CfdValue::String(value)) => {
                    matches!(other, CfdValue::String(needle) if value.contains(needle))
                }
                Some(CfdValue::FormattedString(value)) => {
                    matches!(other, CfdValue::String(needle) if value.rendered.contains(needle))
                }
                Some(CfdValue::Array(values)) => values.iter().any(|value| equal(value, other)),

                _ => false,
            },
        }
    }
}

fn equal(left: &CfdValue, right: &CfdValue) -> bool {
    match (left, right) {
        (CfdValue::FormattedString(left), CfdValue::FormattedString(right)) => {
            left.rendered == right.rendered
        }
        (CfdValue::FormattedString(left), CfdValue::String(right)) => &left.rendered == right,
        (CfdValue::String(left), CfdValue::FormattedString(right)) => left == &right.rendered,
        _ => left == right,
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<LosslessToken>,
    offset: usize,
}
impl Parser<'_> {
    fn token(&self) -> Option<&str> {
        self.tokens
            .get(self.offset)
            .map(|token| token.text(self.source))
    }
    fn take(&mut self, value: &str) -> bool {
        if self.token() == Some(value) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, value: &str) -> Result<()> {
        if self.take(value) {
            Ok(())
        } else {
            Err(Failure::request(format!("filter requires `{value}`")))
        }
    }
    fn any(&mut self, depth: usize) -> Result<Expression> {
        let mut items = vec![self.all(depth)?];
        while self.take("||") {
            items.push(self.all(depth)?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expression::Any(items)
        })
    }
    fn all(&mut self, depth: usize) -> Result<Expression> {
        let mut items = vec![self.atom(depth)?];
        while self.take("&&") {
            items.push(self.atom(depth)?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expression::All(items)
        })
    }
    fn atom(&mut self, depth: usize) -> Result<Expression> {
        if depth > 64 {
            return Err(Failure::request("filter structural budget exceeded"));
        }
        if self.take("!") {
            let grouped = self.token() == Some("(");
            let inner = self.atom(depth + 1)?;
            // ! 的优先级高于比较；括号中的完整谓词仍按整体取反。
            return Ok(match inner {
                Expression::Compare(path, negations, comparison, literal) if !grouped => {
                    Expression::Compare(path, negations + 1, comparison, literal)
                }
                inner => Expression::Not(Box::new(inner)),
            });
        }
        if self.take("(") {
            let expression = self.any(depth + 1)?;
            self.require(")")?;
            return Ok(expression);
        }
        if self.take("true") {
            return Ok(Expression::Constant(true));
        }
        if self.take("false") {
            return Ok(Expression::Constant(false));
        }
        for function in ["contains", "is_some", "is_none", "is_reachable"] {
            if self.token() == Some(function)
                && self
                    .tokens
                    .get(self.offset + 1)
                    .is_some_and(|token| token.text(self.source) == "(")
            {
                self.offset += 2;
                let path = self.path()?;
                if function == "contains" {
                    self.require(",")?;
                    let literal = self.literal()?;
                    self.require(")")?;
                    return Ok(Expression::Contains(path, literal));
                }
                self.require(")")?;
                return Ok(Expression::State(
                    path,
                    match function {
                        "is_some" => State::Some,
                        "is_none" => State::None,
                        _ => State::Reachable,
                    },
                ));
            }
        }
        let path = self.path()?;
        for (token, comparison) in [
            ("==", Comparison::Equal),
            ("!=", Comparison::NotEqual),
            ("<=", Comparison::LessEqual),
            (">=", Comparison::GreaterEqual),
            ("<", Comparison::Less),
            (">", Comparison::Greater),
        ] {
            if self.take(token) {
                return Ok(Expression::Compare(path, 0, comparison, self.literal()?));
            }
        }
        Ok(Expression::Boolean(path))
    }
    fn path(&mut self) -> Result<String> {
        let start = self.offset;
        let first = self
            .tokens
            .get(self.offset)
            .ok_or_else(|| Failure::request("filter requires a field path"))?;
        if first.kind != coflow_language::lexical::LosslessTokenKind::Identifier {
            return Err(Failure::request("filter requires a field path"));
        }
        self.offset += 1;
        loop {
            if self.take(".") {
                let field = self
                    .tokens
                    .get(self.offset)
                    .ok_or_else(|| Failure::request("field path requires a field after the dot"))?;
                if field.kind != coflow_language::lexical::LosslessTokenKind::Identifier {
                    return Err(Failure::request("invalid field path"));
                }
                self.offset += 1;
            } else if self.take("[") {
                let mut nesting = 1;
                while nesting > 0 {
                    let token = self
                        .token()
                        .ok_or_else(|| Failure::request("unclosed field subscript"))?;
                    if token == "[" {
                        nesting += 1;
                    }
                    if token == "]" {
                        nesting -= 1;
                    }
                    self.offset += 1;
                }
            } else {
                break;
            }
        }
        // 表达式中的路径允许 token 间空白；字符串键内的空白仍由原始 token 保留。
        let path: String = self.tokens[start..self.offset]
            .iter()
            .map(|token| token.text(self.source))
            .collect();
        super::execute::public_path(&path)?;
        Ok(path)
    }
    fn literal(&mut self) -> Result<String> {
        let start = self
            .tokens
            .get(self.offset)
            .ok_or_else(|| Failure::request("filter requires a CFD literal"))?
            .span
            .start;
        let mut stack = Vec::new();
        let first = self.offset;
        while let Some(token) = self.token() {
            if stack.is_empty() && matches!(token, ")" | "&&" | "||" | ",") {
                break;
            }
            match token {
                "(" => stack.push(")"),
                "[" => stack.push("]"),
                "{" => stack.push("}"),
                ")" | "]" | "}" if stack.pop() != Some(token) => {
                    return Err(Failure::request("mismatched CFD literal delimiter"));
                }
                _ => {}
            }
            self.offset += 1;
        }
        if self.offset == first || !stack.is_empty() {
            return Err(Failure::request("incomplete CFD literal"));
        }
        Ok(self.source[start..self.tokens[self.offset - 1].span.end].to_string())
    }
}
