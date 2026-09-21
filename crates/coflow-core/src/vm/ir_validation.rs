//! 对解码后的 Contract IR 验证类型及结构，不通过重新解析源码建立信任。
use super::{
    bytecode::Constant,
    ir::{Function, Operation, ValueId},
};
use crate::schema::{CftSchema, CftValueType as Ty};
use std::collections::BTreeSet;

impl Function {
    pub(crate) fn validate_semantics(&self, schema: &CftSchema) -> Result<(), String> {
        self.validate_semantics_at(schema, 0)
    }

    fn validate_semantics_at(&self, schema: &CftSchema, depth: usize) -> Result<(), String> {
        if depth > 256 || self.body.len() > 1_000_000 || self.values.len() > 65_536 {
            return Err("Contract IR 结构超限".into());
        }
        if self.body.is_empty() || !self.values.starts_with(&self.parameters) {
            return Err("IR 参数布局或函数体无效".into());
        }
        let ty = |id: ValueId| {
            self.values
                .get(id.0 as usize)
                .ok_or_else(|| "IR 值编号越界".to_string())
        };
        let same = |actual: &Ty, expected: &Ty| -> Result<(), String> {
            if actual == expected {
                Ok(())
            } else {
                Err(format!("IR 类型不匹配：需要 {expected}，实际 {actual}"))
            }
        };
        let assign = |actual: &Ty, expected: &Ty| -> Result<(), String> {
            if schema.value_type_assignable(actual, expected) {
                Ok(())
            } else {
                Err(format!("IR 赋值类型不匹配：{actual} -> {expected}"))
            }
        };
        let field_type = |owner: &Ty, field: u16| -> Result<Ty, String> {
            let (name, record) = match owner {
                Ty::Object(name) => (name, false),
                Ty::RecordRef(name) => (name, true),
                _ => return Err("IR 字段来源不是对象".into()),
            };
            if record && field == 0 {
                return Ok(Ty::String);
            }
            schema
                .resolve_type(name)
                .and_then(|meta| meta.all_fields().nth(field as usize - usize::from(record)))
                .map(|field| field.runtime_value_type())
                .ok_or_else(|| "IR 字段越界".into())
        };
        // 编译器会保留提前返回之后的不可达出口；只对实际可达的 Return 校验结果。
        let reachable = self.reachable()?;
        for (pc, node) in self.body.iter().enumerate() {
            use Operation as O;
            if node.span.start > node.span.end {
                return Err("IR 源码映射无效".into());
            }
            let result = || ty(node.destination);
            match &node.operation {
                O::Build(operation) => {
                    use super::construction::BuildOp as B;
                    match operation {
                        B::Start { source } => {
                            match result()? {
                                Ty::Array(_) | Ty::Dict(..) => {}
                                Ty::Object(name)
                                    if schema.resolve_type(name).is_some_and(|meta| {
                                        meta.kind
                                            == coflow_language::cft::syntax::ast::TypeKind::Data
                                            && !meta.is_abstract
                                    }) => {}
                                _ => return Err("IR 构造目标必须是具体 data 或集合".into()),
                            }
                            if let Some(source) = source {
                                same(ty(*source)?, result()?)?;
                            }
                        }
                        B::DefaultField { owner, field } => {
                            same(result()?, &field_type(ty(*owner)?, field.0)?)?;
                            let Ty::Object(name) = ty(*owner)? else {
                                return Err("IR 默认字段需要对象".into());
                            };
                            let declaration = schema
                                .resolve_type(name)
                                .and_then(|meta| meta.all_fields().nth(field.0 as usize))
                                .ok_or("IR 默认字段不存在")?;
                            if declaration.default.is_none()
                                && !matches!(
                                    declaration.value_type,
                                    Ty::Option(_) | Ty::Array(_) | Ty::Dict(..)
                                )
                            {
                                return Err("IR 必填字段不能读取默认值".into());
                            }
                        }
                        B::Set {
                            builder,
                            key,
                            value,
                        } => {
                            match ty(*builder)? {
                                Ty::Array(inner) => {
                                    same(ty(*key)?, &Ty::Int)?;
                                    assign(ty(*value)?, inner)?;
                                }
                                Ty::Dict(k, v) => {
                                    assign(ty(*key)?, k)?;
                                    assign(ty(*value)?, v)?;
                                }
                                _ => return Err("IR 构造索引写入需要集合".into()),
                            }
                            same(result()?, &Ty::Unit)?;
                        }
                        B::Append { builder, value } => {
                            let Ty::Array(inner) = ty(*builder)? else {
                                return Err("IR append 需要数组构造能力".into());
                            };
                            assign(ty(*value)?, inner)?;
                            same(result()?, &Ty::Unit)?;
                        }
                        B::Remove { builder, key } => {
                            let key_type = match ty(*builder)? {
                                Ty::Array(_) => &Ty::Int,
                                Ty::Dict(key, _) => key.as_ref(),
                                _ => return Err("IR remove 需要集合构造能力".into()),
                            };
                            assign(ty(*key)?, key_type)?;
                            same(result()?, &Ty::Unit)?;
                        }
                        B::Freeze { builder } => same(result()?, ty(*builder)?)?,
                        B::Drop { builder } => {
                            ty(*builder)?;
                            same(result()?, &Ty::Unit)?;
                        }
                    }
                }
                O::Constant(constant) => match constant {
                    Constant::Unit => same(result()?, &Ty::Unit)?,
                    Constant::None if matches!(result()?, Ty::Option(_)) => {}
                    Constant::None => return Err("IR None 需要可选类型".into()),
                    Constant::Bool(_) => same(result()?, &Ty::Bool)?,
                    Constant::Int(_) => same(result()?, &Ty::Int)?,
                    Constant::Float(_) => same(result()?, &Ty::Float)?,
                    Constant::String(_) => same(result()?, &Ty::String)?,
                    Constant::Enum { name, .. } => {
                        let meta = schema.resolve_enum(name).ok_or("IR 枚举不存在")?;
                        same(result()?, &Ty::Enum(meta.name.clone()))?;
                    }
                },
                O::Owner => same(result()?, self.owner.as_ref().ok_or("IR 缺少 owner 类型")?)?,
                O::Capture(index) => same(
                    result()?,
                    self.captures.get(*index as usize).ok_or("IR 捕获越界")?,
                )?,
                O::Field { receiver, field } => {
                    same(result()?, &field_type(ty(*receiver)?, field.0)?)?
                }
                O::OwnerField(field) => same(
                    result()?,
                    &field_type(self.owner.as_ref().ok_or("IR 缺少 owner")?, field.0)?,
                )?,
                O::Copy(source) => {
                    // 类型收窄的 Copy 仍保持相同表示；运行期的可选/多态检查不可被删除。
                    let source = ty(*source)?;
                    if !schema.value_type_assignable(source, result()?)
                        && !schema.value_type_assignable(result()?, source)
                    {
                        return Err("IR Copy 类型不兼容".into());
                    }
                }
                O::Unary { operator, value } => {
                    let operand = ty(*value)?;
                    let valid = match operator {
                        0 => matches!(operand, Ty::Int | Ty::Float),
                        1 => *operand == Ty::Bool,
                        2 => {
                            *operand == Ty::Int
                                || matches!(operand, Ty::Enum(name) if schema.resolve_enum(name).is_some_and(|meta| meta.is_flag))
                        }
                        _ => false,
                    };
                    if !valid {
                        return Err("IR 一元运算类型无效".into());
                    }
                    same(result()?, operand)?;
                }
                O::Binary {
                    operator,
                    left,
                    right,
                } => {
                    let (left, right) = (ty(*left)?, ty(*right)?);
                    let equality = matches!(operator, 7 | 8);
                    let comparison = matches!(operator, 7..=12);
                    let valid = if equality {
                        matches!(left, Ty::Option(_)) == matches!(right, Ty::Option(_))
                            && (schema.value_type_assignable(left, right)
                                || schema.value_type_assignable(right, left))
                    } else if left != right {
                        false
                    } else {
                        match operator {
                            0 => matches!(left, Ty::Int | Ty::Float | Ty::String),
                            1 | 2 | 6 => matches!(left, Ty::Int | Ty::Float),
                            3 => *left == Ty::Float,
                            4 | 5 | 13 | 14 => *left == Ty::Int,
                            9..=12 => {
                                matches!(left, Ty::Int | Ty::Float | Ty::String | Ty::Enum(_))
                            }
                            15..=17 => {
                                *left == Ty::Int
                                    || matches!(left, Ty::Enum(name) if schema.resolve_enum(name).is_some_and(|meta| meta.is_flag))
                            }
                            _ => false,
                        }
                    };
                    if !valid {
                        return Err("IR 二元运算类型无效".into());
                    }
                    same(result()?, if comparison { &Ty::Bool } else { left })?;
                }
                O::JumpFalse { condition, .. } => same(ty(*condition)?, &Ty::Bool)?,
                O::Return(value) if reachable.contains(&pc) => assign(ty(*value)?, &self.result)?,
                O::Call { target, arguments } => {
                    let Ty::Function(parameters, expected) = ty(*target)? else {
                        return Err("IR 调用目标不是函数".into());
                    };
                    if parameters.len() != arguments.len() {
                        return Err("IR 调用参数数量不匹配".into());
                    }
                    for (parameter, argument) in parameters.iter().zip(arguments) {
                        assign(ty(*argument)?, &parameter.value_type)?;
                    }
                    same(result()?, expected)?;
                }
                O::Closure {
                    function,
                    captures,
                    owner,
                    template,
                } => {
                    if captures.len() != function.captures.len() {
                        return Err("IR 捕获布局不匹配".into());
                    }
                    for (capture, expected) in captures.iter().zip(&function.captures) {
                        same(ty(*capture)?, expected)?;
                    }
                    let actual_owner = owner.map(ty).transpose()?.or(self.owner.as_ref());
                    if actual_owner != function.owner.as_ref() {
                        return Err("IR 闭包 owner 布局不匹配".into());
                    }
                    function.validate_semantics_at(schema, depth + 1)?;
                    if *template {
                        same(result()?, &Ty::FString)?;
                        same(&function.result, &Ty::String)?;
                        if !function.parameters.is_empty() {
                            return Err("IR 模板不能接受参数".into());
                        }
                    } else {
                        let Ty::Function(parameters, expected) = result()? else {
                            return Err("IR 闭包结果不是函数".into());
                        };
                        if parameters
                            .iter()
                            .map(|p| &p.value_type)
                            .ne(function.parameters.iter())
                            || **expected != function.result
                        {
                            return Err("IR 闭包签名不一致".into());
                        }
                    }
                }
                O::Array(values) => {
                    let Ty::Array(inner) = result()? else {
                        return Err("IR 数组类型无效".into());
                    };
                    for value in values {
                        assign(ty(*value)?, inner)?;
                    }
                }
                O::Dictionary(values) => {
                    let Ty::Dict(key, value) = result()? else {
                        return Err("IR 字典类型无效".into());
                    };
                    if values.len() % 2 != 0 {
                        return Err("IR 字典键值数量不匹配".into());
                    }
                    for pair in values.chunks_exact(2) {
                        assign(ty(pair[0])?, key)?;
                        assign(ty(pair[1])?, value)?;
                    }
                }
                O::Concat(values) => {
                    same(result()?, &Ty::String)?;
                    for value in values {
                        same(ty(*value)?, &Ty::String)?;
                    }
                }
                O::AccumulateText { left, right } => {
                    same(result()?, &Ty::String)?;
                    same(ty(*left)?, &Ty::String)?;
                    same(ty(*right)?, &Ty::String)?;
                }
                O::Format(values) => {
                    same(result()?, &Ty::String)?;
                    for value in values {
                        if !matches!(
                            ty(*value)?,
                            Ty::String | Ty::Int | Ty::Float | Ty::Bool | Ty::Enum(_)
                        ) {
                            return Err("IR 插值类型无效".into());
                        }
                    }
                }
                O::ForPrep { limit, .. } | O::ForLoop { limit, .. } => {
                    same(result()?, &Ty::Int)?;
                    same(ty(*limit)?, &Ty::Int)?;
                }
                O::ConvertFloat(value) => {
                    let converted = match ty(*value)? {
                        Ty::Int | Ty::Float => Ty::Float,
                        Ty::Option(inner) if matches!(**inner, Ty::Int | Ty::Float) => {
                            Ty::Option(Box::new(Ty::Float))
                        }
                        _ => return Err("IR 数值提升类型无效".into()),
                    };
                    same(result()?, &converted)?;
                }
                O::IsSome(value) => {
                    if !matches!(ty(*value)?, Ty::Option(_)) {
                        return Err("IR 可选判断类型无效".into());
                    }
                    same(result()?, &Ty::Bool)?;
                }
                O::IsType { value, name } => {
                    let source = match ty(*value)? {
                        Ty::Option(inner) => inner.as_ref(),
                        value => value,
                    };
                    if !matches!(source, Ty::RecordRef(_) | Ty::Object(_))
                        || schema.resolve_type(name).is_none()
                    {
                        return Err("IR 类型判断目标无效".into());
                    }
                    same(result()?, &Ty::Bool)?;
                }
                O::ReadTemplate(value) => {
                    let expected = match ty(*value)? {
                        Ty::FString => Ty::String,
                        Ty::Option(inner) if **inner == Ty::FString => {
                            Ty::Option(Box::new(Ty::String))
                        }
                        _ => return Err("IR 模板读取类型无效".into()),
                    };
                    same(result()?, &expected)?;
                }
                O::Length(value) => {
                    if !matches!(ty(*value)?, Ty::Array(_) | Ty::Dict(..) | Ty::String) {
                        return Err("IR 长度操作类型无效".into());
                    }
                    same(result()?, &Ty::Int)?;
                }
                O::Index { receiver, key } => {
                    let (expected_key, value) = match ty(*receiver)? {
                        Ty::Array(inner) => (Ty::Int, inner.as_ref()),
                        Ty::Dict(key, value) => ((**key).clone(), value.as_ref()),
                        Ty::String => (Ty::Int, &Ty::String),
                        _ => return Err("IR 索引来源无效".into()),
                    };
                    same(ty(*key)?, &expected_key)?;
                    same(result()?, value)?;
                }
                O::IndexConstant { receiver, key } => {
                    if !matches!(key, Constant::Int(_)) {
                        return Err("IR 内联索引键无效".into());
                    }
                    let value = match ty(*receiver)? {
                        Ty::Array(inner) => inner.as_ref(),
                        Ty::Dict(key, value) if **key == Ty::Int => value.as_ref(),
                        Ty::String => &Ty::String,
                        _ => return Err("IR 内联索引来源无效".into()),
                    };
                    same(result()?, value)?;
                }
                O::ReserveObject { type_name } | O::InitializeObject { type_name, .. } => {
                    let meta = schema.resolve_type(type_name).ok_or("IR 对象类型不存在")?;
                    if meta.kind != coflow_language::cft::syntax::ast::TypeKind::Data
                        || meta.is_abstract
                    {
                        return Err("IR 只能构造具体 data 类型".into());
                    }
                    same(result()?, &Ty::Object(meta.name.clone()))?;
                    if let O::InitializeObject { fields, .. } = &node.operation {
                        let mut provided = BTreeSet::new();
                        for (name, value) in fields {
                            if !provided.insert(name.as_str()) {
                                return Err("IR 对象字段重复".into());
                            }
                            let field = meta.field(name).ok_or("IR 对象字段不存在")?;
                            assign(ty(*value)?, &field.value_type)?;
                        }
                        for field in meta.all_fields() {
                            if !provided.contains(field.name.as_str())
                                && field.default.is_none()
                                && !matches!(
                                    field.value_type,
                                    Ty::Option(_) | Ty::Array(_) | Ty::Dict(..)
                                )
                            {
                                return Err("IR 对象缺少必填字段".into());
                            }
                        }
                    }
                }
                O::IteratorValue { collection, index } => {
                    same(ty(*index)?, &Ty::Int)?;
                    let value = match ty(*collection)? {
                        Ty::Array(inner) | Ty::Dict(inner, _) => inner.as_ref(),
                        _ => return Err("IR 迭代来源无效".into()),
                    };
                    same(result()?, value)?;
                }
                O::IterNext {
                    collection,
                    counter,
                    key,
                    value,
                } => {
                    same(ty(*counter)?, &Ty::Int)?;
                    let (expected_key, expected_value) = match ty(*collection)? {
                        Ty::Array(inner) => (&Ty::Int, inner.as_ref()),
                        Ty::Dict(key, value) => (key.as_ref(), value.as_ref()),
                        _ => return Err("IR 双绑定迭代来源无效".into()),
                    };
                    same(ty(*key)?, expected_key)?;
                    same(ty(*value)?, expected_value)?;
                }
                O::Reference(name) => {
                    // 引用必须由声明解释，不能信任序列化值携带的结果类型。
                    if let Some(name) = name.strip_prefix("$const::") {
                        let constant = schema.resolve_const(name).ok_or("IR 常量引用不存在")?;
                        same(result()?, &constant.value_type)?;
                    } else if name == "$host::Coflow::Check::require" {
                        same(
                            result()?,
                            &Ty::Function(
                                vec![
                                    crate::schema::CftFunctionParameter::unnamed(Ty::Bool),
                                    crate::schema::CftFunctionParameter::unnamed(Ty::String),
                                ],
                                Box::new(Ty::Unit),
                            ),
                        )?;
                    } else {
                        let (name, _) = name.rsplit_once("::").ok_or("IR 记录引用无效")?;
                        let meta = schema.resolve_type(name).ok_or("IR 记录类型不存在")?;
                        if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                            return Err("IR data 不能作为记录引用".into());
                        }
                        same(result()?, &Ty::RecordRef(meta.name.clone()))?;
                    }
                }
                O::Builtin {
                    name,
                    receiver,
                    arguments,
                } => {
                    let receiver = ty(*receiver)?;
                    let (parameters, expected) = if let Some(name) = name.strip_prefix("$enum::") {
                        let meta = schema.resolve_enum(name).ok_or("IR enum 构造目标不存在")?;
                        same(receiver, &Ty::Int)?;
                        (Vec::new(), Ty::Enum(meta.name.clone()))
                    } else if let Some(name) = name.strip_prefix("$records::") {
                        let meta = schema.resolve_type(name).ok_or("IR records 目标不存在")?;
                        if meta.kind == coflow_language::cft::syntax::ast::TypeKind::Data {
                            return Err("IR records 不接受 data".into());
                        }
                        same(receiver, &Ty::Unit)?;
                        (
                            Vec::new(),
                            Ty::Array(Box::new(Ty::RecordRef(meta.name.clone()))),
                        )
                    } else {
                        let signature = super::compiler::builtin_signature(schema, receiver, name);
                        signature.ok_or("IR 内建操作与接收者类型不匹配")?
                    };
                    if parameters.len() != arguments.len() {
                        return Err("IR 内建参数数量不匹配".into());
                    }
                    for (argument, parameter) in arguments.iter().zip(&parameters) {
                        assign(ty(*argument)?, parameter)?;
                    }
                    same(result()?, &expected)?;
                }
                O::Jump(_) | O::Return(_) | O::Iteration => {}
            }
        }
        self.validate_flow(schema, &reachable)
    }

    /// 直接验证 IR 的定义/使用关系；契约读取不生成字节码或分配物理寄存器。
    fn validate_flow(&self, schema: &CftSchema, reachable: &BTreeSet<usize>) -> Result<(), String> {
        let super::ir_flow::Flow {
            reads,
            writes,
            successors,
            ..
        } = self.flow()?;
        self.validate_construction(&reads, &writes, &successors)?;
        self.validate_narrowing(schema, &reads, &writes, &successors)?;
        let mut live = vec![BTreeSet::<u32>::new(); self.body.len()];
        let mut steps = 0u64;
        loop {
            let mut changed = false;
            for &pc in reachable.iter().rev() {
                steps += 1;
                if steps > 10_000_000 {
                    return Err("IR 活跃分析工作量超限".into());
                }
                let mut incoming = BTreeSet::new();
                for successor in &successors[pc] {
                    incoming.extend(&live[*successor]);
                }
                for value in &writes[pc] {
                    incoming.remove(&value.0);
                }
                incoming.extend(reads[pc].iter().map(|value| value.0));
                if incoming != live[pc] {
                    live[pc] = incoming;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        if live[0]
            .iter()
            .any(|value| *value as usize >= self.parameters.len())
        {
            return Err("IR 存在未初始化值读取".into());
        }
        Ok(())
    }
}
