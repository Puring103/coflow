use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use coflow_runtime::automation::{Arguments, Failure, Result};
use serde_json::{json, Value};

#[derive(Debug)]
pub(crate) struct SelfUpdateArgs { pub check: bool, pub yes: bool }

const FLAGS: &[&str] = &["include-derived", "related", "dry-run", "check", "rules", "explain", "summary", "include-source", "stdin", "global", "yes", "performance", "from-base", "to-base"];
const QUERY: &str = "type include-derived file key fields where dimension sort limit cursor";
const WRITE: &str = "dry-run rules";
const RECORD: &str = "type key";
const VALUE: &str = "value-cfd value-file";
const DIMENSION: &str = "dimension variant type key field";

fn option(name: &'static str) -> Arg {
    let argument = Arg::new(name).long(name).help(match name {
        "type" => "Actual type name", "key" => "Record key", "file" => "Source path relative to the project",
        "input" => "Request or source content file; - reads stdin", "value-cfd" => "CFD expression parsed using the field type",
        "value-file" => "File containing a CFD expression; - reads stdin", "dimension" => "Dimension name, or name=variant in record queries",
        "fields" => "Comma-separated field paths to return", "where" => "Typed boolean filter",
        "sort" => "Comma-separated paths; -path sorts descending", "limit" => "Maximum number of returned items or diagnostics",
        "cursor" => "Next cursor returned by the same query", "dry-run" => "Validate and preview without writing files",
        "rules" => "Execute all business rules", "rule" => "Execute or inspect a rule by its returned identity",
        "check" => "Check current state without writing files", "expected-fingerprint" => "Require the source content fingerprint to match",
        "from-base" => "Compare from the base value", "to-base" => "Compare to the base value", _ => "Command option",
    });
    if FLAGS.contains(&name) { return argument.action(ArgAction::SetTrue); }
    let argument = argument.action(ArgAction::Set);
    if ["limit", "examples", "ref-limit", "depth", "budget", "start-line", "end-line", "option-layer"].contains(&name) {
        argument.value_parser(move |raw: &str| -> std::result::Result<String, String> {
            let value = raw.parse::<usize>().map_err(|_| "requires a nonnegative integer".to_string())?;
            if ["limit", "budget", "option-layer"].contains(&name) && value == 0 { return Err("must be greater than zero".into()); }
            Ok(raw.into())
        })
    } else { argument.allow_hyphen_values(["value-cfd", "sort"].contains(&name)) }
}

fn leaf(name: &'static str, about: &'static str, groups: &[&'static str]) -> Command {
    let mut command = Command::new(name).about(about);
    let mut names = std::collections::BTreeSet::new();
    for group in groups { for name in group.split_whitespace() { if names.insert(name) { command = command.arg(option(name)); } } }
    if names.contains("value-cfd") { command = command.group(ArgGroup::new("value").args(["value-cfd", "value-file"]).required(true)); }
    if names.contains("check") && names.contains("dry-run") { command = command.mut_arg("check", |argument| argument.conflicts_with("dry-run")); }
    if names.contains("rule") && names.contains("rules") { command = command.mut_arg("rule", |argument| argument.action(ArgAction::Append).conflicts_with("rules")); }
    command
}
fn required(mut command: Command, names: &'static str) -> Command {
    for name in names.split_whitespace() { command = command.mut_arg(name, |argument| argument.required(true)); }
    command
}
fn query(name: &'static str, about: &'static str, extra: &'static str) -> Command {
    leaf(name, about, &[QUERY, extra])
        .mut_arg("dimension", |argument| argument.action(ArgAction::Append).value_parser(|raw: &str| {
            match raw.split_once('=') {
                Some((name, variant)) if !name.is_empty() && !variant.is_empty() => Ok(raw.to_string()),
                _ => Err("requires name=variant".to_string()),
            }
        }))
        .mut_arg("limit", |argument| argument.default_value("50"))
}
fn group(name: &'static str, children: Vec<Command>) -> Command { Command::new(name).subcommand_required(true).subcommands(children) }

pub(crate) fn command() -> Command {
    Command::new("coflow").version(env!("CARGO_PKG_VERSION"))
        .about("Read, edit, validate and generate Coflow configuration").subcommand_required(true)
        .arg(Arg::new("project").long("project").global(true).help("Project directory or coflow.yaml; otherwise search parent directories"))
        .arg(Arg::new("json").long("json").global(true).action(ArgAction::SetTrue).help("Write one structured JSON result"))
        .subcommands(vec![
            leaf("init", "Create a project", &[]).arg(Arg::new("dir").default_value(".")),
            leaf("describe", "Describe commands and request formats", &[]).arg(Arg::new("command")),
            group("project", vec![leaf("inspect", "Inspect project configuration", &[]), leaf("files", "List selected project files", &["kind"]),
                required(leaf("add-input", "Add a project input", &["kind path", WRITE]), "kind path"),
                required(leaf("remove-input", "Remove an input without deleting files", &["kind path", WRITE]), "kind path")]),
            required(query("context", "Read bounded context for a type or record", "examples ref-limit depth budget"), "type")
                .mut_arg("examples", |a| a.default_value("3")).mut_arg("ref-limit", |a| a.default_value("10"))
                .mut_arg("depth", |a| a.default_value("4")).mut_arg("budget", |a| a.default_value("16000")),
            group("schema", vec![leaf("check", "Check current or candidate schema", &["input limit explain"]),
                leaf("inspect", "Inspect declarations", &["type field include-derived related depth limit"]).mut_arg("field", |a| a.requires("type")),
                leaf("symbols", "List schema symbols", &["kind limit cursor"]),
                required(leaf("refs", "Find symbol uses", &["symbol limit cursor"]), "symbol")]),
            group("source", vec![required(leaf("read", "Read selected source text", &["file start-line end-line"]), "file"),
                required(leaf("write", "Replace an existing source file", &["file input expected-fingerprint", WRITE]), "file input"),
                required(leaf("create", "Create and include a source file", &["file input", WRITE]), "file input"),
                required(leaf("delete", "Delete a source file", &["file expected-fingerprint", WRITE]), "file")]),
            group("data", vec![query("list", "List records", "view"),
                required(leaf("get", "Read one record", &[RECORD, "fields dimension view"]), RECORD).mut_arg("dimension", |a| a.action(ArgAction::Append)),
                required(query("search", "Search record text", "text view"), "text"),
                required(leaf("draft", "Prepare a record draft", &[RECORD]), "type"),
                required(leaf("create", "Create a record", &[RECORD, "file input", WRITE]), "type key file input"),
                required(leaf("set", "Set an explicit field value", &[RECORD, "path", VALUE, WRITE]), "type key path"),
                required(leaf("unset", "Remove a field assignment", &[RECORD, "path", WRITE]), "type key path"),
                required(leaf("rename", "Rename a record and its references", &[RECORD, "new-key", WRITE]), "type key new-key"),
                required(leaf("delete", "Delete a record and its overrides", &[RECORD, WRITE]), RECORD),
                required(leaf("transfer", "Move a record to another file", &[RECORD, "to-file", WRITE]), "type key to-file"),
                required(leaf("refs", "Find record references", &[RECORD, "direction depth limit cursor"]), RECORD)]),
            group("dimension", vec![leaf("list", "List dimensions", &[]),
                required(leaf("inspect", "Inspect a dimension", &["dimension"]), "dimension"),
                required(leaf("variants", "List declared variants", &["dimension limit cursor"]), "dimension"),
                required(leaf("get", "Read an override and effective value", &[DIMENSION]), DIMENSION),
                required(leaf("set", "Set an explicit override", &[DIMENSION, "file path", VALUE, WRITE]), DIMENSION),
                required(leaf("clear", "Remove an explicit override", &[DIMENSION, WRITE]), DIMENSION),
                required(leaf("missing", "List fields without overrides", &[QUERY, "variant field"]), "dimension variant"),
                required(leaf("diff", "Compare overrides and effective values", &[QUERY, "from to from-base to-base field"]), "dimension")
                    .group(ArgGroup::new("from-view").args(["from", "from-base"]).required(true))
                    .group(ArgGroup::new("to-view").args(["to", "to-base"]).required(true))]),
            group("stats", vec![query("project", "Report project counts", ""), query("records", "Count records", "group-by"),
                required(query("field", "Compute field statistics", "field metrics group-by option-layer quantiles"), "field"),
                required(leaf("dimension", "Compute explicit coverage", &[QUERY, "variant group-by field"]), "dimension variant"),
                query("refs", "Compute reference counts", "direction")]),
            leaf("check", "Check data and selected business rules", &["rules rule input limit explain performance"]),
            group("rule", vec![leaf("list", "List rules and dependencies", &["type"]), required(leaf("inspect", "Inspect a rule", &["rule"]), "rule")]),
            leaf("explain", "Explain a diagnostic code", &[]).arg(Arg::new("code").required(true)),
            required(leaf("apply", "Validate and write a change set", &["input", WRITE]), "input"),
            leaf("diff", "Compare Git or candidate changes", &["base input summary include-source type view"]).mut_arg("input", |a| a.conflicts_with("base")),
            leaf("format", "Format source files or stdin", &["file stdin language check dry-run"])
                .mut_arg("file", |a| a.action(ArgAction::Append))
                .mut_arg("stdin", |a| a.conflicts_with_all(["file", "project"]).requires("language"))
                .mut_arg("language", |a| a.value_parser(["cft", "cfd"]).requires("stdin")),
            leaf("codegen", "Generate, preview or check code", &["dry-run check"]), leaf("build", "Check all rules and generate code", &[]),
            leaf("lsp", "Run the language server", &[]),
            group("skill", vec![leaf("install", "Install bundled skills", &["global"]), leaf("uninstall", "Remove bundled skills", &["global"]), leaf("status", "Inspect installed skills", &["global"])]),
            leaf("self-update", "Check or install CLI updates", &["check yes"]),
        ])
}

pub(crate) fn decode(matches: &ArgMatches) -> (String, Arguments) {
    let mut parts = Vec::new(); let mut leaf = matches;
    while let Some((name, subcommand)) = leaf.subcommand() { parts.push(name); leaf = subcommand; }
    let mut arguments = Arguments::default();
    for id in leaf.ids() {
        let name = id.as_str();
        if FLAGS.contains(&name) || name == "json" { if leaf.get_flag(name) { arguments.0.insert(name.into(), vec!["true".into()]); } }
        else if let Ok(Some(values)) = leaf.try_get_many::<String>(name) { arguments.0.insert(name.into(), values.cloned().collect()); }
    }
    (parts.join("."), arguments)
}

pub(crate) fn invocation(raw: &[std::ffi::OsString]) -> (String, bool) {
    let mut root = command();
    root.build();
    let mut current = &root;
    let mut parts = Vec::new();
    let mut json = false;
    let mut skip_value = false;
    for token in raw.iter().skip(1) {
        if skip_value { skip_value = false; continue; }
        let token = token.to_string_lossy();
        if token == "--" { break; }
        if token == "--json" { json = true; continue; }
        if let Some(option) = token.strip_prefix("--") {
            if !option.contains('=') {
                skip_value = current.get_arguments().find(|argument| argument.get_long() == Some(option))
                    .is_some_and(|argument| argument.get_action().takes_values());
            }
        } else if let Some(child) = current.get_subcommands().find(|child| child.get_name() == token) {
            parts.push(child.get_name());
            current = child;
        }
    }
    (parts.join("."), json)
}

pub(crate) fn describe(target: Option<&str>) -> Result<Value> {
    let mut root = command(); root.build(); let mut selected = &root;
    if let Some(target) = target { for name in target.split('.') { selected = selected.get_subcommands().find(|command| command.get_name() == name).ok_or_else(|| Failure::missing(format!("unknown command `{target}`")))?; } }
    fn description(command: &Command, name: &str) -> Value {
        let parameters: Vec<_> = command.get_arguments().filter(|a| a.get_id() != "help" && a.get_id() != "version").map(|a| json!({
            "name": a.get_id().as_str(), "long": a.get_long(), "required": a.is_required_set(), "description": a.get_help().map(ToString::to_string),
            "action": format!("{:?}", a.get_action()),
            "allowed_values": a.get_value_parser().possible_values().map(|values| values.map(|v| v.get_name().to_string()).collect::<Vec<_>>()),
            "defaults": a.get_default_values().iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>(),
            "conflicts": command.get_arg_conflicts_with(a).iter().map(|a| a.get_id().as_str()).collect::<Vec<_>>()
        })).collect();
        let groups: Vec<_> = command.get_groups().map(|g| json!({"name": g.get_id().as_str(), "required": g.is_required_set(), "multiple": g.clone().is_multiple(), "parameters": g.get_args().map(|id| id.as_str()).collect::<Vec<_>>()})).collect();
        json!({"command": name, "description": command.get_about().map(ToString::to_string), "parameters": parameters, "groups": groups,
            "subcommands": command.get_subcommands().map(|child| description(child, &if name == "coflow" { child.get_name().to_string() } else { format!("{name}.{}", child.get_name()) })).collect::<Vec<_>>()})
    }
    Ok(json!({"protocol_version": 1, "definition": description(selected, target.unwrap_or("coflow")),
        "exit_codes": {"0": "success", "1": "validation or incomplete result", "2": "invalid request", "3": "conflict", "4": "execution failure"},
        "input_schema": coflow_runtime::automation::change_set_schema(), "output_schema": coflow_runtime::automation::response_schema()}))
}
