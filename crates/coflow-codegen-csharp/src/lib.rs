//! 生成 Unity/AOT 可用的只读数据投影、静态 codec 与执行入口。
use coflow_codegen::{
    CodeArtifactFile, CodeArtifactSet, CodeGenerator, CodegenDescriptor, CodegenError, CodegenInput,
};
use coflow_core::schema::{CftSchema, CftValueType};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpIdAsEnumVariant {
    pub name: String,
    pub value: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenError(String);
impl CsharpCodegenError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CsharpCodegenError {}

pub const CSHARP_CFD_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "csharp",
    language: "csharp",
    file_extensions: &["cs"],
    runtime_package: "Coflow.Runtime",
    runtime_version: "0.11.0",
    needs_model: true,
};
#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpCfdCodeGenerator;
impl CodeGenerator for CsharpCfdCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CSHARP_CFD_CODEGEN_DESCRIPTOR
    }
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError> {
        let options = input
            .target
            .options
            .as_object()
            .ok_or_else(|| CodegenError::Message("C# options must be an object".into()))?;
        for (key, value) in options {
            if key != "namespace" || !value.is_string() {
                return Err(CodegenError::Message(format!(
                    "unsupported C# option `{key}`; namespace must be a string"
                )));
            }
        }
        let namespace = input
            .target
            .options
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("Coflow.Generated");
        let mut variants = BTreeMap::new();
        for (en, values) in input.id_as_enum_values {
            variants.insert(
                en.clone(),
                values
                    .iter()
                    .map(|(name, value)| CsharpIdAsEnumVariant {
                        name: name.clone(),
                        value: *value,
                    })
                    .collect(),
            );
        }
        let (files, contract) = generate(input.schema, &variants, namespace)
            .map_err(|e| CodegenError::Message(e.to_string()))?;
        let mut artifacts = files
            .into_iter()
            .map(|file| CodeArtifactFile::text(file.relative_path, file.contents))
            .collect::<Vec<_>>();
        artifacts.push(CodeArtifactFile::binary("coflow.contract", contract));
        CodeArtifactSet::new(artifacts)
    }
}
pub fn generate_csharp(schema: &CftSchema) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate(schema, &BTreeMap::new(), "Coflow.Generated").map(|(files, _)| files)
}

fn name(source: &str) -> String {
    source
        .split("::")
        .map(|part| format!("@{part}"))
        .collect::<Vec<_>>()
        .join(".")
}

fn qualified(root: &str, source: &str) -> String {
    format!("global::{}.{}", root, name(source))
}
fn namespace(root: &str, source: &str) -> String {
    source.rsplit_once("::").map_or_else(
        || root.to_string(),
        |(ns, _)| format!("{root}.{}", name(ns)),
    )
}
fn short(source: &str) -> &str {
    source.rsplit("::").next().unwrap_or(source)
}
fn quoted(source: &str) -> String {
    format!(
        "\"{}\"",
        source
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}
fn cs_type(ty: &CftValueType, root: &str) -> Result<String, CsharpCodegenError> {
    Ok(match ty {
        CftValueType::Int => "int".into(),
        CftValueType::Float => "float".into(),
        CftValueType::Bool => "bool".into(),
        CftValueType::String => "string".into(),
        CftValueType::FString => "RuntimeTemplate".into(),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => qualified(root, n),
        CftValueType::Enum(n) => qualified(root, n),
        CftValueType::Array(t) => format!("RuntimeArray<{}>", cs_type(t, root)?),
        CftValueType::Dict(k, v) => format!(
            "RuntimeDictionary<{},{}>",
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
            format!("RuntimeFunction<{}>", types.join(","))
        }
        CftValueType::Unit => "Unit".into(),
    })
}
fn pack(ty: &CftValueType, expression: &str) -> String {
    match ty {
        CftValueType::Enum(name) => format!("Projection.EnumValue({}, unchecked((uint)({expression})))", quoted(name)),
        CftValueType::Option(inner) if matches!(inner.as_ref(), CftValueType::Enum(_)) => format!("{expression}.HasValue ? {} : Projection.From(null)", pack(inner, &format!("{expression}.Value"))),
        _ => format!("Projection.From({expression})"),
    }
}
fn codec(
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
        CftValueType::FString => "v => new RuntimeTemplate(v)".into(),
        CftValueType::Enum(n) => format!("{v} => ({})ValueCodecs.Enum({v})", qualified(root, n)),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => {
            format!("{}.Wrap", qualified(root, n))
        }
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
fn invocation_codec(
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
                "ValueCodecs.RuntimeInvocation<{}>({})",
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
fn generate(
    schema: &CftSchema,
    ids: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    root: &str,
) -> Result<(Vec<GeneratedFile>, Vec<u8>), CsharpCodegenError> {
    if root.is_empty() || !root.split('.').all(coflow_language::lexical::is_identifier) {
        return Err(CsharpCodegenError::new("invalid C# namespace"));
    }
    let escaped_root = root
        .split('.')
        .map(|part| format!("@{part}"))
        .collect::<Vec<_>>()
        .join(".");
    let root = escaped_root.as_str();
    // C# 的类型和命名空间共享声明空间，元数据、enum 和 Host 接口也参与冲突检查。
    let mut symbols = BTreeSet::from(["Generated".to_string(), "GeneratedHostBindings".to_string()]);
    let mut names = schema.all_types().map(|ty| ty.name.to_string())
        .chain(schema.all_enums().map(|ty| ty.name.to_string())).collect::<Vec<_>>();
    names.extend(schema.all_types().filter(|ty| ty.is_host).map(|ty| {
        let interface = format!("I{}", short(&ty.name));
        ty.name.rsplit_once("::").map_or_else(|| interface.clone(), |(ns, _)| format!("{ns}::{interface}"))
    }));
    for name in &names {
        if !symbols.insert(name.clone()) { return Err(CsharpCodegenError::new(format!("{name} conflicts with a generated C# type"))); }
    }
    for name in &names {
        let mut parent = name.as_str();
        while let Some((namespace, _)) = parent.rsplit_once("::") {
            if symbols.contains(namespace) { return Err(CsharpCodegenError::new(format!("{namespace} is both a C# type and namespace"))); }
            parent = namespace;
        }
    }
    for ty in schema.all_types() {
        if ["Generated", "GeneratedHostBindings"].contains(&ty.name.as_str()) {
            return Err(CsharpCodegenError::new(
                "type conflicts with generated contract metadata",
            ));
        }
        let mut reserved: BTreeSet<String> = [
            "Value",
            "Read",
            "ActualType",
            "Dispose",
            "Projection",
            "__CoflowCodecs",
            "ValueEquals",
            "Wrap",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        reserved.insert(short(&ty.name).to_string());
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data {
            reserved.insert("Id".into());
        }
        for field in ty
            .all_fields()
            .filter(|field| matches!(field.value_type, CftValueType::FString))
        {
            reserved.insert(format!("Render{}", field.name));
        }
        for field in ty.all_fields().filter(|field| matches!(field.value_type, CftValueType::Function(..)) && field.dimension.is_none()) {
            reserved.insert(format!("{}Function", field.name));
        }
        for field in ty.all_fields() {
            if reserved.contains(field.name.as_str()) {
                return Err(CsharpCodegenError::new(format!(
                    "{}.{} conflicts with generated C# members",
                    ty.name, field.name
                )));
            }
        }
    }
    let mut files = Vec::new();
    for en in schema.all_enums() {
        let mut body = format!(
            "using System;\nnamespace {} {{\n{}public enum @{} : {} {{\n",
            namespace(root, &en.name),
            if en.is_flag { "[Flags]\n" } else { "" },
            short(&en.name),
            if en.is_flag { "uint" } else { "int" }
        );
        if let Some(variants) = ids.get(en.name.as_str()) {
            for v in variants {
                body.push_str(&format!("@{} = {},\n", v.name, v.value));
            }
        } else {
            for v in &en.variants {
                body.push_str(&format!("@{} = {},\n", v.name, v.value));
            }
        }
        body.push_str("}\n}\n");
        files.push(file(&en.name, body));
    }
    for ty in schema.all_types() {
        let type_name = short(&ty.name);
        let base = ty
            .parent
            .as_ref()
            .map_or_else(|| "RuntimeObject".into(), |p| qualified(root, p));
        let guard = format!("value.RequireContract(global::{root}.Generated.ContractIdentity);");
        let mut body = if ty.is_struct {
            format!("#nullable enable\nusing System;\nusing Coflow;\nnamespace {} {{\npublic readonly struct @{} : IRuntimeArgument {{\nprivate readonly Projection Value;\ninternal @{}(Projection value) {{ {} Value = value; }}\nprivate T Read<T>(string field, Func<Projection,T> codec) => Value.Field(field).ReadProjected(codec);\nvoid IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(Value);\n", namespace(root, &ty.name), type_name, type_name, guard)
        } else {
            format!("#nullable enable\nusing System;\nusing Coflow;\nnamespace {} {{\npublic {}class @{} : {} {{\n{} @{}(Projection value) : base(value) {{ {} }}\n",namespace(root,&ty.name),if ty.is_abstract{"abstract "}else if ty.is_sealed||ty.is_singleton{"sealed "}else{""},type_name,base,if ty.is_abstract{"protected"}else{"internal"},type_name,guard)
        };
        if ty.kind == coflow_language::cft::syntax::ast::TypeKind::Data && !ty.is_abstract {
            let fields = ty.all_fields().collect::<Vec<_>>();
            let parameters = fields.iter().map(|field| Ok(format!("{} @{}", cs_type(&field.value_type, root)?, field.name))).collect::<Result<Vec<_>, CsharpCodegenError>>()?;
            let names = fields.iter().map(|field| quoted(&field.name)).collect::<Vec<_>>();
            let values = fields.iter().map(|field| pack(&field.value_type, &format!("@{}", field.name))).collect::<Vec<_>>();
            body.push_str(&format!("public @{}({}) : this(Projection.Data(global::{root}.Generated.ContractIdentity, {}, new string[] {{ {} }}, new Projection[] {{ {} }})) {{ }}\n", type_name, parameters.join(", "), quoted(&ty.name), names.join(", "), values.join(", ")));
        }
        let mut codecs = String::from("private static class __CoflowCodecs {\n");
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data && ty.parent.is_none() {
            codecs.push_str("internal static readonly Func<Projection,string> Id = ValueCodecs.String;\n");
            body.push_str("public string Id => Read(\"id\", __CoflowCodecs.Id);\n");
        }
        for (field_index, field) in ty.own_fields().enumerate() {
            let property_type = if field.dimension.is_some() {
                format!("RuntimeDimension<{}>", cs_type(&field.value_type, root)?)
            } else {
                cs_type(&field.value_type, root)?
            };
            let reader = if field.dimension.is_some() {
                format!(
                    "v => new {}(v, {})",
                    property_type,
                    codec(schema, &field.value_type, root, 1)?
                )
            } else {
                codec(schema, &field.value_type, root, 0)?
            };
            // C# 9 不缓存方法组转换；字段解码委托按生成类型初始化一次。
            codecs.push_str(&format!("internal static readonly Func<Projection,{property_type}> F{field_index} = {reader};\n"));
            let reader = format!("__CoflowCodecs.F{field_index}");
            if let CftValueType::Function(parameters, result) = &field.value_type {
                if field.dimension.is_none() {
                    // 保留声明参数名以支持 C# 命名实参；匿名参数避开所有显式名称。
                    let mut used = parameters.iter().filter_map(|parameter| parameter.name.clone()).collect::<BTreeSet<_>>();
                    let names = parameters.iter().enumerate().map(|(index, parameter)| {
                        let name = parameter.name.clone().unwrap_or_else(|| {
                            let mut name = format!("a{index}");
                            while used.contains(&name) { name.push('_'); }
                            used.insert(name.clone()); name
                        });
                        format!("@{name}")
                    }).collect::<Vec<_>>();
                    let arguments = parameters.iter().zip(&names).map(|(parameter, name)| Ok(format!("{} {name}", cs_type(&parameter.value_type, root)?))).collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                    body.push_str(&format!("public {} @{}Function => Read({}, {});\npublic {} @{}({}) => this.@{}Function.Invoke({});\n", property_type, field.name, quoted(&field.name), reader, cs_type(result, root)?, field.name, arguments.join(", "), field.name, names.join(", ")));
                    continue;
                }
            }
            body.push_str(&format!("public {} @{} => Read({}, {});\n", property_type, field.name, quoted(&field.name), reader));
            if matches!(field.value_type, CftValueType::FString) {
                if field.dimension.is_some() {
                    body.push_str(&format!("public string Render{}(string? variant = null) => (variant == null ? @{}.Default() : @{}.For(variant)).Render();\n", field.name, field.name, field.name));
                } else {
                    body.push_str(&format!("public string Render{}() => @{}.Render();\n", field.name, field.name));
                }
            }
        }

        codecs.push_str("}\n");
        body.push_str(&codecs);

        // 工厂分派由生成器静态列出，不依赖反射或运行时泛型实例生成。
        body.push_str(&format!(
            "internal {}static {} Wrap(Projection value) {{\nvalue = value.Canonical();\nif (value.TryGetProjection<{}>(out var projected)) return projected;\nswitch (value.TypeName) {{\n",
            if ty.parent.is_some() { "new " } else { "" },
            qualified(root, &ty.name),
            qualified(root, &ty.name)
        ));
        for child in schema
            .all_types()
            .filter(|child| !child.is_abstract && schema.is_assignable(&child.name, &ty.name))
        {
            body.push_str(&format!(
                "case {}: return new {}(value);\n",
                quoted(&child.name),
                qualified(root, &child.name)
            ));
        }
        body.push_str(
            "default: throw new CoflowException(\"Unexpected runtime type.\");\n}\n}\n}\n}\n",
        );
        files.push(file(&ty.name, body));
    }
    let contract = coflow_core::contract::Contract::new(schema.clone())
        .map_err(|e| CsharpCodegenError::new(e.to_string()))?;
    let bytes = contract
        .to_bytes()
        .map_err(|e| CsharpCodegenError::new(e.to_string()))?;
    let identity = contract
        .identity()
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let bindings = schema
        .all_types()
        .map(|ty| {
            let initialize = ty.all_fields().map(|field| {
                let suffix = if matches!(field.value_type, CftValueType::Function(..)) && field.dimension.is_none() { "Function" } else { "" };
                format!("_ = value.@{}{suffix};", field.name)
            }).collect::<Vec<_>>().join(" ");
            format!(
                "new TypeBinding<{}>({}, {}.Wrap, value => {{ {initialize} }})",
                qualified(root, &ty.name),
                quoted(&ty.name),
                qualified(root, &ty.name)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    files.push(GeneratedFile { relative_path: "Coflow.Bindings.cs".into(), contents: format!("using Coflow;\nnamespace {root} {{ public static class Generated {{ internal static byte[] ContractIdentity {{ get; }} = new byte[] {{ {identity} }}; private static TypeBinding[] Bindings {{ get; }} = new TypeBinding[] {{ {bindings} }}; public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings); }} }}\n") });
    let mut hosts = format!("#nullable enable\nusing System;\nusing Coflow;\nnamespace {root} {{ public static class GeneratedHostBindings {{\n");
    let mut interfaces = String::new();
    for (index, ty) in schema.all_types().filter(|ty| ty.is_host).enumerate() {
        let interface_name = format!("I{}", short(&ty.name));
        let source_interface = ty.name.rsplit_once("::").map_or_else(
            || interface_name.clone(),
            |(ns, _)| format!("{ns}::{interface_name}"),
        );
        if schema.resolve_type(&source_interface).is_some()
            || schema.resolve_enum(&source_interface).is_some()
        {
            return Err(CsharpCodegenError::new(
                "Host interface name conflicts with a declaration",
            ));
        }
        let interface_type = qualified(root, &source_interface);
        interfaces.push_str(&format!(
            "namespace {} {{ public interface @{} {{\n",
            namespace(root, &ty.name),
            interface_name
        ));
        for field in ty.all_fields() {
            if let CftValueType::Function(parameters, result) = &field.value_type {
                let arguments = parameters
                    .iter()
                    .enumerate()
                    .map(|(parameter_index, parameter)| {
                        Ok(format!(
                            "{} @{}",
                            cs_type(&parameter.value_type, root)?,
                            parameter
                                .name
                                .as_deref()
                                .unwrap_or(&format!("arg{parameter_index}"))
                        ))
                    })
                    .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                interfaces.push_str(&format!(
                    "{} @{}({});\n",
                    cs_type(result, root)?,
                    field.name,
                    arguments.join(", ")
                ));
            } else {
                interfaces.push_str(&format!(
                    "{} @{} {{ get; }}\n",
                    cs_type(&field.value_type, root)?,
                    field.name
                ));
            }
        }
        interfaces.push_str("} }\n");
        hosts.push_str(&format!("public static RuntimeBuilder BindHost(this RuntimeBuilder builder, {interface_type} host) => builder.BindHost(new Adapter{index}(host));\nprivate sealed class Adapter{index} : HostBinding {{ private readonly {interface_type} host; public Adapter{index}({interface_type} host) : base({}) {{ this.host = host ?? throw new ArgumentNullException(nameof(host)); }}\npublic override string MemberType(string field) {{ switch(field) {{\n", quoted(&ty.name)));
        for field in ty.all_fields() {
            hosts.push_str(&format!(
                "case {}: return {};\n",
                quoted(&field.name),
                quoted(&field.value_type.to_string())
            ));
        }
        hosts.push_str("default: throw new CoflowException(\"Unknown Host member.\"); } }\npublic override object? Read(string field) { switch(field) {\n");
        for field in ty
            .all_fields()
            .filter(|field| !matches!(field.value_type, CftValueType::Function(..)))
        {
            let value = match &field.value_type {
                CftValueType::Enum(name) => {
                    format!("new HostEnum({}, (uint)host.@{})", quoted(name), field.name)
                }
                CftValueType::Option(inner) if matches!(inner.as_ref(), CftValueType::Enum(_)) => {
                    let CftValueType::Enum(name) = inner.as_ref() else {
                        return Err(CsharpCodegenError::new("invalid optional enum"));
                    };
                    format!(
                        "host.@{0} is {{ }} __{0} ? (object)new HostEnum({1}, (uint)__{0}) : null",
                        field.name,
                        quoted(name)
                    )
                }
                _ => format!("host.@{}", field.name),
            };
            hosts.push_str(&format!("case {}: return {value};\n", quoted(&field.name)));
        }
        hosts.push_str("default: throw new CoflowException(\"Host function members cannot be read as data.\"); } }\npublic override void Call(string field, HostCall call) { switch(field) {\n");
        for field in ty
            .all_fields()
            .filter(|field| matches!(field.value_type, CftValueType::Function(..)))
        {
            let CftValueType::Function(parameters, result) = &field.value_type else {
                unreachable!();
            };
            let arguments = parameters
                .iter()
                .map(|parameter| {
                    Ok(format!(
                        "call.Argument({})",
                        invocation_codec(schema, &parameter.value_type, root, 0)?
                    ))
                })
                .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
            hosts.push_str(&format!(
                "case {}: call.Return({}, host.@{}({})); return;\n",
                quoted(&field.name),
                invocation_codec(schema, result, root, 0)?,
                field.name,
                arguments.join(", ")
            ));
        }
        hosts.push_str("default: throw new CoflowException(\"Unknown Host function.\"); } } }\n");
    }
    hosts.push_str("} }\n");
    hosts.push_str(&interfaces);
    files.push(GeneratedFile {
        relative_path: "Coflow.Host.cs".into(),
        contents: hosts,
    });
    Ok((files, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_core::schema::{build_schema, parse_modules, CftFile, ModuleId};
    fn generated(sources: &[&str]) -> Result<(Vec<GeneratedFile>, Vec<u8>), CsharpCodegenError> {
        let modules = parse_modules(sources.iter().enumerate().map(|(index, source)|
            CftFile::from_source(ModuleId::from(format!("m{index}")), *source)));
        generate(&build_schema(&modules).unwrap(), &BTreeMap::new(), "Game.Config")
    }
    #[test]
    fn rejects_metadata_namespace_and_inherited_helper_collisions() {
        for sources in [
            vec!["enum Generated { One }"],
            vec!["namespace Generated; data Value { n: int; }"],
            vec!["data Node { n: int; }", "namespace Node; data Child { n: int; }"],
            vec!["table Base { run: fn() -> int => { 1 }; } table Child : Base { runFunction: int = 0; }"],
            vec!["table Base { text: fstring = f\"x\"; } table Child : Base { Rendertext: int = 0; }"],
        ] { assert!(generated(&sources).is_err(), "{sources:?}"); }
    }
    #[test]
    fn named_and_anonymous_arguments_have_stable_escaped_names() {
        let (files, _) = generated(&["table Rule { call: fn(a1: int, int, class: int) -> int; }"]).unwrap();
        let body = &files.iter().find(|file| file.relative_path == PathBuf::from("Rule.cs")).unwrap().contents;
        assert!(body.contains("@call(int @a1, int @a1_, int @class)"), "{body}");
        assert!(body.contains("this.@callFunction.Invoke(@a1, @a1_, @class)"));
    }
}
fn file(source: &str, contents: String) -> GeneratedFile {
    GeneratedFile {
        relative_path: format!("{}.cs", source.replace("::", "/")).into(),
        contents,
    }
}
