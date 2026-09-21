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

fn identifier(source: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "abstract",
        "as",
        "base",
        "bool",
        "break",
        "byte",
        "case",
        "catch",
        "char",
        "checked",
        "class",
        "const",
        "continue",
        "decimal",
        "default",
        "delegate",
        "do",
        "double",
        "else",
        "enum",
        "event",
        "explicit",
        "extern",
        "false",
        "finally",
        "fixed",
        "float",
        "for",
        "foreach",
        "goto",
        "if",
        "implicit",
        "in",
        "int",
        "interface",
        "internal",
        "is",
        "lock",
        "long",
        "namespace",
        "new",
        "null",
        "object",
        "operator",
        "out",
        "override",
        "params",
        "private",
        "protected",
        "public",
        "readonly",
        "ref",
        "return",
        "sbyte",
        "sealed",
        "short",
        "sizeof",
        "stackalloc",
        "static",
        "string",
        "struct",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "uint",
        "ulong",
        "unchecked",
        "unsafe",
        "ushort",
        "using",
        "virtual",
        "void",
        "volatile",
        "while",
    ];
    if KEYWORDS.contains(&source) {
        format!("@{source}")
    } else {
        source.to_string()
    }
}

fn name(source: &str) -> String {
    source
        .split("::")
        .map(identifier)
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
            "RuntimeDictionary<{}, {}>",
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
            format!("RuntimeFunction<{}>", types.join(", "))
        }
        CftValueType::Unit => "Unit".into(),
    })
}
fn pack(ty: &CftValueType, expression: &str) -> String {
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

fn decode(
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
        CftValueType::FString => format!("new RuntimeTemplate({value})"),
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

fn append_materialization(
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
        .map(identifier)
        .collect::<Vec<_>>()
        .join(".");
    let root = escaped_root.as_str();
    // C# 的类型和命名空间共享声明空间，元数据、enum 和 Host 接口也参与冲突检查。
    let mut symbols =
        BTreeSet::from(["Generated".to_string(), "GeneratedHostBindings".to_string()]);
    let mut names = schema
        .all_types()
        .map(|ty| ty.name.to_string())
        .chain(schema.all_enums().map(|ty| ty.name.to_string()))
        .collect::<Vec<_>>();
    names.extend(schema.all_types().filter(|ty| ty.is_host).map(|ty| {
        let interface = format!("I{}", short(&ty.name));
        ty.name
            .rsplit_once("::")
            .map_or_else(|| interface.clone(), |(ns, _)| format!("{ns}::{interface}"))
    }));
    for name in &names {
        if !symbols.insert(name.clone()) {
            return Err(CsharpCodegenError::new(format!(
                "{name} conflicts with a generated C# type"
            )));
        }
    }
    for name in &names {
        let mut parent = name.as_str();
        while let Some((namespace, _)) = parent.rsplit_once("::") {
            if symbols.contains(namespace) {
                return Err(CsharpCodegenError::new(format!(
                    "{namespace} is both a C# type and namespace"
                )));
            }
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
        for field in ty.all_fields().filter(|field| {
            matches!(field.value_type, CftValueType::Function(..)) && field.dimension.is_none()
        }) {
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
        let enum_name = identifier(short(&en.name));
        let mut body = format!(
            "using System;\n\nnamespace {}\n{{\n{}public enum {} : {}\n{{\n",
            namespace(root, &en.name),
            if en.is_flag { "[Flags]\n" } else { "" },
            enum_name,
            if en.is_flag { "uint" } else { "int" }
        );
        if let Some(variants) = ids.get(en.name.as_str()) {
            for v in variants {
                body.push_str(&format!("    {} = {},\n", identifier(&v.name), v.value));
            }
        } else {
            for v in &en.variants {
                body.push_str(&format!("    {} = {},\n", identifier(&v.name), v.value));
            }
        }
        body.push_str("}\n}\n");
        files.push(file(&en.name, body));
    }
    for ty in schema.all_types() {
        let type_name = identifier(short(&ty.name));
        let base = ty
            .parent
            .as_ref()
            .map_or_else(|| "RuntimeObject".into(), |parent| qualified(root, parent));
        let mut body = format!(
            "#nullable enable\nusing System;\nusing Coflow;\n\nnamespace {}\n{{\n",
            namespace(root, &ty.name)
        );
        if ty.is_struct {
            body.push_str(&format!(
                "public readonly struct {type_name} : IRuntimeArgument\n{{\n    private readonly Projection _value;\n"
            ));
        } else {
            body.push_str(&format!(
                "public {}class {type_name} : {base}\n{{\n",
                if ty.is_abstract {
                    "abstract "
                } else if ty.is_sealed || ty.is_singleton {
                    "sealed "
                } else {
                    ""
                }
            ));
        }

        let mut assignments = Vec::new();
        let mut decoders = Vec::new();
        let mut host_codecs = Vec::new();
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data && ty.parent.is_none() {
            if ty.is_host {
                body.push_str("    public string Id => Read(\"id\", __Codecs.Id);\n");
                host_codecs.push("        internal static readonly Func<Projection, string> Id = ValueCodecs.String;\n".to_string());
            } else {
                body.push_str("    public string Id { get; private set; } = default!;\n");
                assignments
                    .push("        Id = ValueCodecs.String(record.Field(\"id\"));\n".to_string());
            }
        }

        for field in ty.own_fields() {
            let field_name = identifier(&field.name);
            let member_name = if matches!(field.value_type, CftValueType::Function(..))
                && field.dimension.is_none()
            {
                format!("{field_name}Function")
            } else {
                field_name.clone()
            };
            let property_type = if field.dimension.is_some() {
                format!("RuntimeDimension<{}>", cs_type(&field.value_type, root)?)
            } else {
                cs_type(&field.value_type, root)?
            };
            let reader = if field.dimension.is_some() {
                format!(
                    "value => new {property_type}(value, {})",
                    codec(schema, &field.value_type, root, 1)?
                )
            } else {
                codec(schema, &field.value_type, root, 0)?
            };
            let value = if field.dimension.is_some() {
                format!(
                    "new {property_type}(__VALUE__, {})",
                    codec(schema, &field.value_type, root, 0)?
                )
            } else {
                decode(schema, &field.value_type, root, "__VALUE__")?
            };

            if ty.is_host {
                body.push_str(&format!(
                    "    public {property_type} {member_name} => Read({}, __Codecs.{field_name});\n",
                    quoted(&field.name)
                ));
                host_codecs.push(format!(
                    "        internal static readonly Func<Projection, {property_type}> {field_name} = {reader};\n"
                ));
            } else if ty.is_struct {
                body.push_str(&format!(
                    "    public {property_type} {member_name} {{ get; }}\n"
                ));
                append_materialization(
                    &mut assignments,
                    &mut decoders,
                    &member_name,
                    &field_name,
                    &property_type,
                    &reader,
                    &value,
                    &format!("projection.Field({})", quoted(&field.name)),
                );
            } else {
                body.push_str(&format!(
                    "    public {property_type} {member_name} {{ get; private set; }} = default!;\n"
                ));
                let projection = format!("record.Field({})", quoted(&field.name));
                let materialized = if field.dimension.is_some() {
                    format!(
                        "new {property_type}({projection}, {})",
                        codec(schema, &field.value_type, root, 0)?
                    )
                } else {
                    decode(schema, &field.value_type, root, &projection)?
                };
                assignments.push(format!("        {member_name} = {materialized};\n"));
            }

            if let CftValueType::Function(parameters, result) = &field.value_type {
                if field.dimension.is_none() {
                    let mut used = parameters
                        .iter()
                        .filter_map(|parameter| parameter.name.clone())
                        .collect::<BTreeSet<_>>();
                    let names = parameters
                        .iter()
                        .enumerate()
                        .map(|(index, parameter)| {
                            let raw = parameter.name.clone().unwrap_or_else(|| {
                                let mut candidate = format!("a{index}");
                                while used.contains(&candidate) {
                                    candidate.push('_');
                                }
                                used.insert(candidate.clone());
                                candidate
                            });
                            identifier(&raw)
                        })
                        .collect::<Vec<_>>();
                    let arguments = parameters
                        .iter()
                        .zip(&names)
                        .map(|(parameter, name)| {
                            Ok(format!("{} {name}", cs_type(&parameter.value_type, root)?))
                        })
                        .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                    body.push_str(&format!(
                        "    public {} {field_name}({}) => {member_name}.Invoke({});\n",
                        cs_type(result, root)?,
                        arguments.join(", "),
                        names.join(", ")
                    ));
                }
            }
            if matches!(field.value_type, CftValueType::FString) {
                if field.dimension.is_some() {
                    body.push_str(&format!(
                        "    public string Render{}(string? variant = null) => (variant == null ? {field_name}.Default() : {field_name}.For(variant)).Render();\n",
                        field.name
                    ));
                } else {
                    body.push_str(&format!(
                        "    public string Render{}() => {field_name}.Render();\n",
                        field.name
                    ));
                }
            }
        }

        if ty.is_struct {
            body.push_str(&format!(
                "\n    internal {type_name}(Projection projection)\n    {{\n        projection.RequireContract(global::{root}.Generated.ContractIdentity);\n        _value = projection;\n{}    }}\n\n    void IRuntimeArgument.Encode(ArgumentWriter writer) => writer.Write(_value);\n",
                assignments.join("")
            ));
        } else {
            body.push_str(&format!(
                "\n    {} {type_name}(Record record) : base(record)\n    {{\n{}    }}\n",
                if ty.is_abstract {
                    "protected"
                } else {
                    "internal"
                },
                assignments.join("")
            ));
        }

        if ty.kind == coflow_language::cft::syntax::ast::TypeKind::Data && !ty.is_abstract {
            let fields = ty.all_fields().collect::<Vec<_>>();
            let parameters = fields
                .iter()
                .map(|field| {
                    Ok(format!(
                        "{} {}",
                        cs_type(&field.value_type, root)?,
                        identifier(&field.name)
                    ))
                })
                .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
            let names = fields
                .iter()
                .map(|field| quoted(&field.name))
                .collect::<Vec<_>>();
            let values = fields
                .iter()
                .map(|field| pack(&field.value_type, &identifier(&field.name)))
                .collect::<Vec<_>>();
            let detached = format!(
                "Projection.Data(global::{root}.Generated.ContractIdentity, {}, new[] {{ {} }}, new[] {{ {} }})",
                quoted(&ty.name),
                names.join(", "),
                values.join(", ")
            );
            body.push_str(&format!(
                "\n    public {type_name}({}) : this({}) {{ }}\n",
                parameters.join(", "),
                if ty.is_struct { detached } else { format!("new Record({detached})") }
            ));
        }

        if !decoders.is_empty() {
            body.push('\n');
            for decoder in decoders {
                body.push_str(&decoder);
            }
        }

        if !host_codecs.is_empty() {
            body.push_str("\n    private static class __Codecs\n    {\n");
            for codec in host_codecs {
                body.push_str(&codec);
            }
            body.push_str("    }\n");
        }

        body.push_str("}\n");
        body.push_str("}\n");
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
            let factory = if ty.is_struct {
                format!("value => new {}(value)", qualified(root, &ty.name))
            } else {
                format!(
                    "value => new {}(new Record(value))",
                    qualified(root, &ty.name)
                )
            };
            format!(
                "        new TypeBinding<{}>({}, {factory})",
                qualified(root, &ty.name),
                quoted(&ty.name)
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    files.push(GeneratedFile {
        relative_path: "Coflow.Bindings.cs".into(),
        contents: format!(
            "using Coflow;\n\nnamespace {root}\n{{\npublic static class Generated\n{{\n    internal static byte[] ContractIdentity {{ get; }} = new byte[] {{ {identity} }};\n\n    private static TypeBinding[] Bindings {{ get; }} = new TypeBinding[]\n    {{\n{bindings}\n    }};\n\n    public static Contract LoadContract(byte[] bytes) => new Contract(bytes, ContractIdentity, Bindings);\n}}\n}}\n"
        ),
    });
    let mut hosts = format!("#nullable enable\nusing System;\nusing Coflow;\n\nnamespace {root}\n{{\npublic static class GeneratedHostBindings\n{{\n");
    for ty in schema.all_types().filter(|ty| ty.is_host) {
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
        let short_name = short(&ty.name);
        let binding_name = if schema
            .all_types()
            .filter(|candidate| candidate.is_host && short(&candidate.name) == short_name)
            .count()
            == 1
        {
            format!("{}Binding", identifier(short_name))
        } else {
            format!("{}Binding", identifier(&ty.name.replace("::", "_")))
        };
        let mut interface = format!(
            "#nullable enable\nusing Coflow;\n\nnamespace {}\n{{\npublic interface {}\n{{\n",
            namespace(root, &ty.name),
            identifier(&interface_name)
        );
        for field in ty.all_fields() {
            if let CftValueType::Function(parameters, result) = &field.value_type {
                let arguments = parameters
                    .iter()
                    .enumerate()
                    .map(|(parameter_index, parameter)| {
                        Ok(format!(
                            "{} {}",
                            cs_type(&parameter.value_type, root)?,
                            identifier(
                                parameter
                                    .name
                                    .as_deref()
                                    .unwrap_or(&format!("arg{parameter_index}"))
                            )
                        ))
                    })
                    .collect::<Result<Vec<_>, CsharpCodegenError>>()?;
                interface.push_str(&format!(
                    "    {} {}({});\n",
                    cs_type(result, root)?,
                    identifier(&field.name),
                    arguments.join(", ")
                ));
            } else {
                interface.push_str(&format!(
                    "    {} {} {{ get; }}\n",
                    cs_type(&field.value_type, root)?,
                    identifier(&field.name)
                ));
            }
        }
        interface.push_str("}\n}\n");
        files.push(file(&source_interface, interface));
        hosts.push_str(&format!("    public static RuntimeBuilder BindHost(this RuntimeBuilder builder, {interface_type} host)\n        => builder.BindHost(new {binding_name}(host));\n\n    private sealed class {binding_name} : HostBinding\n    {{\n        private readonly {interface_type} host;\n\n        public {binding_name}({interface_type} host) : base({})\n        {{\n            this.host = host ?? throw new ArgumentNullException(nameof(host));\n        }}\n\n        public override object? Read(string field) => field switch\n        {{\n", quoted(&ty.name)));
        for field in ty
            .all_fields()
            .filter(|field| !matches!(field.value_type, CftValueType::Function(..)))
        {
            let value = match &field.value_type {
                CftValueType::Enum(name) => {
                    format!(
                        "new HostEnum({}, (uint)host.{})",
                        quoted(name),
                        identifier(&field.name)
                    )
                }
                CftValueType::Option(inner) if matches!(inner.as_ref(), CftValueType::Enum(_)) => {
                    let CftValueType::Enum(name) = inner.as_ref() else {
                        return Err(CsharpCodegenError::new("invalid optional enum"));
                    };
                    format!(
                        "host.{0} is {{ }} __{1} ? (object)new HostEnum({2}, (uint)__{1}) : null",
                        identifier(&field.name),
                        field.name,
                        quoted(name)
                    )
                }
                _ => format!("host.{}", identifier(&field.name)),
            };
            hosts.push_str(&format!(
                "            {} => {value},\n",
                quoted(&field.name)
            ));
        }
        hosts.push_str("            _ => throw new CoflowException(\"Host function members cannot be read as data.\"),\n        };\n\n        public override void Call(string field, HostCall call)\n        {\n            switch (field)\n            {\n");
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
                "                case {}:\n                    call.Return({}, host.{}({}));\n                    return;\n",
                quoted(&field.name),
                invocation_codec(schema, result, root, 0)?,
                identifier(&field.name),
                arguments.join(", ")
            ));
        }
        hosts.push_str("                default: throw new CoflowException(\"Unknown Host function.\");\n            }\n        }\n    }\n\n");
    }
    hosts.push_str("}\n}\n");
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
        let modules = parse_modules(sources.iter().enumerate().map(|(index, source)| {
            CftFile::from_source(ModuleId::from(format!("m{index}")), *source)
        }));
        generate(
            &build_schema(&modules).unwrap(),
            &BTreeMap::new(),
            "Game.Config",
        )
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
        let (files, _) =
            generated(&["table Rule { call: fn(a1: int, int, class: int) -> int; }"]).unwrap();
        let body = &files
            .iter()
            .find(|file| file.relative_path == PathBuf::from("Rule.cs"))
            .unwrap()
            .contents;
        assert!(body.contains("call(int a1, int a1_, int @class)"), "{body}");
        assert!(body.contains("callFunction.Invoke(a1, a1_, @class)"));
    }

    #[test]
    fn generated_api_is_materialized_and_only_escapes_keywords() {
        let (files, _) = generated(&[
            "data Stats { value: int; } table Rule { name: string; stats: Stats; class: int; } @Host singleton Services { environment: string; class: fn(value: int) -> int; }",
        ])
        .unwrap();
        let rule = &files
            .iter()
            .find(|file| file.relative_path == PathBuf::from("Rule.cs"))
            .unwrap()
            .contents;
        assert!(rule.contains("namespace Game.Config\n{"));
        assert!(rule.contains("public string name { get; private set; } = default!;"));
        assert!(rule.contains("public int @class { get; private set; } = default!;"));
        assert!(rule.contains("internal Rule(Record record) : base(record)"));
        assert!(rule.contains("name = ValueCodecs.String(record.Field(\"name\"));"));
        assert!(!rule.contains("__Initialize"));
        assert!(!rule.contains("ReadProjected"));
        assert!(!rule.contains(" F0"));

        let host = &files
            .iter()
            .find(|file| file.relative_path == PathBuf::from("Coflow.Host.cs"))
            .unwrap()
            .contents;
        assert!(host.contains("host.environment"));
        assert!(host.contains("host.@class("));
        assert!(!host.contains("host.@environment"));

        let bindings = &files
            .iter()
            .find(|file| file.relative_path == PathBuf::from("Coflow.Bindings.cs"))
            .unwrap()
            .contents;
        assert!(bindings.contains("Rule(new Record(value))"));
        assert!(!bindings.contains("Initialize"));

        let interface = &files
            .iter()
            .find(|file| file.relative_path == PathBuf::from("IServices.cs"))
            .unwrap()
            .contents;
        assert!(interface.contains("public interface IServices"));
        assert!(interface.contains("int @class(int value);"));
    }
}
fn file(source: &str, contents: String) -> GeneratedFile {
    GeneratedFile {
        relative_path: format!("{}.cs", source.replace("::", "/")).into(),
        contents,
    }
}
