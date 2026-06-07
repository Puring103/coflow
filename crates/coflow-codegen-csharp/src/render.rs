use crate::model::CsharpProject;
use crate::{CsharpCodegenError, GeneratedFile};
use std::path::PathBuf;
use tera::{Context, Tera};

const ENUM_TEMPLATE: &str = include_str!("../templates/enum.cs.tera");
const TYPE_TEMPLATE: &str = include_str!("../templates/type.cs.tera");
const DATABASE_TEMPLATE: &str = include_str!("../templates/database.cs.tera");
const EXCEPTION_TEMPLATE: &str = include_str!("../templates/load_exception.cs.tera");

pub(crate) fn render_project(
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
        contents: render(&tera, "database.cs.tera", &database_context)?,
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
    tera.add_raw_template("database.cs.tera", DATABASE_TEMPLATE)
        .map_err(|err| {
            CsharpCodegenError::new(format!("failed to add database template: {err}"))
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
