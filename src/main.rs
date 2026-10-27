#![allow(clippy::multiple_crate_versions)]

// Windows 大量语法节点和模型小对象使用同一分配器，降低分配及代次回收成本。
#[cfg(windows)]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use coflow_runtime::automation::{Arguments, Failure, InputSnapshot, Response, Result, Status};
use coflow_runtime::Project;
use serde_json::json;
use std::{
    fmt::Write as _,
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
};
mod cli;
mod command_logging;
mod diagnostics;
mod skill_commands;

fn main() -> ExitCode {
    // 文件日志显式启用，绝不污染 JSON、格式化输出或 LSP 的标准输出。
    let _logging = if std::env::var_os("COFLOW_LOG").is_some_and(|value| value == "1") {
        match coflow_observability::initialize("cli") {
            Ok(logging) => Some(logging),
            Err(error) => {
                eprintln!("无法初始化本机日志：{error}");
                None
            }
        }
    } else {
        None
    };
    let raw: Vec<_> = std::env::args_os().collect();
    let (invoked, json_output, pretty) = cli::invocation(&raw);
    let run_id = coflow_observability::run_id();
    let operation_id = coflow_observability::identifier();
    let span = coflow_observability::tracing::info_span!("cli_command", run_id, operation_id, command = %invoked);
    let _entered = span.enter();
    let mut command_log = command_logging::CommandLog::new();
    let matches = match cli::command().try_get_matches_from(&raw) {
        Ok(matches) => matches,
        Err(error) => {
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                command_log.exit(0);
                return ExitCode::SUCCESS;
            }
            let (message, help) = cli::argument_error(&error, &invoked);
            let mut response = Response::new(invoked);
            response.fail(Failure::request(message));
            response.set_diagnostic_help(0, help);
            return output(response, json_output, pretty, false, &mut command_log);
        }
    };
    let (command, args) = cli::decode(&matches);
    if command == "lsp" {
        return match Project::open_schema_only(args.get("project").map(Path::new))
            .map_err(Failure::from)
            .and_then(|p| coflow_lsp::run(p).map_err(|e| Failure::new(Status::Error, "CLI-LSP", e)))
        {
            Ok(true) => {
                command_log.exit(0);
                ExitCode::SUCCESS
            }
            Ok(false) => {
                command_log.exit(1);
                ExitCode::FAILURE
            }
            Err(e) => {
                let exit_code = e.status.exit_code();
                eprintln!("{}", e.diagnostics);
                let mut response = Response::new("lsp");
                response.fail(e);
                command_log.response(&response);
                ExitCode::from(exit_code)
            }
        };
    }
    let mut response = Response::new(&command);
    if let Err(error) = run(&command, &args, &mut response) {
        response.fail(error);
    }
    if command == "format"
        && args.flag("stdin")
        && !args.flag("check")
        && !json_output
        && response.status == Status::Success
    {
        if let Some(source) = response.result["source"].as_str() {
            command_log.response(&response);
            return match io::stdout().lock().write_all(source.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    command_log.output_failed();
                    eprintln!("output error: {error}");
                    ExitCode::from(4)
                }
            };
        }
    }
    output(
        response,
        json_output,
        pretty,
        args.flag("details"),
        &mut command_log,
    )
}
fn output(
    mut response: Response,
    json_output: bool,
    pretty: bool,
    details: bool,
    command_log: &mut command_logging::CommandLog,
) -> ExitCode {
    command_log.response(&response);
    response.prepare_output(details);
    let code = response.exit_code();
    // 先准备完整输出，stdout 只写入结果，错误不会混入 JSON。
    let content = if json_output {
        match if pretty {
            serde_json::to_string_pretty(&response)
        } else {
            serde_json::to_string(&response)
        } {
            Ok(text) => text + "\n",
            Err(error) => {
                command_log.output_failed();
                eprintln!("output error: {error}");
                return ExitCode::from(4);
            }
        }
    } else {
        let mut text = format!("{}: {:?}\n", response.command, response.status);
        render_text(&response.result, 0, &mut text);
        if let Some(scope) = &response.data_scope {
            if !scope.complete {
                text.push_str("data_scope:\n");
                match serde_json::to_value(scope) {
                    Ok(scope) => render_text(&scope, 2, &mut text),
                    Err(error) => {
                        command_log.output_failed();
                        eprintln!("output error: {error}");
                        return ExitCode::from(4);
                    }
                }
            }
        }
        // 文本输出与 JSON 共享公开诊断 DTO 和截断契约，避免丢失逻辑位置。
        if let Ok(serialized) = serde_json::to_value(&response) {
            for diagnostic in serialized["diagnostics"].as_array().into_iter().flatten() {
                render_diagnostic(diagnostic, &mut text);
            }
            if serialized["diagnostics_summary"]["truncated"] == true {
                let _ = writeln!(
                    text,
                    "diagnostics: {} of {}; use --details without --limit for all diagnostics",
                    serialized["diagnostics_summary"]["returned"],
                    serialized["diagnostics_summary"]["total"]
                );
            }
        }
        text
    };
    if let Err(error) = io::stdout().lock().write_all(content.as_bytes()) {
        command_log.output_failed();
        eprintln!("output error: {error}");
        return ExitCode::from(4);
    }
    ExitCode::from(code)
}
/// 终端位置显示为一基行列；JSON 继续保留协议中的零基源码坐标。
fn render_diagnostic(value: &serde_json::Value, text: &mut String) {
    let _ = writeln!(
        text,
        "[{}] {}: {}",
        value["code"].as_str().unwrap_or_default(),
        value["severity"].as_str().unwrap_or_default(),
        value["message"].as_str().unwrap_or_default()
    );
    if let Some(path) = value["location"]["path"].as_str() {
        let _ = write!(text, "  at {path}");
        if let (Some(line), Some(character)) = (
            value["location"]["range"]["start"]["line"].as_u64(),
            value["location"]["range"]["start"]["character"].as_u64(),
        ) {
            let _ = write!(text, ":{}:{}", line + 1, character + 1);
        }
        if let Some(keys) = value["location"]["config_path"].as_array() {
            let keys = keys
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(".");
            if !keys.is_empty() {
                let _ = write!(text, " ({keys})");
            }
        }
        text.push('\n');
    }
    if let Some(record_type) = value["record"]["type"].as_str() {
        let _ = write!(
            text,
            "  target {record_type}/{}",
            value["record"]["id"].as_str().unwrap_or_default()
        );
        if let Some(field) = value["field_path"].as_str() {
            let _ = write!(text, ".{field}");
        }
        text.push('\n');
    } else if let Some(field) = value["field_path"].as_str() {
        let _ = writeln!(text, "  field {field}");
    }
    for context in value["contexts"].as_array().into_iter().flatten() {
        if context["kind"] == "check" {
            if let Some(name) = context["name"].as_str() {
                let _ = writeln!(text, "  rule {name}");
            }
        }
        if let (Some(dimension), Some(variant)) =
            (context["dimension"].as_str(), context["variant"].as_str())
        {
            let _ = writeln!(text, "  dimension {dimension}={variant}");
        }
    }
    for related in value["related"].as_array().into_iter().flatten() {
        if let Some(path) = related["location"]["path"].as_str() {
            let _ = write!(text, "  related {path}");
            if let (Some(line), Some(character)) = (
                related["location"]["range"]["start"]["line"].as_u64(),
                related["location"]["range"]["start"]["character"].as_u64(),
            ) {
                let _ = write!(text, ":{}:{}", line + 1, character + 1);
            }
            if let Some(message) = related["message"].as_str() {
                let _ = write!(text, ": {message}");
            }
            text.push('\n');
        }
    }
    if let Some(help) = value["help"].as_str() {
        let _ = writeln!(text, "  help: {help}");
    }
}

fn render_text(value: &serde_json::Value, indent: usize, output: &mut String) {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, value) in fields {
                let _ = write!(output, "{}{}:", " ".repeat(indent), name);
                if value.is_object() || value.is_array() {
                    output.push('\n');
                    render_text(value, indent + 2, output);
                } else {
                    let _ = writeln!(output, " {}", scalar_text(value));
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                let _ = writeln!(output, "{}-", " ".repeat(indent));
                render_text(item, indent + 2, output);
            }
        }
        value => {
            let _ = writeln!(output, "{}{}", " ".repeat(indent), scalar_text(value));
        }
    }
}
fn scalar_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Null => "—".into(),
        value => value.to_string(),
    }
}
fn run(command: &str, args: &Arguments, response: &mut Response) -> Result<()> {
    match command {
        "describe" => {
            response.result = cli::describe(args.get("command"), args.get("schema"))?;
            return Ok(());
        }
        "init" => {
            let result = coflow_runtime::init_project(Path::new(args.get("dir").unwrap_or(".")))?;
            response.result = serde_json::to_value(cli::InitResult {
                config_path: coflow_runtime::path_to_slash(&result.config_path),
            })?;
            return Ok(());
        }
        "explain" => {
            response.result = coflow_runtime::automation::explain(args.require("code")?)?;
            return Ok(());
        }
        command if command.starts_with("skill.") => {
            let path = args.get("project").map(Path::new);
            let report = match (command, args.flag("global")) {
                ("skill.install", true) => skill_commands::install_global()?,
                ("skill.install", false) => skill_commands::install_project(path)?,
                ("skill.uninstall", true) => skill_commands::uninstall_global()?,
                ("skill.uninstall", false) => skill_commands::uninstall_project(path)?,
                (_, true) => skill_commands::status_global()?,
                (_, false) => skill_commands::status_project(path)?,
            };
            response.result = serde_json::to_value(report)?;
            return Ok(());
        }
        "format" if args.flag("stdin") => {
            let source = read_input("-")?;
            let formatted =
                coflow_runtime::automation::format_source(&source, args.require("language")?)?;
            let changed = source != formatted;
            response.result = json!({"changed":changed,"written":false,"dry_run":args.flag("dry-run"),"source":if args.flag("check"){None}else{Some(formatted)}});
            if args.flag("check") && changed {
                response.status = Status::Invalid;
            }
            return Ok(());
        }
        _ => {}
    }
    let project = Project::open_schema_only(args.get("project").map(Path::new))?;
    response.project = Some(coflow_runtime::path_to_slash(project.root_dir()));
    let snapshot = InputSnapshot::capture(&project)?;
    if args.get("input") == Some("-") && args.get("value-file") == Some("-") {
        return Err(Failure::request("only one request carrier may read stdin"));
    }
    let input = if let Some(path) = args.get("input") {
        Some(read_input(path)?)
    } else {
        None
    };
    let input_dir = if let Some(path) = args.get("input").filter(|p| *p != "-") {
        std::env::current_dir()?
            .join(path)
            .parent()
            .map_or(std::env::current_dir()?, Path::to_path_buf)
    } else {
        std::env::current_dir()?
    };
    let mut args = args.clone();
    if let Some(path) = args.get("value-file") {
        let value = read_input(path)?;
        args.0.insert("value-cfd".into(), vec![value]);
    }
    *response = coflow_runtime::automation::execute(
        command,
        &project,
        &snapshot,
        &args,
        input.as_deref(),
        &input_dir,
    )?;
    Ok(())
}
fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        Ok(text)
    } else {
        std::fs::read_to_string(path).map_err(|error| {
            Failure::new(
                Status::Error,
                "CLI-IO",
                format!("cannot read input `{path}`: {error}"),
            )
        })
    }
}
