use super::{json as to_json, query, Arguments, Failure, InputSnapshot, Response, Result, Status};
use crate::{Project, ReadOnlyProjectSession, Runtime};
use serde_json::Value;

/// 源码读取结果直接使用原始文件元数据，不依赖 Schema 或数据模型加载。
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SourceRead {
    pub file: super::InputFile,
    pub source: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DraftField {
    pub name: String,
    pub value: Option<super::ModelValue>,
    pub source: crate::CreateFieldSource,
    pub required: Option<crate::CreateRequiredInput>,
    pub rendered: Option<Value>,
    pub query: Option<String>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct RecordDraft {
    pub actual_type: String,
    pub fields: Vec<DraftField>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct DraftResult {
    pub draft: RecordDraft,
    pub id: Option<String>,
    pub complete: bool,
    pub required_inputs: Vec<String>,
}

/// 符号种类决定其元数据形状，规则符号共享规则查询的描述契约。
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaSymbol {
    Type {
        name: String,
        module: String,
        location: Option<super::diagnostics::CommandLocation>,
    },
    Field {
        name: String,
        #[serde(rename = "type")]
        actual_type: String,
        declaring_type: String,
        location: Option<super::diagnostics::CommandLocation>,
    },
    Enum {
        name: String,
        module: String,
        location: Option<super::diagnostics::CommandLocation>,
    },
    Const {
        name: String,
        module: String,
        location: Option<super::diagnostics::CommandLocation>,
    },
    Rule {
        name: String,
        definition: super::RuleDescription,
    },
}

impl SchemaSymbol {
    fn name(&self) -> &str {
        match self {
            Self::Type { name, .. }
            | Self::Field { name, .. }
            | Self::Enum { name, .. }
            | Self::Const { name, .. }
            | Self::Rule { name, .. } => name,
        }
    }
    fn kind(&self) -> &str {
        match self {
            Self::Type { .. } => "type",
            Self::Field { .. } => "field",
            Self::Enum { .. } => "enum",
            Self::Const { .. } => "const",
            Self::Rule { .. } => "rule",
        }
    }
}

pub fn read_command(
    command: &str,
    project: &Project,
    snapshot: &InputSnapshot,
    args: &Arguments,
) -> Result<Response> {
    let mut scoped_args = args.clone();
    scoped_args
        .0
        .insert("_command".into(), vec![command.into()]);
    let args = &scoped_args;
    let mut response = Response::new(command);
    response.project = Some(crate::path_to_slash(project.root_dir()));
    match command {
        "project.inspect" => {
            response
                .diagnostics
                .extend(project.schema_diagnostic_set().diagnostics);
            response
                .diagnostics
                .extend(project.data_diagnostic_set().diagnostics);
            response.result = to_json(super::project_inspection::inspect(project)?)?;
        }
        "project.files" => {
            let kind = args.get("kind");
            if kind.is_some_and(|k| k != "schema" && k != "data") {
                return Err(Failure::request("kind must be schema or data"));
            }
            let (files, diagnostics) = super::files::inspect(snapshot, kind)?;
            response.diagnostics.extend(diagnostics);
            response.result = to_json(files)?;
        }
        "source.read" => {
            let file = snapshot.file(project, args.require("file")?)?;
            let source = String::from_utf8(file.contents.clone())
                .map_err(|e| Failure::request(e.to_string()))?;
            let start = args.number("start-line", 0)?;
            // 按原始行片段截取，保留 CRLF、空行和选中范围的末尾换行。
            let lines: Vec<_> = source.split_inclusive('\n').collect();
            let end = args.number("end-line", lines.len())?;
            if end < start {
                return Err(Failure::request("end-line precedes start-line"));
            }
            let content = if args.get("start-line").is_some() || args.get("end-line").is_some() {
                lines
                    .into_iter()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>()
            } else {
                source
            };
            response.result = to_json(SourceRead {
                file: file.clone(),
                source: content,
                start_line: start,
                end_line: end,
            })?;
        }
        "schema.inspect" | "schema.check" | "schema.symbols" | "schema.refs" | "rule.list"
        | "rule.inspect" => {
            let schema = Runtime::open_schema_session(project.clone())?;
            response.set_session_diagnostics(schema.diagnostics());
            if let Some(ty) = args.get("type") {
                if schema
                    .schema()
                    .is_some_and(|s| s.resolve_type(ty).is_none())
                {
                    return Err(Failure::missing(format!("unknown type `{ty}`")));
                }
            }
            let report =
                crate::inspect_schema(&schema, args.get("type"), args.flag("include-derived"));
            response.result = match command {
                "schema.check" => {
                    let validation = super::rules::check_schema(&schema, args);
                    let report = response.set_validation(validation);
                    response
                        .diagnostics
                        .truncate(args.number("limit", usize::MAX)?);
                    report
                }
                "schema.symbols" => {
                    if args.get("kind").is_some_and(|kind| {
                        !["type", "field", "enum", "const", "rule"].contains(&kind)
                    }) {
                        return Err(Failure::request(
                            "kind must be type, field, enum, const or rule",
                        ));
                    }
                    let mut items = Vec::new();
                    let view = schema.schema();
                    let locate = |module, span: coflow_language::source::Span| {
                        view.and_then(|view| view.source(module)).map(|source| {
                            let range = crate::TextRange::from_byte_offsets(
                                &source.source,
                                span.start,
                                span.end,
                            );
                            super::diagnostics::CommandLocation::from_source(
                                &crate::SourceLocation::FileSpan {
                                    path: source.path.clone(),
                                    start_line: range.start.line,
                                    start_character: range.start.character,
                                    end_line: range.end.line,
                                    end_character: range.end.character,
                                },
                                Some(project.root_dir()),
                            )
                        })
                    };
                    for ty in &report.types {
                        let declaration = view.and_then(|view| view.resolve_type(&ty.name));
                        items.push(SchemaSymbol::Type {
                            name: ty.name.clone(),
                            module: ty.module.clone(),
                            location: declaration.and_then(|ty| locate(&ty.module, ty.span)),
                        });
                        for field in &ty.fields {
                            // 继承字段的查询名称属于当前类型，源码位置始终属于实际声明类型。
                            let declaration =
                                view.and_then(|view| view.field(&ty.name, &field.name));
                            let declaring_type = declaration
                                .map(|field| field.declaring_type.to_string())
                                .ok_or_else(|| {
                                    Failure::missing("symbol field declaration is unavailable")
                                })?;
                            let location = declaration.and_then(|field| {
                                view.and_then(|view| view.resolve_type(&field.declaring_type))
                                    .and_then(|ty| locate(&ty.module, field.span))
                            });
                            items.push(SchemaSymbol::Field {
                                name: format!("{}.{}", ty.name, field.name),
                                actual_type: ty.name.clone(),
                                declaring_type,
                                location,
                            });
                        }
                    }
                    for item in &report.enums {
                        items.push(SchemaSymbol::Enum {
                            name: item.name.clone(),
                            module: item.module.clone(),
                            location: view
                                .and_then(|view| view.resolve_enum(&item.name))
                                .and_then(|item| locate(&item.module, item.span)),
                        });
                    }
                    for item in &report.consts {
                        items.push(SchemaSymbol::Const {
                            name: item.name.clone(),
                            module: item.module.clone(),
                            location: view
                                .and_then(|view| view.resolve_const(&item.name))
                                .and_then(|item| locate(&item.module, item.span)),
                        });
                    }
                    if let Some(view) = schema.schema() {
                        for info in super::rules::named_rules(view) {
                            items.push(SchemaSymbol::Rule {
                                name: super::rules::identity(info),
                                definition: super::rules::describe(view, info, project.root_dir()),
                            });
                        }
                    }
                    if let Some(kind) = args.get("kind") {
                        items.retain(|value| value.kind() == kind);
                    }
                    items.sort_by(|a, b| a.name().cmp(b.name()));
                    to_json(query::paginate(items, &args.selector()?)?)?
                }
                "rule.list" | "rule.inspect" => match schema.schema() {
                    Some(schema) => super::rules::query(command, schema, project.root_dir(), args)?,
                    None => to_json(super::RuleQuery { rules: Vec::new() })?,
                },
                "schema.refs" => {
                    let data = Runtime::new().open_data_session(project.clone())?;
                    response.set_session_diagnostics(data.queries().diagnostics());
                    response.set_data_scope(&data, snapshot);
                    super::schema_queries::references(project, &schema, args, &data)?
                }
                _ => {
                    let mut value = super::schema_queries::inspect(&schema, args)?;
                    if let Some(field) = args.get("field") {
                        let ty = args.require("type")?;
                        let entry = value["types"]
                            .as_array()
                            .and_then(|types| types.iter().find(|v| v["name"] == ty))
                            .and_then(|t| t["fields"].as_array())
                            .and_then(|fs| fs.iter().find(|f| f["name"] == field))
                            .cloned()
                            .ok_or_else(|| {
                                Failure::missing(format!("unknown field `{ty}.{field}`"))
                            })?;
                        value = entry;
                    }
                    value
                }
            };
        }
        _ => {
            let session = Runtime::new().open_data_session(project.clone())?;
            response.set_session_diagnostics(session.queries().diagnostics());
            response.set_data_scope(&session, snapshot);
            response.result = data_command(command, project, &session, snapshot, args)?;
        }
    }
    if response
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == crate::Severity::Error)
    {
        response.status = if command == "schema.check" {
            Status::Invalid
        } else {
            Status::Partial
        };
    }
    Ok(response)
}

fn data_command(
    command: &str,
    project: &Project,
    session: &ReadOnlyProjectSession,
    snapshot: &InputSnapshot,
    args: &Arguments,
) -> Result<Value> {
    let selector = args.selector()?;
    match command {
        "data.list" | "data.search" => to_json(query::query_records(project, session, &selector)?),
        "data.get" => {
            let ty = args.require("type")?;
            let key = args.require("id")?;
            let records = query::selected_records(session, &selector)?;
            let record = records
                .iter()
                .find(|r| r.actual_type() == ty && r.id() == key)
                .ok_or_else(|| Failure::missing(format!("record `{ty}::{key}` does not exist")))?;
            query::record_json(project, session, record, &selector)
        }
        "data.draft" => {
            let draft = session.session.create_record_draft(args.require("type")?)?;
            let required: Vec<_> = draft
                .fields
                .iter()
                .filter(|f| !matches!(f.source, crate::CreateFieldSource::SchemaDefault))
                .map(|f| f.name.clone())
                .collect();
            let mut fields = Vec::new();
            for field in draft.fields {
                let expected = &session
                    .schema()
                    .field(&draft.actual_type, &field.name)
                    .ok_or_else(|| Failure::missing("draft field schema is missing"))?
                    .value_type;
                let query = match &field.required {
                    Some(crate::CreateRequiredInput::Ref { target_type }) => {
                        Some(format!("data list --type {target_type} --include-derived"))
                    }
                    Some(crate::CreateRequiredInput::AbstractObject { expected_type, .. }) => Some(
                        format!("schema inspect --type {expected_type} --include-derived"),
                    ),
                    _ => None,
                };
                fields.push(DraftField {
                    name: field.name.clone(),
                    value: field
                        .value
                        .as_ref()
                        .map(|value| {
                            super::ModelValue::from_model(session.schema(), expected, value)
                        })
                        .transpose()?,
                    rendered: field.value.as_ref().and_then(super::model_value::rendered),
                    source: field.source,
                    required: field.required,
                    query,
                });
            }
            to_json(DraftResult {
                draft: RecordDraft {
                    actual_type: draft.actual_type,
                    fields,
                },
                id: args.get("id").map(str::to_string),
                complete: required.is_empty(),
                required_inputs: required,
            })
        }
        "dimension.list" => to_json(super::dimension::list(session)),
        command if command.starts_with("dimension.") => {
            super::dimension::query(command, session, args)
        }
        command if command.starts_with("stats.") => {
            super::statistics::query(command, session, snapshot, args)
        }
        "data.refs" => super::references::query(session, args),
        "context" => super::context::query(project, session, args),
        _ => Err(Failure::request(format!(
            "unknown read command `{command}`"
        ))),
    }
}
