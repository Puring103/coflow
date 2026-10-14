#![allow(clippy::multiple_crate_versions)]
use std::{path::Path,process::ExitCode,io::{self,Read,Write},fmt::Write as _};
use coflow_runtime::automation::{Arguments,Response,Failure,Result,Status,InputSnapshot};
use coflow_runtime::Project;
use serde_json::json;
mod cli;
mod diagnostics;
mod skill_commands;

fn main()->ExitCode {
    let raw:Vec<_>=std::env::args_os().collect();let(invoked,json_output)=cli::invocation(&raw);
    let matches=match cli::command().try_get_matches_from(&raw){Ok(matches)=>matches,Err(error)=>{
        if matches!(error.kind(),clap::error::ErrorKind::DisplayHelp|clap::error::ErrorKind::DisplayVersion){let _=error.print();return ExitCode::SUCCESS;}
        let mut response=Response::new(invoked);response.fail(Failure::request(error.to_string()));return output(response,json_output);
    }};
    let(command,args)=cli::decode(&matches);
    if command=="lsp" {return match Project::open_schema_only(args.get("project").map(Path::new)).map_err(Failure::from).and_then(|p|coflow_lsp::run(p).map_err(|e|Failure::new(Status::Error,"CLI-LSP",e))){
        Ok(true)=>ExitCode::SUCCESS,Ok(false)=>ExitCode::FAILURE,Err(e)=>{eprintln!("{}",e.diagnostics);ExitCode::from(e.status.exit_code())}
    };}
    let mut response=Response::new(&command);if let Err(error)=run(&command,&args,&mut response){response.fail(error);}
    if command=="format"&&args.flag("stdin")&&!args.flag("check")&&!json_output&&response.status==Status::Success {
        if let Some(source)=response.result["source"].as_str(){return match io::stdout().lock().write_all(source.as_bytes()) { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("output error: {error}"); ExitCode::from(4) } };}
    }
    output(response,json_output)
}
fn output(response:Response,json_output:bool)->ExitCode {
    let code=response.exit_code();
    // 先准备完整输出，stdout 只写入结果，错误不会混入 JSON。
    let content=if json_output {match serde_json::to_string_pretty(&response) {Ok(text)=>text+"\n",Err(error)=>{eprintln!("output error: {error}");return ExitCode::from(4);}}}else{
        let mut text=format!("{}: {:?}\n",response.command,response.status);
        render_text(&response.result,0,&mut text);
        for diagnostic in &response.diagnostics {let _=writeln!(text,"[{}] {}",diagnostic.code,diagnostic.message);}
        text
    };
    if let Err(error)=io::stdout().lock().write_all(content.as_bytes()){eprintln!("output error: {error}");return ExitCode::from(4);}
    ExitCode::from(code)
}
fn render_text(value:&serde_json::Value,indent:usize,output:&mut String){
    match value {
        serde_json::Value::Object(fields)=>for (name,value) in fields {
            let _=write!(output,"{}{}:"," ".repeat(indent),name);
            if value.is_object()||value.is_array(){output.push('\n');render_text(value,indent+2,output);}else{let _=writeln!(output," {}",scalar_text(value));}
        },
        serde_json::Value::Array(items)=>for item in items {let _=writeln!(output,"{}-"," ".repeat(indent));render_text(item,indent+2,output);},
        value=>{let _=writeln!(output,"{}{}"," ".repeat(indent),scalar_text(value));}
    }
}
fn scalar_text(value:&serde_json::Value)->String{match value{serde_json::Value::String(text)=>text.clone(),serde_json::Value::Null=>"—".into(),value=>value.to_string()}}
fn run(command:&str,args:&Arguments,response:&mut Response)->Result<()> {
    match command {
        "describe"=>{response.result=cli::describe(args.get("command"))?;return Ok(());}
        "init"=>{let result=coflow_runtime::init_project(Path::new(args.get("dir").unwrap_or(".")))?;response.result=json!({"config_path":result.config_path});return Ok(());}
        "explain"=>{response.result=coflow_runtime::automation::explain(args.require("code")?)?;return Ok(());}
        "self-update"=>{if args.flag("json")&&!args.flag("check")&&!args.flag("yes"){return Err(Failure::request("self-update --json requires --check or --yes"));}
            response.result=self_update_command::run(&cli::SelfUpdateArgs{check:args.flag("check"),yes:args.flag("yes")})?;return Ok(());}
        command if command.starts_with("skill.")=>{
            let path=args.get("project").map(Path::new);let report=match(command,args.flag("global")){
                ("skill.install",true)=>skill_commands::install_global()?,("skill.install",false)=>skill_commands::install_project(path)?,
                ("skill.uninstall",true)=>skill_commands::uninstall_global()?,("skill.uninstall",false)=>skill_commands::uninstall_project(path)?,
                (_,true)=>skill_commands::status_global()?,(_,false)=>skill_commands::status_project(path)?,
            };response.result=serde_json::to_value(report)?;return Ok(());
        }
        "format" if args.flag("stdin")=>{let source=read_input("-")?;let formatted=coflow_runtime::automation::format_source(&source,args.require("language")?)?;
            let changed=source!=formatted;
            response.result=json!({"changed":changed,"written":false,"dry_run":args.flag("dry-run"),"source":if args.flag("check"){None}else{Some(formatted)}});
            if args.flag("check")&&changed{response.status=Status::Invalid;}return Ok(());}
        _=>{}
    }
    let project=Project::open_schema_only(args.get("project").map(Path::new))?;response.project=Some(coflow_runtime::path_to_slash(project.root_dir()));
    let snapshot=InputSnapshot::capture(&project)?;
    if args.get("input")==Some("-")&&args.get("value-file")==Some("-"){return Err(Failure::request("only one request carrier may read stdin"));}
    let input=if let Some(path)=args.get("input"){Some(read_input(path)?)}else{None};
    let input_dir=if let Some(path)=args.get("input").filter(|p|*p!="-"){std::env::current_dir()?.join(path).parent().map(Path::to_path_buf).unwrap_or(std::env::current_dir()?)}else{std::env::current_dir()?};
    let mut args=args.clone();if let Some(path)=args.get("value-file"){let value=read_input(path)?;args.0.insert("value-cfd".into(),vec![value]);}
    *response=coflow_runtime::automation::execute(command,&project,&snapshot,&args,input.as_deref(),&input_dir)?;Ok(())
}
fn read_input(path:&str)->Result<String>{if path=="-"{let mut text=String::new();io::stdin().read_to_string(&mut text)?;Ok(text)}else{Ok(std::fs::read_to_string(path)?)}}
