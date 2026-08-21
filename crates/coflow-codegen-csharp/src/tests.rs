#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use coflow_cft::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use std::collections::BTreeMap;

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &Default::default()).expect("schema compiles")
}

fn all(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn emits_declarations_and_direct_cfd_entrypoint() {
    let files = generate_csharp_cfd(
        &schema("type Item { name: string; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("SourceFiles"));
    assert!(output.contains("ICfdTypeBinding"));
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(output.contains(
        "Load(ICfdSourceProvider provider, CfdLoadOptions? options = null)"
    ));
    assert!(output.contains("CfdParser.ParseAll"));
    assert!(output.contains("Coflow.Cfd.Runtime"));
    assert!(!output.contains("Newtonsoft.Json"));
    assert!(!output.contains("MessagePack"));
    assert!(!output.contains(".json"));
    assert!(!output.contains(".msgpack"));
}

#[test]
fn preserves_display_metadata_as_xml_docs() {
    let files = generate_csharp(
        &schema(
            r#"@label("Item") @description("Description") type Item { @label("Name") name: string; }"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
    )
    .expect("generate");
    let item = files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "Item.cs")
        .expect("item file");
    assert!(item.contents.contains("<summary>Item: Description</summary>"));
    assert!(item.contents.contains("<summary>Name</summary>"));
}

#[test]
fn descriptor_declares_cfd_runtime_contract() {
    assert_eq!(CSHARP_CFD_CODEGEN_DESCRIPTOR.id, "csharp");
    assert_eq!(CSHARP_CFD_CODEGEN_DESCRIPTOR.runtime_package, "Coflow.Cfd.Runtime");
    assert!(CSHARP_CFD_CODEGEN_DESCRIPTOR.needs_model);
}

#[test]
fn registry_generator_returns_safe_code_artifacts() {
    let mut registry = coflow_codegen_api::CodegenRegistry::default();
    registry
        .register(CsharpCfdCodeGenerator)
        .expect("register C#");
    assert!(registry.get("csharp").is_some());
    assert!(registry.register(CsharpCfdCodeGenerator).is_err());
}
