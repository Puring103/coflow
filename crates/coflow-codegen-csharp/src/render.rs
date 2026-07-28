use crate::emit::build_load_steps;
use crate::model::CsharpProject;
use crate::{CsharpCodegenError, GeneratedFile};
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const DATABASE_TEMPLATE: &str = include_str!("../templates/database.cs.tera");
const DATABASE_COMMON_MEMBERS_TEMPLATE: &str =
    include_str!("../templates/database_common_members.cs.tera");
const DATABASE_LOADER_MEMBERS_TEMPLATE: &str =
    include_str!("../templates/database_loader_members.cs.tera");
const TYPE_JSON_LOADER_TEMPLATE: &str = include_str!("../templates/type_json_loader.cs.tera");
const TYPE_JSON_POLYMORPHIC_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_json_polymorphic_loader.cs.tera");
const TYPE_JSON_LOADER_FILE_TEMPLATE: &str =
    include_str!("../templates/type_json_loader_file.cs.tera");
const TYPE_MESSAGEPACK_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_messagepack_loader.cs.tera");
const TYPE_MESSAGEPACK_POLYMORPHIC_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_messagepack_polymorphic_loader.cs.tera");
const TYPE_MESSAGEPACK_LOADER_FILE_TEMPLATE: &str =
    include_str!("../templates/type_messagepack_loader_file.cs.tera");
const DATABASE_JSON_TEMPLATE: &str = include_str!("../templates/json/database_json.cs.tera");
const DATABASE_MESSAGEPACK_TEMPLATE: &str =
    include_str!("../templates/messagepack/database_messagepack.cs.tera");
const LOCALIZED_TEMPLATE: &str = include_str!("../templates/localized.cs.tera");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsharpLoaderKind {
    Json,
    MessagePack,
}

impl CsharpLoaderKind {
    const fn data_extension(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::MessagePack => "msgpack",
        }
    }

    const fn type_template(self) -> &'static str {
        match self {
            Self::Json => "type_json_loader_file.cs.tera",
            Self::MessagePack => "type_messagepack_loader_file.cs.tera",
        }
    }

    const fn database_template(self) -> &'static str {
        match self {
            Self::Json => "database_json.cs.tera",
            Self::MessagePack => "database_messagepack.cs.tera",
        }
    }
}

pub fn render_common_project(
    project: &CsharpProject,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
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
        context.insert("project", project);
        context.insert("type", schema_type);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(format!("{}.cs", schema_type.name)),
            contents: render(&tera, "type.cs.tera", &context)?,
        });
    }

    let mut database_context = Context::new();
    database_context.insert("project", project);
    files.push(GeneratedFile {
        relative_path: PathBuf::from(format!("{}.cs", project.database_class)),
        contents: render(&tera, "database.cs.tera", &database_context)?,
    });

    if project.uses_localization {
        let mut localized_context = Context::new();
        localized_context.insert("project", project);
        files.push(GeneratedFile {
            relative_path: PathBuf::from("Localized.cs"),
            contents: render(&tera, "localized.cs.tera", &localized_context)?,
        });
    }

    Ok(files)
}

pub fn render_loader_project(
    project: &CsharpProject,
    kind: CsharpLoaderKind,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let tera = templates()?;
    let mut files = Vec::new();

    for schema_type in &project.types {
        if schema_type.loader.is_none() {
            continue;
        }
        let mut context = Context::new();
        context.insert("project", project);
        context.insert("type", schema_type);
        files.push(GeneratedFile {
            relative_path: PathBuf::from(format!("{}.Loader.cs", schema_type.name)),
            contents: render(&tera, kind.type_template(), &context)?,
        });
    }

    let mut database_context = Context::new();
    database_context.insert("project", project);
    database_context.insert("data_extension", kind.data_extension());
    database_context.insert(
        "load_steps",
        &build_load_steps(&project.database.tables, kind.data_extension()),
    );
    files.push(GeneratedFile {
        relative_path: PathBuf::from(format!("{}.Loader.cs", project.database_class)),
        contents: render(&tera, kind.database_template(), &database_context)?,
    });

    Ok(files)
}

fn templates() -> Result<Tera, CsharpCodegenError> {
    let mut tera = Tera::default();
    for (name, contents) in [
        ("enum.cs.tera", ENUM_TEMPLATE),
        ("type.cs.tera", TYPE_TEMPLATE),
        ("database.cs.tera", DATABASE_TEMPLATE),
        (
            "database_common_members.cs.tera",
            DATABASE_COMMON_MEMBERS_TEMPLATE,
        ),
        (
            "database_loader_members.cs.tera",
            DATABASE_LOADER_MEMBERS_TEMPLATE,
        ),
        ("type_json_loader.cs.tera", TYPE_JSON_LOADER_TEMPLATE),
        (
            "type_json_polymorphic_loader.cs.tera",
            TYPE_JSON_POLYMORPHIC_LOADER_TEMPLATE,
        ),
        (
            "type_json_loader_file.cs.tera",
            TYPE_JSON_LOADER_FILE_TEMPLATE,
        ),
        (
            "type_messagepack_loader.cs.tera",
            TYPE_MESSAGEPACK_LOADER_TEMPLATE,
        ),
        (
            "type_messagepack_polymorphic_loader.cs.tera",
            TYPE_MESSAGEPACK_POLYMORPHIC_LOADER_TEMPLATE,
        ),
        (
            "type_messagepack_loader_file.cs.tera",
            TYPE_MESSAGEPACK_LOADER_FILE_TEMPLATE,
        ),
        ("database_json.cs.tera", DATABASE_JSON_TEMPLATE),
        (
            "database_messagepack.cs.tera",
            DATABASE_MESSAGEPACK_TEMPLATE,
        ),
        ("localized.cs.tera", LOCALIZED_TEMPLATE),
    ] {
        tera.add_raw_template(name, contents).map_err(|err| {
            CsharpCodegenError::new(format!("failed to add C# template `{name}`: {err}"))
        })?;
    }
    Ok(tera)
}

fn render(tera: &Tera, name: &str, context: &Context) -> Result<String, CsharpCodegenError> {
    tera.render(name, context)
        .map_err(|err| CsharpCodegenError::new(format!("failed to render `{name}`: {err}")))
}
