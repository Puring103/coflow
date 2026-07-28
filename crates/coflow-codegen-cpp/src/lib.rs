//! Conservative C++11 code generator and JSON loader generator.

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
use std::collections::BTreeSet;
use std::fmt::Write;

pub const CPP_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "cpp",
    display_name: "C++11",
    language: "cpp",
    file_extensions: &["h", "cpp"],
    needs_model_for_build: false,
};

pub const CPP_JSON_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "cpp-json",
    code: "cpp",
    data: "json",
};

pub const CPP_PROTOBUF_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "cpp-protobuf",
    code: "cpp",
    data: "protobuf",
};

#[derive(Debug, Default, Clone, Copy)]
pub struct CppCodeGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct CppJsonLoaderGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct CppProtobufLoaderGenerator;

#[derive(Debug)]
struct EmptyOptions;

/// Declares C++ codegen and compatible loader roles.
///
/// # Errors
///
/// Returns an error when a role id conflicts within the bundle.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_codegen(CppCodeGenerator)?;
    bundle.add_loader(CppJsonLoaderGenerator)?;
    bundle.add_loader(CppProtobufLoaderGenerator)?;
    Ok(bundle)
}

impl LoaderGenerator for CppProtobufLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CPP_PROTOBUF_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("cpp-protobuf", options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("cpp-protobuf")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        if !model.dimensions.is_empty() {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "CPP-PROTOBUF-LOCALIZATION",
                "CODEGEN",
                "C++ Protobuf loader does not yet support localized dimension tables",
            )));
        }
        artifacts(vec![ArtifactFile::text(
            "CoflowProtobufLoader.h",
            render_protobuf_loader(&model),
        )])
    }
}

impl CodeGenerator for CppCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CPP_CODEGEN_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("cpp", options)
    }

    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("cpp")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        artifacts(vec![ArtifactFile::text(
            "CoflowTypes.h",
            render_types(&model),
        )])
    }
}

impl LoaderGenerator for CppJsonLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CPP_JSON_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_empty_options("cpp-json", options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<EmptyOptions>("cpp-json")?;
        let model = CodegenModel::build(ctx.schema, ctx.model, ctx.id_as_enum_variants)?;
        artifacts(vec![ArtifactFile::text(
            "CoflowJsonLoader.h",
            render_json_loader(&model),
        )])
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
            "CPP-OPTIONS",
            "CODEGEN",
            format!("{id} does not accept options"),
        )))
    }
}

fn artifacts(files: Vec<ArtifactFile>) -> Result<ArtifactSet, DiagnosticSet> {
    ArtifactSet::new(files).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "CPP-ARTIFACT",
            "ARTIFACT",
            error.to_string(),
        ))
    })
}

fn render_types(model: &CodegenModel) -> String {
    let mut out = String::from(
        "// Generated by Coflow. Do not edit.\n#pragma once\n\
         #include <cstdint>\n#include <map>\n#include <memory>\n#include <string>\n#include <vector>\n\n\
         namespace coflow_generated {\n\n",
    );
    for item in &model.enums {
        render_enum(&mut out, item);
    }
    for ty in &model.types {
        let _ = writeln!(out, "struct {};", pascal(&ty.name));
    }
    out.push_str("\ntemplate <typename T> struct CoflowKey { typedef std::string type; };\n");
    for ty in &model.types {
        if let Some(id_enum) = &ty.id_as_enum {
            let _ = writeln!(
                out,
                "template <> struct CoflowKey<{}> {{ typedef {} type; }};",
                pascal(&ty.name),
                pascal(id_enum)
            );
        }
    }
    out.push_str(
        "template <typename T> struct CoflowOptional { bool has_value; T value; CoflowOptional() : has_value(false), value() {} };\n\
         template <typename T> struct CoflowRef { typename CoflowKey<T>::type id; const T* value; CoflowRef() : value(0) {} };\n\
         template <typename T> struct Localized { T default_value; };\n\
         template <typename T> struct CoflowTable { std::vector<std::unique_ptr<T> > records; std::map<typename CoflowKey<T>::type, T*> by_id; };\n\n",
    );
    for ty in ordered_cpp_types(model) {
        render_type(&mut out, ty, model);
    }
    out.push_str("struct CoflowTables {\n");
    for table in &model.table_names {
        let _ = writeln!(out, "    CoflowTable<{}> {};", pascal(table), member(table));
    }
    for singleton in &model.singleton_names {
        let _ = writeln!(
            out,
            "    std::unique_ptr<{}> {};",
            pascal(singleton),
            member(singleton)
        );
    }
    out.push_str("};\n\n} // namespace coflow_generated\n");
    out
}

fn ordered_cpp_types(model: &CodegenModel) -> Vec<&CodegenType> {
    let mut emitted = BTreeSet::new();
    let mut ordered = Vec::with_capacity(model.types.len());
    while ordered.len() < model.types.len() {
        let mut progressed = false;
        for ty in &model.types {
            if emitted.contains(&ty.name) {
                continue;
            }
            let dependencies = cpp_complete_type_dependencies(ty, model);
            if dependencies
                .iter()
                .all(|dependency| dependency == &ty.name || emitted.contains(dependency))
            {
                emitted.insert(ty.name.clone());
                ordered.push(ty);
                progressed = true;
            }
        }
        if !progressed {
            ordered.extend(
                model
                    .types
                    .iter()
                    .filter(|ty| !emitted.contains(&ty.name)),
            );
            break;
        }
    }
    ordered
}

fn cpp_complete_type_dependencies(ty: &CodegenType, model: &CodegenModel) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    if let Some(parent) = &ty.parent {
        dependencies.insert(parent.clone());
    }
    for field in &ty.own_fields {
        collect_cpp_complete_type_dependencies(&field.value_type, model, &mut dependencies);
    }
    dependencies
}

fn collect_cpp_complete_type_dependencies(
    value: &CftValueType,
    model: &CodegenModel,
    dependencies: &mut BTreeSet<String>,
) {
    match value {
        CftValueType::Object(name) => {
            if model
                .type_by_name(name)
                .is_some_and(|ty| !ty.is_polymorphic())
            {
                dependencies.insert(name.to_string());
            }
        }
        CftValueType::Array(inner) | CftValueType::Nullable(inner) => {
            collect_cpp_complete_type_dependencies(inner, model, dependencies);
        }
        CftValueType::Dict(key, value) => {
            collect_cpp_complete_type_dependencies(key, model, dependencies);
            collect_cpp_complete_type_dependencies(value, model, dependencies);
        }
        CftValueType::Int
        | CftValueType::Float
        | CftValueType::Bool
        | CftValueType::String
        | CftValueType::Enum(_)
        | CftValueType::RecordRef(_) => {}
    }
}

fn render_enum(out: &mut String, item: &CodegenEnum) {
    let name = pascal(&item.name);
    let _ = writeln!(out, "enum {name} {{");
    for variant in &item.variants {
        let _ = writeln!(
            out,
            "    {}_{} = {},",
            name,
            pascal(&variant.name),
            variant.value
        );
    }
    out.push_str("};\n\n");
}

fn render_type(out: &mut String, ty: &CodegenType, model: &CodegenModel) {
    let name = pascal(&ty.name);
    let inheritance = ty.parent.as_ref().map_or_else(String::new, |parent| {
        format!(" : public {}", pascal(parent))
    });
    let _ = writeln!(out, "struct {name}{inheritance} {{");
    if ty.parent.is_none() && !ty.is_struct {
        let id_type = ty
            .id_as_enum
            .as_ref()
            .map_or_else(|| "std::string".to_string(), |name| pascal(name));
        let _ = writeln!(out, "    {id_type} id;");
    }
    if ty.is_polymorphic() {
        let _ = writeln!(out, "    virtual ~{name}() {{}}");
    }
    for field in &ty.own_fields {
        let mut value_type = cpp_type(&field.value_type, model);
        if field.is_localized {
            value_type = format!("Localized<{value_type}> ");
        }
        let _ = writeln!(out, "    {value_type} {};", member(&field.name));
    }
    out.push_str("};\n\n");
}

fn cpp_type(value: &CftValueType, model: &CodegenModel) -> String {
    match value {
        CftValueType::Int => "std::int64_t".to_string(),
        CftValueType::Float => "double".to_string(),
        CftValueType::Bool => "bool".to_string(),
        CftValueType::String => "std::string".to_string(),
        CftValueType::Enum(name) => pascal(name),
        CftValueType::RecordRef(name) => format!("CoflowRef<{}> ", pascal(name)),
        CftValueType::Object(name) => model.type_by_name(name).map_or_else(
            || pascal(name),
            |ty| {
                if ty.is_polymorphic() {
                    format!("std::shared_ptr<{}> ", pascal(name))
                } else {
                    pascal(name)
                }
            },
        ),
        CftValueType::Array(inner) => format!("std::vector<{}> ", cpp_type(inner, model)),
        CftValueType::Dict(key, value) => format!(
            "std::map<{}, {}> ",
            cpp_type(key, model),
            cpp_type(value, model)
        ),
        CftValueType::Nullable(inner) => format!("CoflowOptional<{}> ", cpp_type(inner, model)),
    }
}

fn render_json_loader(model: &CodegenModel) -> String {
    let mut out = String::from(
        "// Generated by Coflow. Do not edit.\n#pragma once\n#include \"CoflowTypes.h\"\n\
         #include <fstream>\n#include <nlohmann/json.hpp>\n#include <sstream>\n\n\
         namespace coflow_generated { namespace json_loader {\n\n\
         typedef nlohmann::json Json;\n\
         inline bool Read(const Json& value, std::string& out) { if (!value.is_string()) return false; out = value.get<std::string>(); return true; }\n\
         inline bool Read(const Json& value, std::int64_t& out) { if (!value.is_number_integer()) return false; out = value.get<std::int64_t>(); return true; }\n\
         inline bool Read(const Json& value, double& out) { if (!value.is_number()) return false; out = value.get<double>(); return true; }\n\
         inline bool Read(const Json& value, bool& out) { if (!value.is_boolean()) return false; out = value.get<bool>(); return true; }\n\
         inline bool ReadKey(const std::string& value, std::string& out) { out = value; return true; }\n\
         inline bool ReadKey(const std::string& value, std::int64_t& out) { std::istringstream stream(value); stream >> out; return !stream.fail() && stream.eof(); }\n\
         inline bool ReadId(const Json& value, std::string& out) { return Read(value, out); }\n\
         \n",
    );
    for item in &model.enums {
        render_enum_reader(&mut out, item);
    }
    for ty in &model.types {
        let name = pascal(&ty.name);
        let _ = writeln!(out, "inline bool Read(const Json& value, CoflowRef<{name}>& out) {{ return ReadId(value, out.id); }}");
    }
    for ty in &model.types {
        let _ = writeln!(
            out,
            "inline bool Read(const Json& value, {}& out);",
            pascal(&ty.name)
        );
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        let _ = writeln!(
            out,
            "inline bool Read(const Json& value, std::shared_ptr<{}>& out);",
            pascal(&ty.name)
        );
    }
    out.push_str("\ntemplate <typename T> bool Read(const Json& value, CoflowOptional<T>& out) { if (value.is_null()) { out.has_value = false; return true; } out.has_value = true; return Read(value, out.value); }\n\
         template <typename T> bool Read(const Json& value, Localized<T>& out) { return Read(value, out.default_value); }\n\
         template <typename T> bool Read(const Json& value, std::vector<T>& out) { if (!value.is_array()) return false; out.clear(); for (Json::const_iterator it = value.begin(); it != value.end(); ++it) { T item; if (!Read(*it, item)) return false; out.push_back(item); } return true; }\n\
         template <typename K, typename V> bool Read(const Json& value, std::map<K, V>& out) { if (!value.is_object()) return false; out.clear(); for (Json::const_iterator it = value.begin(); it != value.end(); ++it) { K key; V item; if (!ReadKey(it.key(), key) || !Read(it.value(), item)) return false; out[key] = item; } return true; }\n\n");
    for ty in &model.types {
        render_type_reader(&mut out, ty);
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        render_polymorphic_reader(&mut out, ty);
    }
    render_resolution_helpers(&mut out, model);
    out.push_str("inline bool LoadFile(const std::string& path, Json& value, std::string& error) { std::ifstream input(path.c_str(), std::ios::binary); if (!input) return true; value = Json::parse(input, 0, false); if (value.is_discarded()) { error = \"invalid JSON file: \" + path; return false; } return true; }\n\n");
    out.push_str("inline bool Load(const std::string& directory, CoflowTables& database, std::string& error) {\n");
    for table in &model.table_names {
        let field = member(table);
        let ty = pascal(table);
        let _ = writeln!(out, "    {{ Json values; if (!LoadFile(directory + \"/{table}.json\", values, error)) return false; if (!values.is_null()) {{ if (!values.is_array()) return false; for (Json::const_iterator it = values.begin(); it != values.end(); ++it) {{ std::unique_ptr<{ty}> record(new {ty}()); if (!Read(*it, *record)) return false; database.{field}.by_id[record->id] = record.get(); database.{field}.records.push_back(std::move(record)); }} }} }}");
    }
    for singleton in &model.singleton_names {
        let field = member(singleton);
        let ty = pascal(singleton);
        let _ = writeln!(out, "    {{ Json values; if (!LoadFile(directory + \"/{singleton}.json\", values, error)) return false; if (values.is_array() && !values.empty()) {{ database.{field}.reset(new {ty}()); if (!Read(values[0], *database.{field})) return false; }} }}");
    }
    render_resolution_calls(&mut out, model);
    out.push_str("    return true;\n}\n\n} } // namespace coflow_generated::json_loader\n");
    out
}

fn render_enum_reader(out: &mut String, item: &CodegenEnum) {
    let name = pascal(&item.name);
    let _ = writeln!(
        out,
        "inline bool Parse{name}(const std::string& text, {name}& out) {{"
    );
    for variant in &item.variants {
        let _ = writeln!(
            out,
            "    if (text == \"{}.{}\") {{ out = {}_{}; return true; }}",
            item.name,
            variant.name,
            name,
            pascal(&variant.name)
        );
    }
    if item.is_flags {
        let _ = writeln!(out, "    const std::string prefix = \"{}(\"; if (text.size() > prefix.size() + 1 && text.compare(0, prefix.size(), prefix) == 0 && text[text.size() - 1] == ')') {{ std::istringstream stream(text.substr(prefix.size(), text.size() - prefix.size() - 1)); std::int64_t value; stream >> value; if (!stream.fail() && stream.eof()) {{ out = static_cast<{name}>(value); return true; }} }}", item.name);
    }
    out.push_str("    return false;\n}\n");
    let _ = writeln!(out, "inline bool Read(const Json& value, {name}& out) {{ if (value.is_number_integer()) {{ const std::int64_t raw = value.get<std::int64_t>();");
    if item.is_flags {
        let _ = writeln!(
            out,
            "        out = static_cast<{name}>(raw); return true; }}"
        );
    } else {
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "        if (raw == {}) {{ out = {}_{}; return true; }}",
                variant.value,
                name,
                pascal(&variant.name)
            );
        }
        out.push_str("        return false; }\n");
    }
    let _ = writeln!(
        out,
        "    return value.is_string() && Parse{name}(value.get<std::string>(), out); }}"
    );
    let _ = writeln!(out, "inline bool ReadKey(const std::string& value, {name}& out) {{ return Parse{name}(value, out); }}\n");
    if item.is_id_as_enum {
        let _ = writeln!(out, "inline bool ReadId(const Json& value, {name}& out) {{ if (!value.is_string()) return false; const std::string id = value.get<std::string>();");
        for variant in &item.variants {
            let _ = writeln!(
                out,
                "    if (id == {:?}) {{ out = {}_{}; return true; }}",
                variant.name,
                name,
                pascal(&variant.name)
            );
        }
        out.push_str("    return false;\n}\n");
    }
}

fn render_type_reader(out: &mut String, ty: &CodegenType) {
    let name = pascal(&ty.name);
    let _ = writeln!(out, "inline bool Read(const Json& value, {name}& out) {{\n    if (!value.is_object()) return false;");
    if let Some(parent) = &ty.parent {
        let _ = writeln!(
            out,
            "    if (!Read(value, static_cast<{}&>(out))) return false;",
            pascal(parent)
        );
    } else if !ty.is_struct {
        out.push_str("    Json::const_iterator id = value.find(\"id\"); if (id != value.end() && !ReadId(*id, out.id)) return false;\n");
    }
    for field in &ty.own_fields {
        let _ = writeln!(out, "    {{ Json::const_iterator field = value.find(\"{}\"); if (field == value.end() || !Read(*field, out.{})) return false; }}", field.name, member(&field.name));
    }
    out.push_str("    return true;\n}\n\n");
}

fn render_polymorphic_reader(out: &mut String, ty: &CodegenType) {
    let name = pascal(&ty.name);
    let _ = writeln!(out, "inline bool Read(const Json& value, std::shared_ptr<{name}>& out) {{\n    if (!value.is_object()) return false; Json::const_iterator tag = value.find(\"$type\"); if (tag == value.end() || !tag->is_string()) return false; const std::string actual = tag->get<std::string>();");
    for actual in &ty.concrete_types {
        let actual_name = pascal(actual);
        let _ = writeln!(out, "    if (actual == \"{actual}\") {{ std::shared_ptr<{actual_name}> item(new {actual_name}()); if (!Read(value, *item)) return false; out = item; return true; }}");
    }
    out.push_str("    return false;\n}\n\n");
}

fn render_protobuf_loader(model: &CodegenModel) -> String {
    let mut out = String::from(
        r#"// Generated by Coflow. Do not edit.
#pragma once
#include "CoflowTypes.h"
#include <cstring>
#include <fstream>
#include <iterator>
#include <limits>
#include <sstream>

namespace coflow_generated { namespace protobuf_loader {

struct WireValue { unsigned tag; unsigned wire; std::uint64_t scalar; std::string bytes; WireValue() : tag(0), wire(0), scalar(0) {} };

inline bool Varint(const std::string& input, std::size_t& offset, std::uint64_t& value) {
    value = 0; unsigned shift = 0;
    while (offset < input.size() && shift < 70) { unsigned char byte = static_cast<unsigned char>(input[offset++]); value |= static_cast<std::uint64_t>(byte & 0x7f) << shift; if (byte < 0x80) return true; shift += 7; }
    return false;
}

inline bool Decode(const std::string& input, std::vector<WireValue>& fields) {
    fields.clear(); std::size_t offset = 0;
    while (offset < input.size()) {
        std::uint64_t key = 0; if (!Varint(input, offset, key)) return false;
        WireValue field; field.tag = static_cast<unsigned>(key >> 3); field.wire = static_cast<unsigned>(key & 7);
        if (field.wire == 0) { if (!Varint(input, offset, field.scalar)) return false; }
        else if (field.wire == 1) { if (offset + 8 > input.size()) return false; std::memcpy(&field.scalar, input.data() + offset, 8); offset += 8; }
        else if (field.wire == 2) { std::uint64_t length = 0; if (!Varint(input, offset, length) || length > input.size() - offset) return false; field.bytes.assign(input.data() + offset, static_cast<std::size_t>(length)); offset += static_cast<std::size_t>(length); }
        else return false;
        fields.push_back(field);
    }
    return true;
}

inline const WireValue* Find(const std::vector<WireValue>& fields, unsigned tag) { for (std::size_t i = 0; i < fields.size(); ++i) if (fields[i].tag == tag) return &fields[i]; return 0; }
inline bool Read(const WireValue& wire, std::string& value) { if (wire.wire != 2) return false; value = wire.bytes; return true; }
inline bool Read(const WireValue& wire, std::int64_t& value) { if (wire.wire != 0) return false; value = static_cast<std::int64_t>((wire.scalar >> 1) ^ (0 - (wire.scalar & 1))); return true; }
inline bool Read(const WireValue& wire, double& value) { if (wire.wire != 1) return false; std::memcpy(&value, &wire.scalar, 8); return true; }
inline bool Read(const WireValue& wire, bool& value) { if (wire.wire != 0) return false; value = wire.scalar != 0; return true; }
inline bool ReadId(const WireValue& wire, std::string& value) { return Read(wire, value); }

"#,
    );
    for item in &model.enums {
        let name = pascal(&item.name);
        let _ = writeln!(out, "inline bool Read(const WireValue& wire, {name}& value) {{ std::int64_t raw = 0; if (!Read(wire, raw)) return false;");
        if item.is_flags {
            let _ = writeln!(out, "    value = static_cast<{name}>(raw); return true; }}");
        } else {
            out.push_str("    switch (raw) {\n");
            for variant in &item.variants {
                let _ = writeln!(
                    out,
                    "    case {}: value = {}_{}; return true;",
                    variant.value,
                    name,
                    pascal(&variant.name)
                );
            }
            out.push_str("    default: return false; }\n}\n");
        }
        if item.is_id_as_enum {
            let _ = writeln!(out, "inline bool ReadId(const WireValue& wire, {name}& value) {{ std::string id; if (!Read(wire, id)) return false;");
            for variant in &item.variants {
                let _ = writeln!(
                    out,
                    "    if (id == {:?}) {{ value = {}_{}; return true; }}",
                    variant.name,
                    name,
                    pascal(&variant.name)
                );
            }
            out.push_str("    return false;\n}\n");
        }
    }
    for ty in &model.types {
        let name = pascal(&ty.name);
        let _ = writeln!(out, "inline bool Read(const WireValue& wire, CoflowRef<{name}>& value) {{ return ReadId(wire, value.id); }}");
    }
    for ty in model.types.iter().filter(|ty| !ty.is_abstract) {
        let _ = writeln!(
            out,
            "inline bool ReadMessage(const std::string& input, {}& value);",
            pascal(&ty.name)
        );
        let _ = writeln!(out, "inline bool Read(const WireValue& wire, {}& value) {{ return wire.wire == 2 && ReadMessage(wire.bytes, value); }}", pascal(&ty.name));
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        let _ = writeln!(
            out,
            "inline bool Read(const WireValue& wire, std::shared_ptr<{}>& value);",
            pascal(&ty.name)
        );
    }
    out.push_str("\ntemplate <typename T> bool Read(const WireValue& wire, CoflowOptional<T>& value) { if (wire.wire != 2) return false; std::vector<WireValue> fields; if (!Decode(wire.bytes, fields)) return false; const WireValue* inner = Find(fields, 1); value.has_value = inner != 0; return inner == 0 || Read(*inner, value.value); }\n\
         template <typename T> bool Read(const WireValue& wire, std::vector<T>& value) { if (wire.wire != 2) return false; std::vector<WireValue> fields; if (!Decode(wire.bytes, fields)) return false; value.clear(); for (std::size_t i = 0; i < fields.size(); ++i) if (fields[i].tag == 1) { T item; if (!Read(fields[i], item)) return false; value.push_back(item); } return true; }\n\
         template <typename K, typename V> bool Read(const WireValue& wire, std::map<K, V>& value) { if (wire.wire != 2) return false; std::vector<WireValue> fields; if (!Decode(wire.bytes, fields)) return false; value.clear(); for (std::size_t i = 0; i < fields.size(); ++i) if (fields[i].tag == 1) { std::vector<WireValue> entry; if (!Decode(fields[i].bytes, entry)) return false; const WireValue* key = Find(entry, 1); const WireValue* item = Find(entry, 2); K decoded_key; V decoded_value; if (!key || !item || !Read(*key, decoded_key) || !Read(*item, decoded_value)) return false; value[decoded_key] = decoded_value; } return true; }\n\n");
    for ty in model.types.iter().filter(|ty| !ty.is_abstract) {
        render_protobuf_type_reader(&mut out, ty, model);
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        let base = pascal(&ty.name);
        let _ = writeln!(out, "inline bool Read(const WireValue& wire, std::shared_ptr<{base}>& value) {{ if (wire.wire != 2) return false; std::vector<WireValue> fields; if (!Decode(wire.bytes, fields)) return false;");
        for (index, actual) in ty.concrete_types.iter().enumerate() {
            let actual_name = pascal(actual);
            let tag = index + 1;
            let _ = writeln!(out, "    if (const WireValue* item = Find(fields, {tag})) {{ std::shared_ptr<{actual_name}> result(new {actual_name}()); if (!Read(*item, *result)) return false; value = result; return true; }}");
        }
        out.push_str("    return false;\n}\n\n");
    }
    render_resolution_helpers(&mut out, model);
    out.push_str("inline bool LoadFile(const std::string& path, std::vector<WireValue>& records, std::string& error) { std::ifstream input(path.c_str(), std::ios::binary); if (!input) { records.clear(); return true; } std::string bytes((std::istreambuf_iterator<char>(input)), std::istreambuf_iterator<char>()); if (!Decode(bytes, records)) { error = \"invalid Protobuf file: \" + path; return false; } return true; }\n\ninline bool Load(const std::string& directory, CoflowTables& database, std::string& error) {\n");
    for table in &model.table_names {
        let field = member(table);
        let name = pascal(table);
        let _ = writeln!(out, "    {{ std::vector<WireValue> records; if (!LoadFile(directory + \"/{table}.pb\", records, error)) return false; for (std::size_t i = 0; i < records.size(); ++i) if (records[i].tag == 1) {{ std::unique_ptr<{name}> record(new {name}()); if (!Read(records[i], *record)) return false; database.{field}.by_id[record->id] = record.get(); database.{field}.records.push_back(std::move(record)); }} }}");
    }
    for singleton in &model.singleton_names {
        let field = member(singleton);
        let name = pascal(singleton);
        let _ = writeln!(out, "    {{ std::vector<WireValue> records; if (!LoadFile(directory + \"/{singleton}.pb\", records, error)) return false; for (std::size_t i = 0; i < records.size(); ++i) if (records[i].tag == 1) {{ database.{field}.reset(new {name}()); if (!Read(records[i], *database.{field})) return false; break; }} }}");
    }
    render_resolution_calls(&mut out, model);
    out.push_str("    return true;\n}\n\n} } // namespace coflow_generated::protobuf_loader\n");
    out
}

fn render_protobuf_type_reader(out: &mut String, ty: &CodegenType, model: &CodegenModel) {
    let name = pascal(&ty.name);
    let _ = writeln!(out, "inline bool ReadMessage(const std::string& input, {name}& value) {{ std::vector<WireValue> fields; if (!Decode(input, fields)) return false;");
    if !ty.is_struct {
        out.push_str("    { const WireValue* field = Find(fields, 1); if (field && !ReadId(*field, value.id)) return false; }\n");
    }
    let mut fields = ty.all_fields.iter().collect::<Vec<_>>();
    fields.sort_by(|left, right| left.name.cmp(&right.name));
    for (index, field) in fields.into_iter().enumerate() {
        let tag = index + 16;
        let target = if field.is_localized {
            format!("value.{}.default_value", member(&field.name))
        } else {
            format!("value.{}", member(&field.name))
        };
        match &field.value_type {
            CftValueType::Nullable(_) => {
                let _ = writeln!(out, "    {{ const WireValue* field = Find(fields, {tag}); {target}.has_value = field != 0; if (field && !Read(*field, {target}.value)) return false; }}");
            }
            CftValueType::Array(inner) => {
                let _ = writeln!(out, "    {target}.clear(); for (std::size_t i = 0; i < fields.size(); ++i) if (fields[i].tag == {tag}) {{ {} item; if (!Read(fields[i], item)) return false; {target}.push_back(item); }}", cpp_type(inner, model));
            }
            CftValueType::Dict(_, _) => {
                let _ = writeln!(out, "    {{ WireValue wrapper; wrapper.wire = 2; for (std::size_t i = 0; i < fields.size(); ++i) if (fields[i].tag == {tag}) {{ wrapper.bytes.push_back(static_cast<char>(0x0a)); std::uint64_t length = fields[i].bytes.size(); while (length >= 0x80) {{ wrapper.bytes.push_back(static_cast<char>((length & 0x7f) | 0x80)); length >>= 7; }} wrapper.bytes.push_back(static_cast<char>(length)); wrapper.bytes += fields[i].bytes; }} if (!Read(wrapper, {target})) return false; }}");
            }
            _ => {
                let _ = writeln!(out, "    {{ const WireValue* field = Find(fields, {tag}); if (!field || !Read(*field, {target})) return false; }}");
            }
        }
    }
    out.push_str("    return true;\n}\n\n");
}

fn render_resolution_helpers(out: &mut String, model: &CodegenModel) {
    for ty in &model.types {
        let _ = writeln!(
            out,
            "inline bool Resolve(CoflowTables& database, {}& value);",
            pascal(&ty.name)
        );
        let _ = writeln!(
            out,
            "inline bool Resolve(CoflowTables& database, CoflowRef<{}>& value);",
            pascal(&ty.name)
        );
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        let _ = writeln!(
            out,
            "inline bool Resolve(CoflowTables& database, std::shared_ptr<{}>& value);",
            pascal(&ty.name)
        );
    }
    out.push_str(
        "template <typename T> inline bool Resolve(CoflowTables&, T&) { return true; }\n\
         template <typename T> inline bool Resolve(CoflowTables& database, CoflowOptional<T>& value) { return !value.has_value || Resolve(database, value.value); }\n\
         template <typename T> inline bool Resolve(CoflowTables& database, Localized<T>& value) { return Resolve(database, value.default_value); }\n\
         template <typename T> inline bool Resolve(CoflowTables& database, std::vector<T>& value) { for (typename std::vector<T>::iterator it = value.begin(); it != value.end(); ++it) if (!Resolve(database, *it)) return false; return true; }\n\
         template <typename K, typename V> inline bool Resolve(CoflowTables& database, std::map<K, V>& value) { for (typename std::map<K, V>::iterator it = value.begin(); it != value.end(); ++it) if (!Resolve(database, it->second)) return false; return true; }\n\n",
    );
    for ty in &model.types {
        let target = pascal(&ty.name);
        let _ = writeln!(
            out,
            "inline bool Resolve(CoflowTables& database, CoflowRef<{target}>& value) {{"
        );
        out.push_str("    (void)database;\n");
        for concrete in &ty.concrete_types {
            if model.table_names.contains(concrete) {
                let table = member(concrete);
                let concrete_name = pascal(concrete);
                let _ = writeln!(out, "    {{ std::map<CoflowKey<{concrete_name}>::type, {concrete_name}*>::const_iterator found = database.{table}.by_id.find(value.id); if (found != database.{table}.by_id.end()) {{ value.value = found->second; return true; }} }}");
            }
        }
        out.push_str("    value.value = 0; return false;\n}\n");
    }
    for ty in &model.types {
        let name = pascal(&ty.name);
        let _ = writeln!(
            out,
            "inline bool Resolve(CoflowTables& database, {name}& value) {{"
        );
        out.push_str("    (void)database; (void)value;\n");
        if let Some(parent) = &ty.parent {
            let _ = writeln!(
                out,
                "    if (!Resolve(database, static_cast<{}&>(value))) return false;",
                pascal(parent)
            );
        }
        for field in &ty.own_fields {
            let _ = writeln!(
                out,
                "    if (!Resolve(database, value.{})) return false;",
                member(&field.name)
            );
        }
        out.push_str("    return true;\n}\n");
    }
    for ty in model.types.iter().filter(|ty| ty.is_polymorphic()) {
        let base = pascal(&ty.name);
        let _ = writeln!(out, "inline bool Resolve(CoflowTables& database, std::shared_ptr<{base}>& value) {{ if (!value) return true;");
        for concrete in &ty.concrete_types {
            let actual = pascal(concrete);
            let _ = writeln!(out, "    if ({actual}* typed = dynamic_cast<{actual}*>(value.get())) return Resolve(database, *typed);");
        }
        out.push_str("    return false;\n}\n");
    }
    out.push('\n');
}

fn render_resolution_calls(out: &mut String, model: &CodegenModel) {
    for table in &model.table_names {
        let field = member(table);
        let _ = writeln!(out, "    for (std::size_t i = 0; i < database.{field}.records.size(); ++i) if (!Resolve(database, *database.{field}.records[i])) {{ error = \"failed to resolve record reference in {table}\"; return false; }}");
    }
    for singleton in &model.singleton_names {
        let field = member(singleton);
        let _ = writeln!(out, "    if (database.{field} && !Resolve(database, *database.{field})) {{ error = \"failed to resolve record reference in {singleton}\"; return false; }}");
    }
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

fn member(value: &str) -> String {
    words(value).join("_").to_lowercase()
}

fn words(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for character in value.chars() {
        if matches!(character, '_' | '-' | ' ') {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        } else if character.is_uppercase() && !word.is_empty() {
            out.push(std::mem::take(&mut word));
            word.push(character);
        } else {
            word.push(character);
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{render_json_loader, render_protobuf_loader, render_types};
    use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, ModuleId};
    use coflow_codegen_core::CodegenModel;

    #[test]
    fn generated_cpp_handles_polymorphism_typed_ids_and_resolved_refs() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "@idAsEnum(ItemId) abstract type Item {} type Weapon : Item {} enum ItemId {} @struct sealed type Details { note: string; } type Holder { item: &Item; value: Item; details: Details; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
        let variants = serde_json::json!({"ItemId": [{"name": "sword", "value": 0}]});
        let model = CodegenModel::build(&schema, None, &variants).expect("model");

        let types = render_types(&model);
        let json = render_json_loader(&model);
        let protobuf = render_protobuf_loader(&model);
        assert!(types.contains("template <> struct CoflowKey<Item> { typedef ItemId type; }"));
        assert!(types.contains("std::shared_ptr<Item>"));
        assert!(!types.contains("CoflowTable<Details>"));
        assert!(json.contains("inline bool ReadId(const Json& value, ItemId& out)"));
        assert!(json.contains("value.value = found->second; return true;"));
        assert!(json.contains("failed to resolve record reference in Holder"));
        assert!(protobuf.contains("std::shared_ptr<Item>& value"));
        assert!(protobuf.contains("inline bool ReadId(const WireValue& wire, ItemId& value)"));
    }

    #[test]
    fn generated_cpp_defines_bases_and_inline_values_before_consumers() {
        let modules = parse_modules([CftFile::from_source(
            ModuleId::from("main"),
            "abstract type ZBase {} type AChild : ZBase {} @struct sealed type ZValue { amount: int; } type AHolder { value: ZValue; }",
        )]);
        let schema = build_schema(&modules, &CftDimensionInputs::default()).expect("schema");
        let model = CodegenModel::build(&schema, None, &serde_json::Value::Null).expect("model");
        let types = render_types(&model);

        let base = types.find("struct ZBase {").expect("base definition");
        let child = types.find("struct AChild : public ZBase {").expect("child definition");
        let value = types.find("struct ZValue {").expect("value definition");
        let holder = types.find("struct AHolder {").expect("holder definition");
        assert!(base < child, "base must be defined before child");
        assert!(value < holder, "inline value must be defined before owner");
    }
}
