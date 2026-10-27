use clap::{Arg, ArgAction, ArgGroup, ArgMatches, Command};
use coflow_runtime::automation::{Arguments, Failure, Result};
use serde_json::{json, Value};

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct InitResult {
    pub config_path: String,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CommandParameter {
    pub name: String,
    pub description: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub positional: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_values: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defaults: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires_any: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_constraints: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
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
pub(crate) struct CommandSummary {
    pub command: String,
    pub description: String,
    pub writes: bool,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct CommandDescription {
    pub command: String,
    pub description: String,
    pub parameters: Vec<CommandParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<CommandGroup>,
    pub read_only: bool,
    pub writes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_formats: Vec<InputFormat>,
    pub examples: Vec<String>,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct DescribeResult {
    pub protocol_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<CommandDescription>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub globals: Vec<CommandParameter>,
    pub exit_codes: std::collections::BTreeMap<String, String>,
    pub next: String,
}

fn output_schema(target: Option<&str>) -> schemars::schema::RootSchema {
    use coflow_runtime::automation::response_schema_of;
    match target {
        Some("init") => response_schema_of::<Option<InitResult>>(),
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
const QUERY: &str = "type include-derived file id fields where dimension sort limit cursor";
const WRITE: &str = "dry-run rules if-revision";
const RECORD: &str = "type id";
const VALUE: &str = "value-cfd value-file";
const DIMENSION: &str = "dimension variant type id field";

#[expect(
    clippy::unreachable,
    reason = "固定命令参数表与帮助分支必须穷举对应，遗漏属于开发错误。"
)]
fn option(name: &'static str) -> Arg {
    let argument = Arg::new(name).long(name).help(match name {
        "type" => "Actual type name",
        "id" => "Record id",
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
        "include-derived" => "Include types derived from --type",
        "related" => "Include declarations referenced by the selected type",
        "explain" => "Include full diagnostic explanations",
        "summary" => "Return diff counts without individual changes",
        "include-source" => "Include source text in the diff",
        "stdin" => "Read source text from stdin",
        "global" => "Use the global skills installation",
        "yes" => "Install the update without prompting",
        "performance" => "Include rule execution timings and work counts",
        "field" => "Field name; use --path for nested value locations",
        "path" => "Field path, or project input path for project commands",
        "variant" => "Variant name within the selected dimension",
        "kind" => "Filter or select the input or schema symbol kind",
        "examples" => "Maximum example records in context",
        "ref-limit" => "Maximum referenced records in context",
        "depth" => "Maximum nested schema or reference traversal depth",
        "budget" => "Maximum serialized context characters",
        "start-line" => "First source line to return (zero-based, inclusive)",
        "end-line" => "First source line to exclude (zero-based)",
        "view" => "Select source assignments, effective values, or a diff view",
        "text" => "Text to find in record ids and values",
        "new-id" => "New record id; references are updated automatically",
        "to-file" => "Destination CFD file relative to the project",
        "direction" => "Reference edge direction relative to the selected record",
        "from" => "Variant to compare from",
        "to" => "Variant to compare to",
        "group-by" => "Group results by the selected property",
        "metrics" => "Comma-separated statistic names",
        "option-layer" => "Option wrapper layer to inspect (one-based)",
        "quantiles" => "Comma-separated quantiles between 0 and 1",
        "symbol" => "Schema symbol name, for example Item or Item.value",
        "base" => "Git revision to compare against",
        "language" => "Source language used when formatting stdin",
        "if-revision" => "Require the project revision returned by the preceding read",
        "expected-cfd" => {
            "Require the current effective field value to equal this typed CFD expression"
        }
        _ => unreachable!("missing parameter help for {name}"),
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
        argument.allow_hyphen_values(["value-cfd", "expected-cfd", "sort"].contains(&name))
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

#[expect(
    clippy::too_many_lines,
    reason = "完整命令或设置分支集中列出，便于核对输入与行为。"
)]
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
        .arg(
            Arg::new("pretty")
                .long("pretty")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Write indented JSON (implies --json)"),
        )
        .arg(
            Arg::new("details")
                .long("details")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Include execution details and all diagnostics (unless --limit is set)"),
        )
        .subcommands(vec![
            leaf("init", "Create a project", &[]).arg(
                Arg::new("dir")
                    .default_value(".")
                    .help("Directory in which to create the project"),
            ),
            leaf("describe", "List commands or describe one command", &[])
                .arg(
                    Arg::new("command")
                        .help("Command or group name using dots, for example data.set"),
                )
                .arg(
                    Arg::new("schema")
                        .long("schema")
                        .value_parser(["input", "output", "both"])
                        .requires("command")
                        .help("Include the selected JSON Schema once; requires a leaf command"),
                ),
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
                        &["input limit explain if-revision"],
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
                        "type id file input",
                    ),
                    required(
                        leaf(
                            "set",
                            "Set an explicit field value",
                            &[RECORD, "path expected-cfd", VALUE, WRITE],
                        ),
                        "type id path",
                    ),
                    required(
                        leaf(
                            "unset",
                            "Remove a field assignment",
                            &[RECORD, "path", WRITE],
                        ),
                        "type id path",
                    ),
                    required(
                        leaf(
                            "rename",
                            "Rename a record and its references",
                            &[RECORD, "new-id", WRITE],
                        ),
                        "type id new-id",
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
                        "type id to-file",
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
                &["rules rule input limit explain performance if-revision"],
            ),
            group(
                "rule",
                vec![
                    leaf("list", "List rules and dependencies", &["type"]),
                    required(leaf("inspect", "Inspect a rule", &["rule"]), "rule"),
                ],
            ),
            leaf("explain", "Explain a diagnostic code", &[]).arg(
                Arg::new("code")
                    .required(true)
                    .help("Diagnostic code returned by a command"),
            ),
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
                &["base input summary include-source type view if-revision"],
            )
            .mut_arg("input", |a| a.conflicts_with("base")),
            leaf(
                "format",
                "Format source files or stdin",
                &["file stdin language check dry-run if-revision"],
            )
            .mut_arg("file", |a| a.action(ArgAction::Append))
            .mut_arg("stdin", |a| {
                a.conflicts_with_all(["file", "project", "if-revision"])
            })
            .mut_arg("language", |a| a.value_parser(["cft", "cfd"])),
            leaf(
                "codegen",
                "Generate, preview or check code",
                &["dry-run check if-revision"],
            ),
            leaf(
                "build",
                "Check all rules and generate code",
                &["if-revision"],
            ),
            leaf("lsp", "Run the language server", &[]),
            group(
                "skill",
                vec![
                    leaf("install", "Install bundled skills", &["global"]),
                    leaf("uninstall", "Remove bundled skills", &["global"]),
                    leaf("status", "Inspect installed skills", &["global"]),
                ],
            ),
        ]);
    configure(root, "")
}

#[expect(
    clippy::too_many_lines,
    reason = "完整命令或设置分支集中列出，便于核对输入与行为。"
)]
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
        "schema.symbols" | "schema.refs" | "dimension.variants" | "dimension.missing"
        | "dimension.diff" => vec![("limit", "50")],
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
        if FLAGS.contains(&name) || ["json", "pretty", "details"].contains(&name) {
            if leaf.get_flag(name) {
                arguments.0.insert(name.into(), vec!["true".into()]);
            }
        } else if let Ok(Some(values)) = leaf.try_get_many::<String>(name) {
            arguments.0.insert(name.into(), values.cloned().collect());
        }
    }
    if arguments.flag("pretty") {
        arguments.0.insert("json".into(), vec!["true".into()]);
    }
    (parts.join("."), arguments)
}

pub(crate) fn invocation(raw: &[std::ffi::OsString]) -> (String, bool, bool) {
    let mut root = command();
    root.build();
    let mut current = &root;
    let mut parts = Vec::new();
    let mut json = false;
    let mut pretty = false;
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
        if token == "--json" || token == "--pretty" {
            json = true;
            pretty |= token == "--pretty";
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
    (parts.join("."), json, pretty)
}

pub(crate) fn describe(target: Option<&str>, schema: Option<&str>) -> Result<Value> {
    let mut root = command();
    root.build();
    let mut selected = &root;
    if let Some(target) = target {
        for name in target.split('.') {
            selected = selected
                .get_subcommands()
                .find(|command| command.get_name() == name && name != "help")
                .ok_or_else(|| {
                    Failure::missing(format!(
                        "unknown command `{target}`; run `coflow describe --json`"
                    ))
                })?;
        }
    }
    let is_leaf = selected.get_subcommands().next().is_none();
    if schema.is_some() && !is_leaf {
        return Err(Failure::request(
            "--schema requires a leaf command, for example `describe data.set --schema output`",
        ));
    }
    let name = target.unwrap_or("coflow");
    let definition = is_leaf.then(|| description(selected, name, schema));
    let mut commands = Vec::new();
    if !is_leaf {
        collect_commands(selected, target.unwrap_or(""), &mut commands);
    }
    let report = DescribeResult {
        protocol_version: 1,
        definition,
        commands,
        globals: if is_leaf {
            Vec::new()
        } else {
            root.get_arguments()
                .filter(|argument| argument.is_global_set())
                .map(|argument| parameter(&root, "coflow", argument))
                .collect()
        },
        exit_codes: [
            ("0", "success"),
            ("1", "validation or incomplete result"),
            ("2", "invalid request"),
            ("3", "conflict"),
            ("4", "execution failure"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect(),
        next: if is_leaf {
            format!("coflow describe {name} --schema output --json")
        } else {
            "coflow describe <command> --json".into()
        },
    };
    Ok(serde_json::to_value(report)?)
}

fn writes(command: &Command, name: &str) -> bool {
    command
        .get_arguments()
        .any(|argument| argument.get_id() == "dry-run")
        || matches!(name, "init" | "build" | "skill.install" | "skill.uninstall")
}

/// 命令目录只返回导航所需信息；大体积 Schema 仅在明确请求时生成一次。
fn collect_commands(command: &Command, prefix: &str, entries: &mut Vec<CommandSummary>) {
    for child in command
        .get_subcommands()
        .filter(|child| child.get_name() != "help")
    {
        let name = if prefix.is_empty() {
            child.get_name().to_string()
        } else {
            format!("{prefix}.{}", child.get_name())
        };
        if child.get_subcommands().next().is_none() {
            entries.push(CommandSummary {
                writes: writes(child, &name),
                command: name,
                description: child
                    .get_about()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            });
        } else {
            collect_commands(child, &name, entries);
        }
    }
}

fn parameter(command: &Command, name: &str, argument: &Arg) -> CommandParameter {
    let id = argument.get_id().as_str();
    CommandParameter {
        name: id.into(),
        description: argument
            .get_help()
            .map(ToString::to_string)
            .unwrap_or_default(),
        action: match argument.get_action() {
            ArgAction::SetTrue | ArgAction::SetFalse => "flag",
            ArgAction::Append => "repeat",
            _ => "value",
        }
        .into(),
        required: argument.is_required_set(),
        positional: argument.get_long().is_none(),
        allowed_values: argument
            .get_value_parser()
            .possible_values()
            .map(|values| values.map(|v| v.get_name().to_string()).collect())
            .unwrap_or_default(),
        defaults: argument
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect(),
        conflicts: command
            .get_arg_conflicts_with(argument)
            .iter()
            .map(|a| a.get_id().as_str().into())
            .collect(),
        requires: if name == "describe" && id == "schema" {
            vec!["command".into()]
        } else {
            parameter_requires(name, id)
                .into_iter()
                .map(str::to_string)
                .collect()
        },
        requires_any: Vec::new(),
        value_constraints: parameter_constraints(name, id),
        conditional_defaults: if name == "diff" && id == "base" {
            vec![json!({"when_present":"input","value":null})]
        } else {
            Vec::new()
        },
    }
}

fn description(command: &Command, name: &str, schema: Option<&str>) -> CommandDescription {
    let writes = writes(command, name);
    CommandDescription {
        command: name.into(),
        description: command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default(),
        parameters: command
            .get_arguments()
            .filter(|argument| !["help", "version"].contains(&argument.get_id().as_str()))
            .map(|argument| parameter(command, name, argument))
            .collect(),
        groups: command
            .get_groups()
            .map(|group| CommandGroup {
                name: group.get_id().as_str().into(),
                required: group.is_required_set(),
                multiple: group.clone().is_multiple(),
                parameters: group.get_args().map(|id| id.as_str().into()).collect(),
            })
            .collect(),
        read_only: !writes,
        writes,
        input_schema: matches!(schema, Some("input" | "both"))
            .then(|| input_schema(name))
            .filter(|value| !value.is_null()),
        output_schema: matches!(schema, Some("output" | "both"))
            .then(|| json!(output_schema(Some(name)))),
        input_formats: command
            .get_arguments()
            .filter_map(|argument| {
                let parameter = argument.get_id().as_str();
                let format = match (name, parameter) {
                    ("apply" | "check" | "schema.check" | "diff" | "data.create", "input") => {
                        "json"
                    }
                    (_, "value-cfd" | "value-file" | "expected-cfd") => "cfd_expression",
                    ("source.write" | "source.create", "input") | ("format", "stdin") => {
                        "source_text"
                    }
                    _ => return None,
                };
                Some(InputFormat {
                    parameter: parameter.into(),
                    format: format.into(),
                })
            })
            .collect(),
        examples: vec![example(command, name)],
    }
}

/// Clap 的错误原因包含参数名与冲突信息，重复的完整 Usage 留给定向帮助。
pub(crate) fn argument_error(error: &clap::Error, invoked: &str) -> (String, String) {
    let rendered = error.to_string();
    let message = rendered
        .split("\nUsage:")
        .next()
        .unwrap_or(&rendered)
        .split("\nFor more information")
        .next()
        .unwrap_or(&rendered)
        .trim()
        .trim_start_matches("error: ")
        .trim()
        .to_string();
    let help = if invoked.is_empty() {
        "coflow describe --json".into()
    } else {
        format!(
            "coflow {} --help; coflow describe {invoked} --json",
            invoked.replace('.', " ")
        )
    };
    (message, help)
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
    if command
        .get_arguments()
        .any(|argument| argument.get_id() == "dry-run")
    {
        parameters.insert("dry-run");
    }
    let mut result = format!("coflow {}", name.replace('.', " "));
    for argument in command
        .get_arguments()
        .filter(|argument| parameters.contains(argument.get_id().as_str()))
    {
        if let Some(long) = argument.get_long() {
            result.push_str(" --");
            result.push_str(long);
        }
        if argument.get_action().takes_values() {
            let value = match argument.get_id().as_str() {
                "type" | "symbol" => "Item",
                "id" | "text" => "sword",
                "new-id" => "sword2",
                "path" if name.starts_with("project.") => "data/",
                "path" | "field" => "value",
                "file" => "data/items.cfd",
                "to-file" => "data/other.cfd",
                "input" if name.starts_with("source.") => "updated.cfd",
                "input" => "request.json",
                "value-cfd" => "1",
                "dimension" => "language",
                "variant" | "from" => "en",
                "kind" => "data",
                "rule" => "Item::Valid",
                "code" => "DATA-006",
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
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        reason = "测试前置条件或结构断言失败应直接报告失败。"
    )]
    #[test]
    fn every_described_leaf_has_documented_parameters_and_an_accepted_example() {
        let report = super::describe(None, None).unwrap();
        for summary in report["commands"].as_array().unwrap() {
            let name = summary["command"].as_str().unwrap();
            let report = super::describe(Some(name), None).unwrap();
            let definition = &report["definition"];
            assert!(definition["read_only"].is_boolean());
            for parameter in definition["parameters"].as_array().unwrap() {
                assert!(
                    !parameter["description"].as_str().unwrap().is_empty(),
                    "{name}: {parameter}"
                );
            }
            for example in definition["examples"].as_array().unwrap() {
                let example = example.as_str().unwrap();
                super::command()
                    .try_get_matches_from(example.split_whitespace())
                    .unwrap_or_else(|error| panic!("{example}: {error}"));
            }
        }
    }
}
