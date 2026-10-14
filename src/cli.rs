use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use coflow_runtime::automation::{Arguments, Failure, Result};
use serde_json::{json, Value};

#[derive(Debug)]
pub(crate) struct SelfUpdateArgs {
    pub check: bool,
    pub yes: bool,
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct InitResult {
    pub config_path: String,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct SelfUpdateResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub updated: bool,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CommandParameter {
    pub name: String,
    pub long: Option<String>,
    pub required: bool,
    pub description: Option<String>,
    pub action: String,
    pub allowed_values: Option<Vec<String>>,
    pub defaults: Vec<String>,
    pub conflicts: Vec<String>,
    pub requires: Vec<String>,
    pub requires_any: Vec<String>,
    pub value_constraints: Option<Value>,
    pub conditional_defaults: Vec<Value>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CommandGroup {
    pub name: String,
    pub required: bool,
    pub multiple: bool,
    pub parameters: Vec<String>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct InputFormat {
    pub parameter: String,
    pub format: String,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CommandDescription {
    pub command: String,
    pub description: Option<String>,
    pub parameters: Vec<CommandParameter>,
    pub groups: Vec<CommandGroup>,
    pub read_only: Option<bool>,
    pub writes: Option<bool>,
    pub input_schema: Option<Value>,
    pub output_schema: Option<Value>,
    pub input_formats: Vec<InputFormat>,
    pub examples: Vec<String>,
    pub subcommands: Vec<CommandDescription>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DescribeResult {
    pub protocol_version: u32,
    pub definition: CommandDescription,
    pub exit_codes: std::collections::BTreeMap<String, String>,
    pub input_schema: Option<Value>,
    pub output_schema: Value,
}

fn output_schema(target: Option<&str>) -> schemars::schema::RootSchema {
    use coflow_runtime::automation::response_schema_of;
    match target {
        Some("init") => response_schema_of::<Option<InitResult>>(),
        Some("self-update") => response_schema_of::<Option<SelfUpdateResult>>(),
        Some("skill.install" | "skill.uninstall" | "skill.status") => {
            response_schema_of::<Option<crate::skill_commands::SkillReport>>()
        }
        Some("describe") => response_schema_of::<Option<DescribeResult>>(),
        Some("lsp") => response_schema_of::<Option<()>>(),
        target => coflow_runtime::automation::response_schema_for(target),
    }
}

const FLAGS: &[&str] = &[
    "include-derived",
    "related",
    "dry-run",
    "check",
    "rules",
    "explain",
    "summary",
    "include-source",
    "stdin",
    "global",
    "yes",
    "performance",
    "from-base",
    "to-base",
];
const QUERY: &str = "type include-derived file key fields where dimension sort limit cursor";
const WRITE: &str = "dry-run rules";
const RECORD: &str = "type key";
const VALUE: &str = "value-cfd value-file";
const DIMENSION: &str = "dimension variant type key field";

fn option(name: &'static str) -> Arg {
    let argument = Arg::new(name).long(name).help(match name {
        "type" => "Actual type name",
        "key" => "Record key",
        "file" => "Source path relative to the project",
        "input" => "Request or source content file; - reads stdin",
        "value-cfd" => "CFD expression parsed using the field type",
        "value-file" => "File containing a CFD expression; - reads stdin",
        "dimension" => "Dimension name, or name=variant in record queries",
        "fields" => "Comma-separated field paths to return",
        "where" => "Typed boolean filter",
        "sort" => "Comma-separated paths; -path sorts descending",
        "limit" => "Maximum number of returned items or diagnostics",
        "cursor" => "Next cursor returned by the same query",
        "dry-run" => "Validate and preview without writing files",
        "rules" => "Execute all business rules",
        "rule" => "Execute or inspect a rule by its returned identity",
        "check" => "Check current state without writing files",
        "expected-fingerprint" => "Require the source content fingerprint to match",
        "from-base" => "Compare from the base value",
        "to-base" => "Compare to the base value",
        _ => "Command option",
    });
    if FLAGS.contains(&name) {
        return if name == "global" {
            argument.short('g').action(ArgAction::SetTrue)
        } else {
            argument.action(ArgAction::SetTrue)
        };
    }
    let argument = argument.action(ArgAction::Set);
    if [
        "limit",
        "examples",
        "ref-limit",
        "depth",
        "budget",
        "start-line",
        "end-line",
        "option-layer",
    ]
    .contains(&name)
    {
        argument.value_parser(move |raw: &str| -> std::result::Result<String, String> {
            let value = raw
                .parse::<usize>()
                .map_err(|_| "requires a nonnegative integer".to_string())?;
            if ["limit", "budget", "option-layer"].contains(&name) && value == 0 {
                return Err("must be greater than zero".into());
            }
            Ok(raw.into())
        })
    } else {
        argument.allow_hyphen_values(["value-cfd", "sort"].contains(&name))
    }
}

fn leaf(name: &'static str, about: &'static str, groups: &[&'static str]) -> Command {
    let mut command = Command::new(name).about(about);
    let mut names = std::collections::BTreeSet::new();
    for group in groups {
        for name in group.split_whitespace() {
            if names.insert(name) {
                command = command.arg(option(name));
            }
        }
    }
    if names.contains("value-cfd") {
        command = command.group(
            ArgGroup::new("value")
                .args(["value-cfd", "value-file"])
                .required(true),
        );
    }
    if names.contains("check") && names.contains("dry-run") {
        command = command.mut_arg("check", |argument| argument.conflicts_with("dry-run"));
    }
    if names.contains("rule") && names.contains("rules") {
        command = command.mut_arg("rule", |argument| {
            argument.action(ArgAction::Append).conflicts_with("rules")
        });
    }
    command
}
fn required(mut command: Command, names: &'static str) -> Command {
    for name in names.split_whitespace() {
        command = command.mut_arg(name, |argument| argument.required(true));
    }
    command
}
fn query(name: &'static str, about: &'static str, extra: &'static str) -> Command {
    leaf(name, about, &[QUERY, extra])
        .mut_arg("dimension", |argument| {
            argument
                .action(ArgAction::Append)
                .value_parser(|raw: &str| match raw.split_once('=') {
                    Some((name, variant)) if !name.is_empty() && !variant.is_empty() => {
                        Ok(raw.to_string())
                    }
                    _ => Err("requires name=variant".to_string()),
                })
        })
        .mut_arg("limit", |argument| argument.default_value("50"))
}
fn group(name: &'static str, children: Vec<Command>) -> Command {
    Command::new(name)
        .subcommand_required(true)
        .subcommands(children)
}

pub(crate) fn command() -> Command {
    let root = Command::new("coflow")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Read, edit, validate and generate Coflow configuration")
        .subcommand_required(true)
        .arg(
            Arg::new("project")
                .long("project")
                .global(true)
                .help("Project directory or coflow.yaml; otherwise search parent directories"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Write one structured JSON result"),
        )
        .subcommands(vec![
            leaf("init", "Create a project", &[]).arg(Arg::new("dir").default_value(".")),
            leaf("describe", "Describe commands and request formats", &[]).arg(Arg::new("command")),
            group(
                "project",
                vec![
                    leaf("inspect", "Inspect project configuration", &[]),
                    leaf("files", "List selected project files", &["kind"]),
                    required(
                        leaf("add-input", "Add a project input", &["kind path", WRITE]),
                        "kind path",
                    ),
                    required(
                        leaf(
                            "remove-input",
                            "Remove an input without deleting files",
                            &["kind path", WRITE],
                        ),
                        "kind path",
                    ),
                ],
            ),
            required(
                query(
                    "context",
                    "Read bounded context for a type or record",
                    "examples ref-limit depth budget",
                ),
                "type",
            )
            .mut_arg("examples", |a| a.default_value("3"))
            .mut_arg("ref-limit", |a| a.default_value("10"))
            .mut_arg("depth", |a| a.default_value("4"))
            .mut_arg("budget", |a| a.default_value("16000")),
            group(
                "schema",
                vec![
                    leaf(
                        "check",
                        "Check current or candidate schema",
                        &["input limit explain"],
                    ),
                    leaf(
                        "inspect",
                        "Inspect declarations",
                        &["type field include-derived related depth limit"],
                    ),
                    leaf("symbols", "List schema symbols", &["kind limit cursor"]),
                    required(
                        leaf("refs", "Find symbol uses", &["symbol limit cursor"]),
                        "symbol",
                    ),
                ],
            ),
            group(
                "source",
                vec![
                    required(
                        leaf(
                            "read",
                            "Read selected source text",
                            &["file start-line end-line"],
                        ),
                        "file",
                    ),
                    required(
                        leaf(
                            "write",
                            "Replace an existing source file",
                            &["file input expected-fingerprint", WRITE],
                        ),
                        "file input",
                    ),
                    required(
                        leaf(
                            "create",
                            "Create and include a source file",
                            &["file input", WRITE],
                        ),
                        "file input",
                    ),
                    required(
                        leaf(
                            "delete",
                            "Delete a source file",
                            &["file expected-fingerprint", WRITE],
                        ),
                        "file",
                    ),
                ],
            ),
            group(
                "data",
                vec![
                    query("list", "List records", "view"),
                    required(
                        leaf("get", "Read one record", &[RECORD, "fields dimension view"]),
                        RECORD,
                    )
                    .mut_arg("dimension", |a| a.action(ArgAction::Append)),
                    required(query("search", "Search record text", "text view"), "text"),
                    required(leaf("draft", "Prepare a record draft", &[RECORD]), "type"),
                    required(
                        leaf("create", "Create a record", &[RECORD, "file input", WRITE]),
                        "type key file input",
                    ),
                    required(
                        leaf(
                            "set",
                            "Set an explicit field value",
                            &[RECORD, "path", VALUE, WRITE],
                        ),
                        "type key path",
                    ),
                    required(
                        leaf(
                            "unset",
                            "Remove a field assignment",
                            &[RECORD, "path", WRITE],
                        ),
                        "type key path",
                    ),
                    required(
                        leaf(
                            "rename",
                            "Rename a record and its references",
                            &[RECORD, "new-key", WRITE],
                        ),
                        "type key new-key",
                    ),
                    required(
                        leaf(
                            "delete",
                            "Delete a record and its overrides",
                            &[RECORD, WRITE],
                        ),
                        RECORD,
                    ),
                    required(
                        leaf(
                            "transfer",
                            "Move a record to another file",
                            &[RECORD, "to-file", WRITE],
                        ),
                        "type key to-file",
                    ),
                    required(
                        leaf(
                            "refs",
                            "Find record references",
                            &[RECORD, "direction depth limit cursor"],
                        ),
                        RECORD,
                    ),
                ],
            ),
            group(
                "dimension",
                vec![
                    leaf("list", "List dimensions", &[]),
                    required(
                        leaf("inspect", "Inspect a dimension", &["dimension"]),
                        "dimension",
                    ),
                    required(
                        leaf(
                            "variants",
                            "List declared variants",
                            &["dimension limit cursor"],
                        ),
                        "dimension",
                    ),
                    required(
                        leaf("get", "Read an override and effective value", &[DIMENSION]),
                        DIMENSION,
                    ),
                    required(
                        leaf(
                            "set",
                            "Set an explicit override",
                            &[DIMENSION, "file path", VALUE, WRITE],
                        ),
                        DIMENSION,
                    ),
                    required(
                        leaf("clear", "Remove an explicit override", &[DIMENSION, WRITE]),
                        DIMENSION,
                    ),
                    required(
                        leaf(
                            "missing",
                            "List fields without overrides",
                            &[QUERY, "variant field"],
                        ),
                        "dimension variant",
                    ),
                    required(
                        leaf(
                            "diff",
                            "Compare overrides and effective values",
                            &[QUERY, "from to from-base to-base field"],
                        ),
                        "dimension",
                    )
                    .group(
                        ArgGroup::new("from-view")
                            .args(["from", "from-base"])
                            .required(true),
                    )
                    .group(
                        ArgGroup::new("to-view")
                            .args(["to", "to-base"])
                            .required(true),
                    ),
                ],
            ),
            group(
                "stats",
                vec![
                    query("project", "Report project counts", ""),
                    query("records", "Count records", "group-by"),
                    required(
                        query(
                            "field",
                            "Compute field statistics",
                            "field metrics group-by option-layer quantiles",
                        ),
                        "field",
                    ),
                    required(
                        leaf(
                            "dimension",
                            "Compute explicit coverage",
                            &[QUERY, "variant group-by field"],
                        ),
                        "dimension variant",
                    ),
                    query("refs", "Compute reference counts", "direction"),
                ],
            ),
            leaf(
                "check",
                "Check data and selected business rules",
                &["rules rule input limit explain performance"],
            ),
            group(
                "rule",
                vec![
                    leaf("list", "List rules and dependencies", &["type"]),
                    required(leaf("inspect", "Inspect a rule", &["rule"]), "rule"),
                ],
            ),
            leaf("explain", "Explain a diagnostic code", &[]).arg(Arg::new("code").required(true)),
            required(
                leaf(
                    "apply",
                    "Validate and write a change set",
                    &["input", WRITE],
                ),
                "input",
            ),
            leaf(
                "diff",
                "Compare Git or candidate changes",
                &["base input summary include-source type view"],
            )
            .mut_arg("input", |a| a.conflicts_with("base")),
            leaf(
                "format",
                "Format source files or stdin",
                &["file stdin language check dry-run"],
            )
            .mut_arg("file", |a| a.action(ArgAction::Append))
            .mut_arg("stdin", |a| a.conflicts_with_all(["file", "project"]))
            .mut_arg("language", |a| a.value_parser(["cft", "cfd"])),
            leaf(
                "codegen",
                "Generate, preview or check code",
                &["dry-run check"],
            ),
            leaf("build", "Check all rules and generate code", &[]),
            leaf("lsp", "Run the language server", &[]),
            group(
                "skill",
                vec![
                    leaf("install", "Install bundled skills", &["global"]),
                    leaf("uninstall", "Remove bundled skills", &["global"]),
                    leaf("status", "Inspect installed skills", &["global"]),
                ],
            ),
            leaf(
                "self-update",
                "Check or install CLI updates",
                &["check yes"],
            ),
        ]);
    configure(root, "")
}

fn configure(mut command: Command, path: &str) -> Command {
    command = command.mut_subcommands(|child| {
        let name = if path.is_empty() {
            child.get_name().to_string()
        } else {
            format!("{path}.{}", child.get_name())
        };
        configure(child, &name)
    });
    // 允许值与执行端的同名语义一致，在读取项目之前完成参数格式校验。
    let allowed: Vec<(&str, Vec<&'static str>)> = match path {
        "project.files" | "project.add-input" | "project.remove-input" => {
            vec![("kind", vec!["schema", "data"])]
        }
        "schema.symbols" => vec![("kind", vec!["type", "field", "enum", "const", "rule"])],
        "data.get" | "data.list" | "data.search" => {
            vec![("view", vec!["source", "effective", "both"])]
        }
        "diff" => vec![(
            "view",
            vec!["source", "schema", "record", "field", "dimension"],
        )],
        "data.refs" | "stats.refs" => vec![("direction", vec!["incoming", "outgoing", "both"])],
        "stats.records" => vec![("group-by", vec!["type", "file"])],
        "stats.dimension" => vec![("group-by", vec!["all", "type", "file"])],
        "stats.field" => vec![("group-by", vec!["value"])],
        _ => Vec::new(),
    };
    for (name, values) in allowed {
        command = command.mut_arg(name, |argument| argument.value_parser(values));
    }
    let defaults: Vec<(&str, &'static str)> = match path {
        "data.get" | "data.list" | "data.search" => vec![("view", "effective")],
        "data.refs" => vec![("direction", "both"), ("depth", "1"), ("limit", "50")],
        "stats.refs" => vec![("direction", "both")],
        "stats.records" => vec![("group-by", "type")],
        "stats.dimension" => vec![("group-by", "all"), ("limit", "50")],
        "stats.field" => vec![("quantiles", "0.5,0.9,0.95,0.99")],
        "schema.inspect" => vec![("depth", "4"), ("limit", "100")],
        "schema.symbols" | "schema.refs" => vec![("limit", "50")],
        "dimension.variants" | "dimension.missing" | "dimension.diff" => vec![("limit", "50")],
        "context" => vec![("depth", "4")],
        _ => Vec::new(),
    };
    for (name, value) in defaults {
        command = command.mut_arg(name, |argument| argument.default_value(value));
    }
    if path == "data.get" {
        command = command.mut_arg("dimension", |argument| {
            argument.value_parser(|raw: &str| match raw.split_once('=') {
                Some((name, variant)) if !name.is_empty() && !variant.is_empty() => {
                    Ok(raw.to_string())
                }
                _ => Err("requires name=variant".to_string()),
            })
        });
    }
    if path == "stats.field" {
        command = command.mut_arg("quantiles", |argument| {
            argument.value_parser(|raw: &str| {
                if raw.split(',').all(|value| {
                    value
                        .parse::<f64>()
                        .is_ok_and(|value| value.is_finite() && (0.0..=1.0).contains(&value))
                }) {
                    Ok(raw.to_string())
                } else {
                    Err("requires comma-separated quantiles in [0,1]".to_string())
                }
            })
        });
        command = command.mut_arg("metrics", |argument| {
            argument.value_parser(|raw: &str| {
                if raw.split(',').all(|value| {
                    [
                        "count",
                        "min",
                        "max",
                        "sum",
                        "mean",
                        "quantiles",
                        "some",
                        "none",
                        "empty_count",
                        "length_distribution",
                        "length_min",
                        "length_max",
                    ]
                    .contains(&value)
                }) {
                    Ok(raw.to_string())
                } else {
                    Err("contains an unknown metric".to_string())
                }
            })
        });
    }
    if path == "data.refs" {
        command = command.mut_arg("depth", |argument| {
            argument.value_parser(|raw: &str| {
                if raw
                    .parse::<usize>()
                    .is_ok_and(|depth| (1..=64).contains(&depth))
                {
                    Ok(raw.to_string())
                } else {
                    Err("requires a depth in 1..=64".to_string())
                }
            })
        });
    }
    if path == "diff" {
        command = command.mut_arg("base", |argument| {
            argument.default_value("HEAD").default_value_if(
                "input",
                clap::builder::ArgPredicate::IsPresent,
                None::<&str>,
            )
        });
    }
    let dependencies: Vec<_> = command
        .get_arguments()
        .filter_map(|argument| {
            let requires = parameter_requires(path, argument.get_id().as_str());
            (!requires.is_empty()).then(|| (argument.get_id().as_str().to_string(), requires))
        })
        .collect();
    for (name, requires) in dependencies {
        command = command.mut_arg(name, |argument| argument.requires_all(requires));
    }
    command
}

/// 参数依赖由解析器与自描述共享，不能分别维护两套命令约束。
fn parameter_requires(command: &str, parameter: &str) -> Vec<&'static str> {
    match (command, parameter) {
        ("schema.inspect", "field") => vec!["type"],
        ("format", "stdin") => vec!["language"],
        ("format", "language") => vec!["stdin"],
        _ => Vec::new(),
    }
}
fn parameter_constraints(command: &str, parameter: &str) -> Option<Value> {
    match (command, parameter) {
        ("data.refs", "depth") => Some(json!({"type":"integer","minimum":1,"maximum":64})),
        (_, "limit" | "budget" | "option-layer") => Some(json!({"type":"integer","minimum":1})),
        (_, "examples" | "ref-limit" | "depth" | "start-line" | "end-line") => {
            Some(json!({"type":"integer","minimum":0}))
        }
        ("stats.field", "quantiles") => Some(
            json!({"format":"comma_separated","items":{"type":"number","minimum":0,"maximum":1}}),
        ),
        ("stats.field", "metrics") => Some(
            json!({"format":"comma_separated","items":{"enum":["count","min","max","sum","mean","quantiles","some","none","empty_count","length_distribution","length_min","length_max"]}}),
        ),
        (
            "data.get" | "data.list" | "data.search" | "context" | "stats.project"
            | "stats.records" | "stats.field" | "stats.refs",
            "dimension",
        ) => Some(json!({"format":"name=variant"})),
        _ => None,
    }
}

pub(crate) fn decode(matches: &ArgMatches) -> (String, Arguments) {
    let mut parts = Vec::new();
    let mut leaf = matches;
    while let Some((name, subcommand)) = leaf.subcommand() {
        parts.push(name);
        leaf = subcommand;
    }
    let mut arguments = Arguments::default();
    for id in leaf.ids() {
        let name = id.as_str();
        if FLAGS.contains(&name) || name == "json" {
            if leaf.get_flag(name) {
                arguments.0.insert(name.into(), vec!["true".into()]);
            }
        } else if let Ok(Some(values)) = leaf.try_get_many::<String>(name) {
            arguments.0.insert(name.into(), values.cloned().collect());
        }
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
        if skip_value {
            skip_value = false;
            continue;
        }
        let token = token.to_string_lossy();
        if token == "--" {
            break;
        }
        if token == "--json" {
            json = true;
            continue;
        }
        if let Some(option) = token.strip_prefix("--") {
            if !option.contains('=') {
                skip_value = current
                    .get_arguments()
                    .find(|argument| argument.get_long() == Some(option))
                    .is_some_and(|argument| argument.get_action().takes_values());
            }
        } else if let Some(child) = current
            .get_subcommands()
            .find(|child| child.get_name() == token)
        {
            parts.push(child.get_name());
            current = child;
        }
    }
    (parts.join("."), json)
}

pub(crate) fn describe(target: Option<&str>) -> Result<Value> {
    let mut root = command();
    root.build();
    let mut selected = &root;
    if let Some(target) = target {
        for name in target.split('.') {
            selected = selected
                .get_subcommands()
                .find(|command| command.get_name() == name)
                .ok_or_else(|| Failure::missing(format!("unknown command `{target}`")))?;
        }
    }
    fn description(command: &Command, name: &str) -> Value {
        let leaf = command.get_subcommands().next().is_none();
        // 写入能力由实际命令选项与无预览模式的宿主命令共同决定。
        let writes = command
            .get_arguments()
            .any(|argument| argument.get_id() == "dry-run")
            || matches!(
                name,
                "init" | "build" | "skill.install" | "skill.uninstall" | "self-update"
            );
        let parameters: Vec<_> = command.get_arguments().filter(|a| a.get_id() != "help" && a.get_id() != "version").map(|a| json!({
            "name": a.get_id().as_str(), "long": a.get_long(), "required": a.is_required_set(), "description": a.get_help().map(ToString::to_string),
            "action": format!("{:?}", a.get_action()),
            "allowed_values": a.get_value_parser().possible_values().map(|values| values.map(|v| v.get_name().to_string()).collect::<Vec<_>>()),
            "defaults": a.get_default_values().iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>(),
            "conflicts": command.get_arg_conflicts_with(a).iter().map(|a| a.get_id().as_str()).collect::<Vec<_>>(),
            "requires": parameter_requires(name,a.get_id().as_str()),
            "requires_any": if name=="self-update" && a.get_id()=="json" {vec!["check","yes"]}else{Vec::<&str>::new()},
            "value_constraints": parameter_constraints(name,a.get_id().as_str()),
            "conditional_defaults": if name=="diff" && a.get_id()=="base" {vec![json!({"when_present":"input","value":null})]}else{Vec::<Value>::new()}
        })).collect();
        let groups: Vec<_> = command.get_groups().map(|g| json!({"name": g.get_id().as_str(), "required": g.is_required_set(), "multiple": g.clone().is_multiple(), "parameters": g.get_args().map(|id| id.as_str()).collect::<Vec<_>>()})).collect();
        json!({"command": name, "description": command.get_about().map(ToString::to_string), "parameters": parameters, "groups": groups,
            "read_only": leaf.then_some(!writes), "writes": leaf.then_some(writes),
            "input_schema": input_schema(name),
            "output_schema": leaf.then(|| output_schema(Some(name))),
            "input_formats": command.get_arguments().filter_map(|argument| {
                let parameter = argument.get_id().as_str();
                let format = match (name, parameter) {
                    ("source.write" | "source.create", "input") => "source_text",
                    (_, "input") if !input_schema(name).is_null() => "json",
                    (_, "value-cfd" | "value-file") => "cfd_expression",
                    ("format", "stdin") => "source_text",
                    _ => return None,
                };
                Some(json!({"parameter": parameter, "format": format}))
            }).collect::<Vec<_>>(),
            "examples": if leaf { vec![example(command, name)] } else { Vec::new() },
            "subcommands": command.get_subcommands().filter(|child| child.get_name() != "help").map(|child| description(child, &if name == "coflow" { child.get_name().to_string() } else { format!("{name}.{}", child.get_name()) })).collect::<Vec<_>>()})
    }
    let result = json!({"protocol_version": 1, "definition": description(selected, target.unwrap_or("coflow")),
        "exit_codes": {"0": "success", "1": "validation or incomplete result", "2": "invalid request", "3": "conflict", "4": "execution failure"},
        "input_schema": target.map(input_schema), "output_schema": output_schema(target)});
    let result: DescribeResult = serde_json::from_value(result)?;
    Ok(serde_json::to_value(result)?)
}

fn input_schema(name: &str) -> Value {
    // Schema 来自执行时使用的 DTO；源码和 CFD 表达式通过独立的格式说明表达。
    match name {
        "apply" | "check" | "schema.check" | "diff" => {
            json!(coflow_runtime::automation::change_set_schema())
        }
        "data.create" => json!(coflow_runtime::automation::record_fields_schema()),
        _ => Value::Null,
    }
}

fn example(command: &Command, name: &str) -> String {
    let mut parameters = std::collections::BTreeSet::new();
    for argument in command
        .get_arguments()
        .filter(|argument| argument.is_required_set())
    {
        parameters.insert(argument.get_id().as_str());
    }
    // 必填互斥组只选一个成员，示例随 Clap 命令树变化而更新。
    for group in command.get_groups().filter(|group| group.is_required_set()) {
        if !group.get_args().any(|id| parameters.contains(id.as_str())) {
            if let Some(argument) = group.get_args().next() {
                parameters.insert(argument.as_str());
            }
        }
    }
    if name == "self-update" {
        parameters.insert("check");
    }
    let mut result = format!("coflow {}", name.replace('.', " "));
    for argument in command
        .get_arguments()
        .filter(|argument| parameters.contains(argument.get_id().as_str()))
    {
        if let Some(long) = argument.get_long() {
            result.push_str(&format!(" --{long}"));
        }
        if argument.get_action().takes_values() {
            let value = match argument.get_id().as_str() {
                "type" | "symbol" => "Item",
                "key" => "sword",
                "new-key" => "sword2",
                "path" | "field" => "value",
                "file" => "data/items.cfd",
                "to-file" => "data/other.cfd",
                "input" => "request.json",
                "value-cfd" => "1",
                "dimension" => "language",
                "variant" => "en",
                "kind" => "data",
                "rule" => "Item#0",
                "code" => "DATA-006",
                "text" => "sword",
                "from" => "en",
                "to" => "zh",
                _ => "example",
            };
            result.push(' ');
            result.push_str(value);
        }
    }
    if name != "lsp" {
        result.push_str(" --json");
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_described_leaf_has_an_example_accepted_by_the_command_parser() {
        let report = super::describe(None).unwrap();
        fn check(definition: &serde_json::Value) {
            let children = definition["subcommands"].as_array().unwrap();
            if children.is_empty() {
                assert!(definition["read_only"].is_boolean());
                for example in definition["examples"].as_array().unwrap() {
                    let example = example.as_str().unwrap();
                    super::command()
                        .try_get_matches_from(example.split_whitespace())
                        .unwrap_or_else(|error| panic!("{example}: {error}"));
                }
            } else {
                for child in children {
                    check(child);
                }
            }
        }
        check(&report["definition"]);
    }
}
