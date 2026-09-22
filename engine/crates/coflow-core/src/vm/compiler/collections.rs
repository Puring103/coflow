//! 高阶集合的类型检查与循环降低。
use super::*;
impl Compiler<'_> {
    pub(super) fn higher_builtin(
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
        // 普通高阶方法只编译一次回调。fold 先取初值的期望类型，再严格按源码顺序编译参数。
        // 匿名函数的声明已经给出完整签名，无需编译函数体来推断初值类型。
        let mut compiled_callback = None;
        let signature = if name == "fold" {
            if let E::Function(function) = &arguments[1].kind {
                Ty::Function(function.parameters.iter().map(|(_, ty)| {
                    self.resolve_type(ty).map(CftFunctionParameter::unnamed)
                }).collect::<Result<Vec<_>>>()?, Box::new(self.resolve_type(&function.result)?))
            } else {
                // 任意回调表达式需要独立的类型探测，不能提前执行或污染初值的流类型状态。
                self.infer_expression_type(&arguments[1])?
            }
        } else {
            let callback = self.expression(&arguments[0], None)?;
            let signature = callback.ty.clone();
            compiled_callback = Some(callback);
            signature
        };
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
        let callback = match compiled_callback {
            Some(callback) => callback,
            None => self.expression(&arguments[1], None)?,
        };
        let result_type = match name {
            "map" => Ty::Array(callback_result.clone()),
            "filter" => receiver.ty.clone(),
            "fold" => (*callback_result).clone(),
            _ => Ty::Bool,
        };
        let result = if let Some(initial) = initial {
            let result = self.slot(result_type, span)?;
            self.emit(result.id, O::Copy(initial.id), span);
            result
        } else if matches!(name, "any" | "all") {
            self.constant(Constant::Bool(name == "all"), Ty::Bool, span)?
        } else {
            let result = self.slot(result_type, span)?;
            self.emit(
                result.id,
                O::Build(crate::vm::construction::BuildOp::Start { source: None }),
                span,
            );
            result
        };
        let index = self.constant(Constant::Int(0), Ty::Int, span)?;
        let length = self.slot(Ty::Int, span)?;
        self.emit(length.id, O::Length(receiver.id), span);
        let start = self.program.body.len();
        let condition = self.binary_values("<", index.clone(), length, span)?;
        let exit = self.jump(Some(condition.id), 0, span)?;
        self.emit(IrValueId(0), O::Iteration, span);
        let stored = self.slot(stored_type, span)?;
        // 键与值一次读出，减少高阶方法每元素的存储访问；
        // IterNext 必须先于 read 的转换指令发出，保证运行时先写入再读取。
        let key = if let Some(key_type) = key_type {
            let key = self.slot(key_type, span)?;
            self.emit(
                IrValueId(0),
                O::IterNext {
                    collection: receiver.id,
                    counter: index.id,
                    key: key.id,
                    value: stored.id,
                },
                span,
            );
            Some(key)
        } else {
            self.emit(
                stored.id,
                O::IteratorValue {
                    collection: receiver.id,
                    index: index.id,
                },
                span,
            );
            None
        };
        let read = self.adapt(stored.clone(), Some(&read_type), span)?;
        let mut args = Vec::new();
        if name == "fold" {
            args.push(result.id);
        }
        if let Some(key) = &key {
            args.push(key.id);
        }
        args.push(read.id);
        let returned = self.slot((*callback_result).clone(), span)?;
        self.emit(
            returned.id,
            O::Call {
                target: callback.id,
                arguments: args,
            },
            span,
        );
        let mut short_circuit = None;
        let mut skip = None;
        match name {
            "fold" => {
                self.emit(result.id, O::Copy(returned.id), span);
            }
            "any" | "all" => {
                self.emit(result.id, O::Copy(returned.id), span);
                if name == "all" {
                    short_circuit = Some(self.jump(Some(returned.id), 0, span)?);
                } else {
                    let inverted = self.slot(Ty::Bool, span)?;
                    self.emit(
                        inverted.id,
                        O::Unary {
                            operator: 1,
                            value: returned.id,
                        },
                        span,
                    );
                    short_circuit = Some(self.jump(Some(inverted.id), 0, span)?);
                }
            }
            _ => {
                if name == "filter" {
                    skip = Some(self.jump(Some(returned.id), 0, span)?);
                }
                let item = if name == "map" {
                    returned.id
                } else {
                    stored.id
                };
                let unit = self.slot(Ty::Unit, span)?;
                let operation = if matches!(result.ty, Ty::Dict(..)) {
                    crate::vm::construction::BuildOp::Set {
                        builder: result.id,
                        key: key.as_ref().map_or(index.id, |key| key.id),
                        value: item,
                    }
                } else {
                    crate::vm::construction::BuildOp::Append { builder: result.id, value: item }
                };
                self.emit(unit.id, O::Build(operation), span);
            }
        }
        if let Some(skip) = skip {
            self.patch(skip, self.program.body.len())?;
        }
        let one = self.constant(Constant::Int(1), Ty::Int, span)?;
        let next = self.binary_values("+", index.clone(), one, span)?;
        self.emit(index.id, O::Copy(next.id), span);
        self.jump(None, start, span)?;
        let end = self.program.body.len();
        self.patch(exit, end)?;
        if let Some(jump) = short_circuit {
            self.patch(jump, end)?;
        }
        // 高阶集合与显式 builder 共用独占构造能力，循环结束后一次冻结。
        if matches!(name, "map" | "filter") {
            let frozen = self.slot(result.ty.clone(), span)?;
            self.emit(frozen.id, O::Build(crate::vm::construction::BuildOp::Freeze { builder: result.id }), span);
            return Ok(frozen);
        }
        Ok(result)
    }
}
