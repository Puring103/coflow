use crate::model::{CsharpAnnotation, CsharpProject, CsharpType};
use crate::{CsharpCodegenError, GeneratedFile};
use serde::Serialize;
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const DIMENSIONS_TEMPLATE: &str = include_str!("../templates/dimensions.cs.tera");
const METADATA_TEMPLATE: &str = include_str!("../templates/metadata.cs.tera");
const METADATA_HELPERS_TEMPLATE: &str = include_str!("../templates/metadata-helpers.cs.tera");

#[derive(Serialize)]
struct MetadataProject {
    namespace: String,
    delegate_adapters: Vec<String>,
    enums: Vec<MetadataEnum>,
    types: Vec<MetadataType>,
    constants: Vec<MetadataConstant>,
    object_factories: Vec<MetadataObjectFactory>,
    readers: Vec<MetadataReader>,
    polymorphic_readers: Vec<MetadataPolymorphicReader>,
    dimensions: Vec<crate::model::CsharpDimension>,
}

#[derive(Serialize)]
struct MetadataConstant {
    source_name: String,
    runtime_type: String,
    value: String,
}

#[derive(Serialize)]
struct MetadataEnum {
    metadata_name: String,
    source_name: String,
    qualified_name: String,
    annotations: String,
    is_flags: bool,
    declared_mask: i64,
    variants: Vec<MetadataEnumVariant>,
}

#[derive(Serialize)]
struct MetadataEnumVariant {
    name: String,
    source_name: String,
    annotations: String,
}

#[derive(Serialize)]
struct MetadataType {
    metadata_name: String,
    source_name: String,
    qualified_name: String,
    key_type: String,
    annotations: String,
    is_singleton: bool,
    is_host: bool,
    is_abstract: bool,
    is_sealed: bool,
    is_record: bool,
    can_create_record: bool,
    assignable_types: Vec<String>,
    fields: Vec<MetadataField>,
    parse_key: String,
    create_host: String,
}

#[derive(Serialize)]
struct MetadataField {
    source_name: String,
    runtime_type: String,
    access: String,
    is_enum: bool,
    annotations: String,
    has_default: bool,
    object_type: Option<String>,
    reference_type: Option<String>,
}

#[derive(Serialize)]
struct MetadataObjectFactory {
    metadata_name: String,
    source_name: String,
    qualified_name: String,
    invalid: bool,
    needs_key: bool,
    arguments: Vec<String>,
    vm_parameters: Vec<String>,
    vm_arguments: Vec<String>,
    defaults: Vec<MetadataDefaultFactory>,
}

#[derive(Serialize)]
struct MetadataDefaultFactory {
    source_name: String,
    runtime_type: String,
    expression: String,
}

#[derive(Serialize)]
struct MetadataReader {
    metadata_name: String,
    source_name: String,
    qualified_name: String,
    is_host: bool,
    expected_fields: Vec<String>,
    constructor_arguments: Vec<String>,
    populate_id: Option<String>,
    populate_assignments: Vec<String>,
    host_constructor_arguments: Vec<String>,
    variants: Vec<MetadataVariant>,
}

#[derive(Serialize)]
struct MetadataPolymorphicReader {
    metadata_name: String,
    source_name: String,
    qualified_name: String,
    variants: Vec<MetadataVariant>,
}

#[derive(Serialize)]
struct MetadataVariant {
    source_name: String,
    reader_name: String,
}

pub fn render_common_project(
    project: &CsharpProject,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let tera = templates()?;
    let mut files = Vec::new();
    for schema_enum in &project.enums {
        let mut context = Context::new();
        context.insert("namespace", &schema_enum.namespace);
        context.insert("enum", schema_enum);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(&schema_enum.relative_path),
            contents: render(&tera, "enum.cs.tera", &context)?,
        });
    }
    for schema_type in &project.types {
        let mut context = Context::new();
        context.insert("project", project);
        context.insert("type", schema_type);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(&schema_type.relative_path),
            contents: render(&tera, "type.cs.tera", &context)?,
        });
    }
    if !project.dimensions.is_empty() {
        let mut context = Context::new();
        context.insert("project", project);
        files.push(GeneratedFile {
            relative_path: PathBuf::from("Dimensions.cs"),
            contents: render(&tera, "dimensions.cs.tera", &context)?,
        });
    }
    Ok(files)
}

pub fn render_cfd_metadata_template(
    project: &CsharpProject,
) -> Result<String, CsharpCodegenError> {
    let view = metadata_project(project);
    let mut context = Context::new();
    context.insert("metadata", &view);
    context.insert("namespace", &view.namespace);
    context.insert("enums", &view.enums);
    context.insert("types", &view.types);
    context.insert("constants", &view.constants);
    context.insert("object_factories", &view.object_factories);
    context.insert("readers", &view.readers);
    context.insert("polymorphic_readers", &view.polymorphic_readers);
    context.insert("dimensions", &view.dimensions);
    render(&templates()?, "metadata.cs.tera", &context)
}

fn metadata_project(project: &CsharpProject) -> MetadataProject {
    MetadataProject {
        namespace: project.namespace.clone(),
        delegate_adapters: project.delegate_adapters.clone(),
        enums: project.enums.iter().map(metadata_enum).collect(),
        types: project.types.iter().filter(|ty| ty.loader_enabled)
            .map(|ty| metadata_type(project, ty)).collect(),
        constants: project.constants.iter().map(|constant| MetadataConstant {
            source_name: escape_csharp_string(&constant.source_name),
            runtime_type: constant.runtime_type.clone(),
            value: if constant.deferred {
                format!("static context => {}", constant.value_expression)
            } else {
                constant.value_expression.clone()
            },
        }).collect(),
        object_factories: project.types.iter().map(metadata_object_factory).collect(),
        readers: project.types.iter().filter(|ty| ty.loader_enabled)
            .map(metadata_reader).collect(),
        polymorphic_readers: project.types.iter()
            .filter(|ty| !ty.loader_enabled && !ty.loader_variants.is_empty())
            .map(|ty| MetadataPolymorphicReader {
                metadata_name: ty.metadata_name.clone(),
                source_name: escape_csharp_string(&ty.source_name),
                qualified_name: ty.qualified_name.clone(),
                variants: metadata_variants(ty),
            }).collect(),
        dimensions: project.dimensions.clone(),
    }
}

fn metadata_enum(schema_enum: &crate::model::CsharpEnum) -> MetadataEnum {
    MetadataEnum {
        metadata_name: schema_enum.metadata_name.clone(),
        source_name: escape_csharp_string(&schema_enum.source_name),
        qualified_name: schema_enum.qualified_name.clone(),
        annotations: render_annotations(&schema_enum.annotations),
        is_flags: schema_enum.is_flags,
        declared_mask: schema_enum.variants.iter().fold(0, |mask, variant| mask | variant.value),
        variants: schema_enum.variants.iter().map(|variant| MetadataEnumVariant {
            name: variant.name.clone(),
            source_name: escape_csharp_string(&variant.source_name),
            annotations: render_annotations(&variant.annotations),
        }).collect(),
    }
}

fn metadata_type(project: &CsharpProject, ty: &CsharpType) -> MetadataType {
    let singleton = project.singletons.iter().any(|item| item.source_name == ty.source_name);
    let key_type = ty.loader_id_type.as_deref().unwrap_or("string");
    let parse_key = if key_type == "string" {
        "key".to_string()
    } else {
        let reader = project.enums.iter()
            .find(|item| item.qualified_name == key_type)
            .map_or(ty.metadata_name.as_str(), |item| item.metadata_name.as_str());
        format!("ReadEnum{reader}Text(key)")
    };
    MetadataType {
        metadata_name: ty.metadata_name.clone(),
        source_name: escape_csharp_string(&ty.source_name),
        qualified_name: ty.qualified_name.clone(),
        key_type: key_type.to_string(),
        annotations: render_annotations(&ty.annotations),
        is_singleton: singleton,
        is_host: ty.is_host,
        is_abstract: ty.is_abstract,
        is_sealed: ty.is_sealed,
        is_record: ty.loader_id_type.is_some(),
        can_create_record: !ty.is_host && !ty.is_struct && !ty.is_abstract,
        assignable_types: ty.loader_assignable_to.iter()
            .map(|value| escape_csharp_string(value)).collect(),
        fields: ty.loader_fields.iter().map(|field| {
            let function = ty.functions.iter().find(|item| item.source_name == field.source_name);
            MetadataField {
                source_name: escape_csharp_string(&field.source_name),
                runtime_type: if field.is_function { "CoflowFunctionEntry".to_string() }
                    else { field.value_type.clone() },
                access: function.map_or_else(|| field.property_name.clone(),
                    |item| format!("{}.RuntimeEntry", item.entry_name)),
                is_enum: project.enums.iter().any(|item| item.qualified_name == field.value_type),
                annotations: render_annotations(&field.annotations),
                has_default: field.default_expression.is_some(),
                object_type: field.object_type.as_deref().map(escape_csharp_string),
                reference_type: field.reference_type.as_deref().map(escape_csharp_string),
            }
        }).collect(),
        parse_key,
        create_host: if ty.is_host { format!("CreateHost{}(context)", ty.metadata_name) }
            else { "throw new InvalidOperationException(\"The generated type is not @Host.\")".to_string() },
    }
}

fn metadata_object_factory(ty: &CsharpType) -> MetadataObjectFactory {
    let invalid = ty.is_abstract || ty.is_host;
    let mut arguments = Vec::new();
    if !invalid {
        if ty.uses_host_slot { arguments.push("null".to_string()); }
        if let Some(id_type) = &ty.loader_id_type {
            arguments.push(if id_type == "string" { "string.Empty".to_string() }
                else { format!("default({id_type})") });
        }
        arguments.extend(ty.loader_fields.iter().enumerate().map(|(index, field)| {
            if field.is_function { return "default!".to_string(); }
            let supplied = format!(
                "fields.TryGetValue(\"{}\", out var value{index}) ? ({})value{index}!",
                escape_csharp_string(&field.source_name), field.value_type);
            field.default_expression.as_ref().map_or_else(
                || format!("{supplied} : throw new ArgumentException(\"missing object field `{}`\", nameof(fields))",
                    escape_csharp_string(&field.source_name)),
                |default| format!("{supplied} : {default}"))
        }));
    }
    MetadataObjectFactory {
        metadata_name: ty.metadata_name.clone(),
        source_name: escape_csharp_string(&ty.source_name),
        qualified_name: ty.qualified_name.clone(),
        invalid,
        needs_key: ty.loader_fields.iter().filter_map(|field| field.default_expression.as_deref())
            .any(|default| default.contains("key")),
        arguments,
        vm_parameters: ty.loader_fields.iter().filter(|field| !field.is_function)
            .enumerate().map(|(index, field)| format!("{} value{index}", field.value_type)).collect(),
        vm_arguments: {
            let mut values = Vec::new();
            if ty.uses_host_slot { values.push("null".to_string()); }
            if let Some(id_type) = &ty.loader_id_type {
                values.push(if id_type == "string" { "string.Empty".to_string() }
                    else { format!("default({id_type})") });
            }
            let mut value_index = 0usize;
            values.extend(ty.loader_fields.iter().map(|field| {
                if field.is_function { "default!".to_string() } else {
                    let value = format!("value{value_index}");
                    value_index += 1;
                    value
                }
            }));
            values
        },
        defaults: ty.loader_fields.iter().filter(|field| !field.is_function)
            .filter_map(|field| field.default_expression.as_ref().map(|expression| MetadataDefaultFactory {
                source_name: escape_csharp_string(&field.source_name),
                runtime_type: field.value_type.clone(),
                expression: expression.clone(),
            })).collect(),
    }
}

fn metadata_reader(ty: &CsharpType) -> MetadataReader {
    let mut arguments = ty.loader_id_type.as_ref().map(|_| {
        ty.loader_id_reader.as_ref().map_or_else(|| "key".to_string(),
            |reader| format!("ReadEnum{reader}Text(key)"))
    }).into_iter().chain(ty.loader_fields.iter().map(|field| reader_argument(field))).collect::<Vec<_>>();
    if ty.uses_host_slot { arguments.insert(0, "null".to_string()); }
    let host_arguments = std::iter::once("context.Host()".to_string()).chain(
        ty.loader_fields.iter().filter(|field| field.is_function).map(|field| {
            field.reader_expression.replace("VALUE", "null")
                .replace("CONTEXT", "context").replace("RECORD_KEY", "string.Empty")
        })).collect();
    MetadataReader {
        metadata_name: ty.metadata_name.clone(),
        source_name: escape_csharp_string(&ty.source_name),
        qualified_name: ty.qualified_name.clone(),
        is_host: ty.is_host,
        expected_fields: ty.loader_fields.iter()
            .map(|field| escape_csharp_string(&field.source_name)).collect(),
        constructor_arguments: arguments,
        populate_id: ty.loader_id_type.as_ref().map(|_| {
            let value = ty.loader_id_reader.as_ref().map_or_else(|| "key".to_string(),
                |reader| format!("ReadEnum{reader}Text(key)"));
            format!("target.Id = {value};")
        }),
        populate_assignments: ty.loader_fields.iter().map(|field| {
            format!("target.{} = {};", assignment_target(ty, field), reader_argument(field))
        }).collect(),
        host_constructor_arguments: host_arguments,
        variants: metadata_variants(ty),
    }
}

fn assignment_target(
    ty: &CsharpType,
    field: &crate::model::CsharpLoaderField,
) -> String {
    if field.is_function {
        format!("_coflow{}", field.property_name)
    } else if ty.is_struct {
        field.property_name.clone()
    } else {
        crate::emit::backing_field_name(&field.property_name, false)
            .unwrap_or_else(|| field.property_name.clone())
    }
}

fn reader_argument(field: &crate::model::CsharpLoaderField) -> String {
    let node = if field.is_function || field.default_expression.is_some() {
        format!("CfdValueReader.FindField(fields, \"{}\")", escape_csharp_string(&field.source_name))
    } else {
        format!("CfdValueReader.Field(fields, \"{}\")", escape_csharp_string(&field.source_name))
    };
    let expression = field.reader_expression.replace("VALUE", &node)
        .replace("CONTEXT", "context").replace("RECORD_KEY", "key");
    field.default_expression.as_ref().map_or(expression, |default| {
        let value_name = format!("value{}", field.property_name);
        let value_expression = field.reader_expression.replace("VALUE", &value_name)
            .replace("CONTEXT", "context").replace("RECORD_KEY", "key");
        format!("{node} is {{ }} {value_name} ? {value_expression} : {default}")
    })
}

fn metadata_variants(ty: &CsharpType) -> Vec<MetadataVariant> {
    ty.loader_variants.iter().map(|variant| MetadataVariant {
        source_name: escape_csharp_string(&variant.source_name),
        reader_name: variant.type_name.clone(),
    }).collect()
}

fn render_annotations(annotations: &[CsharpAnnotation]) -> String {
    if annotations.is_empty() { return "Array.Empty<CoflowAnnotation>()".to_string(); }
    let values = annotations.iter().map(|annotation| {
        let arguments = annotation.arguments.iter().map(|argument| format!(
            "new CoflowAnnotationArgument(CoflowAnnotationArgumentKind.{}, {})",
            argument.kind, argument.value_expression)).collect::<Vec<_>>().join(", ");
        format!("new CoflowAnnotation(\"{}\", new CoflowAnnotationArgument[] {{ {} }})",
            escape_csharp_string(&annotation.name), arguments)
    }).collect::<Vec<_>>().join(", ");
    format!("new CoflowAnnotation[] {{ {values} }}")
}

pub(crate) fn escape_csharp_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
        .replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t")
}

fn templates() -> Result<Tera, CsharpCodegenError> {
    let mut tera = Tera::default();
    for (name, contents) in [
        ("enum.cs.tera", ENUM_TEMPLATE),
        ("type.cs.tera", TYPE_TEMPLATE),
        ("dimensions.cs.tera", DIMENSIONS_TEMPLATE),
        ("metadata.cs.tera", METADATA_TEMPLATE),
        ("metadata-helpers.cs.tera", METADATA_HELPERS_TEMPLATE),
    ] {
        tera.add_raw_template(name, contents).map_err(|error| {
            CsharpCodegenError::new(format!("failed to add C# template `{name}`: {error}"))
        })?;
    }
    Ok(tera)
}

fn render(tera: &Tera, name: &str, context: &Context) -> Result<String, CsharpCodegenError> {
    tera.render(name, context)
        .map_err(|error| CsharpCodegenError::new(format!("failed to render `{name}`: {error}")))
}
