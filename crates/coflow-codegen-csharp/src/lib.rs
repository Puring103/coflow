//! C# code generator for Coflow runtime declarations and data loaders.

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

mod emit;
mod ir;
mod lowering;
mod model;
mod names;
mod render;

use coflow_api::{
    ArtifactContent, ArtifactFile, ArtifactSet, CodeGenerator, CodegenContext, CodegenDescriptor,
    DecodedOutputOptions, Diagnostic, DiagnosticSet, LoaderDescriptor, LoaderGenerationContext,
    LoaderGenerator, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CftSchema;
use coflow_data_model::CfdDataModel;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

pub use ir::{CsharpCodegenOptions, CsharpIdAsEnumVariant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenError {
    messages: Vec<String>,
}

impl CsharpCodegenError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            messages: vec![message.into()],
        }
    }

    fn from_messages(messages: impl IntoIterator<Item = String>) -> Self {
        Self {
            messages: messages.into_iter().collect(),
        }
    }

    fn messages(&self) -> impl Iterator<Item = &str> {
        self.messages.iter().map(String::as_str)
    }
}

impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.messages.join("\n").fmt(f)
    }
}

impl std::error::Error for CsharpCodegenError {}

fn build_csharp_project(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<model::CsharpProject, CsharpCodegenError> {
    ir::build_project(schema, options, id_as_enum_variants, non_empty_tables)
}

/// Generates format-independent C# declarations.
///
/// # Errors
///
/// Returns an error when the schema cannot be mapped to C# runtime code or a
/// template fails to render.
pub fn generate_csharp(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_common_with_id_as_enum_variants(schema, options, BTreeMap::new(), None)
}

fn generate_common_with_id_as_enum_variants(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = build_csharp_project(schema, options, id_as_enum_variants, non_empty_tables)?;
    render::render_common_project(&project)
}

fn generate_loader_with_id_as_enum_variants(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    kind: render::CsharpLoaderKind,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = build_csharp_project(schema, options, id_as_enum_variants, non_empty_tables)?;
    render::render_loader_project(&project, kind)
}

fn generate_complete(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    kind: render::CsharpLoaderKind,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = build_csharp_project(schema, options, BTreeMap::new(), None)?;
    let mut files = render::render_common_project(&project)?;
    for loader in render::render_loader_project(&project, kind)? {
        let loader_name = loader.relative_path.to_string_lossy();
        let common_name = loader_name.replace(".Loader.cs", ".cs");
        if let Some(common) = files
            .iter_mut()
            .find(|file| file.relative_path == PathBuf::from(&common_name))
        {
            merge_legacy_csharp_contents(&mut common.contents, &loader.contents)?;
        } else {
            files.push(loader);
        }
    }
    Ok(files)
}

/// Generates C# declarations and a Newtonsoft.Json loader.
///
/// # Errors
///
/// Returns an error when generation fails.
pub fn generate_csharp_json(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_complete(schema, options, render::CsharpLoaderKind::Json)
}

/// Generates C# declarations and a MessagePack loader.
///
/// # Errors
///
/// Returns an error when generation fails.
pub fn generate_csharp_messagepack(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_complete(schema, options, render::CsharpLoaderKind::MessagePack)
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpCodeGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpJsonLoaderGenerator;

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpMessagePackLoaderGenerator;

pub const CSHARP_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "csharp",
    display_name: "C#",
    language: "csharp",
    file_extensions: &["cs"],
    needs_model_for_build: true,
};

pub const CSHARP_JSON_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "csharp-json",
    code: "csharp",
    data: "json",
};

pub const CSHARP_MESSAGEPACK_LOADER_DESCRIPTOR: LoaderDescriptor = LoaderDescriptor {
    id: "csharp-messagepack",
    code: "csharp",
    data: "messagepack",
};

/// Declares the C# code and loader generator roles implemented by this package.
///
/// # Errors
///
/// Returns an error if the package declares a provider id more than once.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_codegen(CsharpCodeGenerator)?;
    bundle.add_loader(CsharpJsonLoaderGenerator)?;
    bundle.add_loader(CsharpMessagePackLoaderGenerator)?;
    Ok(bundle)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CsharpOutputOptionsConfig {
    namespace: Option<String>,
    database_class: Option<String>,
    int_32: bool,
    float_32: bool,
}

#[derive(Debug)]
struct CsharpOutputOptions {
    codegen: CsharpCodegenOptions,
}

impl CodeGenerator for CsharpCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CSHARP_CODEGEN_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        let raw = CsharpOutputOptionsConfig::deserialize(options).map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSHARP-OPTIONS",
                "CODEGEN",
                format!("invalid C# output options: {err}"),
            ))
        })?;
        let codegen = CsharpCodegenOptions::new(raw.namespace.as_deref().unwrap_or("Game.Config"))
            .with_database_class(raw.database_class.as_deref().unwrap_or("CoflowTables"))
            .with_int_32(raw.int_32)
            .with_float_32(raw.float_32);
        Ok(DecodedOutputOptions::new(
            "csharp",
            CsharpOutputOptions { codegen },
        ))
    }

    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let options = options.require::<CsharpOutputOptions>("csharp")?;
        let variants = id_as_enum_variants_from_context(ctx.id_as_enum_variants)?;
        let non_empty_tables = ctx.model.map(non_empty_tables);
        let files = generate_common_with_id_as_enum_variants(
            ctx.schema,
            &options.codegen,
            variants,
            non_empty_tables.as_ref(),
        )
        .map_err(codegen_diagnostics)?;
        generated_artifacts(files)
    }
}

impl LoaderGenerator for CsharpJsonLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CSHARP_JSON_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_loader_options(self.descriptor().id, options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<()>(self.descriptor().id)?;
        generate_loader_artifacts(ctx, render::CsharpLoaderKind::Json)
    }

    fn merge_legacy_artifacts(
        &self,
        common: ArtifactSet,
        loader: ArtifactSet,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        merge_legacy_csharp_artifacts(common, loader)
    }
}

impl LoaderGenerator for CsharpMessagePackLoaderGenerator {
    fn descriptor(&self) -> &'static LoaderDescriptor {
        &CSHARP_MESSAGEPACK_LOADER_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        decode_loader_options(self.descriptor().id, options)
    }

    fn generate(
        &self,
        ctx: LoaderGenerationContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        options.require::<()>(self.descriptor().id)?;
        generate_loader_artifacts(ctx, render::CsharpLoaderKind::MessagePack)
    }

    fn merge_legacy_artifacts(
        &self,
        common: ArtifactSet,
        loader: ArtifactSet,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        merge_legacy_csharp_artifacts(common, loader)
    }
}

fn merge_legacy_csharp_artifacts(
    common: ArtifactSet,
    loader: ArtifactSet,
) -> Result<ArtifactSet, DiagnosticSet> {
    let mut files = common.into_files();
    for loader_file in loader.into_files() {
        let loader_name = loader_file.relative_path.to_string_lossy();
        let Some(common_name) = loader_name.strip_suffix(".Loader.cs") else {
            files.push(loader_file);
            continue;
        };
        let common_path = PathBuf::from(format!("{common_name}.cs"));
        let Some(common_file) = files
            .iter_mut()
            .find(|file| file.relative_path == common_path)
        else {
            files.push(loader_file);
            continue;
        };
        match (&mut common_file.content, loader_file.content) {
            (ArtifactContent::Text(common), ArtifactContent::Text(loader)) => {
                merge_legacy_csharp_contents(common, &loader).map_err(codegen_diagnostics)?;
            }
            _ => {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CSHARP-ARTIFACT",
                    "ARTIFACT",
                    "C# loader companions must be text artifacts",
                )));
            }
        }
    }
    ArtifactSet::new(files).map_err(|error| {
        DiagnosticSet::one(Diagnostic::error(
            "CSHARP-ARTIFACT",
            "ARTIFACT",
            error.to_string(),
        ))
    })
}

fn merge_legacy_csharp_contents(
    common: &mut String,
    loader: &str,
) -> Result<(), CsharpCodegenError> {
    let namespace_start = loader.find("namespace ").ok_or_else(|| {
        CsharpCodegenError::new("C# loader companion is missing a namespace declaration")
    })?;
    let common_namespace_start = common.find("namespace ").ok_or_else(|| {
        CsharpCodegenError::new("C# common artifact is missing a namespace declaration")
    })?;
    let imports = loader[..namespace_start]
        .lines()
        .filter(|line| line.starts_with("using "))
        .filter(|line| {
            !common[..common_namespace_start]
                .lines()
                .any(|item| item == *line)
        })
        .collect::<Vec<_>>();
    if !imports.is_empty() {
        let mut block = imports.join("\n");
        block.push('\n');
        common.insert_str(common_namespace_start, &block);
    }
    common.push('\n');
    common.push_str(&loader[namespace_start..]);
    Ok(())
}

fn decode_loader_options(
    id: &'static str,
    options: &serde_json::Value,
) -> Result<DecodedOutputOptions, DiagnosticSet> {
    let Some(options) = options.as_object() else {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "CSHARP-LOADER-OPTIONS",
            "CODEGEN",
            "loader options must be an object",
        )));
    };
    if let Some(option) = options.keys().next() {
        return Err(DiagnosticSet::one(Diagnostic::error(
            "CSHARP-LOADER-OPTIONS",
            "CODEGEN",
            format!("unknown {id} loader option `{option}`"),
        )));
    }
    Ok(DecodedOutputOptions::new(id, ()))
}

fn generate_loader_artifacts(
    ctx: LoaderGenerationContext<'_>,
    kind: render::CsharpLoaderKind,
) -> Result<ArtifactSet, DiagnosticSet> {
    let options = ctx.code_options.require::<CsharpOutputOptions>("csharp")?;
    let variants = id_as_enum_variants_from_context(ctx.id_as_enum_variants)?;
    let non_empty_tables = ctx.model.map(non_empty_tables);
    let files = generate_loader_with_id_as_enum_variants(
        ctx.schema,
        &options.codegen,
        kind,
        variants,
        non_empty_tables.as_ref(),
    )
    .map_err(codegen_diagnostics)?;
    generated_artifacts(files)
}

fn non_empty_tables(model: &CfdDataModel) -> BTreeSet<String> {
    model
        .tables()
        .filter(|(_, table)| !table.records.is_empty())
        .map(|(name, _)| name.to_string())
        .collect()
}

fn generated_artifacts(files: Vec<GeneratedFile>) -> Result<ArtifactSet, DiagnosticSet> {
    ArtifactSet::new(
        files
            .into_iter()
            .map(|file| ArtifactFile::text(file.relative_path, file.contents))
            .collect(),
    )
    .map_err(|err| {
        DiagnosticSet::one(Diagnostic::error(
            "CSHARP-ARTIFACT",
            "ARTIFACT",
            err.to_string(),
        ))
    })
}

fn codegen_diagnostics(error: CsharpCodegenError) -> DiagnosticSet {
    DiagnosticSet {
        diagnostics: error
            .messages()
            .map(|message| Diagnostic::error("CODEGEN-CSHARP-001", "CODEGEN", message))
            .collect(),
    }
}

fn id_as_enum_variants_from_context(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<CsharpIdAsEnumVariant>>, DiagnosticSet> {
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_value(value.clone()).map_err(|err| {
        DiagnosticSet::one(Diagnostic::error(
            "CSHARP-OPTIONS",
            "CODEGEN",
            format!("invalid generated id_as_enum_variants: {err}"),
        ))
    })
}

#[cfg(test)]
mod tests;
