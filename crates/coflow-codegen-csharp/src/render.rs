use crate::model::CsharpProject;
use crate::{CsharpCodegenError, CsharpDatabaseTemplates, GeneratedFile};
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const DATABASE_COMMON_MEMBERS_TEMPLATE: &str =
    include_str!("../templates/database_common_members.cs.tera");
const DATABASE_COMMON_RESOLVE_TEMPLATE: &str =
    include_str!("../templates/database_common_resolve.cs.tera");
const DATABASE_COMMON_INDEXES_TEMPLATE: &str =
    include_str!("../templates/database_common_indexes.cs.tera");
const EXCEPTION_TEMPLATE: &str = include_str!("../templates/load_exception.cs.tera");

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
        context.insert("namespace", &project.namespace);
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

    let mut exception_context = Context::new();
    exception_context.insert("namespace", &project.namespace);
    files.push(GeneratedFile {
        relative_path: PathBuf::from("CftLoadException.cs"),
        contents: render(&tera, "load_exception.cs.tera", &exception_context)?,
    });

    Ok(files)
}

fn templates(database_templates: &CsharpDatabaseTemplates) -> Result<Tera, CsharpCodegenError> {
    let mut tera = Tera::default();
    tera.add_raw_template("enum.cs.tera", ENUM_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add enum template: {err}")))?;
    tera.add_raw_template("type.cs.tera", TYPE_TEMPLATE)
        .map_err(|err| CsharpCodegenError::new(format!("failed to add type template: {err}")))?;
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
    for template in database_templates.partials {
        tera.add_raw_template(template.name, template.contents)
            .map_err(|err| {
                CsharpCodegenError::new(format!(
                    "failed to add database partial template `{}`: {err}",
                    template.name
                ))
            })?;
    }
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
