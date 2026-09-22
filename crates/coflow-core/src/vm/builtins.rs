//! 前端、IR 验证和执行适配共享的内建类型契约。
use crate::schema::{CftSchema, CftValueType as Ty};
pub(crate) fn builtin_signature(schema: &CftSchema, ty: &Ty, name: &str) -> Option<(Vec<Ty>, Ty)> {
    // 前端与反序列化验证共用签名规则，维度模板必须保持相同的普通读取类型。
    if let Ty::RecordRef(type_name) = ty {
        if let Some((_, field)) =
            crate::schema::dimensions::dimension_field_for_marker(schema, type_name)
        {
            let read = ordinary_type(&field.value_type);
            match name {
                "for" => return Some((vec![Ty::String], read)),
                "default" => return Some((vec![], read)),
                "variants" => {
                    return Some((vec![], Ty::Dict(Box::new(Ty::String), Box::new(read))))
                }
                _ => {}
            }
        }
    }
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

pub(crate) fn ordinary_type(ty: &Ty) -> Ty {
    match ty {
        Ty::FString => Ty::String,
        Ty::Option(inner) if **inner == Ty::FString => Ty::Option(Box::new(Ty::String)),
        _ => ty.clone(),
    }
}
