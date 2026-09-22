//! 表达式类型检查与 IR 发射。
use super::*;
impl Compiler<'_> {
    pub(super) fn expression(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let condition = self.condition;
        // 只有正向测试和 && 链能向成功分支传递保证；比较、调用和取反均隔离收窄。
        self.condition &= matches!(&expression.kind, E::IsSome { .. } | E::IsType { .. })
            || matches!(&expression.kind,E::Binary{operator,..} if operator=="&&");
        let result = match &expression.kind {
            E::Binary { operator, .. } if matches!(operator.as_str(), "&&" | "||") => self.logical_expression(expression, expected),
            E::Binary { .. } => self.binary_expression(expression, expected),
            _ => self.expression_inner(expression, expected),
        };
        self.condition = condition;
        result
    }
    // 中缀链由显式工作栈降低，源程序长度不再占用本机调用栈。
    fn binary_expression(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        enum Task<'a> { Read(&'a Expr), Right(&'a Expr), Finish(&'a Expr, Value) }
        let mut tasks = vec![Task::Read(expression)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Read(expr) => {
                    if let E::Binary { operator, left, right } = &expr.kind {
                        if !matches!(operator.as_str(), "&&" | "||") {
                            if matches!(operator.as_str(), "==" | "!=") && matches!(left.kind, E::None) {
                                // None 需要右侧提供期望类型；其自身没有副作用。
                                let right = self.expression(right, None)?;
                                let left = self.expression(left, Some(&right.ty))?;
                                values.push(self.binary_values(operator, left, right, expr.span)?);
                            } else {
                                tasks.push(Task::Right(expr));
                                tasks.push(Task::Read(left));
                            }
                            continue;
                        }
                    }
                    values.push(self.expression(expr, None)?);
                }
                Task::Right(expr) => {
                    let left = values.pop().expect("左操作数已降低");
                    let E::Binary { operator, right, .. } = &expr.kind else { unreachable!() };
                    if matches!(operator.as_str(), "==" | "!=") && matches!(right.kind, E::None) {
                        let right = self.expression(right, Some(&left.ty))?;
                        values.push(self.binary_values(operator, left, right, expr.span)?);
                    } else {
                        tasks.push(Task::Finish(expr, left));
                        tasks.push(Task::Read(right));
                    }
                }
                Task::Finish(expr, left) => {
                    let right = values.pop().expect("右操作数已降低");
                    let E::Binary { operator, .. } = &expr.kind else { unreachable!() };
                    values.push(self.binary_values(operator, left, right, expr.span)?);
                }
            }
        }
        self.adapt(values.pop().expect("表达式已降低"), expected, expression.span)
    }
    // 短路链同样使用显式栈，作用域及成功分支收窄仍按源程序顺序进入和退出。
    fn logical_expression(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        enum Task<'a> { Read(&'a Expr), Left(&'a Expr, bool), Right(&'a Expr, Value, usize, bool) }
        let mut tasks = vec![Task::Read(expression)];
        let mut values = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Read(expr) => {
                    if let E::Binary { operator, left, .. } = &expr.kind {
                        if matches!(operator.as_str(), "&&" | "||") {
                            let saved = self.condition;
                            self.condition &= operator == "&&";
                            if operator == "||" { self.push_scope(); }
                            tasks.push(Task::Left(expr, saved));
                            tasks.push(Task::Read(left));
                            continue;
                        }
                    }
                    values.push(self.expression(expr, Some(&Ty::Bool))?);
                }
                Task::Left(expr, saved) => {
                    let left = values.pop().expect("左条件已降低");
                    let E::Binary { operator, right, .. } = &expr.kind else { unreachable!() };
                    if operator == "||" { self.pop_scope(); self.push_scope(); }
                    let result = self.slot(Ty::Bool, expr.span)?;
                    self.emit(result.id, O::Copy(left.id), expr.span);
                    let condition = if operator == "||" {
                        let inverted = self.slot(Ty::Bool, expr.span)?;
                        self.emit(inverted.id, O::Unary { operator: 1, value: left.id }, expr.span);
                        inverted
                    } else { left };
                    let end = self.jump(Some(condition.id), 0, expr.span)?;
                    tasks.push(Task::Right(expr, result, end, saved));
                    tasks.push(Task::Read(right));
                }
                Task::Right(expr, result, end, saved) => {
                    let right = values.pop().expect("右条件已降低");
                    self.emit(result.id, O::Copy(right.id), expr.span);
                    self.patch(end, self.program.body.len())?;
                    if matches!(&expr.kind, E::Binary { operator, .. } if operator == "||") { self.pop_scope(); }
                    self.condition = saved;
                    values.push(result);
                }
            }
        }
        self.adapt(values.pop().expect("短路表达式已降低"), expected, expression.span)
    }
    pub(super) fn expression_inner(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let mut value = match &expression.kind {
            E::Build { source, binding, body } => self.build_expression(source, binding, body, span)?,
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
            E::Reference { .. } => self.lower_reference(expression, expected)?,
            E::Unary { .. } => self.lower_unary(expression, expected)?,
            E::Binary { .. } => unreachable!("二元表达式由工作栈处理"),
            E::Field { .. } => self.lower_field(expression, expected)?,
            E::Index { .. } => self.lower_index(expression, expected)?,
            E::If { .. } => self.lower_if(expression, expected)?,
            E::Block(block) => self.block(block, expected, true)?,
            E::Return(..) => self.lower_return(expression, expected)?,
            E::Propagate(..) => self.lower_propagate(expression, expected)?,
            E::IsType { .. } => self.lower_is_type(expression, expected)?,
            E::IsSome { .. } => self.lower_is_some(expression, expected)?,
            E::Call {
                function,
                arguments,
            } => self.call(function, arguments, span)?,
            E::Function(function) => self.closure(function, false, span)?,
            E::Template(parts) => self.template(parts, span)?,
            E::Array(..) => self.lower_array(expression, expected)?,
            E::Dictionary(..) => self.lower_dictionary(expression, expected)?,
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
                .cloned()
            {
                if value.ty != ty {
                    let narrowed = self.slot(ty, span)?;
                    self.emit(narrowed.id, O::Copy(value.id), span);
                    value = narrowed;
                }
            }
        }
        self.adapt(value, expected, span)
    }
    fn lower_reference(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Reference { type_name, key } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
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
                self.emit(value.id, O::Reference(format!("{name}::{key}")), span);
                value
        })
    }
    fn lower_unary(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        // 一元链同样使用显式栈，保留负数字面量对 i32::MIN 的特殊处理。
        let mut operators = Vec::new();
        let mut current = expression;
        let mut value = loop {
            if let E::Unary { operator, value } = &current.kind {
                if operator == "-" {
                    if let E::Number(number) = &value.kind {
                        break self.number(&format!("-{number}"), current.span)?;
                    }
                }
                operators.push((operator.as_str(), current.span));
                current = value;
            } else { break self.expression(current, None)?; }
        };
        for (operator, span) in operators.into_iter().rev() {
            let flags = match (operator, &value.ty) {
                ("-", Ty::Int | Ty::Float) => 0,
                ("!", Ty::Bool) => 1,
                ("~", Ty::Int) => 2,
                ("~", Ty::Enum(name)) if self.schema.resolve_enum(name).is_some_and(|meta| meta.is_flag) => 2,
                _ => return Err(self.error(span, "一元运算符与操作数类型不匹配")),
            };
            let result = self.slot(value.ty, span)?;
            self.emit(result.id, O::Unary { operator: flags, value: value.id }, span);
            value = result;
        }
        Ok(value)
    }
    fn lower_field(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Field { value, name } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
                if let Some(builder) = self.builder_binding(value) {
                    let value = builder.builder.as_ref().and_then(|fields| fields.get(name)).cloned()
                        .ok_or_else(|| self.error(span, "构造目标没有该字段"))?;
                    let copied = self.slot(value.ty, span)?;
                    self.emit(copied.id, O::Copy(value.id), span);
                    return self.adapt(copied, expected, span);
                }
                // self 字段保留显式 owner 语义，供映像数据专化使用。
                if let E::Name(owner) = &value.kind {
                    if owner == "self"
                        && self.local("self", span)?.is_none()
                        && self.context.owner.is_some()
                    {
                        let owner_ty = self.context.owner.clone().unwrap();
                        let type_name = match &owner_ty {
                            Ty::Object(type_name) | Ty::RecordRef(type_name) => type_name,
                            _ => return Err(self.error(span, "字段读取需要对象或记录")),
                        };
                        let meta = self
                            .schema
                            .resolve_type(type_name)
                            .ok_or_else(|| self.error(span, "未知对象类型"))?;
                        let (index, ty) = if name == "id" && matches!(owner_ty, Ty::RecordRef(_)) {
                            (0, Ty::String)
                        } else {
                            let (index, field) = meta
                                .all_fields()
                                .enumerate()
                                .find(|(_, field)| field.name.as_str() == name)
                                .ok_or_else(|| self.error(span, format!("未知字段 {name}")))?;
                            (
                                index + usize::from(matches!(owner_ty, Ty::RecordRef(_))),
                                field.runtime_value_type(),
                            )
                        };
                        let index =
                            u16::try_from(index).map_err(|_| self.error(span, "字段槽超限"))?;
                        let result = self.slot(ty, span)?;
                        self.emit(result.id, O::OwnerField(index.into()), span);
                        // 融合路径同样要走尾部适配，保持与通用路径一致的类型收窄。
                        return self.adapt(result, expected, span);
                    }
                }
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
                    result.id,
                    O::Field {
                        receiver: receiver.id,
                        field: index.into(),
                    },
                    span,
                );
                result
        })
    }
    fn lower_index(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Index { value, index } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
                let value = if let Some(builder) = self.builder_binding(value) {
                    Value { id: builder.id, ty: builder.ty, terminated: false }
                } else { self.expression(value, None)? };
                let (key, ty) = match &value.ty {
                    Ty::Array(inner) => (Ty::Int, (**inner).clone()),
                    Ty::Dict(key, inner) => ((**key).clone(), (**inner).clone()),
                    Ty::String => (Ty::Int, Ty::String),
                    _ => return Err(self.error(span, "类型不支持索引")),
                };
                let result = self.slot(ty, span)?;
                // 整数字面量索引保留为语义常量，编码方式由降低阶段决定。
                if let Some(key) = self.inline_index_key(index) {
                    self.emit(
                        result.id,
                        O::IndexConstant {
                            receiver: value.id,
                            key: Constant::Int(key),
                        },
                        span,
                    );
                    return self.adapt(result, expected, span);
                }
                let index = self.expression(index, Some(&key))?;
                self.emit(
                    result.id,
                    O::Index {
                        receiver: value.id,
                        key: index.id,
                    },
                    span,
                );
                result
        })
    }
    fn lower_if(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::If {
                condition,
                then,
                otherwise,
            } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
                self.push_scope();
                let condition = self.condition_expression(condition)?;
                let branch = self.jump(Some(condition.id), 0, span)?;
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
                self.emit(result.id, O::Copy(left.id), span);
                let end = self.jump(None, 0, span)?;
                self.patch(branch, self.program.body.len())?;
                self.pop_scope();
                let right = if let Some(otherwise) = otherwise {
                    self.expression(otherwise, Some(&result.ty))?
                } else {
                    self.constant(Constant::Unit, Ty::Unit, span)?
                };
                self.emit(result.id, O::Copy(right.id), span);
                self.patch(end, self.program.body.len())?;
                Value {
                    terminated: left.terminated && right.terminated,
                    ..result
                }
        })
    }
    fn lower_return(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Return(expression) = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
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
                self.emit(value.id, O::Return(value.id), span);
                value.terminated = true;
                value
        })
    }
    fn lower_propagate(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Propagate(expression) = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
                if !matches!(self.program.result, Ty::Option(_)) {
                    return Err(self.error(span, "可选传播需要可选返回类型"));
                }
                let value = self.expression(expression, None)?;
                let Ty::Option(inner) = &value.ty else {
                    return Err(self.error(span, "只能传播可选值"));
                };
                let some = self.slot(Ty::Bool, span)?;
                self.emit(some.id, O::IsSome(value.id), span);
                let absent = self.jump(Some(some.id), 0, span)?;
                let end = self.jump(None, 0, span)?;
                self.patch(absent, self.program.body.len())?;
                self.emit(value.id, O::Return(value.id), span);
                self.patch(end, self.program.body.len())?;
                // 收窄必须产生独立类型化值，不能只修改编译器临时视图而保留可选槽类型。
                let narrowed = self.slot((**inner).clone(), span)?;
                self.emit(narrowed.id, O::Copy(value.id), span);
                narrowed
        })
    }
    fn lower_is_type(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::IsType { value, name } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
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
                let result = self.slot(Ty::Bool, span)?;
                self.emit(
                    result.id,
                    O::IsType {
                        value: value.id,
                        name,
                    },
                    span,
                );
                result
        })
    }
    fn lower_is_some(&mut self, expression: &Expr, _expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::IsSome { value, binding } = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
                let value = self.expression(value, None)?;
                let Ty::Option(inner) = &value.ty else {
                    return Err(self.error(span, "is Some 需要可选值"));
                };
                if self.condition {
                    let bound = self.slot((**inner).clone(), span)?;
                    self.emit(bound.id, O::Copy(value.id), span);
                    self.declare(binding, &bound, false, span)?;
                }
                let result = self.slot(Ty::Bool, span)?;
                self.emit(result.id, O::IsSome(value.id), span);
                result
        })
    }
    fn lower_array(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Array(values) = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
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
                self.emit(
                    result.id,
                    O::Array(values.iter().map(|v| v.id).collect()),
                    span,
                );
                result
        })
    }
    fn lower_dictionary(&mut self, expression: &Expr, expected: Option<&Ty>) -> Result<Value> {
        let span = expression.span;
        let E::Dictionary(entries) = &expression.kind else { unreachable!("表达式派发已匹配") };
        Ok({
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
                    values.extend([key.id, value.id]);
                }
                let (key, value) = types.ok_or_else(|| self.error(span, "空字典需要明确类型"))?;
                if !matches!(key, Ty::Int | Ty::Bool | Ty::String | Ty::Enum(_)) {
                    return Err(self.error(span, "无效的字典 key 类型"));
                }
                let result = self.slot(Ty::Dict(Box::new(key), Box::new(value)), span)?;
                self.emit(result.id, O::Dictionary(values), span);
                result
        })
    }
}
