#![allow(clippy::multiple_crate_versions)]
use std::{path::Path,process::ExitCode,io::{self,Read}};
use coflow_runtime::automation::{Arguments,Response,Failure,Result,Status,InputSnapshot};
use coflow_runtime::Project;
use serde_json::json;
mod cli;
mod diagnostics;
mod skill_commands;
mod self_update_command;

fn main()->ExitCode {
    let raw:Vec<_>=std::env::args_os().collect();let json_output=raw.iter().any(|s|s=="--json");
    let matches=match cli::command().try_get_matches_from(&raw){Ok(matches)=>matches,Err(error)=>{
        if matches!(error.kind(),clap::error::ErrorKind::DisplayHelp|clap::error::ErrorKind::DisplayVersion){let _=error.print();return ExitCode::SUCCESS;}
        let mut response=Response::new(raw.get(1).map(|v|v.to_string_lossy().into_owned()).unwrap_or_default());response.fail(Failure::invalid(error.to_string()));return output(response,json_output);
    }};
    let(command,args)=cli::decode(&matches);
    if command=="lsp" {return match Project::open_schema_only(args.get("project").map(Path::new)).map_err(Failure::from).and_then(|p|coflow_lsp::run(p).map_err(|e|Failure::new(Status::Error,"CLI-LSP",e))){
        Ok(true)=>ExitCode::SUCCESS,Ok(false)=>ExitCode::FAILURE,Err(e)=>{eprintln!("{}",e.diagnostics);ExitCode::from(e.status.exit_code())}
    };}
    let mut response=Response::new(&command);if let Err(error)=run(&command,&args,&mut response){response.fail(error);}
    if command=="format"&&args.flag("stdin")&&!json_output&&response.status==Status::Success {if let Some(source)=response.result["source"].as_str(){print!("{source}");return ExitCode::SUCCESS;}}
    output(response,json_output)
}
fn output(response:Response,json_output:bool)->ExitCode {
    let code=response.status.exit_code();
    let result=if json_output {serde_json::to_writer_pretty(io::stdout().lock(),&response)}else{serde_json::to_writer_pretty(io::stdout().lock(),&response.result)};
    if let Err(error)=result{eprintln!("output error: {error}");return ExitCode::from(4);}println!();
    if !json_output {for diagnostic in &response.diagnostics{eprintln!("[{}] {}",diagnostic.code,diagnostic.message);}}
    ExitCode::from(code)
}
fn run(command:&str,args:&Arguments,response:&mut Response)->Result<()> {
    match command {
        "describe"=>{response.result=cli::describe(args.get("command"))?;return Ok(());}
        "init"=>{let result=coflow_runtime::init_project(Path::new(args.get("dir").unwrap_or(".")))?;response.result=json!({"config_path":result.config_path});return Ok(());}
        "explain"=>{response.result=coflow_runtime::automation::explain(args.require("code")?)?;return Ok(());}
        "self-update"=>{let result=self_update_command::run(&cli::SelfUpdateArgs{check:args.flag("check"),yes:args.flag("yes")})?;response.result=json!({"completed":result});return Ok(());}
        command if command.starts_with("skill.")=>{
            let path=args.get("project").map(Path::new);let report=match(command,args.flag("global")){
                ("skill.install",true)=>skill_commands::install_global()?,("skill.install",false)=>skill_commands::install_project(path)?,
                ("skill.uninstall",true)=>skill_commands::uninstall_global()?,("skill.uninstall",false)=>skill_commands::uninstall_project(path)?,
                (_,true)=>skill_commands::status_global()?,(_,false)=>skill_commands::status_project(path)?,
            };response.result=serde_json::to_value(report)?;return Ok(());
        }
        "format" if args.flag("stdin")=>{let source=read_input("-")?;response.result=json!({"source":coflow_runtime::automation::format_source(&source,args.require("language")?)?});return Ok(());}
        _=>{}
    }
    let project=Project::open_schema_only(args.get("project").map(Path::new))?;response.project=Some(coflow_runtime::path_to_slash(project.root_dir()));
    let snapshot=InputSnapshot::capture(&project)?;response.snapshot=Some(snapshot.id.clone());
    if let Some(expected)=args.get("expected-snapshot"){if expected!=snapshot.id{return Err(Failure::conflict("input snapshot changed"));}}
    let input=if let Some(path)=args.get("input"){Some(read_input(path)?)}else{None};
    let input_dir=if let Some(path)=args.get("input").filter(|p|*p!="-"){std::env::current_dir()?.join(path).parent().map(Path::to_path_buf).unwrap_or(std::env::current_dir()?)}else{std::env::current_dir()?};
    let mut args=args.clone();if let Some(path)=args.get("value-file"){let value=read_input(path)?;args.0.insert("value-cfd".into(),vec![value]);}
    *response=coflow_runtime::automation::execute(command,&project,&snapshot,&args,input.as_deref(),&input_dir)?;Ok(())
}
fn read_input(path:&str)->Result<String>{if path=="-"{let mut text=String::new();io::stdin().read_to_string(&mut text)?;Ok(text)}else{Ok(std::fs::read_to_string(path)?)}}
