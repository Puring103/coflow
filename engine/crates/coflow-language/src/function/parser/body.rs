//! 函数体状态机：表达式、语句块和插值使用显式续延，共用源码结构预算。
use super::*;
use crate::syntax::ast::TypeRef;

struct Results {
    expression: Option<Expr>,
    block: Option<Block>,
}

enum Task<'a> {
    Enter(u8, bool),
    Exit,
    Prefix(bool),
    Tail(u8, bool, u64),
    Unary(usize, String),
    Group(usize),
    Binary(Expr, String),
    Index(Expr),
    ListStart(List),
    ListNext(List, Vec<Expr>),
    DictStart(usize, Vec<(Expr, Expr)>),
    DictKey(usize, Vec<(Expr, Expr)>),
    DictValue(usize, Vec<(Expr, Expr)>, Expr),
    ObjectStart(usize, String, Vec<(String, Expr)>),
    ObjectValue(usize, String, Vec<(String, Expr)>, String),
    Return(usize),
    Template(
        usize,
        std::vec::IntoIter<TemplateInput<'a>>,
        Vec<TemplatePart>,
    ),
    TemplateValue(
        usize,
        std::vec::IntoIter<TemplateInput<'a>>,
        Vec<TemplatePart>,
        Parser<'a>,
    ),
    Function(usize, Vec<(String, TypeRef)>, TypeRef),
    IfCondition(usize),
    IfThen(usize, Expr),
    IfElse(usize, Expr, Block),
    BlockExpression,
    BuildSource(usize),
    BuildBody(usize, BuildSource, String),
    BlockStart,
    BlockNext(usize, Vec<Statement>),
    StatementDone(usize, Vec<Statement>, usize, StatementKind),
    Variable(usize, Vec<Statement>, usize, String, TypeRef),
    Assign(usize, Vec<Statement>, usize, String, String),
    StatementExpression(usize, Vec<Statement>, usize),
    Set(usize, Vec<Statement>, usize, Expr),
    LoopCondition(usize, Vec<Statement>, usize, Option<Vec<String>>),
    LoopBody(usize, Vec<Statement>, usize, Option<Vec<String>>, Expr),
}
enum List {
    Array(usize),
    Call(Expr),
}
impl List {
    fn closing(&self) -> &'static str {
        match self {
            Self::Array(_) => "]",
            Self::Call(_) => ")",
        }
    }
    fn finish(self, values: Vec<Expr>, end: usize) -> Expr {
        let (start, kind) = match self {
            Self::Array(start) => (start, ExprKind::Array(values)),
            Self::Call(function) => (
                function.span.start,
                ExprKind::Call {
                    function: Box::new(function),
                    arguments: values,
                },
            ),
        };
        Expr {
            kind,
            span: Span::new(start, end),
        }
    }
}
impl<'a> Parser<'a> {
    pub(super) fn expression(&mut self, minimum: u8, object_allowed: bool) -> Parsed<Expr> {
        let depth = self.depth;
        let result = self
            .parse_tasks(Task::Enter(minimum, object_allowed))
            .map(|r| r.expression.expect("表达式解析完成"));
        // 错误退出也归还深度，模板子解析器与外层结构预算保持一致。
        self.depth = depth;
        result
    }
    pub(super) fn block(&mut self) -> Parsed<Block> {
        let depth = self.depth;
        let result = self
            .parse_tasks(Task::BlockStart)
            .map(|r| r.block.expect("语句块解析完成"));
        self.depth = depth;
        result
    }
    fn parse_tasks(&mut self, task: Task<'a>) -> Parsed<Results> {
        let mut tasks = vec![task];
        let mut result = None::<Expr>;
        let mut block = None::<Block>;
        while let Some(task) = tasks.pop() {
            match task {
                Task::Enter(minimum, objects) => {
                    self.enter_structure()?;
                    tasks.push(Task::Exit);
                    tasks.push(Task::Tail(minimum, objects, 0));
                    tasks.push(Task::Prefix(objects));
                }
                Task::Exit => self.depth -= 1,
                Task::Prefix(objects) => {
                    let start = self.span().start;
                    let token = self.peek();
                    match token {
                        "fn" => {
                            self.enter_structure()?;
                            let (start, parameters, ty) = self.function_header()?;
                            tasks.push(Task::Exit);
                            tasks.push(Task::Function(start, parameters, ty));
                            tasks.push(Task::BlockStart);
                        }
                        "if" => {
                            self.pos += 1;
                            tasks.push(Task::IfCondition(start));
                            tasks.push(Task::Enter(0, false));
                        }
                        "build" => {
                            self.pos += 1;
                            if self.take("(") {
                                tasks.push(Task::BuildSource(start));
                                tasks.push(Task::Enter(0, true));
                            } else {
                                let source = BuildSource::Type(self.ty()?);
                                self.expect("as")?;
                                let binding = self.identifier(false)?;
                                tasks.push(Task::BuildBody(start, source, binding));
                                tasks.push(Task::BlockStart);
                            }
                        }
                        "-" | "!" | "~" => {
                            self.pos += 1;
                            tasks.push(Task::Unary(start, token.into()));
                            tasks.push(Task::Enter(14, objects));
                        }
                        "(" => {
                            self.pos += 1;
                            if self.take(")") {
                                result = Some(Expr {
                                    kind: ExprKind::Unit,
                                    span: Span::new(start, self.end()),
                                });
                            } else {
                                tasks.push(Task::Group(start));
                                tasks.push(Task::Enter(0, true));
                            }
                        }
                        "[" => {
                            self.pos += 1;
                            tasks.push(Task::ListStart(List::Array(start)));
                        }
                        "{" => {
                            self.pos += 1;
                            tasks.push(Task::DictStart(start, Vec::new()));
                        }
                        "return" => {
                            self.pos += 1;
                            if matches!(self.peek(), "}" | ";" | "," | ")" | "") {
                                result = Some(Expr {
                                    kind: ExprKind::Return(None),
                                    span: Span::new(start, self.end()),
                                });
                            } else {
                                tasks.push(Task::Return(start));
                                tasks.push(Task::Enter(0, objects));
                            }
                        }
                        _ if token.starts_with("f\"") => {
                            self.pos += 1;
                            tasks.push(Task::Template(
                                start,
                                self.template(token, start)?.into_iter(),
                                Vec::new(),
                            ));
                        }
                        _ => {
                            let value = self.atom()?;
                            if objects && matches!(value.kind, ExprKind::Name(_)) && self.take("{")
                            {
                                let ExprKind::Name(name) = value.kind else {
                                    unreachable!()
                                };
                                tasks.push(Task::ObjectStart(start, name, Vec::new()));
                            } else {
                                result = Some(value);
                            }
                        }
                    }
                }
                Task::Tail(minimum, objects, chain) => {
                    let chain = chain + 1;
                    if chain + self.depth > self.limits.max_depth {
                        return Err(self.error("表达式嵌套超限"));
                    }
                    let left = result.take().expect("前缀或续延已产生表达式");
                    let start = left.span.start;
                    tasks.push(Task::Tail(minimum, objects, chain));
                    if minimum <= 15 && self.take(".") {
                        result = Some(Expr {
                            kind: ExprKind::Field {
                                value: Box::new(left),
                                name: self.identifier(true)?,
                            },
                            span: Span::new(start, self.end()),
                        });
                    } else if minimum <= 15 && self.take("[") {
                        tasks.push(Task::Index(left));
                        tasks.push(Task::Enter(0, true));
                    } else if minimum <= 15 && self.take("(") {
                        tasks.push(Task::ListStart(List::Call(left)));
                    } else if minimum <= 15 && self.take("?") {
                        if matches!(left.kind, ExprKind::Propagate(_)) {
                            return Err(self.error("不能连续使用可选传播"));
                        }
                        result = Some(Expr {
                            kind: ExprKind::Propagate(Box::new(left)),
                            span: Span::new(start, self.end()),
                        });
                    } else if minimum <= 3 && self.take("is") {
                        let kind = if self.take("Some") {
                            self.expect("(")?;
                            let binding = self.identifier(false)?;
                            self.expect(")")?;
                            ExprKind::IsSome {
                                value: Box::new(left),
                                binding,
                            }
                        } else {
                            ExprKind::IsType {
                                value: Box::new(left),
                                name: self.name()?,
                            }
                        };
                        result = Some(Expr {
                            kind,
                            span: Span::new(start, self.end()),
                        });
                    } else {
                        let operator = self.peek();
                        if let Some((precedence, right)) =
                            precedence(operator).filter(|(p, _)| *p >= minimum)
                        {
                            if precedence == 4
                                && matches!(&left.kind, ExprKind::Binary { operator, .. } if precedence_of_comparison(operator))
                            {
                                return Err(self.error("不支持链式比较"));
                            }
                            self.pos += 1;
                            tasks.push(Task::Binary(left, operator.into()));
                            tasks.push(Task::Enter(precedence + u8::from(!right), objects));
                        } else {
                            tasks.pop();
                            result = Some(left);
                            continue;
                        }
                    }
                    self.nodes += 1;
                    if self.nodes > self.limits.max_nodes {
                        return Err(self.error("表达式节点数超限"));
                    }
                }
                Task::Template(start, mut inputs, mut parts) => {
                    match inputs.next() {
                        Some(TemplateInput::Text(text)) => {
                            parts.push(TemplatePart::Text(text));
                            tasks.push(Task::Template(start, inputs, parts));
                        }
                        Some(TemplateInput::Expression(source, offset)) => {
                            // 插值切换词法上下文，外层状态放入续延；不递归调用解析器。
                            let limits = StructuralLimits {
                                max_depth: self.limits.max_depth.saturating_sub(self.depth),
                                max_nodes: self.limits.max_nodes.saturating_sub(self.nodes),
                                ..self.limits
                            };
                            let child = Parser::new(source, offset, limits)?;
                            let parent = std::mem::replace(self, child);
                            tasks.push(Task::TemplateValue(start, inputs, parts, parent));
                            tasks.push(Task::Enter(0, true));
                        }
                        None => {
                            result = Some(Expr {
                                kind: ExprKind::Template(parts),
                                span: Span::new(start, self.end()),
                            });
                        }
                    }
                }
                Task::TemplateValue(start, inputs, mut parts, mut parent) => {
                    self.finish()?;
                    parent.nodes += self.nodes;
                    *self = parent;
                    parts.push(TemplatePart::Expression(result.take().unwrap()));
                    tasks.push(Task::Template(start, inputs, parts));
                }
                Task::Function(start, parameters, ty) => {
                    result = Some(Expr {
                        kind: ExprKind::Function(Function {
                            parameters,
                            result: ty,
                            body: block.take().unwrap(),
                            span: Span::new(start, self.end()),
                        }),
                        span: Span::new(start, self.end()),
                    });
                }
                Task::IfCondition(start) => {
                    tasks.push(Task::IfThen(start, result.take().unwrap()));
                    tasks.push(Task::BlockStart);
                }
                Task::IfThen(start, condition) => {
                    let then = block.take().unwrap();
                    if self.take("else") {
                        tasks.push(Task::IfElse(start, condition, then));
                        if self.peek() == "if" {
                            self.enter_structure()?;
                            tasks.push(Task::Exit);
                            tasks.push(Task::Prefix(false));
                        } else {
                            tasks.push(Task::BlockExpression);
                            tasks.push(Task::BlockStart);
                        }
                    } else {
                        result = Some(Expr {
                            kind: ExprKind::If {
                                condition: Box::new(condition),
                                then,
                                otherwise: None,
                            },
                            span: Span::new(start, self.end()),
                        });
                    }
                }
                Task::BlockExpression => {
                    let body = block.take().unwrap();
                    result = Some(Expr {
                        span: body.span,
                        kind: ExprKind::Block(body),
                    });
                }
                Task::IfElse(start, condition, then) => {
                    result = Some(Expr {
                        kind: ExprKind::If {
                            condition: Box::new(condition),
                            then,
                            otherwise: Some(Box::new(result.take().unwrap())),
                        },
                        span: Span::new(start, self.end()),
                    });
                }
                Task::BuildSource(start) => {
                    self.expect(")")?;
                    self.expect("as")?;
                    let binding = self.identifier(false)?;
                    tasks.push(Task::BuildBody(
                        start,
                        BuildSource::Value(Box::new(result.take().unwrap())),
                        binding,
                    ));
                    tasks.push(Task::BlockStart);
                }
                Task::BuildBody(start, source, binding) => {
                    let body = block.take().unwrap();
                    if body.tail.is_some() {
                        return Err(self.error("构造块只接受语句，正常结束自动冻结"));
                    }
                    result = Some(Expr {
                        kind: ExprKind::Build {
                            source,
                            binding,
                            body,
                        },
                        span: Span::new(start, self.end()),
                    });
                }
                Task::BlockStart => {
                    self.enter_structure()?;
                    let start = self.span().start;
                    self.expect("{")?;
                    tasks.push(Task::Exit);
                    tasks.push(Task::BlockNext(start, Vec::new()));
                }
                Task::BlockNext(start, statements) => {
                    if self.take("}") {
                        block = Some(Block {
                            statements,
                            tail: None,
                            span: Span::new(start, self.end()),
                        });
                        continue;
                    }
                    let position = self.span().start;
                    if self.take("var") {
                        let name = self.identifier(false)?;
                        self.expect(":")?;
                        let ty = self.ty()?;
                        self.expect("=")?;
                        tasks.push(Task::Variable(start, statements, position, name, ty));
                        tasks.push(Task::Enter(0, true));
                    } else if self.take("while") {
                        tasks.push(Task::LoopCondition(start, statements, position, None));
                        tasks.push(Task::Enter(0, false));
                    } else if self.take("for") {
                        let mut bindings = vec![self.identifier(false)?];
                        if self.take(",") {
                            bindings.push(self.identifier(false)?);
                        }
                        self.expect("in")?;
                        tasks.push(Task::LoopCondition(
                            start,
                            statements,
                            position,
                            Some(bindings),
                        ));
                        tasks.push(Task::Enter(0, false));
                    } else if matches!(self.peek(), "break" | "continue") {
                        let kind = if self.take("break") {
                            StatementKind::Break
                        } else {
                            self.pos += 1;
                            StatementKind::Continue
                        };
                        self.expect(";")?;
                        tasks.push(Task::StatementDone(start, statements, position, kind));
                    } else if matches!(
                        self.nth(1),
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
                        let name = self.identifier(false)?;
                        let operator = self.peek().to_string();
                        self.pos += 1;
                        tasks.push(Task::Assign(start, statements, position, name, operator));
                        tasks.push(Task::Enter(0, true));
                    } else {
                        tasks.push(Task::StatementExpression(start, statements, position));
                        tasks.push(Task::Enter(0, true));
                    }
                }
                Task::StatementDone(start, mut statements, position, kind) => {
                    statements.push(Statement {
                        kind,
                        span: Span::new(position, self.end()),
                    });
                    if statements.len() as u64 >= self.limits.max_nodes {
                        return Err(self.error("函数语句数超限"));
                    }
                    tasks.push(Task::BlockNext(start, statements));
                }
                Task::Variable(start, statements, position, name, ty) => {
                    self.expect(";")?;
                    tasks.push(Task::StatementDone(
                        start,
                        statements,
                        position,
                        StatementKind::Variable {
                            name,
                            ty,
                            value: result.take().unwrap(),
                        },
                    ));
                }
                Task::Assign(start, statements, position, name, operator) => {
                    self.expect(";")?;
                    tasks.push(Task::StatementDone(
                        start,
                        statements,
                        position,
                        StatementKind::Assign {
                            name,
                            operator,
                            value: result.take().unwrap(),
                        },
                    ));
                }
                Task::StatementExpression(start, statements, position) => {
                    let expression = result.take().unwrap();
                    if self.take("=") {
                        if !matches!(
                            expression.kind,
                            ExprKind::Field { .. } | ExprKind::Index { .. }
                        ) {
                            return Err(self.error("构造赋值需要直接字段或索引目标"));
                        }
                        tasks.push(Task::Set(start, statements, position, expression));
                        tasks.push(Task::Enter(0, true));
                    } else if self.take(";") {
                        tasks.push(Task::StatementDone(
                            start,
                            statements,
                            position,
                            StatementKind::Expression(expression),
                        ));
                    } else if self.take("}") {
                        block = Some(Block {
                            statements,
                            tail: Some(Box::new(expression)),
                            span: Span::new(start, self.end()),
                        });
                    } else if matches!(expression.kind, ExprKind::If { .. }) {
                        tasks.push(Task::StatementDone(
                            start,
                            statements,
                            position,
                            StatementKind::Expression(expression),
                        ));
                    } else {
                        return Err(self.error("表达式语句需要分号"));
                    }
                }
                Task::Set(start, statements, position, target) => {
                    self.expect(";")?;
                    tasks.push(Task::StatementDone(
                        start,
                        statements,
                        position,
                        StatementKind::Set {
                            target,
                            value: result.take().unwrap(),
                        },
                    ));
                }
                Task::LoopCondition(start, statements, position, bindings) => {
                    tasks.push(Task::LoopBody(
                        start,
                        statements,
                        position,
                        bindings,
                        result.take().unwrap(),
                    ));
                    tasks.push(Task::BlockStart);
                }
                Task::LoopBody(start, statements, position, bindings, condition) => {
                    let body = block.take().unwrap();
                    let kind = match bindings {
                        Some(bindings) => StatementKind::For {
                            bindings,
                            iterable: condition,
                            body,
                        },
                        None => StatementKind::While { condition, body },
                    };
                    tasks.push(Task::StatementDone(start, statements, position, kind));
                }
                Task::Unary(start, operator) => {
                    result = Some(Expr {
                        kind: ExprKind::Unary {
                            operator,
                            value: Box::new(result.take().unwrap()),
                        },
                        span: Span::new(start, self.end()),
                    });
                }
                Task::Return(start) => {
                    result = Some(Expr {
                        kind: ExprKind::Return(Some(Box::new(result.take().unwrap()))),
                        span: Span::new(start, self.end()),
                    });
                }
                Task::Group(start) => {
                    self.expect(")")?;
                    result.as_mut().unwrap().span = Span::new(start, self.end());
                }
                Task::Binary(left, operator) => {
                    let start = left.span.start;
                    result = Some(Expr {
                        kind: ExprKind::Binary {
                            operator,
                            left: Box::new(left),
                            right: Box::new(result.take().unwrap()),
                        },
                        span: Span::new(start, self.end()),
                    });
                }
                Task::Index(value) => {
                    self.expect("]")?;
                    let start = value.span.start;
                    result = Some(Expr {
                        kind: ExprKind::Index {
                            value: Box::new(value),
                            index: Box::new(result.take().unwrap()),
                        },
                        span: Span::new(start, self.end()),
                    });
                }
                Task::ListStart(list) => {
                    if self.take(list.closing()) {
                        result = Some(list.finish(Vec::new(), self.end()));
                    } else {
                        tasks.push(Task::ListNext(list, Vec::new()));
                        tasks.push(Task::Enter(0, true));
                    }
                }
                Task::ListNext(list, mut values) => {
                    values.push(result.take().unwrap());
                    if !self.take(",") {
                        self.expect(list.closing())?;
                        result = Some(list.finish(values, self.end()));
                    } else if self.take(list.closing()) {
                        result = Some(list.finish(values, self.end()));
                    } else {
                        tasks.push(Task::ListNext(list, values));
                        tasks.push(Task::Enter(0, true));
                    }
                }
                Task::DictStart(start, values) => {
                    if self.take("}") {
                        result = Some(Expr {
                            kind: ExprKind::Dictionary(values),
                            span: Span::new(start, self.end()),
                        });
                    } else {
                        tasks.push(Task::DictKey(start, values));
                        tasks.push(Task::Enter(0, true));
                    }
                }
                Task::DictKey(start, values) => {
                    self.expect(":")?;
                    tasks.push(Task::DictValue(start, values, result.take().unwrap()));
                    tasks.push(Task::Enter(0, true));
                }
                Task::DictValue(start, mut values, key) => {
                    values.push((key, result.take().unwrap()));
                    if self.take(",") {
                        tasks.push(Task::DictStart(start, values));
                    } else {
                        self.expect("}")?;
                        result = Some(Expr {
                            kind: ExprKind::Dictionary(values),
                            span: Span::new(start, self.end()),
                        });
                    }
                }
                Task::ObjectStart(start, name, fields) => {
                    if self.take("}") {
                        result = Some(Expr {
                            kind: ExprKind::Object {
                                type_name: name,
                                fields,
                            },
                            span: Span::new(start, self.end()),
                        });
                    } else {
                        let field = self.identifier(true)?;
                        self.expect(":")?;
                        tasks.push(Task::ObjectValue(start, name, fields, field));
                        tasks.push(Task::Enter(0, true));
                    }
                }
                Task::ObjectValue(start, name, mut fields, field) => {
                    fields.push((field, result.take().unwrap()));
                    if self.take(",") {
                        tasks.push(Task::ObjectStart(start, name, fields));
                    } else {
                        self.expect("}")?;
                        result = Some(Expr {
                            kind: ExprKind::Object {
                                type_name: name,
                                fields,
                            },
                            span: Span::new(start, self.end()),
                        });
                    }
                }
            }
        }
        Ok(Results {
            expression: result,
            block,
        })
    }
}
