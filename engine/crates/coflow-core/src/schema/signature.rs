use super::{CftFunctionParameter, CftSchema, CftValueType};
use coflow_language::cft::syntax::ast::{TypeKind, TypeRef, TypeRefKind};

impl CftSchema {
    /// 值传递按声明类型检查；集合保持不变性，记录及 data 支持继承赋值。
    pub fn value_type_assignable(&self, actual: &CftValueType, expected: &CftValueType) -> bool {
        if actual == expected {
            return true;
        }
        match (actual, expected) {
            (CftValueType::Int, CftValueType::Float) => true,
            (CftValueType::Object(actual), CftValueType::Object(expected))
            | (CftValueType::RecordRef(actual), CftValueType::RecordRef(expected)) => {
                self.is_assignable(actual, expected)
            }
            (CftValueType::Option(actual), CftValueType::Option(expected)) => {
                self.value_type_assignable(actual, expected)
            }
            (actual, CftValueType::Option(expected)) => {
                self.value_type_assignable(actual, expected)
            }
            _ => false,
        }
    }
    /// CFD 使用契约检查显式函数签名，不启动 CFT 声明编译器。
    pub fn resolve_type_ref(&self, ty: &TypeRef) -> Result<CftValueType, String> {
        Ok(match &ty.kind {
            TypeRefKind::Int => CftValueType::Int,
            TypeRefKind::Float => CftValueType::Float,
            TypeRefKind::Bool => CftValueType::Bool,
            TypeRefKind::String => CftValueType::String,
            TypeRefKind::FString => CftValueType::FString,
            TypeRefKind::Unit => CftValueType::Unit,
            TypeRefKind::Named(name) => {
                if let Some(ty) = self.aliases.get(name) {
                    ty.clone()
                } else if let Some(ty) = self.resolve_type(name) {
                    if ty.kind == TypeKind::Data {
                        CftValueType::Object(ty.name.clone())
                    } else {
                        CftValueType::RecordRef(ty.name.clone())
                    }
                } else if let Some(en) = self.resolve_enum(name) {
                    CftValueType::Enum(en.name.clone())
                } else {
                    return Err(format!("unknown type {name}"));
                }
            }
            TypeRefKind::Array(t) => CftValueType::Array(Box::new(self.resolve_type_ref(t)?)),
            TypeRefKind::Option(t) => {
                let inner = self.resolve_type_ref(t)?;
                if matches!(inner, CftValueType::Option(_)) {
                    return Err("optional types cannot be nested".into());
                }
                CftValueType::Option(Box::new(inner))
            }
            TypeRefKind::Dict(k, v) => {
                let key = self.resolve_type_ref(k)?;
                if !matches!(
                    key,
                    CftValueType::Int
                        | CftValueType::Bool
                        | CftValueType::String
                        | CftValueType::Enum(_)
                ) {
                    return Err("dictionary keys must be int, bool, string, or enum".into());
                }
                CftValueType::Dict(Box::new(key), Box::new(self.resolve_type_ref(v)?))
            }
            TypeRefKind::Function(args, result) => CftValueType::Function(
                args.iter()
                    .map(|arg| {
                        Ok(CftFunctionParameter {
                            name: arg.name.as_ref().map(|name| name.name.clone()),
                            value_type: self.resolve_type_ref(&arg.value_type)?,
                        })
                    })
                    .collect::<Result<_, String>>()?,
                Box::new(self.resolve_type_ref(result)?),
            ),
        })
    }
}
