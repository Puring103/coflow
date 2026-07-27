//! Rust 2021 code generator and JSON loader generator.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

use coflow_api::{
    ArtifactFile, ArtifactSet, CodeGenerator, CodegenContext, CodegenDescriptor,
    DecodedOutputOptions, Diagnostic, DiagnosticSet, LoaderDescriptor, LoaderGenerationContext,
    LoaderGenerator, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CftValueType;
use coflow_codegen_core::{CodegenEnum, CodegenModel, CodegenType};
use std::fmt::Write;

pub const RUST_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "rust",
    display_name: "Rust",
    language: "rust",
    file_extensions: &["rs"],
    needs_model_for_build: false,
};

pub const RUST_JSON_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "rust-json",
    code: "rust",
    data: "json",
};

pub const RUST_PROTOBUF_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "rust-protobuf",
    code: "rust",
    data: "protobuf",
};

#[derive(Debug, Default, Clone, Copy)]
pub struct RustCodeGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct RustJsonLoaderGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct RustProtobufLoaderGenerator;

#[derive(Debug)]
struct EmptyOptions;

/// Declares Rust codegen and compatible loader roles.
///
/// # Errors
///
/// Returns an error when a role id conflicts within the bundle.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_codegen(RustCodeGenerator)?;
    bundle.add_loader(RustJsonLoaderGenerator)?;
    bundle.add_loader(RustProtobufLoaderGenerator)?;
    Ok(bundle)
}

impl LoaderGenerator for RustProtobufLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &RUST_PROTOBUF_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("rust-protobuf", options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("rust-protobuf")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        reject_protobuf_dimensions(&model)?;
        artifact_set(vec![
            ArtifactFile::text(
                "mod.rs",
                "mod protobuf;\nmod types;\npub use protobuf::load;\npub use types::*;\n",
            ),
            ArtifactFile::text("protobuf.rs", render_protobuf_loader(&model)),
        ])
    }
}

fn reject_protobuf_dimensions(model: &CodegenModel) -> Result<(), DiagnosticSet> {
    if model.dimensions.is_empty() {
        Ok(())
    } else {
        Err(DiagnosticSet::one(Diagnostic::error(
            "RUST-PROTOBUF-LOCALIZATION",
            "CODEGEN",
            "Rust Protobuf loader does not yet support localized dimension tables",
        )))
    }
}

impl CodeGenerator for RustCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &RUST_CODEGEN_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("rust", options)
    }

    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("rust")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        artifact_set(vec![ArtifactFile::text("types.rs", render_types(&model))])
    }
}

impl LoaderGenerator for RustJsonLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &RUST_JSON_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("rust-json", options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("rust-json")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        artifact_set(vec![
            ArtifactFile::text(
                "mod.rs",
                "mod json;\nmod types;\npub use json::load;\npub use types::*;\n",
            ),
            ArtifactFile::text("json.rs", render_json_loader(&model)),
        ])
    }
}

fn decode_empty_options(
    id: &'static str,
    options: &serde_json::Value,
) -> Result<DecodedOutputOptions, DiagnosticSet> {
    if options.as_object().is_some_and(serde_json::Map::is_empty) {
        Ok(DecodedOutputOptions::new(id, EmptyOptions))
    } else {
        Err(DiagnosticSet::one(Diagnostic::error(
            "RUST-OPTIONS",
            "CODEGEN",
            format!("{id} does not accept options"),
        )))
    }
}

fn artifact_set(files: Vec<ArtifactFile>) -> Result<ArtifactSet, DiagnosticSet> {
    ArtifactSet::new(files).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "RUST-ARTIFACT",
            "ARTIFACT",
            error.to_string(),
        ))
    })
}

fn render_types(model: &CodegenModel) -> String {
    let mut out = String::from(
        "// Generated by Coflow. Do not edit.\n\
         use std::collections::BTreeMap;\n\
         use std::marker::PhantomData;\n\n\
         #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\n\
         pub struct RecordId<T> { pub key: String, marker: PhantomData<fn() -> T> }\n\
         impl<T> RecordId<T> { pub fn new(key: String) -> Self { Self { key, marker: PhantomData } } }\n\n\
         #[derive(Debug, Clone, PartialEq)]\n\
         pub struct Localized<T> { pub default: T }\n\
         impl<T> Localized<T> { pub fn new(default: T) -> Self { Self { default } } }\n\n",
    );
    for item in &model.enums {
        render_enum(&mut out, item);
    }
    for ty in &model.types {
        render_type(&mut out, ty, model);
    }
    for ty in model.types.iter().filter(|ty| ty.concrete_types.len() > 1) {
        let wrapper = format!("{}Value", pascal(&ty.name));
        let _ = writeln!(
            out,
            "#[derive(Debug, Clone, PartialEq)]\npub enum {wrapper} {{"
        );
        for actual in &ty.concrete_types {
            let name = pascal(actual);
            let _ = writeln!(out, "    {name}({name}),");
        }
        out.push_str("}\n\n");
    }
    out.push_str("#[derive(Debug, Default)]\npub struct CoflowTables {\n");
    for table in &model.table_names {
        let _ = writeln!(
            out,
            "    pub {}: BTreeMap<String, {}>,",
            rust_ident(&snake(table)),
            pascal(table)
        );
    }
    for singleton in &model.singleton_names {
        let _ = writeln!(
            out,
            "    pub {}: Option<{}>,",
            rust_ident(&snake(singleton)),
            pascal(singleton)
        );
    }
    out.push_str("}\n");
    out
}

fn render_enum(out: &mut String, item: &CodegenEnum) {
    let name = pascal(&item.name);
    if item.is_flags {
        let _ = writeln!(
            out,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub struct {name}(pub i64);"
        );
        let _ = writeln!(out, "impl {name} {{");
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "    pub const {}: Self = Self({});",
                upper_snake(&variant.name),
                variant.value
            );
        }
        out.push_str("}\n\n");
    } else {
        let _ = writeln!(
            out,
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\n#[repr(i64)]\npub enum {name} {{"
        );
        for variant in &item.variants {
            let _ = writeln!(out, "    {} = {},", pascal(&variant.name), variant.value);
        }
        out.push_str("}\n\n");
    }
}

fn render_type(out: &mut String, ty: &CodegenType, model: &CodegenModel) {
    let name = pascal(&ty.name);
    let _ = writeln!(
        out,
        "#[derive(Debug, Clone, PartialEq)]\npub struct {name} {{"
    );
    if !ty.is_abstract {
        out.push_str("    pub id: String,\n");
    }
    for field in &ty.all_fields {
        let mut field_type = rust_type(&field.value_type, model);
        if field.is_localized {
            field_type = format!("Localized<{field_type}>");
        }
        let _ = writeln!(
            out,
            "    pub {}: {},",
            rust_ident(&snake(&field.name)),
            field_type
        );
    }
    out.push_str("}\n\n");
}

fn rust_type(value: &CftValueType, model: &CodegenModel) -> String {
    match value {
        CftValueType::Int => "i64".to_string(),
        CftValueType::Float => "f64".to_string(),
        CftValueType::Bool => "bool".to_string(),
        CftValueType::String => "String".to_string(),
        CftValueType::Enum(name) => pascal(name),
        CftValueType::RecordRef(name) => format!("RecordId<{}>", pascal(name)),
        CftValueType::Object(name) => model.type_by_name(name).map_or_else(
            || pascal(name),
            |ty| {
                if ty.concrete_types.len() > 1 {
                    format!("{}Value", pascal(name))
                } else {
                    pascal(name)
                }
            },
        ),
        CftValueType::Array(inner) => format!("Vec<{}>", rust_type(inner, model)),
        CftValueType::Dict(key, value) => format!(
            "BTreeMap<{}, {}>",
            rust_type(key, model),
            rust_type(value, model)
        ),
        CftValueType::Nullable(inner) => format!("Option<{}>", rust_type(inner, model)),
    }
}

fn render_json_loader(model: &CodegenModel) -> String {
    let mut out = String::from(
        "// Generated by Coflow. Do not edit.\n\
         use super::types::*;\n\
         use serde_json::{Map, Value};\n\
         use std::collections::BTreeMap;\n\
         use std::fs;\n\
         use std::path::Path;\n\n\
         fn object(value: &Value) -> Result<&Map<String, Value>, String> { value.as_object().ok_or_else(|| \"expected JSON object\".to_string()) }\n\
         fn array(value: &Value) -> Result<&Vec<Value>, String> { value.as_array().ok_or_else(|| \"expected JSON array\".to_string()) }\n\
         fn string(value: &Value) -> Result<String, String> { value.as_str().map(str::to_string).ok_or_else(|| \"expected JSON string\".to_string()) }\n\
         fn integer(value: &Value) -> Result<i64, String> { value.as_i64().ok_or_else(|| \"expected JSON integer\".to_string()) }\n\
         fn number(value: &Value) -> Result<f64, String> { value.as_f64().ok_or_else(|| \"expected JSON number\".to_string()) }\n\
         fn boolean(value: &Value) -> Result<bool, String> { value.as_bool().ok_or_else(|| \"expected JSON bool\".to_string()) }\n\
         fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, String> { object.get(name).ok_or_else(|| format!(\"missing JSON field `{name}`\")) }\n\n",
    );
    for item in &model.enums {
        render_enum_parser(&mut out, item);
    }
    for ty in &model.types {
        render_type_parser(&mut out, ty, model);
    }
    for ty in model.types.iter().filter(|ty| ty.concrete_types.len() > 1) {
        render_polymorphic_parser(&mut out, ty);
    }
    out.push_str("pub fn load(directory: &Path) -> Result<CoflowTables, String> {\n    let mut database = CoflowTables::default();\n");
    for table in &model.table_names {
        let field_name = rust_ident(&snake(table));
        let parser = format!("parse_{}", snake(table));
        let _ = writeln!(
            out,
            "    for value in load_table(directory, \"{table}\")? {{ let record = {parser}(&value)?; database.{field_name}.insert(record.id.clone(), record); }}"
        );
    }
    for singleton in &model.singleton_names {
        let field_name = rust_ident(&snake(singleton));
        let parser = format!("parse_{}", snake(singleton));
        let _ = writeln!(
            out,
            "    if let Some(value) = load_table(directory, \"{singleton}\")?.first() {{ database.{field_name} = Some({parser}(value)?); }}"
        );
    }
    out.push_str("    Ok(database)\n}\n\nfn load_table(directory: &Path, name: &str) -> Result<Vec<Value>, String> {\n    let path = directory.join(format!(\"{name}.json\"));\n    if !path.exists() { return Ok(Vec::new()); }\n    let text = fs::read_to_string(&path).map_err(|error| format!(\"failed to read {}: {error}\", path.display()))?;\n    let value: Value = serde_json::from_str(&text).map_err(|error| format!(\"failed to parse {}: {error}\", path.display()))?;\n    array(&value).cloned()\n}\n");
    out
}

fn render_enum_parser(out: &mut String, item: &CodegenEnum) {
    let name = pascal(&item.name);
    let function = format!("parse_{}", snake(&item.name));
    let _ = writeln!(
        out,
        "fn {function}(value: &Value) -> Result<{name}, String> {{"
    );
    if item.is_flags {
        let _ = writeln!(
            out,
            "    if let Some(raw) = value.as_i64() {{ return Ok({name}(raw)); }}"
        );
    } else {
        out.push_str("    if let Some(raw) = value.as_i64() { return match raw {\n");
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "        {} => Ok({name}::{}),",
                variant.value,
                pascal(&variant.name)
            );
        }
        let _ = writeln!(
            out,
            "        _ => Err(format!(\"invalid {name} value {{raw}}\")),\n    }}; }}"
        );
    }
    out.push_str("    let text = value.as_str().ok_or_else(|| \"expected symbolic enum string\".to_string())?;\n    match text {\n");
    for variant in &item.variants {
        let expression = if item.is_flags {
            format!("{name}({})", variant.value)
        } else {
            format!("{name}::{}", pascal(&variant.name))
        };
        let _ = writeln!(
            out,
            "        \"{}.{}\" => Ok({expression}),",
            item.name, variant.name
        );
    }
    if item.is_flags {
        let _ = writeln!(
            out,
            "        _ => text.strip_prefix(\"{}(\").and_then(|value| value.strip_suffix(')')).and_then(|value| value.parse::<i64>().ok()).map({name}).ok_or_else(|| format!(\"invalid {} value `{{text}}`\")),",
            item.name, item.name
        );
    } else {
        let _ = writeln!(
            out,
            "        _ => Err(format!(\"invalid {} value `{{text}}`\")),",
            item.name
        );
    }
    out.push_str("    }\n}\n\n");
}

fn render_type_parser(out: &mut String, ty: &CodegenType, model: &CodegenModel) {
    let name = pascal(&ty.name);
    let function = format!("parse_{}", snake(&ty.name));
    let _ = writeln!(out, "fn {function}(value: &Value) -> Result<{name}, String> {{\n    let object = object(value)?;\n    Ok({name} {{");
    if !ty.is_abstract {
        out.push_str("        id: string(field(object, \"id\")?)?,\n");
    }
    for field_meta in &ty.all_fields {
        let field_name = rust_ident(&snake(&field_meta.name));
        let value = format!("field(object, {:?})?", field_meta.name);
        let mut expression = parse_expression(&field_meta.value_type, &value, model);
        if field_meta.is_localized {
            expression = format!("Localized::new({expression})");
        }
        let _ = writeln!(out, "        {field_name}: {expression},");
    }
    out.push_str("    })\n}\n\n");
}

fn render_polymorphic_parser(out: &mut String, ty: &CodegenType) {
    let name = pascal(&ty.name);
    let function = format!("parse_{}_value", snake(&ty.name));
    let _ = writeln!(out, "fn {function}(value: &Value) -> Result<{name}Value, String> {{\n    let object = object(value)?;\n    let actual = string(field(object, \"$type\")?)?;\n    match actual.as_str() {{");
    for actual in &ty.concrete_types {
        let actual_name = pascal(actual);
        let _ = writeln!(
            out,
            "        \"{actual}\" => Ok({name}Value::{actual_name}(parse_{}(value)?)),",
            snake(actual)
        );
    }
    out.push_str(
        "        _ => Err(format!(\"unknown polymorphic type `{actual}`\")),\n    }\n}\n\n",
    );
}

fn parse_expression(value_type: &CftValueType, value: &str, model: &CodegenModel) -> String {
    match value_type {
        CftValueType::Int => format!("integer({value})?"),
        CftValueType::Float => format!("number({value})?"),
        CftValueType::Bool => format!("boolean({value})?"),
        CftValueType::String => format!("string({value})?"),
        CftValueType::Enum(name) => format!("parse_{}({value})?", snake(name)),
        CftValueType::RecordRef(name) => {
            format!("RecordId::<{}>::new(string({value})?)", pascal(name))
        }
        CftValueType::Object(name) => model.type_by_name(name).map_or_else(
            || format!("parse_{}({value})?", snake(name)),
            |ty| {
                if ty.concrete_types.len() > 1 {
                    format!("parse_{}_value({value})?", snake(name))
                } else {
                    format!("parse_{}({value})?", snake(name))
                }
            },
        ),
        CftValueType::Array(inner) => {
            let item = parse_expression(inner, "item", model);
            format!("array({value})?.iter().map(|item| Ok({item})).collect::<Result<Vec<_>, String>>()?")
        }
        CftValueType::Dict(key, inner) => {
            let key = parse_dict_key_expression(key, "key");
            let item = parse_expression(inner, "item", model);
            format!("object({value})?.iter().map(|(key, item)| Ok(({key}, {item}))).collect::<Result<BTreeMap<_, _>, String>>()?")
        }
        CftValueType::Nullable(inner) => {
            let inner = parse_expression(inner, "value", model);
            format!("if {value}.is_null() {{ None }} else {{ let value = {value}; Some({inner}) }}")
        }
    }
}

fn parse_dict_key_expression(value_type: &CftValueType, value: &str) -> String {
    match value_type.non_nullable() {
        CftValueType::String => format!("{value}.clone()"),
        CftValueType::Int => format!("{value}.parse::<i64>().map_err(|_| format!(\"invalid integer dictionary key `{{{value}}}`\"))?"),
        CftValueType::Enum(name) => format!("parse_{}(&Value::String({value}.clone()))?", snake(name)),
        _ => format!("return Err(\"unsupported dictionary key type\".to_string())"),
    }
}

fn render_protobuf_loader(model: &CodegenModel) -> String {
    let mut out = String::from(
        "// Generated by Coflow. Do not edit.\n\
         use super::types::*;\n\
         use std::collections::BTreeMap;\n\
         use std::fs;\n\
         use std::path::Path;\n\n\
         #[derive(Clone, Copy)]\n\
         enum WireValue<'a> { Varint(u64), Fixed64(u64), Bytes(&'a [u8]) }\n\n\
         fn read_varint(input: &[u8], offset: &mut usize) -> Result<u64, String> {\n\
             let mut value = 0u64; let mut shift = 0u32;\n\
             while *offset < input.len() && shift < 70 { let byte = input[*offset]; *offset += 1; value |= u64::from(byte & 0x7f) << shift; if byte < 0x80 { return Ok(value); } shift += 7; }\n\
             Err(\"invalid Protobuf varint\".to_string())\n\
         }\n\n\
         fn decode(input: &[u8]) -> Result<Vec<(u32, WireValue<'_>)>, String> {\n\
             let mut fields = Vec::new(); let mut offset = 0usize;\n\
             while offset < input.len() {\n\
                 let key = read_varint(input, &mut offset)?; let tag = u32::try_from(key >> 3).map_err(|_| \"field tag overflow\".to_string())?;\n\
                 let value = match key & 7 {\n\
                     0 => WireValue::Varint(read_varint(input, &mut offset)?),\n\
                     1 => { if offset + 8 > input.len() { return Err(\"truncated fixed64\".to_string()); } let mut bytes = [0u8; 8]; bytes.copy_from_slice(&input[offset..offset + 8]); offset += 8; WireValue::Fixed64(u64::from_le_bytes(bytes)) },\n\
                     2 => { let len = usize::try_from(read_varint(input, &mut offset)?).map_err(|_| \"length overflow\".to_string())?; if offset + len > input.len() { return Err(\"truncated bytes\".to_string()); } let bytes = &input[offset..offset + len]; offset += len; WireValue::Bytes(bytes) },\n\
                     wire => return Err(format!(\"unsupported Protobuf wire type {wire}\")),\n\
                 }; fields.push((tag, value));\n\
             }\n\
             Ok(fields)\n\
         }\n\n\
         fn required<'a>(message: &'a [(u32, WireValue<'a>)], tag: u32) -> Result<&'a WireValue<'a>, String> { message.iter().find(|(field, _)| *field == tag).map(|(_, value)| value).ok_or_else(|| format!(\"missing Protobuf field {tag}\")) }\n\
         fn optional<'a>(message: &'a [(u32, WireValue<'a>)], tag: u32) -> Option<&'a WireValue<'a>> { message.iter().find(|(field, _)| *field == tag).map(|(_, value)| value) }\n\
         fn values<'a>(message: &'a [(u32, WireValue<'a>)], tag: u32) -> impl Iterator<Item = &'a WireValue<'a>> { message.iter().filter(move |(field, _)| *field == tag).map(|(_, value)| value) }\n\
         fn bytes<'a>(value: &'a WireValue<'a>) -> Result<&'a [u8], String> { if let WireValue::Bytes(value) = value { Ok(value) } else { Err(\"expected length-delimited field\".to_string()) } }\n\
         fn text(value: &WireValue<'_>) -> Result<String, String> { std::str::from_utf8(bytes(value)?).map(str::to_string).map_err(|error| error.to_string()) }\n\
         fn sint(value: &WireValue<'_>) -> Result<i64, String> { if let WireValue::Varint(value) = value { Ok(((value >> 1) as i64) ^ (-((value & 1) as i64))) } else { Err(\"expected varint\".to_string()) } }\n\
         fn boolean(value: &WireValue<'_>) -> Result<bool, String> { if let WireValue::Varint(value) = value { Ok(*value != 0) } else { Err(\"expected varint\".to_string()) } }\n\
         fn number(value: &WireValue<'_>) -> Result<f64, String> { if let WireValue::Fixed64(value) = value { Ok(f64::from_bits(*value)) } else { Err(\"expected fixed64\".to_string()) } }\n\n",
    );
    for item in &model.enums {
        let name = pascal(&item.name);
        let function = format!("decode_{}", snake(&item.name));
        let _ = writeln!(
            out,
            "fn {function}(value: i64) -> Result<{name}, String> {{"
        );
        if item.is_flags {
            let _ = writeln!(out, "    Ok({name}(value))");
        } else {
            out.push_str("    match value {\n");
            for variant in &item.variants {
                let _ = writeln!(
                    out,
                    "        {} => Ok({name}::{}),",
                    variant.value,
                    pascal(&variant.name)
                );
            }
            let _ = writeln!(
                out,
                "        _ => Err(format!(\"unknown {name} value {{value}}\")),\n    }}"
            );
        }
        out.push_str("}\n\n");
    }
    for ty in model.types.iter().filter(|ty| !ty.is_abstract) {
        render_protobuf_type_parser(&mut out, ty, model);
    }
    for ty in model.types.iter().filter(|ty| ty.concrete_types.len() > 1) {
        let name = pascal(&ty.name);
        let function = format!("decode_{}_value", snake(&ty.name));
        let _ = writeln!(out, "fn {function}(input: &[u8]) -> Result<{name}Value, String> {{\n    let message = decode(input)?;");
        for (index, actual) in ty.concrete_types.iter().enumerate() {
            let actual_name = pascal(actual);
            let tag = index + 1;
            let _ = writeln!(out, "    if let Some(value) = optional(&message, {tag}) {{ return Ok({name}Value::{actual_name}(decode_{}(bytes(value)?)?)); }}", snake(actual));
        }
        out.push_str("    Err(\"missing polymorphic Protobuf value\".to_string())\n}\n\n");
    }
    out.push_str("pub fn load(directory: &Path) -> Result<CoflowTables, String> {\n    let mut database = CoflowTables::default();\n");
    for table in &model.table_names {
        let field = rust_ident(&snake(table));
        let parser = format!("decode_{}", snake(table));
        let _ = writeln!(out, "    for value in load_table(directory, \"{table}\")? {{ let record = {parser}(&value)?; database.{field}.insert(record.id.clone(), record); }}");
    }
    for singleton in &model.singleton_names {
        let field = rust_ident(&snake(singleton));
        let parser = format!("decode_{}", snake(singleton));
        let _ = writeln!(out, "    if let Some(value) = load_table(directory, \"{singleton}\")?.first() {{ database.{field} = Some({parser}(value)?); }}");
    }
    out.push_str("    Ok(database)\n}\n\nfn load_table(directory: &Path, name: &str) -> Result<Vec<Vec<u8>>, String> {\n    let path = directory.join(format!(\"{name}.pb\"));\n    if !path.exists() { return Ok(Vec::new()); }\n    let input = fs::read(&path).map_err(|error| format!(\"failed to read {}: {error}\", path.display()))?;\n    let envelope = decode(&input)?; values(&envelope, 1).map(|value| bytes(value).map(<[u8]>::to_vec)).collect()\n}\n");
    out
}

fn render_protobuf_type_parser(out: &mut String, ty: &CodegenType, model: &CodegenModel) {
    let name = pascal(&ty.name);
    let function = format!("decode_{}", snake(&ty.name));
    let _ = writeln!(out, "fn {function}(input: &[u8]) -> Result<{name}, String> {{\n    let message = decode(input)?;\n    Ok({name} {{");
    if !ty.is_struct {
        out.push_str("        id: text(required(&message, 1)?)?,\n");
    }
    let mut fields = ty.all_fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, field) in fields.into_iter().enumerate() {
        let tag = index + 16;
        let mut expression = protobuf_field_expression(&field.value_type, tag, model);
        if field.is_localized {
            expression = format!("Localized::new({expression})");
        }
        let _ = writeln!(
            out,
            "        {}: {expression},",
            rust_ident(&snake(&field.name))
        );
    }
    out.push_str("    })\n}\n\n");
}

fn protobuf_field_expression(
    value_type: &CftValueType,
    tag: usize,
    model: &CodegenModel,
) -> String {
    match value_type {
        CftValueType::Nullable(inner) => {
            let value = protobuf_singular_expression(inner, "value", model);
            format!("optional(&message, {tag}).map(|value| {value}).transpose()?")
        }
        CftValueType::Array(inner) => protobuf_array_expression(inner, "&message", tag, model),
        CftValueType::Dict(key, value) => {
            protobuf_dict_expression(key, value, "&message", tag, model)
        }
        other => protobuf_singular_expression(other, &format!("required(&message, {tag})?"), model),
    }
}

fn protobuf_singular_expression(
    value_type: &CftValueType,
    value: &str,
    model: &CodegenModel,
) -> String {
    match value_type {
        CftValueType::Int => format!("sint({value})?"),
        CftValueType::Float => format!("number({value})?"),
        CftValueType::Bool => format!("boolean({value})?"),
        CftValueType::String => format!("text({value})?"),
        CftValueType::Enum(name) => format!("decode_{}(sint({value})?)?", snake(name)),
        CftValueType::RecordRef(name) => {
            format!("RecordId::<{}>::new(text({value})?)", pascal(name))
        }
        CftValueType::Object(name) => model.type_by_name(name).map_or_else(
            || format!("decode_{}(bytes({value})?)?", snake(name)),
            |ty| {
                if ty.concrete_types.len() > 1 {
                    format!("decode_{}_value(bytes({value})?)?", snake(name))
                } else {
                    format!("decode_{}(bytes({value})?)?", snake(name))
                }
            },
        ),
        CftValueType::Nullable(inner) => {
            let nested = protobuf_singular_expression(inner, "nested", model);
            format!("{{ let wrapper = decode(bytes({value})?)?; optional(&wrapper, 1).map(|nested| {nested}).transpose()? }}")
        }
        CftValueType::Array(inner) => {
            let items = protobuf_array_expression(inner, "&wrapper", 1, model);
            format!("{{ let wrapper = decode(bytes({value})?)?; {items} }}")
        }
        CftValueType::Dict(key, inner) => {
            let entries = protobuf_dict_expression(key, inner, "&wrapper", 1, model);
            format!("{{ let wrapper = decode(bytes({value})?)?; {entries} }}")
        }
    }
}

fn protobuf_array_expression(
    inner: &CftValueType,
    message: &str,
    tag: usize,
    model: &CodegenModel,
) -> String {
    let item = protobuf_singular_expression(inner, "value", model);
    format!("values({message}, {tag}).map(|value| Ok({item})).collect::<Result<Vec<_>, String>>()?")
}

fn protobuf_dict_expression(
    key: &CftValueType,
    value: &CftValueType,
    message: &str,
    tag: usize,
    model: &CodegenModel,
) -> String {
    let key = protobuf_singular_expression(key.non_nullable(), "required(&entry, 1)?", model);
    let value = protobuf_singular_expression(value, "required(&entry, 2)?", model);
    format!("values({message}, {tag}).map(|wire| {{ let entry = decode(bytes(wire)?)?; Ok(({key}, {value})) }}).collect::<Result<BTreeMap<_, _>, String>>()?")
}

fn pascal(value: &str) -> String {
    words(value)
        .into_iter()
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().collect::<String>() + chars.as_str()
            })
        })
        .collect()
}

fn snake(value: &str) -> String {
    words(value).join("_").to_lowercase()
}

fn upper_snake(value: &str) -> String {
    snake(value).to_uppercase()
}

fn words(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for character in value.chars() {
        if matches!(character, '_' | '-' | ' ') {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
        } else if character.is_uppercase() && !current.is_empty() {
            words.push(std::mem::take(&mut current));
            current.push(character);
        } else {
            current.push(character);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn rust_ident(value: &str) -> String {
    if matches!(
        value,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
    ) {
        format!("r#{value}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{render_protobuf_loader, render_types, CodegenModel};
    use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};

    #[test]
    fn generated_rust_and_protobuf_loader_are_valid_syntax() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "enum Rarity { Common = 0, Rare = 2, } type Item { rarity: Rarity; tags: [string]; attrs: {string: int}; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
        let model = CodegenModel::build(&schema, None, &serde_json::Value::Null).expect("model");

        syn::parse_file(&render_types(&model)).expect("generated types syntax");
        syn::parse_file(&render_protobuf_loader(&model)).expect("generated loader syntax");
    }
}
