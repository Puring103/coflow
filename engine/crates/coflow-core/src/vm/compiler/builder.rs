use super::*;
use crate::vm::construction::BuildOp as B;
use coflow_language::function::BuildSource;

impl Compiler<'_> {
    pub(super) fn drop_builders(&mut self, depth: usize, span: Span) -> Result<()> {
        let builders = self.scopes[depth..]
            .iter()
            .rev()
            .flat_map(|scope| scope.values())
            .filter(|local| local.builder.is_some())
            .map(|local| local.id)
            .collect::<Vec<_>>();
        for builder in builders {
            let unit = self.slot(Ty::Unit, span)?;
            self.emit(unit.id, O::Build(B::Drop { builder }), span);
        }
        Ok(())
    }
    pub(super) fn builder_binding(&self, expression: &Expr) -> Option<Local> {
        let E::Name(name) = &expression.kind else {
            return None;
        };
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .filter(|local| local.builder.is_some())
            .cloned()
    }

    pub(super) fn build_expression(
        &mut self,
        source: &BuildSource,
        name: &str,
        body: &Block,
        span: Span,
    ) -> Result<Value> {
        let (ty, source) = match source {
            BuildSource::Type(ty) => (self.resolve_type(ty)?, None),
            BuildSource::Value(expression) => {
                let value = self.expression(expression, None)?;
                (value.ty, Some(value.id))
            }
        };
        let metadata = match &ty {
            Ty::Object(name) => {
                let meta = self
                    .schema
                    .resolve_type(name)
                    .ok_or_else(|| self.error(span, "未知构造类型"))?;
                if meta.kind != coflow_language::cft::syntax::ast::TypeKind::Data
                    || meta.is_abstract
                {
                    return Err(self.error(span, "只能构造具体 data 类型"));
                }
                Some(meta.clone())
            }
            Ty::Array(_) | Ty::Dict(..) => None,
            _ => return Err(self.error(span, "build 只接受 data、数组或字典")),
        };
        let capability = self.slot(ty.clone(), span)?;
        self.emit(capability.id, O::Build(B::Start { source }), span);
        let mut fields = BTreeMap::new();
        if let Some(meta) = metadata {
            for (index, field) in meta.all_fields().enumerate() {
                let index = u16::try_from(index).map_err(|_| self.error(span, "构造字段数量超限"))?;
                let value = self.slot(field.value_type.clone(), span)?;
                if let Some(source) = source {
                    self.emit(value.id, O::Field { receiver: source, field: index.into() }, span);
                } else if field.default.is_some() || matches!(field.value_type, Ty::Option(_) | Ty::Array(_) | Ty::Dict(..)) {
                    self.emit(value.id, O::Build(B::DefaultField { owner: capability.id, field: index.into() }), span);
                }
                // 必填字段保留未定义虚拟值，CFG 检查会拒绝任意路径上的提前读取或缺失初始化。
                fields.insert(field.name.to_string(), value);
            }
        }
        self.push_scope();
        self.scopes.last_mut().unwrap().insert(name.into(), Local { id: capability.id, ty: ty.clone(), mutable: false, builder: Some(fields.clone()) });
        let body_result = self.block(body, Some(&Ty::Unit), false)?;
        self.pop_scope();
        if let Ty::Object(type_name) = &ty {
            self.emit(capability.id, O::InitializeObject {
                type_name: type_name.to_string(), fields: fields.into_iter().map(|(name, value)| (name, value.id)).collect(),
            }, span);
        }
        let mut result = self.slot(ty, span)?;
        self.emit(result.id, O::Build(B::Freeze { builder: capability.id }), span);
        result.terminated = body_result.terminated;
        Ok(result)
    }

    pub(super) fn builder_set(&mut self, target: &Expr, expression: &Expr, span: Span) -> Result<()> {
        match &target.kind {
            E::Field { value, name } => {
                let builder = self.builder_binding(value).ok_or_else(|| self.error(span, "只有直接构造绑定允许字段写入"))?;
                let field = builder.builder.as_ref().and_then(|fields| fields.get(name)).ok_or_else(|| self.error(span, "未知构造字段"))?.clone();
                // 字段 RHS 内新建的函数/模板字面量统一绑定候选对象；已有值只被读取，身份不变。
                let previous = self.literal_owner.clone();
                self.literal_owner = Some((builder.id, builder.ty.clone()));
                let value = self.expression(expression, Some(&field.ty))?;
                self.literal_owner = previous;
                self.emit(field.id, O::Copy(value.id), span);
            }
            E::Index { value, index } => {
                let builder = self.builder_binding(value).ok_or_else(|| self.error(span, "只有直接构造绑定允许索引写入"))?;
                let (key_ty, value_ty) = match &builder.ty {
                    Ty::Array(inner) => (Ty::Int, (**inner).clone()),
                    Ty::Dict(key, value) => ((**key).clone(), (**value).clone()),
                    _ => return Err(self.error(span, "构造索引写入需要集合")),
                };
                let key = self.expression(index, Some(&key_ty))?;
                let value = self.expression(expression, Some(&value_ty))?;
                let unit = self.slot(Ty::Unit, span)?;
                self.emit(
                    unit.id,
                    O::Build(B::Set {
                        builder: builder.id,
                        key: key.id,
                        value: value.id,
                    }),
                    span,
                );
            }
            _ => return Err(self.error(span, "构造赋值需要字段或索引")),
        }
        Ok(())
    }

    pub(super) fn builder_call(
        &mut self,
        builder: Local,
        name: &str,
        arguments: &[Expr],
        span: Span,
    ) -> Result<Value> {
        if name == "len"
            && arguments.is_empty()
            && matches!(builder.ty, Ty::Array(_) | Ty::Dict(..))
        {
            let result = self.slot(Ty::Int, span)?;
            self.emit(result.id, O::Length(builder.id), span);
            return Ok(result);
        }
        let [argument] = arguments else {
            return Err(self.error(span, "构造操作需要一个参数"));
        };
        let expected = match (&builder.ty, name) {
            (Ty::Array(inner), "append") => (**inner).clone(),
            (Ty::Array(_), "remove") => Ty::Int,
            (Ty::Dict(key, _), "remove") => (**key).clone(),
            _ => return Err(self.error(span, "构造能力只提供 append/remove/len 操作")),
        };
        let argument = self.expression(argument, Some(&expected))?;
        let unit = self.slot(Ty::Unit, span)?;
        let operation = if name == "append" {
            B::Append {
                builder: builder.id,
                value: argument.id,
            }
        } else {
            B::Remove {
                builder: builder.id,
                key: argument.id,
            }
        };
        self.emit(unit.id, O::Build(operation), span);
        Ok(unit)
    }
}
