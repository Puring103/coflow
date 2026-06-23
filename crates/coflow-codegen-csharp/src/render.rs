use crate::model::CsharpProject;
use crate::{CsharpCodegenError, CsharpDatabaseTemplates, GeneratedFile};
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const TYPE_JSON_LOADER_TEMPLATE: &str = include_str!("../templates/type_json_loader.cs.tera");
const TYPE_JSON_POLYMORPHIC_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_json_polymorphic_loader.cs.tera");
const TYPE_MESSAGEPACK_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_messagepack_loader.cs.tera");
const TYPE_MESSAGEPACK_POLYMORPHIC_LOADER_TEMPLATE: &str =
    include_str!("../templates/type_messagepack_polymorphic_loader.cs.tera");
const DATABASE_COMMON_MEMBERS_TEMPLATE: &str =
    include_str!("../templates/database_common_members.cs.tera");

pub fn render_project(
    project: &CsharpProject,
    database_templates: &CsharpDatabaseTemplates,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let tera = templates(database_templates)?;
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
        contents: render(
            &tera,
            database_templates.database_template.name,
            &database_context,
        )?,
    });

    Ok(files)
}

fn templates(database_templates: &CsharpDatabaseTemplates) -> Result<Tera, CsharpCodegenError> {
    let mut tera = Tera::default();
    tera.add_raw_template("enum.cs.tera", ENUM_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add enum template: {err}")))?;
    tera.add_raw_template("type.cs.tera", TYPE_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add type template: {err}")))?;
    tera.add_raw_template("type_json_loader.cs.tera", TYPE_JSON_LOADER_TEMPLATE)
        .map_err(|err| {
            CsharpCodegenError::new(format!("failed to add JSON type loader template: {err}"))
        })?;
    tera.add_raw_template(
        "type_json_polymorphic_loader.cs.tera",
        TYPE_JSON_POLYMORPHIC_LOADER_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add JSON polymorphic type loader template: {err}"
        ))
    })?;
    tera.add_raw_template(
        "type_messagepack_loader.cs.tera",
        TYPE_MESSAGEPACK_LOADER_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add MessagePack type loader template: {err}"
        ))
    })?;
    tera.add_raw_template(
        "type_messagepack_polymorphic_loader.cs.tera",
        TYPE_MESSAGEPACK_POLYMORPHIC_LOADER_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add MessagePack polymorphic type loader template: {err}"
        ))
    })?;
    tera.add_raw_template(
        database_templates.database_template.name,
        database_templates.database_template.contents,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!(
            "failed to add database template `{}`: {err}",
            database_templates.database_template.name
        ))
    })?;
    tera.add_raw_template(
        "database_common_members.cs.tera",
        DATABASE_COMMON_MEMBERS_TEMPLATE,
    )
    .map_err(|err| {
        CsharpCodegenError::new(format!("failed to add database members template: {err}"))
    })?;
    for template in database_templates.partials {
        tera.add_raw_template(template.name, template.contents)
            .map_err(|err| {
                CsharpCodegenError::new(format!(
                    "failed to add database partial template `{}`: {err}",
                    template.name
                ))
            })?;
    }
    Ok(tera)
}

fn render(tera: &Tera, name: &str, context: &Context) -> Result<String, CsharpCodegenError> {
    tera.render(name, context)
        .map_err(|err| CsharpCodegenError::new(format!("failed to render `{name}`: {err}")))
}
