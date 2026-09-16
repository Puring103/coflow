//! 静态函数检查和寄存器 lowering；所有分支都检查，只有控制流选中的分支执行。
use super::bytecode::*;
use crate::{
    schema::{CftFunctionParameter, CftSchema, CftValueType as Ty},
    source::Span,
};
use coflow_language::{
    cft::syntax::ast::{TypeRef, TypeRefKind},
    function::{self, Block, Expr, ExprKind as E, Function, StatementKind as S, TemplatePart},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub span: Span,
    pub message: String,
}
impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}
impl std::error::Error for CompileError {}
impl From<function::SyntaxError> for CompileError {
    fn from(error: function::SyntaxError) -> Self {
        Self {
            span: error.span,
            message: error.message,
        }
    }
}
type Result<T> = std::result::Result<T, CompileError>;
#[derive(Debug, Clone, Default)]
pub struct CompileContext {
    pub owner: Option<Ty>,
    pub namespace: String,
    pub imports: BTreeMap<String, String>,
    pub check: bool,
}
#[derive(Debug, Clone)]
struct Local {
    register: Register,
    ty: Ty,
    mutable: bool,
}
#[derive(Debug, Clone)]
struct Value {
    register: Register,
    ty: Ty,
    terminated: bool,
}
#[derive(Debug, Clone, Default)]
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}
#[derive(Clone)]
struct Compiler<'a> {
    schema: &'a CftSchema,
    context: CompileContext,
    program: Program,
    scopes: Vec<BTreeMap<String, Local>>,
    outer: BTreeMap<String, Local>,
    captured: BTreeMap<String, (Register, usize)>,
    capture_sources: Vec<Register>,
    loops: Vec<Loop>,
    condition: bool,
    literal_owner: Option<(Register, Ty)>,
    refinements: Vec<BTreeMap<String, Ty>>,
}

pub fn compile(
    schema: &CftSchema,
    source: &str,
    name: &str,
    context: CompileContext,
) -> Result<Program> {
    let function = function::parse_function(source)?;
    let mut compiler = Compiler::new(schema, name, source, context);
    compiler.function(&function)?;
    compiler
        .program
        .allocate_registers()
        .map_err(|message| CompileError {
            span: function.span,
            message,
        })?;
    compiler
        .program
        .validate()
        .map_err(|message| CompileError {
            span: function.span,
            message,
        })?;
    Ok(compiler.program)
}
pub fn compile_template(
    schema: &CftSchema,
    source: &str,
    name: &str,
    context: CompileContext,
) -> Result<Program> {
    let expression = function::parse_expression(source)?;
    let E::Template(parts) = &expression.kind else {
        return Err(CompileError {
            span: expression.span,
            message: "需要 fstring 模板".into(),
        });
    };
    let mut compiler = Compiler::new(schema, name, source, context);
    compiler.template(parts, expression.span)?;
    let mut program = std::sync::Arc::unwrap_or_clone(
        compiler
            .program
            .closures
            .pop()
            .ok_or_else(|| compiler.error(expression.span, "缺少模板程序"))?
            .program,
    );
    program
        .allocate_registers()
        .map_err(|message| CompileError {
            span: expression.span,
            message,
        })?;
    program.validate().map_err(|message| CompileError {
        span: expression.span,
        message,
    })?;
    Ok(program)
}
pub fn compile_check(
    schema: &CftSchema,
    check: &function::Check,
    source: &str,
    name: &str,
    mut context: CompileContext,
) -> Result<Program> {
    context.check = true;
    let mut compiler = Compiler::new(schema, name, source, context);
    let result = compiler.block(&check.body, Some(&Ty::Unit), false)?;
    compiler.emit(
        Instruction::new(Opcode::Return, result.register, 0, 0, 0),
        check.span,
    );
    compiler
        .program
        .allocate_registers()
        .map_err(|message| CompileError {
            span: check.span,
            message,
        })?;
    compiler
        .program
        .validate()
        .map_err(|message| CompileError {
            span: check.span,
            message,
        })?;
    Ok(compiler.program)
}
pub fn module_context(
    schema: &CftSchema,
    module: &crate::schema::ModuleId,
    owner: Option<Ty>,
) -> CompileContext {
    let mut context = CompileContext {
        owner,
        ..CompileContext::default()
    };
    if let Some(source) = schema.source(module) {
        let tokens: Vec<_> = coflow_language::lexical::tokenize_lossless(&source.source)
            .into_iter()
            .filter(|token| !token.is_trivia())
            .collect();
        let mut index = 0;
        while index < tokens.len() {
            let kind = tokens[index].text(&source.source);
            if !matches!(kind, "namespace" | "use") {
                break;
            }
            index += 1;
            let mut name = String::new();
            while index < tokens.len() && tokens[index].text(&source.source) != ";" {
                name.push_str(tokens[index].text(&source.source));
                index += 1;
            }
            index += 1;
            if kind == "namespace" {
                context.namespace = name;
            } else {
                context
                    .imports
                    .insert(name.rsplit("::").next().unwrap_or(&name).into(), name);
            }
        }
    }
    context
}
impl<'a> Compiler<'a> {
    fn new(schema: &'a CftSchema, name: &str, source: &str, context: CompileContext) -> Self {
        Self {
            schema,
            context,
            program: Program::new(name.into(), source.into(), Vec::new(), Ty::Unit),
            scopes: vec![BTreeMap::new()],
            outer: BTreeMap::new(),
            captured: BTreeMap::new(),
            capture_sources: Vec::new(),
            loops: Vec::new(),
            condition: false,
            literal_owner: None,
            refinements: vec![BTreeMap::new()],
        }
    }
    fn error(&self, span: Span, message: impl Into<String>) -> CompileError {
        CompileError {
            span,
            message: message.into(),
        }
    }
    fn resolve_name(&self, name: &str) -> String {
        let (head, tail) = name.split_once("::").map_or((name, ""), |(a, b)| (a, b));
        if let Some(import) = self.context.imports.get(head) {
            return if tail.is_empty() {
                import.clone()
            } else {
                format!("{import}::{tail}")
            };
        }
        if self.context.namespace.is_empty() {
            return name.into();
        }
        let local_head = format!("{}::{head}", self.context.namespace);
        if self.schema.resolve_type(&local_head).is_some()
            || self.schema.resolve_enum(&local_head).is_some()
            || self.schema.resolve_const(&local_head).is_some()
        {
            format!("{}::{name}", self.context.namespace)
        } else {
            name.into()
        }
    }
    fn resolve_type(&self, reference: &TypeRef) -> Result<Ty> {
        let mut reference = reference.clone();
        fn walk(reference: &mut TypeRef, resolve: &impl Fn(&str) -> String) {
            match &mut reference.kind {
                TypeRefKind::Named(name) => *name = resolve(name),
                TypeRefKind::Array(inner) | TypeRefKind::Option(inner) => walk(inner, resolve),
                TypeRefKind::Dict(key, value) => {
                    walk(key, resolve);
                    walk(value, resolve);
                }
                TypeRefKind::Function(args, result) => {
                    for arg in args {
                        walk(&mut arg.value_type, resolve);
                    }
                    walk(result, resolve);
                }
                _ => {}
            }
        }
        walk(&mut reference, &|name| self.resolve_name(name));
        self.schema
            .resolve_type_ref(&reference)
            .map_err(|message| self.error(reference.span, message))
    }
    fn slot(&mut self, ty: Ty, span: Span) -> Result<Value> {
        let register = u16::try_from(self.program.registers.len())
            .map_err(|_| self.error(span, "函数寄存器数量超限"))?;
        self.program.registers.push(ty.clone());
        Ok(Value {
            register,
            ty,
            terminated: false,
        })
    }
    fn emit(&mut self, instruction: Instruction, span: Span) -> usize {
        let index = self.program.instructions.len();
        self.program.instructions.push(instruction);
        self.program.spans.push(span);
        index
    }
    fn emit_index(
        &mut self,
        opcode: Opcode,
        register: Register,
        index: usize,
        span: Span,
    ) -> Result<usize> {
        let index = u32::try_from(index).map_err(|_| self.error(span, "程序附表过大"))?;
        Ok(self.emit(Instruction::indexed(opcode, register, index), span))
    }
    fn patch(&mut self, instruction: usize, target: usize) -> Result<()> {
        let old = self.program.instructions[instruction];
        let opcode = old
            .opcode()
            .ok_or_else(|| self.error(self.program.spans[instruction], "无效指令"))?;
        let target = u32::try_from(target)
            .map_err(|_| self.error(self.program.spans[instruction], "函数体过大"))?;
        self.program.instructions[instruction] = Instruction::indexed(opcode, old.a(), target);
        Ok(())
    }
    fn constant(&mut self, constant: Constant, ty: Ty, span: Span) -> Result<Value> {
        let value = self.slot(ty, span)?;
        let index = self.program.constants.len();
        self.program.constants.push(constant);
        self.emit_index(Opcode::Constant, value.register, index, span)?;
        Ok(value)
    }
    fn declare(&mut self, name: &str, value: &Value, mutable: bool, span: Span) -> Result<()> {
        let scope = self.scopes.last_mut().ok_or_else(|| CompileError {
            span,
            message: "缺少作用域".into(),
        })?;
        if scope.contains_key(name) {
            return Err(self.error(span, format!("重复局部名称 {name}")));
        }
        scope.insert(
            name.into(),
            Local {
                register: value.register,
                ty: value.ty.clone(),
                mutable,
            },
        );
        Ok(())
    }
    fn local(&mut self, name: &str, span: Span) -> Result<Option<Value>> {
        if let Some(local) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
        {
            return Ok(Some(Value {
                register: local.register,
                ty: local.ty,
                terminated: false,
            }));
        }
        let Some(outer) = self.outer.get(name).cloned() else {
            return Ok(None);
        };
        if let Some((register, index)) = self.captured.get(name).copied() {
            self.emit_index(Opcode::Capture, register, index, span)?;
            return Ok(Some(Value {
                register,
                ty: outer.ty,
                terminated: false,
            }));
        }
        let value = self.slot(outer.ty.clone(), span)?;
        let index = self.capture_sources.len();
        self.capture_sources.push(outer.register);
        self.program.captures.push(outer.ty);
        self.emit_index(Opcode::Capture, value.register, index, span)?;
        self.captured.insert(name.into(), (value.register, index));
        Ok(Some(value))
    }
    fn function(&mut self, function: &Function) -> Result<()> {
        self.program.result = self.resolve_type(&function.result)?;
        for (name, reference) in &function.parameters {
            let ty = self.resolve_type(reference)?;
            self.program.parameters.push(ty.clone());
            let value = self.slot(ty, reference.span)?;
            self.declare(name, &value, false, reference.span)?;
        }
        let result_type = self.program.result.clone();
        let result = self.block(&function.body, Some(&result_type), false)?;
        self.emit(
            Instruction::new(Opcode::Return, result.register, 0, 0, 0),
            function.body.span,
        );
        Ok(())
    }
    fn adapt(&mut self, mut value: Value, expected: Option<&Ty>, span: Span) -> Result<Value> {
        if value.terminated {
            return Ok(value);
        }
        let keep_template = matches!(expected, Some(Ty::FString))
            || matches!(expected, Some(Ty::Option(inner)) if **inner == Ty::FString);
        if !keep_template {
            let read_type = match &value.ty {
                Ty::FString => Some(Ty::String),
                Ty::Option(inner) if **inner == Ty::FString => {
                    Some(Ty::Option(Box::new(Ty::String)))
                }
                _ => None,
            };
            if let Some(ty) = read_type {
                let result = self.slot(ty, span)?;
                self.emit(
                    Instruction::new(Opcode::ReadTemplate, result.register, value.register, 0, 0),
                    span,
                );
                value = result;
            }
        }
        if let Some(expected) = expected {
            if !self.schema.value_type_assignable(&value.ty, expected) {
                return Err(self.error(span, format!("需要 {expected}，实际为 {}", value.ty)));
            }
            let float_expected = *expected == Ty::Float
                || matches!(expected, Ty::Option(inner) if **inner == Ty::Float);
            if float_expected
                && (value.ty == Ty::Int
                    || matches!(&value.ty, Ty::Option(inner) if **inner == Ty::Int))
            {
                let result = self.slot(expected.clone(), span)?;
                self.emit(
                    Instruction::new(Opcode::ConvertFloat, result.register, value.register, 0, 0),
                    span,
                );
                value = result;
            }
        }
        Ok(value)
    }
    fn block(&mut self, block: &Block, expected: Option<&Ty>, nested: bool) -> Result<Value> {
        if nested {
            self.push_scope();
        }
        let mut terminated = false;
        for statement in &block.statements {
            let span = statement.span;
            match &statement.kind {
                S::Variable { name, ty, value } => {
                    let ty = self.resolve_type(ty)?;
                    let initial = self.expression(value, Some(&ty))?;
                    let local = self.slot(ty, span)?;
                    self.emit(
                        Instruction::new(Opcode::Move, local.register, initial.register, 0, 0),
                        span,
                    );
                    self.declare(name, &local, true, span)?;
                    terminated |= initial.terminated;
                }
                S::Assign {
                    name,
                    operator,
                    value,
                } => {
                    let local = self
                        .scopes
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(name))
                        .cloned()
                        .ok_or_else(|| self.error(span, format!("未知局部变量 {name}")))?;
                    if !local.mutable {
                        return Err(self.error(span, "只能给可变局部变量赋值"));
                    }
                    let next = if operator == "=" {
                        self.expression(value, Some(&local.ty))?
                    } else {
                        let lhs = Expr {
                            kind: E::Name(name.clone()),
                            span,
                        };
                        let binary = Expr {
                            kind: E::Binary {
                                operator: operator.trim_end_matches('=').into(),
                                left: Box::new(lhs),
                                right: Box::new(value.clone()),
                            },
                            span,
                        };
                        self.expression(&binary, Some(&local.ty))?
                    };
                    self.emit(
                        Instruction::new(Opcode::Move, local.register, next.register, 0, 0),
                        span,
                    );
                    terminated |= next.terminated;
                    for refinements in &mut self.refinements {
                        refinements.retain(|path, _| {
                            path != name && !path.starts_with(&format!("{name}."))
                        });
                    }
                }
                S::Expression(expression) => {
                    terminated |= self.expression(expression, None)?.terminated;
                }
                S::While { condition, body } => {
                    let start = self.program.instructions.len();
                    self.push_scope();
                    let condition = self.condition_expression(condition)?;
                    let exit = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
                    self.emit_index(Opcode::Iteration, 0, 0, span)?;
                    self.loops.push(Loop::default());
                    self.block(body, Some(&Ty::Unit), true)?;
                    self.emit_index(Opcode::Jump, 0, start, span)?;
                    self.end_loop(exit, start)?;
                    self.pop_scope();
                }
                S::For {
                    bindings,
                    iterable,
                    body,
                } => self.for_loop(bindings, iterable, body, span)?,
                S::Break | S::Continue => {
                    if self.loops.is_empty() {
                        return Err(self.error(span, "break/continue 只能位于当前函数的循环中"));
                    }
                    let jump = self.emit_index(Opcode::Jump, 0, 0, span)?;
                    if let Some(current) = self.loops.last_mut() {
                        if matches!(statement.kind, S::Break) {
                            current.breaks.push(jump);
                        } else {
                            current.continues.push(jump);
                        }
                    }
                    terminated = true;
                }
            }
        }
        let mut result = if let Some(tail) = &block.tail {
            self.expression(tail, expected)?
        } else {
            self.constant(Constant::Unit, Ty::Unit, block.span)?
        };
        result.terminated |= terminated;
        let result = self.adapt(result, expected, block.span)?;
        if nested {
            self.pop_scope();
        }
        Ok(result)
    }
    fn end_loop(&mut self, exit: usize, continuation: usize) -> Result<()> {
        let end = self.program.instructions.len();
        self.patch(exit, end)?;
        if let Some(current) = self.loops.pop() {
            for jump in current.breaks {
                self.patch(jump, end)?;
            }
            for jump in current.continues {
                self.patch(jump, continuation)?;
            }
        }
        Ok(())
    }
    fn for_loop(
        &mut self,
        bindings: &[String],
        iterable: &Expr,
        body: &Block,
        span: Span,
    ) -> Result<()> {
        self.scopes.push(BTreeMap::new());
        if let E::Binary {
            operator,
            left,
            right,
        } = &iterable.kind
        {
            if matches!(operator.as_str(), ".." | "..=") {
                if bindings.len() != 1 {
                    return Err(self.error(span, "区间循环只接受一个绑定"));
                }
                let first = self.expression(left, Some(&Ty::Int))?;
                let last = self.expression(right, Some(&Ty::Int))?;
                let current = self.slot(Ty::Int, span)?;
                self.emit(
                    Instruction::new(Opcode::Move, current.register, first.register, 0, 0),
                    span,
                );
                self.declare(&bindings[0], &current, false, span)?;
                let start = self.program.instructions.len();
                let condition = self.binary_values(
                    if operator == "..=" { "<=" } else { "<" },
                    current.clone(),
                    last.clone(),
                    span,
                )?;
                let exit = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
                self.emit_index(Opcode::Iteration, 0, 0, span)?;
                self.loops.push(Loop::default());
                self.block(body, Some(&Ty::Unit), true)?;
                let continuation = self.program.instructions.len();
                // 闭区间在最大整数处直接结束，不能为了退出而执行溢出的加一。
                let at_end = self.binary_values("==", current.clone(), last, span)?;
                let increment = self.emit_index(Opcode::JumpFalse, at_end.register, 0, span)?;
                let finish = self.emit_index(Opcode::Jump, 0, 0, span)?;
                self.patch(increment, self.program.instructions.len())?;
                let one = self.constant(Constant::Int(1), Ty::Int, span)?;
                let next = self.binary_values("+", current.clone(), one, span)?;
                self.emit(
                    Instruction::new(Opcode::Move, current.register, next.register, 0, 0),
                    span,
                );
                self.emit_index(Opcode::Jump, 0, start, span)?;
                self.patch(finish, self.program.instructions.len())?;
                self.end_loop(exit, continuation)?;
                self.scopes.pop();
                return Ok(());
            }
        }
        let iterable = self.expression(iterable, None)?;
        let (key_type, value_type) = match &iterable.ty {
            Ty::Array(inner) => (Ty::Int, (**inner).clone()),
            Ty::Dict(key, value) if bindings.len() == 2 => ((**key).clone(), (**value).clone()),
            _ => return Err(self.error(span, "for 需要数组、双绑定字典或整数区间")),
        };
        let index = self.constant(Constant::Int(0), Ty::Int, span)?;
        let length = self.slot(Ty::Int, span)?;
        self.emit(
            Instruction::new(Opcode::Length, length.register, iterable.register, 0, 0),
            span,
        );
        let start = self.program.instructions.len();
        let condition = self.binary_values("<", index.clone(), length, span)?;
        let exit = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
        self.emit_index(Opcode::Iteration, 0, 0, span)?;
        let stored = self.slot(value_type, span)?;
        self.emit(
            Instruction::new(
                Opcode::IteratorValue,
                stored.register,
                iterable.register,
                index.register,
                0,
            ),
            span,
        );
        let value = self.adapt(stored, None, span)?;
        self.declare(&bindings[bindings.len() - 1], &value, false, span)?;
        if bindings.len() == 2 {
            let key = self.slot(key_type, span)?;
            self.emit(
                Instruction::new(
                    Opcode::IteratorKey,
                    key.register,
                    iterable.register,
                    index.register,
                    0,
                ),
                span,
            );
            self.declare(&bindings[0], &key, false, span)?;
        }
        self.loops.push(Loop::default());
        self.block(body, Some(&Ty::Unit), true)?;
        let continuation = self.program.instructions.len();
        let one = self.constant(Constant::Int(1), Ty::Int, span)?;
        let next = self.binary_values("+", index.clone(), one, span)?;
        self.emit(
            Instruction::new(Opcode::Move, index.register, next.register, 0, 0),
            span,
        );
        self.emit_index(Opcode::Jump, 0, start, span)?;
        self.end_loop(exit, continuation)?;
        self.scopes.pop();
        Ok(())
    }
    fn expression(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let condition = self.condition;
        // 只有正向测试和 && 链能向成功分支传递保证；比较、调用和取反均隔离收窄。
        self.condition &= matches!(&expression.kind, E::IsSome { .. } | E::IsType { .. })
            || matches!(&expression.kind,E::Binary{operator,..} if operator=="&&");
        let result = self.expression_inner(expression, expected);
        self.condition = condition;
        result
    }
    fn expression_inner(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let mut value = match &expression.kind {
            E::Unit => self.constant(Constant::Unit, Ty::Unit, span)?,
            E::None => {
                let ty = expected
                    .filter(|ty| matches!(ty, Ty::Option(_)))
                    .ok_or_else(|| self.error(span, "None 需要明确的可选期望类型"))?;
                self.constant(Constant::None, ty.clone(), span)?
            }
            E::Bool(value) => self.constant(Constant::Bool(*value), Ty::Bool, span)?,
            E::String(value) => self.constant(Constant::String(value.clone()), Ty::String, span)?,
            E::Number(number) => self.number(number, span)?,
            E::Name(name) => self.name_value(name, span)?,
            E::Reference { type_name, key } => {
                let name = if let Some(name) = type_name {
                    self.resolve_name(name)
                } else if let Some(Ty::RecordRef(name)) = &self.context.owner {
                    name.to_string()
                } else {
                    return Err(self.error(span, "裸记录引用需要静态记录上下文"));
                };
                let meta = self
                    .schema
                    .resolve_type(&name)
                    .ok_or_else(|| self.error(span, "未知记录类型"))?;
                if matches!(meta.kind, coflow_language::cft::syntax::ast::TypeKind::Data) {
                    return Err(self.error(span, "data 不能作为记录引用"));
                }
                let value = self.slot(Ty::RecordRef(meta.name.clone()), span)?;
                let index = self.program.names.len();
                self.program.names.push(format!("{name}::{key}"));
                self.emit_index(Opcode::Reference, value.register, index, span)?;
                value
            }
            E::Unary { operator, value } => {
                if operator == "-" {
                    if let E::Number(number) = &value.kind {
                        let value = self.number(&format!("-{number}"), span)?;
                        return self.adapt(value, expected, span);
                    }
                }
                let condition = self.condition;
                self.condition = false;
                let value = self.expression(value, None)?;
                self.condition = condition;
                let flags = match (operator.as_str(), &value.ty) {
                    ("-", Ty::Int | Ty::Float) => 0,
                    ("!", Ty::Bool) => 1,
                    ("~", Ty::Int) => 2,
                    ("~", Ty::Enum(name))
                        if self
                            .schema
                            .resolve_enum(name)
                            .is_some_and(|meta| meta.is_flag) =>
                    {
                        2
                    }
                    _ => return Err(self.error(span, "一元运算符与操作数类型不匹配")),
                };
                let result = self.slot(value.ty, span)?;
                self.emit(
                    Instruction::new(Opcode::Unary, result.register, value.register, 0, flags),
                    span,
                );
                result
            }
            E::Binary {
                operator,
                left,
                right,
            } if matches!(operator.as_str(), "&&" | "||") => {
                if operator == "||" {
                    self.push_scope();
                }
                let left = self.expression(left, Some(&Ty::Bool))?;
                if operator == "||" {
                    self.pop_scope();
                    self.push_scope();
                }
                let result = self.slot(Ty::Bool, span)?;
                self.emit(
                    Instruction::new(Opcode::Move, result.register, left.register, 0, 0),
                    span,
                );
                let condition = if operator == "||" {
                    let inverted = self.slot(Ty::Bool, span)?;
                    self.emit(
                        Instruction::new(Opcode::Unary, inverted.register, left.register, 0, 1),
                        span,
                    );
                    inverted
                } else {
                    left
                };
                let end = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
                let right = self.expression(right, Some(&Ty::Bool))?;
                self.emit(
                    Instruction::new(Opcode::Move, result.register, right.register, 0, 0),
                    span,
                );
                self.patch(end, self.program.instructions.len())?;
                if operator == "||" {
                    self.pop_scope();
                }
                result
            }
            E::Binary {
                operator,
                left,
                right,
            } => {
                let (left, right) =
                    if matches!(operator.as_str(), "==" | "!=") && matches!(left.kind, E::None) {
                        let right = self.expression(right, None)?;
                        let left = self.expression(left, Some(&right.ty))?;
                        (left, right)
                    } else {
                        let left = self.expression(left, None)?;
                        let expected = if matches!(operator.as_str(), "==" | "!=")
                            && matches!(right.kind, E::None)
                        {
                            Some(&left.ty)
                        } else {
                            None
                        };
                        let right = self.expression(right, expected)?;
                        (left, right)
                    };
                self.binary_values(operator, left, right, span)?
            }
            E::Field { value, name } => {
                let receiver = self.expression(value, None)?;
                let type_name = match &receiver.ty {
                    Ty::Object(name) | Ty::RecordRef(name) => name,
                    _ => return Err(self.error(span, "字段读取需要对象或记录")),
                };
                let meta = self
                    .schema
                    .resolve_type(type_name)
                    .ok_or_else(|| self.error(span, "未知对象类型"))?;
                let (index, ty) = if name == "id" && matches!(receiver.ty, Ty::RecordRef(_)) {
                    (0, Ty::String)
                } else {
                    let (index, field) = meta
                        .all_fields()
                        .enumerate()
                        .find(|(_, field)| field.name.as_str() == name)
                        .ok_or_else(|| self.error(span, format!("未知字段 {name}")))?;
                    (
                        index + usize::from(matches!(receiver.ty, Ty::RecordRef(_))),
                        field.runtime_value_type(),
                    )
                };
                let index = u16::try_from(index).map_err(|_| self.error(span, "字段槽超限"))?;
                let result = self.slot(ty, span)?;
                self.emit(
                    Instruction::new(Opcode::Field, result.register, receiver.register, index, 0),
                    span,
                );
                result
            }
            E::Index { value, index } => {
                let value = self.expression(value, None)?;
                let (key, ty) = match &value.ty {
                    Ty::Array(inner) => (Ty::Int, (**inner).clone()),
                    Ty::Dict(key, inner) => ((**key).clone(), (**inner).clone()),
                    Ty::String => (Ty::Int, Ty::String),
                    _ => return Err(self.error(span, "类型不支持索引")),
                };
                let index = self.expression(index, Some(&key))?;
                let result = self.slot(ty, span)?;
                self.emit(
                    Instruction::new(
                        Opcode::Index,
                        result.register,
                        value.register,
                        index.register,
                        0,
                    ),
                    span,
                );
                result
            }
            E::If {
                condition,
                then,
                otherwise,
            } => {
                self.push_scope();
                let condition = self.condition_expression(condition)?;
                let branch = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
                let branch_expected = if otherwise.is_none() {
                    Some(&Ty::Unit)
                } else {
                    expected
                };
                let left = self.block(then, branch_expected, true)?;
                let result = self.slot(
                    if otherwise.is_none() {
                        Ty::Unit
                    } else {
                        expected.cloned().unwrap_or_else(|| left.ty.clone())
                    },
                    span,
                )?;
                self.emit(
                    Instruction::new(Opcode::Move, result.register, left.register, 0, 0),
                    span,
                );
                let end = self.emit_index(Opcode::Jump, 0, 0, span)?;
                self.patch(branch, self.program.instructions.len())?;
                self.pop_scope();
                let right = if let Some(otherwise) = otherwise {
                    self.expression(otherwise, Some(&result.ty))?
                } else {
                    self.constant(Constant::Unit, Ty::Unit, span)?
                };
                self.emit(
                    Instruction::new(Opcode::Move, result.register, right.register, 0, 0),
                    span,
                );
                self.patch(end, self.program.instructions.len())?;
                Value {
                    terminated: left.terminated && right.terminated,
                    ..result
                }
            }
            E::Block(block) => self.block(block, expected, true)?,
            E::Return(expression) => {
                if self.context.check {
                    return Err(self.error(span, "check 本体不能使用 return"));
                }
                let result_type = self.program.result.clone();
                let mut value = if let Some(expression) = expression {
                    self.expression(expression, Some(&result_type))?
                } else {
                    let unit = self.constant(Constant::Unit, Ty::Unit, span)?;
                    self.adapt(unit, Some(&result_type), span)?
                };
                self.emit(
                    Instruction::new(Opcode::Return, value.register, 0, 0, 0),
                    span,
                );
                value.terminated = true;
                value
            }
            E::Propagate(expression) => {
                if !matches!(self.program.result, Ty::Option(_)) {
                    return Err(self.error(span, "可选传播需要可选返回类型"));
                }
                let value = self.expression(expression, None)?;
                let Ty::Option(inner) = &value.ty else {
                    return Err(self.error(span, "只能传播可选值"));
                };
                let some = self.slot(Ty::Bool, span)?;
                self.emit(
                    Instruction::new(Opcode::IsSome, some.register, value.register, 0, 0),
                    span,
                );
                let absent = self.emit_index(Opcode::JumpFalse, some.register, 0, span)?;
                let end = self.emit_index(Opcode::Jump, 0, 0, span)?;
                self.patch(absent, self.program.instructions.len())?;
                self.emit(
                    Instruction::new(Opcode::Return, value.register, 0, 0, 0),
                    span,
                );
                self.patch(end, self.program.instructions.len())?;
                Value {
                    ty: (**inner).clone(),
                    ..value
                }
            }
            E::IsType { value, name } => {
                let path = stable_path(value);
                let value = self.expression(value, None)?;
                let name = self.resolve_name(name);
                if !matches!(value.ty, Ty::RecordRef(_) | Ty::Object(_) | Ty::Option(_))
                    || self.schema.resolve_type(&name).is_none()
                {
                    return Err(self.error(span, "is 需要对象或记录类型"));
                }
                if self.condition {
                    if let Some(path) = path {
                        let source = if let Ty::Option(inner) = &value.ty {
                            inner.as_ref()
                        } else {
                            &value.ty
                        };
                        let narrowed = match source {
                            Ty::RecordRef(_) => Ty::RecordRef(
                                self.schema
                                    .resolve_type(&name)
                                    .ok_or_else(|| self.error(span, "未知类型"))?
                                    .name
                                    .clone(),
                            ),
                            Ty::Object(_) => Ty::Object(
                                self.schema
                                    .resolve_type(&name)
                                    .ok_or_else(|| self.error(span, "未知类型"))?
                                    .name
                                    .clone(),
                            ),
                            _ => return Err(self.error(span, "is 类型判断需要对象或记录")),
                        };
                        if !self.schema.value_type_assignable(&narrowed, source)
                            && !self.schema.value_type_assignable(source, &narrowed)
                        {
                            return Err(self.error(span, "is 两侧类型没有继承关系"));
                        }
                        let root = path.split('.').next().unwrap_or(&path);
                        let local = self.scopes.iter().any(|scope| scope.contains_key(root));
                        let dynamic_host = !local
                            && self
                                .schema
                                .resolve_type(&self.resolve_name(root))
                                .is_some_and(|meta| meta.is_host);
                        if !dynamic_host {
                            if let Some(scope) = self.refinements.last_mut() {
                                scope.insert(path, narrowed);
                            }
                        }
                    }
                }
                let index = u16::try_from(self.program.names.len())
                    .map_err(|_| self.error(span, "类型测试附表超限"))?;
                self.program.names.push(name);
                let result = self.slot(Ty::Bool, span)?;
                self.emit(
                    Instruction::new(Opcode::IsType, result.register, value.register, index, 0),
                    span,
                );
                result
            }
            E::IsSome { value, binding } => {
                let value = self.expression(value, None)?;
                let Ty::Option(inner) = &value.ty else {
                    return Err(self.error(span, "is Some 需要可选值"));
                };
                if self.condition {
                    let bound = self.slot((**inner).clone(), span)?;
                    self.emit(
                        Instruction::new(Opcode::Move, bound.register, value.register, 0, 0),
                        span,
                    );
                    self.declare(binding, &bound, false, span)?;
                }
                let result = self.slot(Ty::Bool, span)?;
                self.emit(
                    Instruction::new(Opcode::IsSome, result.register, value.register, 0, 0),
                    span,
                );
                result
            }
            E::Call {
                function,
                arguments,
            } => self.call(function, arguments, span)?,
            E::Function(function) => self.closure(function, false, span)?,
            E::Template(parts) => self.template(parts, span)?,
            E::Array(values) => {
                let inner = match expected {
                    Some(Ty::Array(inner)) => Some(inner.as_ref()),
                    Some(Ty::Option(inner)) => match inner.as_ref() {
                        Ty::Array(inner) => Some(inner.as_ref()),
                        _ => None,
                    },
                    _ => None,
                };
                let mut values = values
                    .iter()
                    .map(|value| self.expression(value, inner))
                    .collect::<Result<Vec<_>>>()?;
                let ty = inner
                    .cloned()
                    .or_else(|| values.first().map(|value| value.ty.clone()))
                    .ok_or_else(|| self.error(span, "空数组需要明确类型"))?;
                for value in &mut values {
                    *value = self.adapt(value.clone(), Some(&ty), span)?;
                }
                let result = self.slot(Ty::Array(Box::new(ty)), span)?;
                let index = self.program.collections.len();
                self.program
                    .collections
                    .push(values.iter().map(|v| v.register).collect());
                self.emit_index(Opcode::Array, result.register, index, span)?;
                result
            }
            E::Dictionary(entries) => {
                let pair = match expected {
                    Some(Ty::Dict(key, value)) => Some((key.as_ref(), value.as_ref())),
                    _ => None,
                };
                let mut values = Vec::new();
                let mut types = pair.map(|(k, v)| (k.clone(), v.clone()));
                for (key, value) in entries {
                    let key = self.expression(key, types.as_ref().map(|p| &p.0))?;
                    let value = self.expression(value, types.as_ref().map(|p| &p.1))?;
                    if types.is_none() {
                        types = Some((key.ty.clone(), value.ty.clone()));
                    }
                    values.extend([key.register, value.register]);
                }
                let (key, value) = types.ok_or_else(|| self.error(span, "空字典需要明确类型"))?;
                if !matches!(key, Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)) {
                    return Err(self.error(span, "无效的字典 key 类型"));
                }
                let result = self.slot(Ty::Dict(Box::new(key), Box::new(value)), span)?;
                let index = self.program.collections.len();
                self.program.collections.push(values);
                self.emit_index(Opcode::Dictionary, result.register, index, span)?;
                result
            }
            E::Object { type_name, fields } => self.object(type_name, fields, span)?,
        };
        if let Some(path) = stable_path(expression) {
            let root = path.split('.').next().unwrap_or(&path);
            let declaration = self
                .scopes
                .iter()
                .rposition(|scope| scope.contains_key(root))
                .unwrap_or(0);
            if let Some(ty) = self.refinements[declaration..]
                .iter()
                .rev()
                .find_map(|scope| scope.get(&path))
            {
                value.ty = ty.clone();
            }
        }
        self.adapt(value, expected, span)
    }
    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
        self.refinements.push(BTreeMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.refinements.pop();
    }
    fn condition_expression(&mut self, expression: &Expr) -> Result<Value> {
        let previous = self.condition;
        self.condition = true;
        let value = self.expression(expression, Some(&Ty::Bool));
        self.condition = previous;
        value
    }
    fn number(&mut self, number: &str, span: Span) -> Result<Value> {
        let normalized = number.replace('_', "");
        if normalized.contains(['.', 'e', 'E']) || normalized.ends_with("inf") {
            let number = normalized
                .parse::<f32>()
                .map_err(|_| self.error(span, "无效的 float"))?;
            self.constant(Constant::Float(number), Ty::Float, span)
        } else {
            let number = normalized
                .parse::<i32>()
                .map_err(|_| self.error(span, "int 超出 i32 范围"))?;
            self.constant(Constant::Int(number), Ty::Int, span)
        }
    }
    fn name_value(&mut self, name: &str, span: Span) -> Result<Value> {
        if let Some(value) = self.local(name, span)? {
            return Ok(value);
        }
        if name == "self" {
            let ty = self
                .context
                .owner
                .clone()
                .ok_or_else(|| self.error(span, "当前函数没有 self"))?;
            let value = self.slot(ty, span)?;
            self.emit(
                Instruction::new(Opcode::SelfValue, value.register, 0, 0, 0),
                span,
            );
            return Ok(value);
        }
        let name = self.resolve_name(name);
        if name == "Coflow::Check::require" {
            let value = self.slot(
                Ty::Function(
                    vec![
                        CftFunctionParameter::unnamed(Ty::Bool),
                        CftFunctionParameter::unnamed(Ty::String),
                    ],
                    Box::new(Ty::Unit),
                ),
                span,
            )?;
            let index = self.program.names.len();
            self.program
                .names
                .push("$host::Coflow::Check::require".into());
            self.emit_index(Opcode::Reference, value.register, index, span)?;
            return Ok(value);
        }
        if let Some((owner, variant)) = name.rsplit_once("::") {
            if let Some(meta) = self.schema.resolve_enum(owner) {
                let value = self
                    .schema
                    .enum_variant_value(owner, variant)
                    .ok_or_else(|| self.error(span, "未知 enum 变体"))?;
                return self.constant(
                    Constant::Enum {
                        name: owner.into(),
                        value: value as u32,
                    },
                    Ty::Enum(meta.name.clone()),
                    span,
                );
            }
        }
        if let Some(constant) = self.schema.resolve_const(&name) {
            let value = self.slot(constant.value_type.clone(), span)?;
            let index = self.program.names.len();
            self.program.names.push(format!("$const::{name}"));
            self.emit_index(Opcode::Reference, value.register, index, span)?;
            return Ok(value);
        }
        if let Some(meta) = self.schema.resolve_type(&name) {
            if meta.is_singleton {
                let value = self.slot(Ty::RecordRef(meta.name.clone()), span)?;
                let index = self.program.names.len();
                self.program.names.push(format!(
                    "{name}::{}",
                    name.rsplit("::").next().unwrap_or(&name)
                ));
                self.emit_index(Opcode::Reference, value.register, index, span)?;
                return Ok(value);
            }
        }
        Err(self.error(span, format!("未知名称 {name}")))
    }
    fn binary_values(
        &mut self,
        operator: &str,
        mut left: Value,
        mut right: Value,
        span: Span,
    ) -> Result<Value> {
        let flags = binary_code(operator).ok_or_else(|| self.error(span, "无效的二元运算符"))?;
        let numeric =
            matches!(left.ty, Ty::Int | Ty::Float) && matches!(right.ty, Ty::Int | Ty::Float);
        if numeric && (left.ty == Ty::Float || right.ty == Ty::Float || operator == "/") {
            left = self.adapt(left, Some(&Ty::Float), span)?;
            right = self.adapt(right, Some(&Ty::Float), span)?;
        }
        let comparison = matches!(operator, "==" | "!=" | "<" | "<=" | ">" | ">=");
        let equality = matches!(operator, "==" | "!=");
        let valid = if equality {
            matches!(left.ty, Ty::Option(_)) == matches!(right.ty, Ty::Option(_))
                && (self.schema.value_type_assignable(&left.ty, &right.ty)
                    || self.schema.value_type_assignable(&right.ty, &left.ty))
        } else if comparison {
            left.ty == right.ty && matches!(left.ty, Ty::Int | Ty::Float | Ty::String | Ty::Enum(_))
        } else if matches!(operator, "+" | "-" | "*" | "/" | "**") {
            numeric || (operator == "+" && left.ty == Ty::String && right.ty == Ty::String)
        } else if matches!(operator, "&" | "|" | "^") && left.ty == right.ty {
            left.ty == Ty::Int
                || matches!(&left.ty, Ty::Enum(name) if self.schema.resolve_enum(name).is_some_and(|meta| meta.is_flag))
        } else {
            left.ty == Ty::Int && right.ty == Ty::Int
        };
        if !valid {
            return Err(self.error(
                span,
                format!("运算符 {operator} 不适用于 {} 和 {}", left.ty, right.ty),
            ));
        }
        let result = self.slot(if comparison { Ty::Bool } else { left.ty }, span)?;
        self.emit(
            Instruction::new(
                Opcode::Binary,
                result.register,
                left.register,
                right.register,
                flags,
            ),
            span,
        );
        Ok(result)
    }
    fn call(&mut self, target: &Expr, arguments: &[Expr], span: Span) -> Result<Value> {
        if let E::Name(name) = &target.kind {
            let resolved = self.resolve_name(name);
            if let Some(meta) = self.schema.resolve_enum(&resolved) {
                let [argument] = arguments else {
                    return Err(self.error(span, "enum 构造需要一个 int 参数"));
                };
                let argument = self.expression(argument, Some(&Ty::Int))?;
                let result = self.slot(Ty::Enum(meta.name.clone()), span)?;
                let index = self.program.builtins.len();
                self.program.builtins.push(BuiltinSite {
                    name: format!("$enum::{resolved}"),
                    receiver: argument.register,
                    arguments: Vec::new(),
                });
                self.emit_index(Opcode::Builtin, result.register, index, span)?;
                return Ok(result);
            }
            if resolved == "Coflow::Check::records" {
                if !self.context.check || self.context.owner.is_some() {
                    return Err(self.error(span, "records 只用于顶层 check"));
                }
                let [Expr {
                    kind: E::Name(name),
                    ..
                }] = arguments
                else {
                    return Err(self.error(span, "records 需要静态记录类型"));
                };
                let name = self.resolve_name(name);
                let meta = self
                    .schema
                    .resolve_type(&name)
                    .ok_or_else(|| self.error(span, "未知记录类型"))?;
                if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                    return Err(self.error(span, "records 不接受 data"));
                }
                let result =
                    self.slot(Ty::Array(Box::new(Ty::RecordRef(meta.name.clone()))), span)?;
                let receiver = self.constant(Constant::Unit, Ty::Unit, span)?;
                let index = self.program.builtins.len();
                self.program.builtins.push(BuiltinSite {
                    name: format!("$records::{name}"),
                    receiver: receiver.register,
                    arguments: Vec::new(),
                });
                self.emit_index(Opcode::Builtin, result.register, index, span)?;
                return Ok(result);
            }
        }
        let target = if let E::Field { value, name } = &target.kind {
            let receiver = self.expression(value, None)?;
            let field = match &receiver.ty {
                Ty::Object(ty) | Ty::RecordRef(ty) => self
                    .schema
                    .resolve_type(ty)
                    .and_then(|meta| {
                        meta.all_fields()
                            .enumerate()
                            .find(|(_, field)| field.name.as_str() == name)
                    })
                    .map(|(index, field)| (index, field.runtime_value_type())),
                _ => None,
            };
            if let Some((index, ty)) = field {
                let slot = index + usize::from(matches!(receiver.ty, Ty::RecordRef(_)));
                let slot = u16::try_from(slot).map_err(|_| self.error(span, "字段槽超限"))?;
                let target = self.slot(ty, span)?;
                self.emit(
                    Instruction::new(Opcode::Field, target.register, receiver.register, slot, 0),
                    span,
                );
                target
            } else {
                return self.builtin(receiver, name, arguments, span);
            }
        } else {
            self.expression(target, None)?
        };
        let Ty::Function(parameters, result) = &target.ty else {
            return Err(self.error(span, "调用目标不是函数"));
        };
        if arguments.len() != parameters.len() {
            return Err(self.error(span, "函数参数数量不匹配"));
        }
        let arguments = arguments
            .iter()
            .zip(parameters)
            .map(|(arg, parameter)| {
                self.expression(arg, Some(&parameter.value_type))
                    .map(|v| v.register)
            })
            .collect::<Result<Vec<_>>>()?;
        let result = self.slot((**result).clone(), span)?;
        let index = self.program.calls.len();
        self.program.calls.push(CallSite {
            target: target.register,
            arguments,
        });
        self.emit_index(Opcode::Call, result.register, index, span)?;
        Ok(result)
    }
    fn capture_environment(
        &self,
        span: Span,
    ) -> Result<(BTreeMap<String, Local>, BTreeMap<Register, String>)> {
        let mut environment = BTreeMap::new();
        for scope in &self.scopes {
            environment.extend(scope.clone());
        }
        let mut inherited = BTreeMap::new();
        for (name, local) in &self.outer {
            if environment.contains_key(name) {
                continue;
            }
            let register = Register::try_from(self.program.registers.len() + inherited.len())
                .map_err(|_| self.error(span, "捕获候选超限"))?;
            environment.insert(
                name.clone(),
                Local {
                    register,
                    ty: local.ty.clone(),
                    mutable: false,
                },
            );
            inherited.insert(register, name.clone());
        }
        Ok((environment, inherited))
    }
    fn resolve_capture_sources(
        &mut self,
        sources: Vec<Register>,
        inherited: &BTreeMap<Register, String>,
        span: Span,
    ) -> Result<Vec<Register>> {
        // 先编译子程序，再仅为实际使用的祖先变量在本层建立转发捕获。
        sources
            .into_iter()
            .map(|register| {
                if let Some(name) = inherited.get(&register) {
                    self.local(name, span)?
                        .map(|value| value.register)
                        .ok_or_else(|| self.error(span, "捕获来源丢失"))
                } else {
                    Ok(register)
                }
            })
            .collect()
    }
    fn closure(&mut self, function: &Function, template: bool, span: Span) -> Result<Value> {
        let mut context = self.context.clone();
        context.check = false;
        if let Some((_, ty)) = &self.literal_owner {
            context.owner = Some(ty.clone());
        }
        let mut child = Compiler::new(
            self.schema,
            &format!("{}::<closure>", self.program.name),
            &self.program.source,
            context,
        );
        // 所有可见局部值只作为捕获候选；真正读取时才建立捕获槽。
        let (environment, inherited) = self.capture_environment(span)?;
        child.outer = environment;
        child.function(function)?;
        let captures = self.resolve_capture_sources(child.capture_sources, &inherited, span)?;
        let program = child.program;
        let ty = if template {
            Ty::FString
        } else {
            Ty::Function(
                program
                    .parameters
                    .iter()
                    .cloned()
                    .map(CftFunctionParameter::unnamed)
                    .collect(),
                Box::new(program.result.clone()),
            )
        };
        let result = self.slot(ty, span)?;
        let index = self.program.closures.len();
        self.program.closures.push(ClosureSite {
            program: std::sync::Arc::new(program),
            captures,
            owner: self.literal_owner.as_ref().map(|(register, _)| *register),
            template,
        });
        self.emit_index(Opcode::Closure, result.register, index, span)?;
        Ok(result)
    }
    fn template(&mut self, parts: &[TemplatePart], span: Span) -> Result<Value> {
        let mut context = self.context.clone();
        context.check = false;
        if let Some((_, ty)) = &self.literal_owner {
            context.owner = Some(ty.clone());
        }
        let mut child = Compiler::new(
            self.schema,
            &format!("{}::<template>", self.program.name),
            &self.program.source,
            context,
        );
        child.program.result = Ty::String;
        let (environment, inherited) = self.capture_environment(span)?;
        child.outer = environment;
        let mut registers = Vec::new();
        for part in parts {
            let value = match part {
                TemplatePart::Text(text) => {
                    child.constant(Constant::String(text.clone()), Ty::String, span)?
                }
                TemplatePart::Expression(expression) => {
                    let value = child.expression(expression, None)?;
                    if !matches!(
                        value.ty,
                        Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Enum(_)
                    ) || value.terminated
                    {
                        return Err(
                            self.error(expression.span, "插值需要标量文本且不能向外转移控制流")
                        );
                    }
                    value
                }
            };
            registers.push(value.register);
        }
        let result = child.slot(Ty::String, span)?;
        child.program.collections.push(registers);
        child.emit_index(Opcode::Format, result.register, 0, span)?;
        child.emit(
            Instruction::new(Opcode::Return, result.register, 0, 0, 0),
            span,
        );
        let result = self.slot(Ty::FString, span)?;
        let index = self.program.closures.len();
        let captures = self.resolve_capture_sources(child.capture_sources, &inherited, span)?;
        self.program.closures.push(ClosureSite {
            program: std::sync::Arc::new(child.program),
            captures,
            owner: self.literal_owner.as_ref().map(|(register, _)| *register),
            template: true,
        });
        self.emit_index(Opcode::Closure, result.register, index, span)?;
        Ok(result)
    }
    fn object(&mut self, name: &str, fields: &[(String, Expr)], span: Span) -> Result<Value> {
        let name = self.resolve_name(name);
        let meta = self
            .schema
            .resolve_type(&name)
            .ok_or_else(|| self.error(span, "未知 data 类型"))?;
        if !matches!(meta.kind, coflow_language::cft::syntax::ast::TypeKind::Data)
            || meta.is_abstract
        {
            return Err(self.error(span, "只能构造非 abstract data"));
        }
        let result = self.slot(Ty::Object(meta.name.clone()), span)?;
        let site = self.program.objects.len();
        self.program.objects.push(ObjectSite {
            type_name: name.clone(),
            fields: Vec::new(),
        });
        let reservation = self.emit_index(Opcode::Object, result.register, site, span)?;
        let instruction = self.program.instructions[reservation];
        self.program.instructions[reservation] = Instruction::new(
            Opcode::Object,
            instruction.a(),
            instruction.b(),
            instruction.c(),
            1,
        );
        let previous_owner = self
            .literal_owner
            .replace((result.register, result.ty.clone()));
        let mut supplied = BTreeMap::new();
        let mut values = Vec::new();
        for (name, expression) in fields {
            if supplied.insert(name, ()).is_some() {
                return Err(self.error(expression.span, "重复对象字段"));
            }
            let field = meta
                .field(name)
                .ok_or_else(|| self.error(expression.span, "未知对象字段"))?;
            let value = self.expression(expression, Some(&field.value_type))?;
            values.push((name.clone(), value.register));
        }
        for field in meta.all_fields() {
            if !supplied.contains_key(&field.name.to_string())
                && field.default.is_none()
                && !matches!(
                    field.value_type,
                    Ty::Option(_) | Ty::Array(_) | Ty::Dict(..)
                )
            {
                return Err(self.error(span, format!("缺少必填字段 {}", field.name)));
            }
        }
        self.literal_owner = previous_owner;
        self.program.objects[site].fields = values;
        self.emit_index(Opcode::Object, result.register, site, span)?;
        Ok(result)
    }
    fn higher_builtin(
        &mut self,
        receiver: Value,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Value> {
        let (key_type, stored_type) = match &receiver.ty {
            Ty::Array(inner) => (None, (**inner).clone()),
            Ty::Dict(key, inner) => (Some((**key).clone()), (**inner).clone()),
            _ => return Err(self.error(span, "高阶方法需要数组或字典")),
        };
        let callback_index = usize::from(name == "fold");
        if arguments.len() != callback_index + 1 {
            return Err(self.error(span, "高阶方法参数数量不匹配"));
        }
        // 期望类型分析在独立编译状态中完成，不把回调的运行期求值提前到 fold 初值之前。
        let mut analysis = self.clone();
        let signature = analysis.expression(&arguments[callback_index], None)?.ty;
        let Ty::Function(parameters, callback_result) = signature else {
            return Err(self.error(span, "高阶方法需要函数回调"));
        };
        let read_type = match &stored_type {
            Ty::FString => Ty::String,
            Ty::Option(inner) if **inner == Ty::FString => Ty::Option(Box::new(Ty::String)),
            _ => stored_type.clone(),
        };
        let mut expected_parameters = Vec::new();
        if name == "fold" {
            expected_parameters.push((*callback_result).clone());
        }
        if let Some(key) = &key_type {
            expected_parameters.push(key.clone());
        }
        expected_parameters.push(read_type.clone());
        if parameters
            .iter()
            .map(|parameter| &parameter.value_type)
            .ne(expected_parameters.iter())
        {
            return Err(self.error(span, "回调签名与集合读取类型不匹配"));
        }
        if matches!(name, "filter" | "any" | "all") && *callback_result != Ty::Bool {
            return Err(self.error(span, "判定回调必须返回 bool"));
        }
        let initial = if name == "fold" {
            Some(self.expression(&arguments[0], Some(&callback_result))?)
        } else {
            None
        };
        let callback = self.expression(&arguments[callback_index], None)?;
        let result_type = match name {
            "map" => Ty::Array(callback_result.clone()),
            "filter" => receiver.ty.clone(),
            "fold" => (*callback_result).clone(),
            _ => Ty::Bool,
        };
        let result = if let Some(initial) = initial {
            let result = self.slot(result_type, span)?;
            self.emit(
                Instruction::new(Opcode::Move, result.register, initial.register, 0, 0),
                span,
            );
            result
        } else if matches!(name, "any" | "all") {
            self.constant(Constant::Bool(name == "all"), Ty::Bool, span)?
        } else {
            let result = self.slot(result_type, span)?;
            let index = self.program.collections.len();
            self.program.collections.push(Vec::new());
            self.emit_index(
                if matches!(result.ty, Ty::Dict(..)) {
                    Opcode::Dictionary
                } else {
                    Opcode::Array
                },
                result.register,
                index,
                span,
            )?;
            result
        };
        let index = self.constant(Constant::Int(0), Ty::Int, span)?;
        let length = self.slot(Ty::Int, span)?;
        self.emit(
            Instruction::new(Opcode::Length, length.register, receiver.register, 0, 0),
            span,
        );
        let start = self.program.instructions.len();
        let condition = self.binary_values("<", index.clone(), length, span)?;
        let exit = self.emit_index(Opcode::JumpFalse, condition.register, 0, span)?;
        self.emit_index(Opcode::Iteration, 0, 0, span)?;
        let stored = self.slot(stored_type, span)?;
        self.emit(
            Instruction::new(
                Opcode::IteratorValue,
                stored.register,
                receiver.register,
                index.register,
                0,
            ),
            span,
        );
        let read = self.adapt(stored.clone(), Some(&read_type), span)?;
        let mut args = Vec::new();
        if name == "fold" {
            args.push(result.register);
        }
        let key = if let Some(key_type) = key_type {
            let key = self.slot(key_type, span)?;
            self.emit(
                Instruction::new(
                    Opcode::IteratorKey,
                    key.register,
                    receiver.register,
                    index.register,
                    0,
                ),
                span,
            );
            args.push(key.register);
            Some(key)
        } else {
            None
        };
        args.push(read.register);
        let returned = self.slot((*callback_result).clone(), span)?;
        let site = self.program.calls.len();
        self.program.calls.push(CallSite {
            target: callback.register,
            arguments: args,
        });
        self.emit_index(Opcode::Call, returned.register, site, span)?;
        let mut short_circuit = None;
        let mut skip = None;
        match name {
            "fold" => {
                self.emit(
                    Instruction::new(Opcode::Move, result.register, returned.register, 0, 0),
                    span,
                );
            }
            "any" | "all" => {
                self.emit(
                    Instruction::new(Opcode::Move, result.register, returned.register, 0, 0),
                    span,
                );
                if name == "all" {
                    short_circuit =
                        Some(self.emit_index(Opcode::JumpFalse, returned.register, 0, span)?);
                } else {
                    let inverted = self.slot(Ty::Bool, span)?;
                    self.emit(
                        Instruction::new(Opcode::Unary, inverted.register, returned.register, 0, 1),
                        span,
                    );
                    short_circuit =
                        Some(self.emit_index(Opcode::JumpFalse, inverted.register, 0, span)?);
                }
            }
            _ => {
                if name == "filter" {
                    skip = Some(self.emit_index(Opcode::JumpFalse, returned.register, 0, span)?);
                }
                let item = if name == "map" {
                    returned.register
                } else {
                    stored.register
                };
                self.emit(
                    Instruction::new(
                        Opcode::Append,
                        result.register,
                        item,
                        key.as_ref().map_or(index.register, |key| key.register),
                        u8::from(matches!(result.ty, Ty::Dict(..))),
                    ),
                    span,
                );
            }
        }
        if let Some(skip) = skip {
            self.patch(skip, self.program.instructions.len())?;
        }
        let one = self.constant(Constant::Int(1), Ty::Int, span)?;
        let next = self.binary_values("+", index.clone(), one, span)?;
        self.emit(
            Instruction::new(Opcode::Move, index.register, next.register, 0, 0),
            span,
        );
        self.emit_index(Opcode::Jump, 0, start, span)?;
        let end = self.program.instructions.len();
        self.patch(exit, end)?;
        if let Some(jump) = short_circuit {
            self.patch(jump, end)?;
        }
        Ok(result)
    }
    fn builtin(
        &mut self,
        receiver: Value,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Value> {
        if matches!(name, "map" | "filter" | "fold" | "any" | "all") {
            return self.higher_builtin(receiver, name, arguments, span);
        }
        let dimension = if let Ty::RecordRef(type_name) = &receiver.ty {
            crate::loading::dimension_source(self.schema, type_name)
        } else {
            None
        };
        let signature = if let Some((_, field)) = dimension {
            let read = ordinary_type(&field.value_type);
            match name {
                "for" => Some((vec![Ty::String], read)),
                "default" => Some((vec![], read)),
                "variants" => Some((vec![], Ty::Dict(Box::new(Ty::String), Box::new(read)))),
                _ => builtin_signature(&receiver.ty, name),
            }
        } else {
            builtin_signature(&receiver.ty, name)
        };
        let (parameters, result) =
            signature.ok_or_else(|| self.error(span, format!("{} 不提供 {name}", receiver.ty)))?;
        if arguments.len() != parameters.len() {
            return Err(self.error(span, "内建参数数量不匹配"));
        }
        let mut registers = Vec::new();
        for (expression, ty) in arguments.iter().zip(parameters) {
            registers.push(self.expression(expression, Some(&ty))?.register);
        }
        if name == "matches" {
            let Some(Expr {
                kind: E::String(pattern),
                ..
            }) = arguments.first()
            else {
                return Err(self.error(span, "正则模式必须是普通字符串字面量"));
            };
            regex::Regex::new(pattern).map_err(|error| self.error(span, error.to_string()))?;
        }
        let result = self.slot(result, span)?;
        let index = self.program.builtins.len();
        self.program.builtins.push(BuiltinSite {
            name: name.into(),
            receiver: receiver.register,
            arguments: registers,
        });
        self.emit_index(Opcode::Builtin, result.register, index, span)?;
        Ok(result)
    }
}
pub fn binary_code(operator: &str) -> Option<u8> {
    [
        "+", "-", "*", "/", "//", "%", "**", "==", "!=", "<", "<=", ">", ">=", "<<", ">>", "&",
        "|", "^",
    ]
    .iter()
    .position(|value| *value == operator)
    .map(|value| value as u8)
}
fn builtin_signature(ty: &Ty, name: &str) -> Option<(Vec<Ty>, Ty)> {
    Some(match (ty, name) {
        (Ty::String | Ty::Array(_) | Ty::Dict(..), "len") => (vec![], Ty::Int),
        (Ty::String, "contains" | "startsWith" | "endsWith" | "matches") => {
            (vec![Ty::String], Ty::Bool)
        }
        (Ty::String, "isBlank") => (vec![], Ty::Bool),
        (Ty::String, "parseInt") => (vec![], Ty::Option(Box::new(Ty::Int))),
        (Ty::String, "parseFloat") => (vec![], Ty::Option(Box::new(Ty::Float))),
        (Ty::Int, "float") => (vec![], Ty::Float),
        (Ty::Float, "int") => (vec![], Ty::Int),
        (Ty::Int | Ty::Float | Ty::Bool | Ty::Enum(_), "string") => (vec![], Ty::String),
        (Ty::Int | Ty::Float, "abs") => (vec![], ty.clone()),
        (Ty::Float, "isFinite") => (vec![], Ty::Bool),
        (Ty::Float, "approxEqual") => (vec![Ty::Float, Ty::Float], Ty::Bool),
        (Ty::Option(_), "isSome" | "isNone") => (vec![], Ty::Bool),
        (Ty::Array(inner), "contains") => (vec![ordinary_type(inner)], Ty::Bool),
        (Ty::Array(inner), "min" | "max")
            if matches!(**inner, Ty::Int | Ty::Float | Ty::Enum(_)) =>
        {
            (vec![], (**inner).clone())
        }
        (Ty::Array(inner), "sum") if matches!(**inner, Ty::Int | Ty::Float) => {
            (vec![], (**inner).clone())
        }
        (Ty::Array(inner), "isUnique")
            if matches!(**inner, Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)) =>
        {
            (vec![], Ty::Bool)
        }
        (Ty::Array(inner), "isSorted" | "isStrictlySorted")
            if matches!(**inner, Ty::Int | Ty::Float | Ty::String | Ty::Enum(_)) =>
        {
            (vec![], Ty::Bool)
        }
        (Ty::Array(inner), "intersects" | "isDisjoint" | "isSubsetOf" | "isSupersetOf")
            if matches!(**inner, Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)) =>
        {
            (vec![ty.clone()], Ty::Bool)
        }
        (Ty::Dict(key, _), "contains" | "containsKey") => (vec![(**key).clone()], Ty::Bool),
        (Ty::Dict(_, value), "containsValue") => (vec![ordinary_type(value)], Ty::Bool),
        (Ty::Dict(key, _), "keys") => (vec![], Ty::Array(key.clone())),
        (Ty::Dict(_, value), "values") => (vec![], Ty::Array(value.clone())),
        _ => return None,
    })
}

fn stable_path(expression: &Expr) -> Option<String> {
    match &expression.kind {
        E::Name(name) => Some(name.clone()),
        E::Field { value, name } => Some(format!("{}.{name}", stable_path(value)?)),
        _ => None,
    }
}

fn ordinary_type(ty: &Ty) -> Ty {
    match ty {
        Ty::FString => Ty::String,
        Ty::Option(inner) if **inner == Ty::FString => Ty::Option(Box::new(Ty::String)),
        _ => ty.clone(),
    }
}
