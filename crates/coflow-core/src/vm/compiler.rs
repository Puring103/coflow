//! CFT/CFD 共用语义前端，直接生成类型化 IR；所有分支均检查。
use super::builtins::builtin_signature;
use super::bytecode::{Constant, Program};
use super::ir::{Function as IrFunction, NodeIndex, Node, Operation as O, IrValueId};
use crate::{
    schema::{CftFunctionParameter, CftSchema, CftValueType as Ty},
    source::Span,
};
use coflow_language::{
    cft::syntax::ast::{TypeRef, TypeRefKind},
    function::{self, Block, Expr, ExprKind as E, Function, StatementKind as S, TemplatePart},
};
use std::collections::BTreeMap;
mod builder;
mod expressions;
mod closures;
mod collections;
mod inference;

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
    id: IrValueId,
    ty: Ty,
    mutable: bool,
    builder: Option<BTreeMap<String, Value>>,
}
#[derive(Debug, Clone)]
struct Value {
    id: IrValueId,
    ty: Ty,
    terminated: bool,
}
#[derive(Debug, Clone, Default)]
struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
    scope_depth: usize,
}
struct Compiler<'a> {
    schema: &'a CftSchema,
    context: CompileContext,
    program: IrFunction,
    scopes: Vec<BTreeMap<String, Local>>,
    outer: BTreeMap<String, Local>,
    captured: BTreeMap<String, (IrValueId, usize)>,
    capture_sources: Vec<IrValueId>,
    loops: Vec<Loop>,
    condition: bool,
    literal_owner: Option<(IrValueId, Ty)>,
    refinements: Vec<BTreeMap<String, Ty>>,
}

pub fn compile(
    schema: &CftSchema,
    source: &str,
    name: &str,
    context: CompileContext,
) -> Result<Program> {
    lower_analysis(analyze(schema, source, name, context)?)
}
fn lower_analysis(function: super::ir::Function) -> Result<Program> {
    function.lower().map_err(|message| CompileError {
        span: function
            .body
            .first()
            .map_or(Span::default(), |node| node.span),
        message,
    })
}
fn finish_analysis(schema: &CftSchema, function: IrFunction) -> Result<IrFunction> {
    // CFD 与 CFT 共用同一语义边界；不能让即时编译绕过 Contract 的 IR 检查。
    function
        .validate_semantics(schema)
        .map_err(|message| CompileError {
            span: function
                .body
                .first()
                .map_or(Span::default(), |node| node.span),
            message,
        })?;
    Ok(function)
}
pub fn analyze(
    schema: &CftSchema,
    source: &str,
    name: &str,
    context: CompileContext,
) -> Result<super::ir::Function> {
    let function = function::parse_function(source)?;
    let mut compiler = Compiler::new(schema, name, source, context);
    compiler.function(&function)?;
    finish_analysis(schema, compiler.program)
}
pub fn analyze_template(
    schema: &CftSchema,
    source: &str,
    name: &str,
    context: CompileContext,
) -> Result<super::ir::Function> {
    let expression = function::parse_expression(source)?;
    let E::Template(parts) = &expression.kind else {
        return Err(CompileError {
            span: expression.span,
            message: "需要 fstring 模板".into(),
        });
    };
    let mut compiler = Compiler::new(schema, name, source, context);
    compiler.template(parts, expression.span)?;
    let Some(Node {
        operation: O::Closure { function, .. },
        ..
    }) = compiler.program.body.pop()
    else {
        return Err(compiler.error(expression.span, "缺少模板程序"));
    };
    finish_analysis(schema, *function)
}
pub fn analyze_check(
    schema: &CftSchema,
    check: &function::Check,
    source: &str,
    name: &str,
    mut context: CompileContext,
) -> Result<super::ir::Function> {
    context.check = true;
    let mut compiler = Compiler::new(schema, name, source, context);
    let result = compiler.block(&check.body, Some(&Ty::Unit), false)?;
    compiler.emit(result.id, O::Return(result.id), check.span);
    finish_analysis(schema, compiler.program)
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
    fn new(schema: &'a CftSchema, name: &str, source: impl Into<std::sync::Arc<str>>, context: CompileContext) -> Self {
        let program = IrFunction {
            owner: context.owner.clone(),
            name: name.into(),
            source: source.into(),
            module: None,
            path: None,
            parameters: Vec::new(),
            result: Ty::Unit,
            values: Vec::new(),
            captures: Vec::new(),
            body: Vec::new(),
        };
        Self {
            schema,
            context,
            program,
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
        let id = u32::try_from(self.program.values.len())
            .map(IrValueId)
            .map_err(|_| self.error(span, "函数虚拟值数量超限"))?;
        self.program.values.push(ty.clone());
        Ok(Value {
            id,
            ty,
            terminated: false,
        })
    }
    /// 前端只生成语义节点，操作数编码和物理寄存器分配属于映像降低阶段。
    fn emit(&mut self, destination: IrValueId, operation: O, span: Span) -> usize {
        let index = self.program.body.len();
        self.program.body.push(Node {
            destination,
            operation,
            span,
        });
        index
    }
    fn jump(&mut self, condition: Option<IrValueId>, target: usize, span: Span) -> Result<usize> {
        let target = NodeIndex(u32::try_from(target).map_err(|_| self.error(span, "函数体过大"))?);
        Ok(self.emit(
            IrValueId(0),
            match condition {
                Some(condition) => O::JumpFalse { condition, target },
                None => O::Jump(target),
            },
            span,
        ))
    }
    fn patch(&mut self, instruction: usize, target: usize) -> Result<()> {
        let target = NodeIndex(
            u32::try_from(target).map_err(|_| self.error(Span::default(), "函数体过大"))?,
        );
        match &mut self.program.body[instruction].operation {
            O::Jump(location)
            | O::JumpFalse {
                target: location, ..
            }
            | O::ForPrep {
                target: location, ..
            }
            | O::ForLoop {
                target: location, ..
            } => *location = target,
            _ => return Err(self.error(self.program.body[instruction].span, "只能回填控制流节点")),
        }
        Ok(())
    }
    fn constant(&mut self, constant: Constant, ty: Ty, span: Span) -> Result<Value> {
        let value = self.slot(ty, span)?;
        self.emit(value.id, O::Constant(constant), span);
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
                id: value.id,
                ty: value.ty.clone(),
                mutable,
                builder: None,
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
            if local.builder.is_some() { return Err(self.error(span, "构造能力不能复制、传参或返回")); }
            return Ok(Some(Value {
                id: local.id,
                ty: local.ty,
                terminated: false,
            }));
        }
        let Some(outer) = self.outer.get(name).cloned() else {
            return Ok(None);
        };
        if outer.builder.is_some() { return Err(self.error(span, "闭包不能捕获构造能力")); }
        if let Some((id, index)) = self.captured.get(name).copied() {
            self.emit(id, O::Capture(index as u32), span);
            return Ok(Some(Value {
                id,
                ty: outer.ty,
                terminated: false,
            }));
        }
        let value = self.slot(outer.ty.clone(), span)?;
        let index = self.capture_sources.len();
        self.capture_sources.push(outer.id);
        self.program.captures.push(outer.ty);
        self.emit(value.id, O::Capture(index as u32), span);
        self.captured.insert(name.into(), (value.id, index));
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
        self.emit(result.id, O::Return(result.id), function.body.span);
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
                self.emit(result.id, O::ReadTemplate(value.id), span);
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
                self.emit(result.id, O::ConvertFloat(value.id), span);
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
                S::Set { target, value } => { self.builder_set(target, value, span)?; }
                S::Variable { name, ty, value } => {
                    let ty = self.resolve_type(ty)?;
                    let initial = self.expression(value, Some(&ty))?;
                    // 表达式结果已有独立虚拟值，局部声明直接绑定该值。
                    self.declare(name, &initial, true, span)?;
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
                    if operator == "=" {
                        let next = self.expression(value, Some(&local.ty))?;
                        self.emit(local.id, O::Copy(next.id), span);
                        terminated |= next.terminated;
                    } else {
                        // 复合赋值更新局部绑定；SSA 阶段拆分定义，物理槽位由降低阶段决定。
                        let left = Expr {
                            kind: E::Name(name.clone()),
                            span,
                        };
                        let right = self.expression(value, Some(&local.ty))?;
                        let left = self.expression(&left, Some(&local.ty))?;
                        let next = self.binary_values_into(
                            operator.trim_end_matches('='),
                            left,
                            right,
                            span,
                            local.id,
                        )?;
                        terminated |= next.terminated;
                    }
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
                    let start = self.program.body.len();
                    self.push_scope();
                    let condition = self.condition_expression(condition)?;
                    let exit = self.jump(Some(condition.id), 0, span)?;
                    self.emit(IrValueId(0), O::Iteration, span);
                    self.loops.push(Loop { scope_depth: self.scopes.len(), ..Loop::default() });
                    self.block(body, Some(&Ty::Unit), true)?;
                    self.jump(None, start, span)?;
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
                    self.drop_builders(self.loops.last().unwrap().scope_depth, span)?;
                    let jump = self.jump(None, 0, span)?;
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
        let end = self.program.body.len();
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
                self.emit(current.id, O::Copy(first.id), span);
                self.declare(&bindings[0], &current, false, span)?;
                // 区间语义保存在节点中；出口位置在分析完循环体后回填。
                let exclusive = operator == "..";
                let start = self.program.body.len();
                let guard = self.emit(
                    current.id,
                    O::ForPrep {
                        limit: last.id,
                        target: NodeIndex(0),
                        exclusive,
                    },
                    span,
                );
                self.loops.push(Loop { scope_depth: self.scopes.len(), ..Loop::default() });
                self.block(body, Some(&Ty::Unit), true)?;
                // 回边：value == limit（含）或 value >= limit（排）则落出；
                // 否则自增并跳回循环体。自增前已确认 value < limit，无溢出。
                self.emit(
                    current.id,
                    O::ForLoop {
                        limit: last.id,
                        target: NodeIndex(start as u32),
                        exclusive,
                    },
                    span,
                );
                let exit = self.program.body.len();
                self.patch(guard, exit)?;
                if let Some(current) = self.loops.pop() {
                    for jump in current.breaks {
                        self.patch(jump, exit)?;
                    }
                    for jump in current.continues {
                        self.patch(jump, self.program.body.len() - 1)?;
                    }
                }
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
        self.emit(length.id, O::Length(iterable.id), span);
        let start = self.program.body.len();
        let condition = self.binary_values("<", index.clone(), length, span)?;
        let exit = self.jump(Some(condition.id), 0, span)?;
        self.emit(IrValueId(0), O::Iteration, span);
        let stored = self.slot(value_type, span)?;
        if bindings.len() == 2 {
            // 双绑定迭代在同一语义节点定义键和值，保持共同的迭代位置。
            let key = self.slot(key_type, span)?;
            self.emit(
                IrValueId(0),
                O::IterNext {
                    collection: iterable.id,
                    counter: index.id,
                    key: key.id,
                    value: stored.id,
                },
                span,
            );
            let value = self.adapt(stored, None, span)?;
            self.declare(&bindings[bindings.len() - 1], &value, false, span)?;
            self.declare(&bindings[0], &key, false, span)?;
        } else {
            self.emit(
                stored.id,
                O::IteratorValue {
                    collection: iterable.id,
                    index: index.id,
                },
                span,
            );
            let value = self.adapt(stored, None, span)?;
            self.declare(&bindings[bindings.len() - 1], &value, false, span)?;
        }
        self.loops.push(Loop { scope_depth: self.scopes.len(), ..Loop::default() });
        self.block(body, Some(&Ty::Unit), true)?;
        let continuation = self.program.body.len();
        let one = self.constant(Constant::Int(1), Ty::Int, span)?;
        let next = self.binary_values("+", index.clone(), one, span)?;
        self.emit(index.id, O::Copy(next.id), span);
        self.jump(None, start, span)?;
        self.end_loop(exit, continuation)?;
        self.scopes.pop();
        Ok(())
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
    /// 字面量 int 键提取：`0`、`-3` 直接内联进索引附表；其余返回 None 走通用路径。
    fn inline_index_key(&self, expression: &Expr) -> Option<i32> {
        let parse = |number: &str| -> Option<i32> {
            let normalized = number.replace('_', "");
            if normalized.contains(['.', 'e', 'E']) || normalized.ends_with("inf") {
                return None;
            }
            normalized.parse::<i32>().ok()
        };
        match &expression.kind {
            E::Number(number) => parse(number),
            E::Unary { operator, value } if operator == "-" => match &value.kind {
                E::Number(number) => parse(&format!("-{number}")),
                _ => None,
            },
            _ => None,
        }
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
            self.emit(value.id, O::Owner, span);
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
            self.emit(
                value.id,
                O::Reference("$host::Coflow::Check::require".into()),
                span,
            );
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
            self.emit(value.id, O::Reference(format!("$const::{name}")), span);
            return Ok(value);
        }
        if let Some(meta) = self.schema.resolve_type(&name) {
            if meta.is_singleton {
                let value = self.slot(Ty::RecordRef(meta.name.clone()), span)?;
                self.emit(
                    value.id,
                    O::Reference(format!(
                        "{name}::{}",
                        name.rsplit("::").next().unwrap_or(&name)
                    )),
                    span,
                );
                return Ok(value);
            }
        }
        Err(self.error(span, format!("未知名称 {name}")))
    }
    fn binary_values(
        &mut self,
        operator: &str,
        left: Value,
        right: Value,
        span: Span,
    ) -> Result<Value> {
        self.binary_values_dest(operator, left, right, span, None)
    }
    /// 带目标寄存器的二元运算：复合赋值直接写入局部寄存器，省去临时寄存器与 Move。
    fn binary_values_dest(
        &mut self,
        operator: &str,
        mut left: Value,
        mut right: Value,
        span: Span,
        dest: Option<IrValueId>,
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
        let ty = if comparison { Ty::Bool } else { left.ty };
        let result = match dest {
            // 复合赋值：结果直接写入局部寄存器（左操作数即该寄存器，写入不影响读取）。
            Some(dest) => {
                if left.id != dest {
                    return Err(self.error(span, "复合赋值的左操作数必须是目标局部变量"));
                }
                Value {
                    id: dest,
                    ty,
                    terminated: false,
                }
            }
            None => self.slot(ty, span)?,
        };
        self.emit(
            result.id,
            O::Binary {
                operator: flags,
                left: left.id,
                right: right.id,
            },
            span,
        );
        Ok(result)
    }
    /// 带目标寄存器的二元运算：复合赋值直接写入局部寄存器，省去临时寄存器与 Move。
    fn binary_values_into(
        &mut self,
        operator: &str,
        left: Value,
        right: Value,
        span: Span,
        dest: IrValueId,
    ) -> Result<Value> {
        // 左操作数即局部变量本身（寄存器相同），写目标寄存器不影响读取。
        if left.id != dest {
            return Err(self.error(span, "复合赋值的左操作数必须是目标局部变量"));
        }
        let original_ty = left.ty.clone();
        let result = self.binary_values_dest(operator, left, right, span, Some(dest))?;
        // 复合赋值不允许运算改变目标类型（如 int 局部被提升为 float）。
        if result.ty != original_ty {
            return Err(self.error(span, "复合赋值不允许改变局部变量类型"));
        }
        Ok(result)
    }
    fn call(&mut self, target: &Expr, arguments: &[Expr], span: Span) -> Result<Value> {
        if let E::Field { value, name } = &target.kind {
            if let Some(builder) = self.builder_binding(value) { return self.builder_call(builder, name, arguments, span); }
        }
        if let E::Name(name) = &target.kind {
            let resolved = self.resolve_name(name);
            if let Some(meta) = self.schema.resolve_enum(&resolved) {
                let [argument] = arguments else {
                    return Err(self.error(span, "enum 构造需要一个 int 参数"));
                };
                let argument = self.expression(argument, Some(&Ty::Int))?;
                let result = self.slot(Ty::Enum(meta.name.clone()), span)?;
                self.emit(
                    result.id,
                    O::Builtin {
                        name: format!("$enum::{resolved}"),
                        receiver: argument.id,
                        arguments: Vec::new(),
                    },
                    span,
                );
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
                self.emit(
                    result.id,
                    O::Builtin {
                        name: format!("$records::{name}"),
                        receiver: receiver.id,
                        arguments: Vec::new(),
                    },
                    span,
                );
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
                    target.id,
                    O::Field {
                        receiver: receiver.id,
                        field: slot.into(),
                    },
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
                    .map(|v| v.id)
            })
            .collect::<Result<Vec<_>>>()?;
        let result = self.slot((**result).clone(), span)?;
        self.emit(
            result.id,
            O::Call {
                target: target.id,
                arguments: arguments,
            },
            span,
        );
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
        self.emit(
            result.id,
            O::ReserveObject {
                type_name: name.clone(),
            },
            span,
        );
        let previous_owner = self.literal_owner.replace((result.id, result.ty.clone()));
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
            values.push((name.clone(), value.id));
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
        self.emit(
            result.id,
            O::InitializeObject {
                type_name: name,
                fields: values,
            },
            span,
        );
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
        let signature = builtin_signature(self.schema, &receiver.ty, name);
        let (parameters, result) =
            signature.ok_or_else(|| self.error(span, format!("{} 不提供 {name}", receiver.ty)))?;
        if arguments.len() != parameters.len() {
            return Err(self.error(span, "内建参数数量不匹配"));
        }
        let mut value_ids = Vec::new();
        for (expression, ty) in arguments.iter().zip(parameters) {
            value_ids.push(self.expression(expression, Some(&ty))?.id);
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
        self.emit(
            result.id,
            O::Builtin {
                name: name.into(),
                receiver: receiver.id,
                arguments: value_ids,
            },
            span,
        );
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

fn stable_path(expression: &Expr) -> Option<String> {
    match &expression.kind {
        E::Name(name) => Some(name.clone()),
        E::Field { value, name } => Some(format!("{}.{name}", stable_path(value)?)),
        _ => None,
    }
}

