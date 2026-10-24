use super::{
    Arguments, Candidate, ChangeSet, Failure, FieldPath, FileOperation, InputSnapshot, InputValue,
    Operation, RecordTarget, Response, Result, Status,
};
use crate::{Project, Runtime};
use serde_json::{json, Value};
use std::path::Path;

#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
/// # Errors
///
/// 参数、项目输入、校验或文件发布失败时返回相应诊断。
pub fn execute(
    command: &str,
    project: &Project,
    snapshot: &InputSnapshot,
    args: &Arguments,
    input: Option<&str>,
    carrier_dir: &Path,
) -> Result<Response> {
    let mut response = Response::new(command);
    response.project = Some(crate::path_to_slash(project.root_dir()));
    response.revision = Some(snapshot.revision());
    snapshot.require_revision(args.get("if-revision"))?;
    match command {
        "check" if input.is_none() => {
            let session = Runtime::new().open_data_session(project.clone())?;
            let validation = super::rules::check(&session, args)?;
            response.result = response.set_validation(validation);
            let limit = args.number("limit", usize::MAX)?;
            response.diagnostics.truncate(limit);
        }
        "format" => {
            let mut changes = Vec::new();
            let files = args.all("file");
            for name in &files {
                snapshot.file(project, name)?;
            }
            for file in snapshot
                .files
                .iter()
                .filter(|f| ["schema", "data"].contains(&f.kind.as_str()))
            {
                if !files.is_empty()
                    && !files.iter().any(|p| {
                        snapshot
                            .file(project, p)
                            .is_ok_and(|f| f.absolute == file.absolute)
                    })
                {
                    continue;
                }
                let source = String::from_utf8(file.contents.clone())
                    .map_err(|e| Failure::request(e.to_string()))?;
                let formatted =
                    format_source(&source, if file.kind == "schema" { "cft" } else { "cfd" })?;
                if source != formatted {
                    changes.push(coflow_staging::transaction::FileChange {
                        path: file.absolute.clone(),
                        before: Some(file.contents.clone()),
                        after: Some(formatted.into_bytes()),
                    });
                }
            }
            let preview = args.flag("check") || args.flag("dry-run");
            // 文件格式化与候选修改使用相同的精简补丁，完整源码由 source.read 按需获取。
            let files = changes
                .iter()
                .map(|change| {
                    Ok(
                        json!({"path":crate::project_path(project.root_dir(), &change.path),
                    "change":"modified", "patch":crate::diff::unified_hunks(
                        &String::from_utf8_lossy(change.before.as_deref().unwrap_or_default()),
                        &String::from_utf8_lossy(change.after.as_deref().unwrap_or_default())
                    )?}),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            response.result = json!({"changed":!changes.is_empty(),"written":false,"dry_run":args.flag("dry-run"),"check":args.flag("check"),"written_files":[],
                "files":files});
            if !preview && !changes.is_empty() {
                snapshot.verify(project).map_err(|mut failure| {
                    let mut report = response.result.clone();
                    if let (Value::Object(report), Some(Value::Object(details))) =
                        (&mut report, failure.result.take())
                    {
                        report.extend(details);
                    }
                    failure.with_result(report)
                })?;
                let written = coflow_staging::transaction::publish(&changes).map_err(|error| {
                    super::transaction::publication_failure(
                        project.root_dir(),
                        *error,
                        response.result.clone(),
                    )
                })?;
                response.result["written"] = json!(true);
                response.result["written_files"] = json!(written
                    .iter()
                    .map(|path| crate::project_path(project.root_dir(), path))
                    .collect::<Vec<_>>());
            }
            if args.flag("check") && !changes.is_empty() {
                response.status = Status::Invalid;
            }
        }
        "codegen" | "build" => {
            if command == "build" {
                let session = Runtime::new().open_data_session(project.clone())?;
                let mut options = args.clone();
                options.0.insert("rules".into(), vec!["true".into()]);
                let validation = super::rules::check(&session, &options)?;
                let passed = validation.passed();
                response.result = json!({"validation":response.set_validation(validation)});
                if !passed {
                    snapshot.verify(project)?;
                    return Ok(response);
                }
            }
            let outcome = crate::commands::codegen_automation_with_snapshot(
                project,
                args.flag("check") || args.flag("dry-run"),
                snapshot,
            )
            .map_err(|mut failure| {
                if let Some(validation) = response.result.get("validation") {
                    if let Some(result) = failure.result.as_mut() {
                        result["validation"] = validation.clone();
                    }
                }
                failure
            })?;
            match outcome {
                crate::commands::CommandOutcome::Success(report) => {
                    if args.flag("check") && report["changed"] == true {
                        response.status = Status::Invalid;
                    }
                    if response.result.is_null() {
                        response.result = json!({});
                    }
                    response.result["artifacts"] = report;
                }
                crate::commands::CommandOutcome::Diagnostics(d) => {
                    response.status = Status::Invalid;
                    response.diagnostics = d.diagnostics;
                }
            }
        }
        "diff" if input.is_none() => {
            response.result = crate::diff::automation_diff(project, snapshot, args)?;
            if response.result["semantic_available"] == false {
                response.status = Status::Partial;
            }
        }
        "apply" | "check" | "diff" | "schema.check" if input.is_some() => {
            let request = parse_change_set(input.unwrap_or_default())?;
            let if_revision = match &request {
                ChangeSet::Files { if_revision, .. }
                | ChangeSet::Operations { if_revision, .. } => if_revision.as_deref(),
            };
            if let (Some(argument), Some(request)) = (args.get("if-revision"), if_revision) {
                if argument != request {
                    return Err(Failure::request(
                        "--if-revision and change set if_revision must match",
                    ));
                }
            }
            snapshot.require_revision(if_revision)?;
            let version = match &request {
                ChangeSet::Files {
                    protocol_version, ..
                }
                | ChangeSet::Operations {
                    protocol_version, ..
                } => *protocol_version,
            };
            if version != 1 {
                return Err(Failure::request("unsupported protocol_version"));
            }
            let mut candidate = Candidate::new(project, snapshot)?;
            match request {
                ChangeSet::Files { changes, .. } => candidate
                    .apply_files(&changes, carrier_dir)
                    .map_err(|failure| candidate.failure_result(failure, args))?,
                ChangeSet::Operations { ops, .. } => candidate
                    .apply_operations(&ops)
                    .map_err(|failure| candidate.failure_result(failure, args))?,
            }
            let completed = if command == "schema.check" {
                candidate.preview_schema(args)
            } else {
                candidate.finish(args, command != "apply")
            };
            let (result, validation) =
                completed.map_err(|failure| candidate.failure_result(failure, args))?;
            response.set_validation(validation);
            response.result = result;
            if command == "diff" && response.result["diff"]["semantic_available"] == false {
                response.status = Status::Partial;
            }
            if command == "diff" {
                let mut diff = response.result["diff"].take();
                diff["comparison"] = json!("candidate");
                diff["base"] = Value::Null;
                diff["endpoints"] = response.result["endpoints"].take();
                diff["validation"] = response.result["validation"].take();
                diff["diagnostics"] = json!([]);
                if args.get("view").is_none_or(|view| view == "source") || args.flag("summary") {
                    diff["files"] = response.result["files"].take();
                }
                response.result = diff;
            }
            if matches!(command, "check" | "schema.check") {
                response
                    .diagnostics
                    .truncate(args.number("limit", usize::MAX)?);
            }
        }
        "apply" => return Err(Failure::request("--input is required")),
        command if is_write(command) => {
            let mut candidate = Candidate::new(project, snapshot)?;
            if command.starts_with("source.") || command.starts_with("project.") {
                let operation = match command {
                    "source.write" => FileOperation::Write {
                        file: args.require("file")?.into(),
                        content: Some(
                            input
                                .ok_or_else(|| Failure::request("--input is required"))?
                                .into(),
                        ),
                        content_file: None,
                        expected_fingerprint: args.get("expected-fingerprint").map(str::to_string),
                    },
                    "source.create" => FileOperation::Create {
                        file: args.require("file")?.into(),
                        content: Some(
                            input
                                .ok_or_else(|| Failure::request("--input is required"))?
                                .into(),
                        ),
                        content_file: None,
                    },
                    "source.delete" => FileOperation::Delete {
                        file: args.require("file")?.into(),
                        expected_fingerprint: args.get("expected-fingerprint").map(str::to_string),
                    },
                    "project.add-input" => FileOperation::AddInput {
                        kind: args.require("kind")?.into(),
                        path: args.require("path")?.into(),
                    },
                    _ => FileOperation::RemoveInput {
                        kind: args.require("kind")?.into(),
                        path: args.require("path")?.into(),
                    },
                };
                candidate
                    .apply_files(&[operation], carrier_dir)
                    .map_err(|failure| candidate.failure_result(failure, args))?;
            } else {
                let record = RecordTarget {
                    actual_type: args.require("type")?.into(),
                    id: args.require("id")?.into(),
                };
                let path = || -> Result<Vec<FieldPath>> { public_path(args.require("path")?) };
                let value = || -> Result<InputValue> {
                    Ok(InputValue::Expression {
                        source: args.require("value-cfd")?.into(),
                    })
                };
                let operation = match command {
                    "data.create" => Operation::CreateRecord {
                        record,
                        file: args.require("file")?.into(),
                        fields: serde_json::from_str(
                            input.ok_or_else(|| Failure::request("--input is required"))?,
                        )?,
                    },
                    "data.set" => Operation::SetField {
                        record,
                        path: path()?,
                        value: value()?,
                        expected: args
                            .get("expected-cfd")
                            .map(|source| InputValue::Expression {
                                source: source.into(),
                            }),
                    },
                    "data.unset" => Operation::UnsetField {
                        record,
                        path: path()?,
                    },
                    "data.rename" => Operation::RenameRecord {
                        record,
                        new_id: args.require("new-id")?.into(),
                    },
                    "data.delete" => Operation::DeleteRecord { record },
                    "data.transfer" => Operation::TransferRecord {
                        record,
                        to_file: args.require("to-file")?.into(),
                    },
                    "dimension.set" => Operation::SetDimensionValue {
                        record,
                        dimension: args.require("dimension")?.into(),
                        variant: args.require("variant")?.into(),
                        field: args.require("field")?.into(),
                        path: args
                            .get("path")
                            .map(public_path)
                            .transpose()?
                            .unwrap_or_default(),
                        value: value()?,
                        file: args.get("file").map(str::to_string),
                    },
                    _ => Operation::ClearDimensionValue {
                        record,
                        dimension: args.require("dimension")?.into(),
                        variant: args.require("variant")?.into(),
                        field: args.require("field")?.into(),
                    },
                };
                candidate
                    .apply_operations(&[operation])
                    .map_err(|failure| candidate.failure_result(failure, args))?;
            }
            let (result, validation) = candidate
                .finish(args, false)
                .map_err(|failure| candidate.failure_result(failure, args))?;
            response.set_validation(validation);
            response.result = result;
        }
        _ => return super::read_command(command, project, snapshot, args),
    }
    if response.result["written"] == true || response.result["artifacts"]["written"] == true {
        // 事务已经捕获发布后的修订，直接复用；格式化和代码生成也必须保留已发布事实。
        response.revision = Some(match response.result["revision_after"].as_str() {
            Some(revision) => revision.to_owned(),
            None => super::transaction::capture_published_revision(project, &response.result)?,
        });
    } else {
        snapshot.verify(project)?;
    }
    Ok(response)
}
/// 先定位每项请求的错误，再解析完整信封，避免批量请求只返回无位置的 serde 报错。
fn parse_change_set(source: &str) -> Result<ChangeSet> {
    let value: Value = serde_json::from_str(source).map_err(|error| {
        Failure::request(error.to_string()).with_result(json!({"written":false,
            "request_path":"$", "line":error.line(), "column":error.column()}))
    })?;
    let operations = match value.get("kind").and_then(Value::as_str) {
        Some("operations") => Some(("ops", true)),
        Some("files") => Some(("changes", false)),
        _ => None,
    };
    if let Some((field, data)) = operations {
        if let Some(items) = value.get(field).and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate() {
                let parsed = if data {
                    serde_json::from_value::<Operation>(item.clone()).map(|_| ())
                } else {
                    serde_json::from_value::<FileOperation>(item.clone()).map(|_| ())
                };
                if let Err(error) = parsed {
                    let path = format!("{field}[{index}]");
                    return Err(Failure::request(format!("{path}: {error}")).with_result(
                        json!({"written":false, "request_path":path, "operation_index":index,
                            "target":item.get("record").or_else(|| item.get("file"))}),
                    ));
                }
            }
        }
    }
    serde_json::from_str(source).map_err(|error| {
        Failure::request(error.to_string()).with_result(json!({"written":false,
            "request_path":"$", "line":error.line(), "column":error.column()}))
    })
}

fn is_write(command: &str) -> bool {
    [
        "source.write",
        "source.create",
        "source.delete",
        "project.add-input",
        "project.remove-input",
        "data.create",
        "data.set",
        "data.unset",
        "data.rename",
        "data.delete",
        "data.transfer",
        "dimension.set",
        "dimension.clear",
    ]
    .contains(&command)
}
pub(crate) fn public_path(path: &str) -> Result<Vec<FieldPath>> {
    super::query::parse_path(path)?
        .into_iter()
        .map(|s| {
            Ok(match s {
                crate::CfdPathSegment::Field(name) => FieldPath::Field { name },
                crate::CfdPathSegment::Index(index) => FieldPath::Index { index },
                crate::CfdPathSegment::DictKey(key) => FieldPath::DictKey {
                    key: if key.starts_with('"') {
                        super::ModelValue::String {
                            value: serde_json::from_str(&key)?,
                        }
                    } else if key.parse::<i64>().is_ok() {
                        super::ModelValue::Int { value: key }
                    } else if let Some((name, variant)) =
                        key.split_once("::").or_else(|| key.rsplit_once('.'))
                    {
                        super::ModelValue::Enum {
                            type_name: name.into(),
                            value: None,
                            variant: Some(variant.into()),
                        }
                    } else {
                        return Err(Failure::request("dictionary key requires a typed literal"));
                    },
                },
            })
        })
        .collect()
}
/// # Errors
///
/// language 不是 cft 或 cfd 时返回请求错误。
pub fn format_source(source: &str, language: &str) -> Result<String> {
    match language {
        "cft" => Ok(coflow_format::format_cft(source)),
        "cfd" => Ok(coflow_format::format_cfd(source)),
        _ => Err(Failure::request("language must be cft or cfd")),
    }
}

/// # Errors
///
/// 诊断码不存在或解释无法序列化时返回错误。
pub fn explain(code: &str) -> Result<Value> {
    super::json(super::explanations::explain(code)?)
}
