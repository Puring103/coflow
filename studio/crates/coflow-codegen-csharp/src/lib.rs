//! 生成 Unity/AOT 可用的只读数据投影、静态 codec 与执行入口。
use coflow_codegen::{
    CodeArtifactFile, CodeArtifactSet, CodeGenerator, CodegenDescriptor, CodegenError, CodegenInput,
};
use coflow_core::schema::CftSchema;
use std::{collections::BTreeMap, fmt, path::PathBuf};

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

mod declarations;
mod generate;
mod hosts;
mod names;
mod value_codecs;
use generate::generate;
