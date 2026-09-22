//! 闭包捕获、绑定与模板编译。
use super::*;
impl Compiler<'_> {
    pub(super) fn capture_environment(
        &self,
        span: Span,
    ) -> Result<(BTreeMap<String, Local>, BTreeMap<IrValueId, String>)> {
        let mut environment = BTreeMap::new();
        for scope in &self.scopes {
            environment.extend(scope.clone());
        }
        let mut inherited = BTreeMap::new();
        for (name, local) in &self.outer {
            if environment.contains_key(name) {
                continue;
            }
            let id = u32::try_from(self.program.values.len() + inherited.len())
                .map(IrValueId)
                .map_err(|_| self.error(span, "捕获候选超限"))?;
            environment.insert(
                name.clone(),
                Local {
                    id,
                    ty: local.ty.clone(),
                    mutable: false,
                    builder: local.builder.clone(),
                },
            );
            inherited.insert(id, name.clone());
        }
        Ok((environment, inherited))
    }
    pub(super) fn resolve_capture_sources(
        &mut self,
        sources: Vec<IrValueId>,
        inherited: &BTreeMap<IrValueId, String>,
        span: Span,
    ) -> Result<Vec<IrValueId>> {
        // 先编译子程序，再仅为实际使用的祖先变量在本层建立转发捕获。
        sources
            .into_iter()
            .map(|id| {
                if let Some(name) = inherited.get(&id) {
                    self.local(name, span)?
                        .map(|value| value.id)
                        .ok_or_else(|| self.error(span, "捕获来源丢失"))
                } else {
                    Ok(id)
                }
            })
            .collect()
    }
    pub(super) fn closure(&mut self, function: &Function, template: bool, span: Span) -> Result<Value> {
        let mut context = self.context.clone();
        context.check = false;
        if let Some((_, ty)) = &self.literal_owner {
            context.owner = Some(ty.clone());
        }
        let mut child = Compiler::new(
            self.schema,
            &format!("{}::<closure>", self.program.name),
            self.program.source.clone(),
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
        self.emit(
            result.id,
            O::Closure {
                function: Box::new(program),
                captures,
                owner: self.literal_owner.as_ref().map(|(id, _)| *id),
                template,
            },
            span,
        );
        Ok(result)
    }
    pub(super) fn template(&mut self, parts: &[TemplatePart], span: Span) -> Result<Value> {
        let mut context = self.context.clone();
        context.check = false;
        if let Some((_, ty)) = &self.literal_owner {
            context.owner = Some(ty.clone());
        }
        let mut child = Compiler::new(
            self.schema,
            &format!("{}::<template>", self.program.name),
            self.program.source.clone(),
            context,
        );
        child.program.result = Ty::String;
        let (environment, inherited) = self.capture_environment(span)?;
        child.outer = environment;
        let mut value_ids = Vec::new();
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
            value_ids.push(value.id);
        }
        let result = child.slot(Ty::String, span)?;
        child.emit(result.id, O::Format(value_ids), span);
        child.emit(result.id, O::Return(result.id), span);
        let result = self.slot(Ty::FString, span)?;
        let captures = self.resolve_capture_sources(child.capture_sources, &inherited, span)?;
        self.emit(
            result.id,
            O::Closure {
                function: Box::new(child.program),
                captures,
                owner: self.literal_owner.as_ref().map(|(id, _)| *id),
                template: true,
            },
            span,
        );
        Ok(result)
    }
}
