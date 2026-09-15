//! C# 只生成 Unity/AOT 可用的类型包装，不生成解析器、编译器或数据副本。
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
        let files = generate(input.schema, &variants, namespace)
            .map_err(|e| CodegenError::Message(e.to_string()))?;
        CodeArtifactSet::new(
            files
                .into_iter()
                .map(|f| CodeArtifactFile {
                    relative_path: f.relative_path,
                    contents: f.contents,
                })
                .collect(),
        )
    }
}
pub fn generate_csharp(schema: &CftSchema) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate(schema, &BTreeMap::new(), "Coflow.Generated")
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
        CftValueType::String | CftValueType::FString => "string".into(),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => qualified(root, n),
        CftValueType::Enum(n) => qualified(root, n),
        CftValueType::Array(t) => format!("CoflowArray<{}>", cs_type(t, root)?),
        CftValueType::Dict(k, v) => format!(
            "CoflowDictionary<{},{}>",
            cs_type(k, root)?,
            cs_type(v, root)?
        ),
        CftValueType::Option(t) => format!("CoflowOptional<{}>", cs_type(t, root)?),
        CftValueType::Function(..) => "CoflowFunction".into(),
        CftValueType::Unit => "CoflowValue".into(),
    })
}
fn codec(ty: &CftValueType, root: &str, depth: usize) -> Result<String, CsharpCodegenError> {
    let v = format!("v{depth}");
    let next = depth + 1;
    Ok(match ty {
        CftValueType::Int => "CoflowCodecs.Int".into(),
        CftValueType::Float => "CoflowCodecs.Float".into(),
        CftValueType::Bool => "CoflowCodecs.Bool".into(),
        CftValueType::String | CftValueType::FString => "CoflowCodecs.String".into(),
        CftValueType::Enum(n) => format!("{v} => ({})CoflowCodecs.Enum({v})", qualified(root, n)),
        CftValueType::Object(n) | CftValueType::RecordRef(n) => {
            format!("{}.Wrap", qualified(root, n))
        }
        CftValueType::Array(t) => format!(
            "{v} => new {}({v}, {})",
            cs_type(ty, root)?,
            codec(t, root, next)?
        ),
        CftValueType::Dict(k, t) => format!(
            "{v} => new {}({v}, {}, {})",
            cs_type(ty, root)?,
            codec(k, root, next)?,
            codec(t, root, next)?
        ),
        CftValueType::Option(t) => format!(
            "{v} => new {}({v}, {})",
            cs_type(ty, root)?,
            codec(t, root, next)?
        ),
        CftValueType::Function(..) => format!("{v} => new CoflowFunction({v})"),
        CftValueType::Unit => format!("{v} => {v}"),
    })
}
fn generate(
    schema: &CftSchema,
    ids: &BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    root: &str,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    if root.is_empty() || !root.split('.').all(coflow_language::lexical::is_identifier) {
        return Err(CsharpCodegenError::new("invalid C# namespace"));
    }
    let escaped_root = root
        .split('.')
        .map(|part| format!("@{part}"))
        .collect::<Vec<_>>()
        .join(".");
    let root = escaped_root.as_str();
    for ty in schema.all_types() {
        if ty.name.as_str() == "CoflowSchema" {
            return Err(CsharpCodegenError::new(
                "CoflowSchema conflicts with generated contract metadata",
            ));
        }
        let mut reserved: BTreeSet<String> = [
            "Value",
            "Read",
            "ActualType",
            "Dispose",
            "RetainValue",
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
            reserved.insert(format!("Get_{}_Template", field.name));
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
            .map_or_else(|| "CoflowObject".into(), |p| qualified(root, p));
        let guard = format!("value.RequireContract(global::{root}.CoflowSchema.Identity);");
        let mut body = if ty.is_struct {
            format!("using System;\nusing Coflow.Runtime;\nnamespace {} {{\npublic readonly struct @{} : IDisposable, ICoflowValue {{\nprivate readonly CoflowValue Value;\npublic @{}(CoflowValue value) {{ {} Value = value; }}\nprivate T Read<T>(string field, Func<CoflowValue,T> codec) => codec(Value.Field(field));\npublic CoflowValue RetainValue() => Value.Retain();\npublic void Dispose() => Value.Dispose();\n", namespace(root, &ty.name), type_name, type_name, guard)
        } else {
            format!("using System;\nusing Coflow.Runtime;\nnamespace {} {{\npublic {}class @{} : {} {{\n{} @{}(CoflowValue value) : base(value) {{ {} }}\n",namespace(root,&ty.name),if ty.is_abstract{"abstract "}else if ty.is_sealed||ty.is_singleton{"sealed "}else{""},type_name,base,if ty.is_abstract{"protected"}else{"public"},type_name,guard)
        };
        if ty.kind != coflow_language::cft::syntax::ast::TypeKind::Data && ty.parent.is_none() {
            body.push_str("public string Id => Read(\"id\", CoflowCodecs.String);\n");
        }
        for field in ty.own_fields() {
            let property_type = if field.dimension.is_some() {
                format!("CoflowDimension<{}>", cs_type(&field.value_type, root)?)
            } else {
                cs_type(&field.value_type, root)?
            };
            let reader = if field.dimension.is_some() {
                format!(
                    "v => new {}(v, {})",
                    property_type,
                    codec(&field.value_type, root, 1)?
                )
            } else {
                codec(&field.value_type, root, 0)?
            };
            body.push_str(&format!(
                "public {} @{} => Read({}, {});\n",
                property_type,
                field.name,
                quoted(&field.name),
                reader
            ));
            if matches!(field.value_type, CftValueType::FString) {
                if field.dimension.is_some() {
                    // 维度字段存储记录句柄，模板句柄取自其基础值或回退后的变体。
                    body.push_str(&format!("public CoflowValue Get_{}_Template(string variant = null) {{ using (var dimension = Value.Field({})) {{ return variant == null ? dimension.DimensionDefault() : dimension.DimensionValue(variant); }} }}\n", field.name, quoted(&field.name)));
                } else {
                    body.push_str(&format!(
                        "public CoflowValue Get_{}_Template() => Value.Field({});\n",
                        field.name,
                        quoted(&field.name)
                    ));
                }
            }
        }
        // 工厂分派由生成器静态列出，不依赖反射或运行时泛型实例生成。
        body.push_str(&format!(
            "public {}static {} Wrap(CoflowValue value) {{\nswitch (value.TypeName) {{\n",
            if ty.parent.is_some() { "new " } else { "" },
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
        body.push_str("default: value.Dispose(); throw new CoflowException(\"Unexpected runtime type.\");\n}\n}\n}\n}\n");
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
    let payload = bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    files.push(GeneratedFile{relative_path:"Coflow.Contract.cs".into(),contents:format!("using Coflow.Runtime;\nnamespace {root} {{ public static class CoflowSchema {{ public static byte[] Identity => new byte[] {{ {identity} }}; public static CoflowContract Load() => CoflowContract.Load(new byte[] {{ {payload} }}); }} }}\n")});
    Ok(files)
}
fn file(source: &str, contents: String) -> GeneratedFile {
    GeneratedFile {
        relative_path: format!("{}.cs", source.replace("::", "/")).into(),
        contents,
    }
}
