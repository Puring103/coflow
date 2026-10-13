use clap::{Arg, ArgAction, ArgMatches, Command};
use coflow_runtime::automation::Arguments;
use serde_json::{Value,json};

#[derive(Debug)]
pub(crate) struct SelfUpdateArgs { pub check: bool, pub yes: bool }
const FLAGS: &[&str] = &["include-derived","related","dry-run","check","rules","explain","summary","include-source","stdin","global","yes","performance"];
const QUERY: &str = "type include-derived file key fields where dimension sort limit cursor";
const WRITE: &str = "dry-run rules expected-snapshot";

fn option(name: &'static str) -> Arg {
    let arg=Arg::new(name).long(name);
    if FLAGS.contains(&name) {arg.action(ArgAction::SetTrue)} else if ["dimension","file","rule"].contains(&name) {arg.action(ArgAction::Append)} else {arg.action(ArgAction::Set)}
}
fn leaf(name: &'static str, about: &'static str, groups: &[&'static str]) -> Command {
    let mut command=Command::new(name).about(about);
    let mut seen=std::collections::BTreeSet::new();
    for group in groups {for name in group.split_whitespace() {if seen.insert(name){command=command.arg(option(name));}}}
    if seen.contains("value-cfd") {command=command.mut_arg("value-cfd",|a|a.conflicts_with("value-file"));}
    command
}
fn group(name:&'static str, children:Vec<Command>)->Command{Command::new(name).subcommand_required(true).subcommands(children)}

pub(crate) fn command()->Command {
    let record="type key";let value="value-cfd value-file";let dimension="dimension variant type key field";
    Command::new("coflow").version(env!("CARGO_PKG_VERSION")).about("Coflow configuration queries, validation and transactions")
        .subcommand_required(true).arg(Arg::new("project").long("project").global(true))
        .arg(Arg::new("json").long("json").global(true).action(ArgAction::SetTrue)).subcommands(vec![
        leaf("init","Create a project",&[]).arg(Arg::new("dir")),
        leaf("describe","Describe command and request protocols",&[]).arg(Arg::new("command")),
        group("project",vec![leaf("inspect","Inspect effective project configuration",&[]),leaf("files","List selected project files",&["kind"]),
            leaf("add-input","Add a project input",&["kind path",WRITE]),leaf("remove-input","Remove a project input",&["kind path",WRITE])]),
        leaf("context","Collect bounded context for a type or record",&[QUERY,"examples ref-limit depth budget"]),
        group("schema",vec![leaf("check","Check candidate or current schema",&["input limit explain"]),
            leaf("inspect","Inspect type and field declarations",&["type field include-derived related depth limit"]),
            leaf("symbols","List schema symbols",&["kind limit cursor"]),leaf("refs","Find semantic symbol uses",&["symbol limit cursor"])]),
        group("source",vec![leaf("read","Read selected source text",&["file start-line end-line"]),
            leaf("write","Replace a source file",&["file input expected-fingerprint",WRITE]),
            leaf("create","Create and include a source file",&["file input",WRITE]),leaf("delete","Delete a source file",&["file expected-fingerprint",WRITE])]),
        group("data",vec![leaf("list","List records",&[QUERY,"view"]),leaf("get","Read a record",&[QUERY,"view"]),leaf("search","Search record text",&[QUERY,"text view"]),
            leaf("draft","Prepare a schema-guided record draft",&[record]),leaf("create","Create a record",&[record,"file input",WRITE]),
            leaf("set","Set a field value",&[record,"path",value,WRITE]),leaf("unset","Remove a field assignment",&[record,"path",WRITE]),
            leaf("rename","Rename a record and its references",&[record,"new-key",WRITE]),leaf("delete","Delete a record and its overrides",&[record,WRITE]),
            leaf("transfer","Move a record to another file",&[record,"to-file",WRITE]),leaf("refs","Find record reference edges",&[record,"direction depth limit cursor"])]),
        group("dimension",vec![leaf("list","List dimensions",&[]),leaf("inspect","Inspect dimension metadata",&["dimension"]),leaf("variants","List declared variants",&["dimension limit cursor"]),
            leaf("get","Read explicit override and effective value",&[dimension]),leaf("set","Set a dimension override",&[dimension,"file path",value,WRITE]),leaf("clear","Clear an explicit override",&[dimension,WRITE]),
            leaf("missing","List fields without explicit overrides",&[QUERY,"variant field"]),leaf("diff","Compare dimension coverage and values",&[QUERY,"from to field"])]),
        group("stats",vec![leaf("project","Report project counts",&[QUERY]),leaf("records","Count records by type or file",&[QUERY,"group-by"]),
            leaf("field","Compute field distributions and numeric statistics",&[QUERY,"field metrics group-by option-layer quantiles"]),
            leaf("dimension","Compute explicit coverage",&[QUERY,"variant group-by field"]),leaf("refs","Compute reference counts",&[QUERY,"direction"])]),
        leaf("check","Validate data and optionally business rules",&["rules rule input limit explain performance expected-snapshot"]),
        group("rule",vec![leaf("list","List rules and static dependencies",&["type"]),leaf("inspect","Inspect a rule",&["rule expected-snapshot"])]),
        leaf("explain","Explain a diagnostic code",&[]).arg(Arg::new("code").required(true)),
        leaf("apply","Validate and publish a change set atomically",&["input",WRITE]),
        leaf("diff","Compare Git or candidate project sources and data",&["base input summary include-source type view"]).mut_arg("input",|a|a.conflicts_with("base")),
        leaf("format","Format project sources or stdin",&["file stdin language check dry-run"]).mut_arg("stdin",|a|a.conflicts_with("file")),
        leaf("codegen","Generate, preview or check code artifacts",&["dry-run check"]),leaf("build","Validate all rules and generate code",&[]),
        leaf("lsp","Run the language server over stdin/stdout",&[]),
        group("skill",vec![leaf("install","Install bundled skills",&["global"]),leaf("uninstall","Remove bundled skills",&["global"]),leaf("status","Inspect bundled skill installation",&["global"])]),
        leaf("self-update","Check or install CLI updates",&["check yes"]),
    ])
}
pub(crate) fn decode(matches:&ArgMatches)->(String,Arguments) {
    let mut parts=Vec::new();let mut leaf=matches;
    while let Some((name,sub))=leaf.subcommand(){parts.push(name);leaf=sub;}
    let mut arguments=Arguments::default();
    for id in leaf.ids(){let name=id.as_str();
        if FLAGS.contains(&name)||name=="json"{if leaf.get_flag(name){arguments.0.insert(name.into(),vec!["true".into()]);}}
        else if let Ok(Some(values))=leaf.try_get_many::<String>(name){arguments.0.insert(name.into(),values.cloned().collect());}
    }
    (parts.join("."),arguments)
}
pub(crate) fn describe(target:Option<&str>)->coflow_runtime::automation::Result<Value>{
    let mut root=command();root.build();let mut selected=&root;
    if let Some(target)=target{for name in target.split('.') {selected=selected.get_subcommands().find(|c|c.get_name()==name).ok_or_else(||coflow_runtime::automation::Failure::missing(format!("unknown command `{target}`")))?;}}
    fn description(command:&Command,prefix:&str)->Value{
        let name=if prefix.is_empty(){command.get_name().to_string()}else{format!("{prefix}.{}",command.get_name())};
        let parameters:Vec<_>=command.get_arguments().map(|a|json!({"name":a.get_id().as_str(),"long":a.get_long(),"required":a.is_required_set(),"action":format!("{:?}",a.get_action()),
            "defaults":a.get_default_values().iter().map(|s|s.to_string_lossy()).collect::<Vec<_>>(),"conflicts":command.get_arg_conflicts_with(a).iter().map(|a|a.get_id().as_str()).collect::<Vec<_>>()})).collect();
        json!({"command":name.trim_start_matches("coflow."),"description":command.get_about().map(ToString::to_string),"parameters":parameters,
            "subcommands":command.get_subcommands().map(|c|description(c,&name)).collect::<Vec<_>>()})
    }
    Ok(json!({"protocol_version":1,"definition":description(selected,""),"exit_codes":{"0":"success","1":"validation or incomplete result","2":"invalid request","3":"conflict","4":"execution failure"},
        "input_schema":coflow_runtime::automation::change_set_schema(),"output_schema":coflow_runtime::automation::response_schema()}))
}
