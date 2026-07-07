use super::builtins::Builtin;
use coflow_cft::{CftSchemaCheckExpr, CftSchemaCheckExprKind};

pub(super) enum CallTarget {
    EnumConstructor,
    Builtin(Builtin),
}

pub(super) struct CallSignature {
    pub(super) target: CallTarget,
}

impl CallSignature {
    pub(super) fn resolve_function(
        name: &str,
        arg_count: usize,
        is_enum_name: bool,
    ) -> Result<Self, CallSignatureError> {
        if is_enum_name {
            if arg_count == 1 {
                return Ok(Self {
                    target: CallTarget::EnumConstructor,
                });
            }
            return Err(CallSignatureError::Arity {
                message: "枚举构造函数需要 1 个参数".to_string(),
            });
        }

        let Some(builtin) = Builtin::by_name(name) else {
            return Err(CallSignatureError::UnknownFunction {
                name: name.to_string(),
            });
        };
        require_arity(builtin, arg_count, builtin.arity())?;
        Ok(Self {
            target: CallTarget::Builtin(builtin),
        })
    }

    pub(super) fn resolve_method(name: &str, arg_count: usize) -> Result<Self, CallSignatureError> {
        let Some(builtin) = Builtin::by_name(name) else {
            return Err(CallSignatureError::UnknownFunction {
                name: name.to_string(),
            });
        };
        let expected_args = builtin.arity().saturating_sub(1);
        require_arity(builtin, arg_count, expected_args)?;
        Ok(Self {
            target: CallTarget::Builtin(builtin),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CallSignatureError {
    UnknownFunction { name: String },
    Arity { message: String },
}

pub(super) fn matches_pattern_arg(arg: &CftSchemaCheckExpr) -> Result<&str, CallSignatureError> {
    let CftSchemaCheckExprKind::String(pattern) = &arg.kind else {
        return Err(CallSignatureError::Arity {
            message: "matches 的 pattern 必须是字符串字面量".to_string(),
        });
    };
    Ok(pattern)
}

fn require_arity(
    builtin: Builtin,
    actual: usize,
    expected: usize,
) -> Result<(), CallSignatureError> {
    if actual == expected {
        return Ok(());
    }
    Err(CallSignatureError::Arity {
        message: format!("{} 需要 {} 个参数", builtin.name(), expected),
    })
}
