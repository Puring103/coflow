//! 类型映射、值转换与静态 codec 构造。
use super::names::{qualified, quoted};
use super::CsharpCodegenError;
use coflow_core::schema::{CftSchema, CftValueType};
pub(super) fn cs_type(ty: &CftValueType, root: &str) -> Result<String, CsharpCodegenError> {
    Ok(match ty {
        CftValueType::Int => "int".into(),
        CftValueType::Float => "float".into(),
        CftValueType::Bool => "bool".into(),
        CftValueType::String => "string".into(),
        CftValueType::FString => "CoflowTemplate".into(),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => qualified(root, n),
        CftValueType::Enum(n) => qualified(root, n),
        CftValueType::Array(t) => format!("CoflowArray<{}>", cs_type(t, root)?),
        CftValueType::Dict(k, v) => format!(
            "CoflowDictionary<{}, {}>",
            cs_type(k, root)?,
            cs_type(v, root)?
        ),
        CftValueType::Option(t) => format!("{}?", cs_type(t, root)?),
        CftValueType::Function(parameters, result) => {
            if parameters.len() > 8 {
                return Err(CsharpCodegenError::new(
                    "C# functions support at most 8 parameters",
                ));
            }
            let mut types = parameters
                .iter()
                .map(|parameter| cs_type(&parameter.value_type, root))
                .collect::<Result<Vec<_>, _>>()?;
            types.push(cs_type(result, root)?);
            format!("CoflowFunction<{}>", types.join(", "))
        }
        CftValueType::Unit => "Unit".into(),
    })
}
pub(super) fn pack(ty: &CftValueType, expression: &str) -> String {
    match ty {
        CftValueType::Enum(name) => format!(
            "Projection.EnumValue({}, unchecked((uint)({expression})))",
            quoted(name)
        ),
        CftValueType::Option(inner) if matches!(inner.as_ref(), CftValueType::Enum(_)) => format!(
            "{expression}.HasValue ? {} : Projection.From(null)",
            pack(inner, &format!("{expression}.Value"))
        ),
        _ => format!("Projection.From({expression})"),
    }
}
pub(super) fn codec(
    schema: &CftSchema,
    ty: &CftValueType,
    root: &str,
    depth: usize,
) -> Result<String, CsharpCodegenError> {
    let v = format!("v{depth}");
    let next = depth + 1;
    Ok(match ty {
        CftValueType::Int => "ValueCodecs.Int".into(),
        CftValueType::Float => "ValueCodecs.Float".into(),
        CftValueType::Bool => "ValueCodecs.Bool".into(),
        CftValueType::String => "ValueCodecs.String".into(),
        CftValueType::FString => "v => new CoflowTemplate(v)".into(),
        CftValueType::Enum(n) => format!("{v} => ({})ValueCodecs.Enum({v})", qualified(root, n)),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => match schema.resolve_type(n) {
            Some(ty) if ty.is_struct => format!("{v} => new {}({v})", qualified(root, n)),
            _ => format!("{v} => {v}.Resolve<{}>()", qualified(root, n)),
        },
        CftValueType::Array(t) => format!(
            "{v} => new {}({v}, {})",
            cs_type(ty, root)?,
            codec(schema, t, root, next)?
        ),
        CftValueType::Dict(k, t) => format!(
            "{v} => new {}({v}, {}, {})",
            cs_type(ty, root)?,
            codec(schema, k, root, next)?,
            codec(schema, t, root, next)?
        ),
        CftValueType::Option(t) => {
            let value_type = matches!(
                t.as_ref(),
                CftValueType::Int
                    | CftValueType::Float
                    | CftValueType::Bool
                    | CftValueType::Enum(_)
            ) || matches!(t.as_ref(), CftValueType::Object(name) if schema.resolve_type(name).is_some_and(|ty| ty.is_struct));
            format!(
                "{v} => ValueCodecs.{}({v}, {})",
                if value_type {
                    "OptionalValue"
                } else {
                    "OptionalReference"
                },
                codec(schema, t, root, next)?
            )
        }
        CftValueType::Function(parameters, result) => {
            let codecs = parameters
                .iter()
                .map(|parameter| invocation_codec(schema, &parameter.value_type, root, next))
                .chain(std::iter::once(invocation_codec(
                    schema, result, root, next,
                )))
                .collect::<Result<Vec<_>, _>>()?;
            format!(
                "{v} => new {}({v}, {})",
                cs_type(ty, root)?,
                codecs.join(", ")
            )
        }
        CftValueType::Unit => format!("{v} => default"),
    })
}

pub(super) fn decode(
    schema: &CftSchema,
    ty: &CftValueType,
    root: &str,
    value: &str,
) -> Result<String, CsharpCodegenError> {
    Ok(match ty {
        CftValueType::Int => format!("ValueCodecs.Int({value})"),
        CftValueType::Float => format!("ValueCodecs.Float({value})"),
        CftValueType::Bool => format!("ValueCodecs.Bool({value})"),
        CftValueType::String => format!("ValueCodecs.String({value})"),
        CftValueType::FString => format!("new CoflowTemplate({value})"),
        CftValueType::Enum(name) => {
            format!("({})ValueCodecs.Enum({value})", qualified(root, name))
        }
        CftValueType::Object(name) | CftValueType::RecordRef(name) => {
            match schema.resolve_type(name) {
                Some(ty) if ty.is_struct => format!("new {}({value})", qualified(root, name)),
                _ => format!("{value}.Resolve<{}>()", qualified(root, name)),
            }
        }
        CftValueType::Array(inner) => format!(
            "new {}({value}, {})",
            cs_type(ty, root)?,
            codec(schema, inner, root, 0)?
        ),
        CftValueType::Dict(key, item) => format!(
            "new {}({value}, {}, {})",
            cs_type(ty, root)?,
            codec(schema, key, root, 0)?,
            codec(schema, item, root, 0)?
        ),
        CftValueType::Option(inner) => {
            let value_type = matches!(
                inner.as_ref(),
                CftValueType::Int
                    | CftValueType::Float
                    | CftValueType::Bool
                    | CftValueType::Enum(_)
            ) || matches!(inner.as_ref(), CftValueType::Object(name) if schema.resolve_type(name).is_some_and(|ty| ty.is_struct));
            format!(
                "ValueCodecs.{}({value}, {})",
                if value_type {
                    "OptionalValue"
                } else {
                    "OptionalReference"
                },
                codec(schema, inner, root, 0)?
            )
        }
        CftValueType::Function(parameters, result) => {
            let codecs = parameters
                .iter()
                .map(|parameter| invocation_codec(schema, &parameter.value_type, root, 0))
                .chain(std::iter::once(invocation_codec(schema, result, root, 0)))
                .collect::<Result<Vec<_>, _>>()?;
            format!("new {}({value}, {})", cs_type(ty, root)?, codecs.join(", "))
        }
        CftValueType::Unit => "default".into(),
    })
}
pub(super) fn invocation_codec(
    schema: &CftSchema,
    ty: &CftValueType,
    root: &str,
    depth: usize,
) -> Result<String, CsharpCodegenError> {
    let v = format!("v{depth}");
    let next = depth + 1;
    Ok(match ty {
        CftValueType::Unit => "ValueCodecs.UnitInvocation".into(),
        CftValueType::Int => "ValueCodecs.IntInvocation".into(),
        CftValueType::Float => "ValueCodecs.FloatInvocation".into(),
        CftValueType::Bool => "ValueCodecs.BoolInvocation".into(),
        CftValueType::String => "ValueCodecs.StringInvocation".into(),
        CftValueType::FString => "ValueCodecs.TemplateInvocation".into(),
        CftValueType::Enum(name) => format!(
            "ValueCodecs.EnumInvocation<{}>({}, {v} => ({}){v}, {v} => (uint){v})",
            qualified(root, name),
            quoted(name),
            qualified(root, name)
        ),
        CftValueType::Object(_)
        | CftValueType::RecordRef(_)
        | CftValueType::Array(_)
        | CftValueType::Dict(_, _)
        | CftValueType::Function(..) => {
            format!(
                "ValueCodecs.CoflowInvocation<{}>({})",
                cs_type(ty, root)?,
                codec(schema, ty, root, next)?
            )
        }
        CftValueType::Option(inner) => {
            let value_type = matches!(
                inner.as_ref(),
                CftValueType::Int
                    | CftValueType::Float
                    | CftValueType::Bool
                    | CftValueType::Enum(_)
            ) || matches!(inner.as_ref(), CftValueType::Object(name) if schema.resolve_type(name).is_some_and(|ty| ty.is_struct));
            format!(
                "ValueCodecs.{}({})",
                if value_type {
                    "OptionalValueInvocation"
                } else {
                    "OptionalReferenceInvocation"
                },
                invocation_codec(schema, inner, root, next)?
            )
        }
    })
}

pub(super) fn append_materialization(
    assignments: &mut Vec<String>,
    decoders: &mut Vec<String>,
    member_name: &str,
    field_name: &str,
    property_type: &str,
    reader: &str,
    value: &str,
    projection: &str,
) {
    // 简单字段直接物化；复杂嵌套保留具名解码器，避免初始化代码被泛型表达式淹没。
    if value.len() <= 120 {
        assignments.push(format!(
            "        {member_name} = {};\n",
            value.replace("__VALUE__", projection)
        ));
    } else {
        let decoder = format!("__Decode_{}", field_name.trim_start_matches('@'));
        assignments.push(format!(
            "        {member_name} = {decoder}({projection});\n"
        ));
        decoders.push(format!(
            "    private static readonly Func<Projection, {property_type}> {decoder} =\n        {reader};\n"
        ));
    }
}
