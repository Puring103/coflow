use serde_json::{Value, json};
use crate::{Project, Runtime, ReadOnlyProjectSession};
use super::{Arguments, Failure, Result, InputSnapshot, Response, Status, query, json as to_json};

/// 源码读取结果直接使用原始文件元数据，不依赖 Schema 或数据模型加载。
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SourceRead {
    pub file: super::InputFile,
    pub source: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub fn read_command(command: &str, project: &Project, snapshot: &InputSnapshot, args: &Arguments) -> Result<Response> {
    let mut scoped_args = args.clone();
    scoped_args.0.insert("_command".into(), vec![command.into()]);
    let args = &scoped_args;
    let mut response = Response::new(command);
    response.project = Some(crate::path_to_slash(project.root_dir()));
    match command {
        "project.inspect" => {
            response.diagnostics.extend(project.schema_diagnostic_set().diagnostics);
            response.diagnostics.extend(project.data_diagnostic_set().diagnostics);
            response.result = to_json(super::project_inspection::inspect(project)?)?;
        }
        "project.files" => {
            let kind = args.get("kind");
            if kind.is_some_and(|k| k != "schema" && k != "data") { return Err(Failure::request("kind must be schema or data")); }
            let (files, diagnostics) = super::files::inspect(snapshot, kind)?;
            response.diagnostics.extend(diagnostics);
            response.result = to_json(files)?;
        }
        "source.read" => {
            let file = snapshot.file(project, args.require("file")?)?;
            let source = String::from_utf8(file.contents.clone()).map_err(|e| Failure::request(e.to_string()))?;
            let start = args.number("start-line", 0)?;
            // 按原始行片段截取，保留 CRLF、空行和选中范围的末尾换行。
            let lines: Vec<_> = source.split_inclusive('\n').collect();
            let end = args.number("end-line", lines.len())?;
            if end < start { return Err(Failure::request("end-line precedes start-line")); }
            let content = if args.get("start-line").is_some() || args.get("end-line").is_some() { lines.into_iter().skip(start).take(end-start).collect::<String>() } else { source };
            response.result = to_json(SourceRead { file: file.clone(), source: content, start_line: start, end_line: end })?;
        }
        "schema.inspect" | "schema.check" | "schema.symbols" | "schema.refs" => {
            let schema = Runtime::open_schema_session(project.clone())?;
            response.set_session_diagnostics(schema.diagnostics());
            if let Some(ty) = args.get("type") {
                if schema.schema().is_some_and(|s| s.resolve_type(ty).is_none()) { return Err(Failure::missing(format!("unknown type `{ty}`"))); }
            }
            let report = crate::inspect_schema(&schema, args.get("type"), args.flag("include-derived"));
            response.result = match command {
                "schema.check" => {
                    let validation = super::rules::check_schema(&schema, args);
                    let report = response.set_validation(validation);
                    response.diagnostics.truncate(args.number("limit",usize::MAX)?);
                    report
                }
                "schema.symbols" => {
                    if args.get("kind").is_some_and(|kind| !["type", "field", "enum", "const", "rule"].contains(&kind)) {
                        return Err(Failure::request("kind must be type, field, enum, const or rule"));
                    }
                    let mut items = Vec::new();
                    for ty in &report.types {
                        items.push(json!({"kind": "type", "name": ty.name, "module": ty.module}));
                        for field in &ty.fields { items.push(json!({"kind": "field", "name": format!("{}.{}", ty.name, field.name), "type": ty.name})); }
                    }
                    for item in &report.enums { items.push(json!({"kind": "enum", "name": item.name, "module": item.module})); }
                    for item in &report.consts { items.push(json!({"kind": "const", "name": item.name, "module": item.module})); }
                    if let Some(view)=schema.schema(){for info in view.all_check_statements(){items.push(json!({"kind":"rule","name":super::rules::identity(info),"definition":super::rules::describe(view,info,project.root_dir())}));}}
                    if let Some(kind) = args.get("kind") { items.retain(|v| v["kind"] == kind); }
                    items.sort_by(|a,b| a["name"].as_str().cmp(&b["name"].as_str()));
                    to_json(query::paginate(items, &args.selector()?)?)?
                }
                "schema.refs" => super::schema_queries::references(project, &schema, args)?,
                _ => {
                    let mut value = super::schema_queries::inspect(&schema,args)?;
                    if let Some(field) = args.get("field") {
                        let ty = args.require("type")?;
                        let entry = value["types"].as_array().and_then(|types| types.iter().find(|v| v["name"] == ty))
                            .and_then(|t| t["fields"].as_array()).and_then(|fs| fs.iter().find(|f| f["name"] == field)).cloned()
                            .ok_or_else(|| Failure::missing(format!("unknown field `{ty}.{field}`")))?;
                        value = entry;
                    }
                    value
                }
            };
        }
        _ => {
            let session = Runtime::new().open_data_session(project.clone())?;
            response.set_session_diagnostics(session.queries().diagnostics());
            response.result = data_command(command, project, &session, snapshot, args)?;
        }
    }
    if response.diagnostics.iter().any(|diagnostic| diagnostic.severity == crate::Severity::Error) { response.status = if command == "schema.check" { Status::Invalid } else { Status::Partial }; }
    Ok(response)
}

fn data_command(command: &str, project: &Project, session: &ReadOnlyProjectSession, snapshot: &InputSnapshot, args: &Arguments) -> Result<Value> {
    let selector = args.selector()?;
    match command {
        "data.list" | "data.search" => to_json(query::query_records(project, session, &selector)?),
        "data.get" => {
            let ty = args.require("type")?; let key = args.require("key")?;
            let records = query::selected_records(session, &selector)?;
            let record = records.iter().find(|r| r.actual_type() == ty && r.key() == key).ok_or_else(|| Failure::missing(format!("record `{ty}::{key}` does not exist")))?;
            query::record_json(project, session, record, &selector)
        }
        "data.draft" => {
            let draft = session.session.create_record_draft(args.require("type")?)?;
            let required: Vec<_> = draft.fields.iter().filter(|f| !matches!(f.source, crate::CreateFieldSource::SchemaDefault)).map(|f| &f.name).collect();
            let mut encoded = to_json(&draft)?;
            for (field, entry) in draft.fields.iter().zip(encoded["fields"].as_array_mut().into_iter().flatten()) {
                let expected = &session.schema().field(&draft.actual_type, &field.name).ok_or_else(|| Failure::missing("draft field schema is missing"))?.value_type;
                entry["value"] = super::model_value::encode(session.schema(), expected, field.value.as_ref())?;
                entry["rendered"] = json!(field.value.as_ref().and_then(super::model_value::rendered));
            }
            Ok(json!({"draft": encoded, "key": args.get("key"), "complete": required.is_empty(), "required_inputs": required}))
        }
        "dimension.list" => to_json(super::dimension::list(session)),
        command if command.starts_with("dimension.") => super::dimension::query(command, session, args),
        command if command.starts_with("stats.") => super::statistics::query(command, session, snapshot, args),
        "data.refs" => super::references::query(session, args),
        "rule.list" | "rule.inspect" => super::rules::query(command, session, args),
        "context" => super::context::query(project, session, args),
        _ => Err(Failure::request(format!("unknown read command `{command}`"))),
    }
}
