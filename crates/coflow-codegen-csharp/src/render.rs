use crate::ir::CsharpDataFormat;
use crate::model::CsharpProject;
use crate::{CsharpCodegenError, GeneratedFile};
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const DATABASE_JSON_TEMPLATE: &str = include_str!("../templates/database_json.cs.tera");
const DATABASE_MESSAGEPACK_TEMPLATE: &str =
    include_str!("../templates/database_messagepack.cs.tera");
const DATABASE_COMMON_MEMBERS_TEMPLATE: &str =
    include_str!("../templates/database_common_members.cs.tera");
const DATABASE_COMMON_RESOLVE_TEMPLATE: &str =
    include_str!("../templates/database_common_resolve.cs.tera");
const DATABASE_COMMON_INDEXES_TEMPLATE: &str =
    include_str!("../templates/database_common_indexes.cs.tera");
const DATABASE_JSON_LOADERS_TEMPLATE: &str =
    include_str!("../templates/database_json_loaders.cs.tera");
const DATABASE_JSON_READERS_TEMPLATE: &str =
    include_str!("../templates/database_json_readers.cs.tera");
const DATABASE_MESSAGEPACK_LOADERS_TEMPLATE: &str =
    include_str!("../templates/database_messagepack_loaders.cs.tera");
const DATABASE_MESSAGEPACK_READERS_TEMPLATE: &str =
    include_str!("../templates/database_messagepack_readers.cs.tera");
const EXCEPTION_TEMPLATE: &str = include_str!("../templates/load_exception.cs.tera");

pub fn render_project(project: &CsharpProject) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let tera = templates()?;
    let mut files = Vec::new();

    for schema_enum in &project.enums {
        let mut context = Context::new();
        context.insert("namespace", &project.namespace);
        context.insert("enum", schema_enum);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(format!("{}.cs", schema_enum.name)),
            contents: render(&tera, "enum.cs.tera", &context)?,
        });
    }

    for schema_type in &project.types {
        let mut context = Context::new();
        context.insert("namespace", &project.namespace);
        context.insert("type", schema_type);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(format!("{}.cs", schema_type.name)),
            contents: render(&tera, "type.cs.tera", &context)?,
        });
    }

    let mut database_context = Context::new();
    database_context.insert("project", project);
    let database_template = match project.data_format {
        CsharpDataFormat::Json => "database_json.cs.tera",
        CsharpDataFormat::MessagePack => "database_messagepack.cs.tera",
    };
    files.push(GeneratedFile {
        relative_path: PathBuf::from(format!("{}.cs", project.database_class)),
        contents: render(&tera, database_template, &database_context)?,
    });

    let mut exception_context = Context::new();
    exception_context.insert("namespace", &project.namespace);
    files.push(GeneratedFile {
        relative_path: PathBuf::from("CftLoadException.cs"),
        contents: render(&tera, "load_exception.cs.tera", &exception_context)?,
    });

    Ok(files)
}

fn templates() -> Result<Tera, CsharpCodegenError> {
    let mut tera = Tera::default();
    tera.add_raw_template("enum.cs.tera", ENUM_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add enum template: {err}")))?;
    tera.add_raw_template("type.cs.tera", TYPE_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add type template: {err}")))?;
    tera.add_raw_template("database_json.cs.tera", DATABASE_JSON_TEMPLATE)
        .map_err(|err| {
            CsharpCodegenError::new(format!("failed to add JSON database template: {err}"))
        })?;
    tera.add_raw_template(
        "database_messagepack.cs.tera",
        DATABASE_MESSAGEPACK_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add MessagePack database template: {err}"
        ))
    })?;
    tera.add_raw_template(
        "database_common_members.cs.tera",
        DATABASE_COMMON_MEMBERS_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!("failed to add database members template: {err}"))
    })?;
    tera.add_raw_template(
        "database_common_resolve.cs.tera",
        DATABASE_COMMON_RESOLVE_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!("failed to add database resolve template: {err}"))
    })?;
    tera.add_raw_template(
        "database_common_indexes.cs.tera",
        DATABASE_COMMON_INDEXES_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add database index helpers template: {err}"
        ))
    })?;
    tera.add_raw_template(
        "database_json_loaders.cs.tera",
        DATABASE_JSON_LOADERS_TEMPLATE,
    )
    .map_err(|err| CsharpCodegenError::new(format!("failed to add JSON loader template: {err}")))?;
    tera.add_raw_template(
        "database_json_readers.cs.tera",
        DATABASE_JSON_READERS_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!("failed to add JSON reader helpers template: {err}"))
    })?;
    tera.add_raw_template(
        "database_messagepack_loaders.cs.tera",
        DATABASE_MESSAGEPACK_LOADERS_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!("failed to add MessagePack loader template: {err}"))
    })?;
    tera.add_raw_template(
        "database_messagepack_readers.cs.tera",
        DATABASE_MESSAGEPACK_READERS_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add MessagePack reader helpers template: {err}"
        ))
    })?;
    tera.add_raw_template("load_exception.cs.tera", EXCEPTION_TEMPLATE)
        .map_err(|err| {
            CsharpCodegenError::new(format!("failed to add exception template: {err}"))
        })?;
    Ok(tera)
}

fn render(tera: &Tera, name: &str, context: &Context) -> Result<String, CsharpCodegenError> {
    tera.render(name, context)
        .map_err(|err| CsharpCodegenError::new(format!("failed to render `{name}`: {err}")))
}
