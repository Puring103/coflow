//! 类型续延与表达式共用结构预算，函数类型和容器类型不递归解析。
use super::*;
enum Task {
    Enter,
    Finish(usize),
    Array,
    Group,
    DictKey,
    DictValue(TypeRef),
    Parameters(Vec<FunctionParameterRef>),
    Parameter(Vec<FunctionParameterRef>, Option<NameRef>),
    Function(Vec<FunctionParameterRef>),
}
impl Parser<'_> {
    pub(super) fn ty(&mut self) -> Parsed<TypeRef> {
        let depth = self.depth;
        let result = self.type_tasks();
        self.depth = depth;
        result
    }
    fn type_tasks(&mut self) -> Parsed<TypeRef> {
        let mut tasks = vec![Task::Enter];
        let mut result = None::<TypeRef>;
        while let Some(task) = tasks.pop() {
            match task {
                Task::Enter => {
                    self.enter_structure()?;
                    let start = self.span().start;
                    tasks.push(Task::Finish(start));
                    if self.take("[") {
                        tasks.push(Task::Array);
                        tasks.push(Task::Enter);
                    } else if self.take("{") {
                        tasks.push(Task::DictKey);
                        tasks.push(Task::Enter);
                    } else if self.take("(") {
                        if self.take(")") {
                            result = Some(TypeRef {
                                kind: TypeRefKind::Unit,
                                span: Span::new(start, self.end()),
                            });
                        } else {
                            tasks.push(Task::Group);
                            tasks.push(Task::Enter);
                        }
                    } else if self.take("fn") {
                        self.expect("(")?;
                        tasks.push(Task::Parameters(Vec::new()));
                    } else {
                        let kind = match self.name()?.as_str() {
                            "int" => TypeRefKind::Int,
                            "float" => TypeRefKind::Float,
                            "bool" => TypeRefKind::Bool,
                            "string" => TypeRefKind::String,
                            "fstring" => TypeRefKind::FString,
                            name => TypeRefKind::Named(name.into()),
                        };
                        result = Some(TypeRef {
                            kind,
                            span: Span::new(start, self.end()),
                        });
                    }
                }
                Task::Finish(start) => {
                    let mut value = result.take().unwrap();
                    if self.take("?") {
                        value.span = Span::new(start, self.end() - 1);
                        value = TypeRef {
                            kind: TypeRefKind::Option(Box::new(value)),
                            span: Span::new(start, self.end()),
                        };
                    }
                    value.span = Span::new(start, self.end());
                    result = Some(value);
                    self.depth -= 1;
                }
                Task::Array => {
                    self.expect("]")?;
                    let value = result.take().unwrap();
                    result = Some(TypeRef {
                        span: value.span,
                        kind: TypeRefKind::Array(Box::new(value)),
                    });
                }
                Task::Group => self.expect(")")?,
                Task::DictKey => {
                    self.expect(":")?;
                    tasks.push(Task::DictValue(result.take().unwrap()));
                    tasks.push(Task::Enter);
                }
                Task::DictValue(key) => {
                    self.expect("}")?;
                    result = Some(TypeRef {
                        span: key.span,
                        kind: TypeRefKind::Dict(Box::new(key), Box::new(result.take().unwrap())),
                    });
                }
                Task::Parameters(parameters) => {
                    if self.take(")") {
                        self.expect("->")?;
                        tasks.push(Task::Function(parameters));
                        tasks.push(Task::Enter);
                    } else {
                        let name = if self.nth(1) == ":" {
                            let span = self.span();
                            let name = self.identifier(false)?;
                            self.expect(":")?;
                            Some(NameRef { name, span })
                        } else {
                            None
                        };
                        tasks.push(Task::Parameter(parameters, name));
                        tasks.push(Task::Enter);
                    }
                }
                Task::Parameter(mut parameters, name) => {
                    parameters.push(FunctionParameterRef {
                        name,
                        value_type: result.take().unwrap(),
                    });
                    if self.take(",") {
                        tasks.push(Task::Parameters(parameters));
                    } else {
                        self.expect(")")?;
                        self.expect("->")?;
                        tasks.push(Task::Function(parameters));
                        tasks.push(Task::Enter);
                    }
                }
                Task::Function(parameters) => {
                    let value = result.take().unwrap();
                    result = Some(TypeRef {
                        span: value.span,
                        kind: TypeRefKind::Function(parameters, Box::new(value)),
                    });
                }
            }
        }
        Ok(result.expect("类型解析完成"))
    }
}
