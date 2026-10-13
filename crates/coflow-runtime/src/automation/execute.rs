use std::path::Path;
use serde_json::{json,Value};
use crate::{Project,Runtime};
use super::{Arguments,Response,Status,Result,Failure,InputSnapshot,Candidate,ChangeSet,FileOperation,Operation,RecordTarget,InputValue,FieldPath};

pub fn execute(command:&str,project:&Project,snapshot:&InputSnapshot,args:&Arguments,input:Option<&str>,carrier_dir:&Path)->Result<Response>{
    let mut response=Response::new(command);response.project=Some(crate::path_to_slash(project.root_dir()));response.snapshot=Some(snapshot.id.clone());
    match command {
        "check" if input.is_none()=>{
            let session=Runtime::new().open_data_session(project.clone())?;
            let(result,diagnostics)=super::rules::check(&session,args)?;response.result=result;response.diagnostics=diagnostics.diagnostics;
            response.result["diagnostic_count"]=json!(response.diagnostics.len());
            let limit=args.number("limit",usize::MAX)?;response.diagnostics.truncate(limit);
            if response.result["diagnostic_count"]!=0{response.status=Status::Partial;}
        }
        "format"=>{
            let mut changes=Vec::new();let files=args.all("file");
            for name in &files{snapshot.file(project,name)?;}
            for file in snapshot.files.iter().filter(|f|["schema","data"].contains(&f.kind.as_str())){
                if !files.is_empty()&&!files.iter().any(|p|snapshot.file(project,p).is_ok_and(|f|f.absolute==file.absolute)){continue;}
                let source=String::from_utf8(file.contents.clone()).map_err(|e|Failure::invalid(e.to_string()))?;
                let formatted=format_source(&source,if file.kind=="schema"{"cft"}else{"cfd"})?;
                if source!=formatted{changes.push(coflow_staging::transaction::FileChange{path:file.absolute.clone(),before:Some(file.contents.clone()),after:Some(formatted.into_bytes())});}
            }
            let preview=args.flag("check")||args.flag("dry-run");
            let written=if preview{Vec::new()}else{snapshot.verify(project)?;coflow_staging::transaction::publish(&changes).map_err(|e|Failure::new(if e.conflict{Status::Conflict}else{Status::Error},"CLI-PUBLISH",e.message))?};
            response.result=json!({"changed":!changes.is_empty(),"written":!written.is_empty(),"files":changes.iter().map(|c|json!({"path":crate::project_path(project.root_dir(),&c.path),"before":c.before.as_ref().map(|b|String::from_utf8_lossy(b)),"after":c.after.as_ref().map(|b|String::from_utf8_lossy(b))})).collect::<Vec<_>>()});
            if args.flag("check")&&!changes.is_empty(){response.status=Status::Partial;}
        }
        "codegen"|"build"=>{
            if command=="build"{let session=Runtime::new().open_data_session(project.clone())?;let mut options=args.clone();options.0.insert("rules".into(),vec!["true".into()]);
                let(validation,diagnostics)=super::rules::check(&session,&options)?;response.result=json!({"validation":validation});
                if !diagnostics.is_empty(){response.status=Status::Partial;response.diagnostics=diagnostics.diagnostics;return Ok(response);}}
            let outcome=crate::commands::codegen_automation(project,args.flag("check")||args.flag("dry-run"))?;
            match outcome{crate::commands::CommandOutcome::Success(report)=>{if args.flag("check")&&report["changed"]==true{response.status=Status::Partial;}response.result["artifacts"]=report;},
                crate::commands::CommandOutcome::Diagnostics(d)=>{response.status=Status::Partial;response.diagnostics=d.diagnostics;}}
        }
        "diff" if input.is_none()=>{
            let session=Runtime::new().open_data_session(project.clone())?;
            let mut diff=crate::diff::diff_against_revision(&session.session,0,args.get("base").unwrap_or("HEAD"))?;
            if !diff.semantic_available{response.status=Status::Partial;}
            if let Some(ty)=args.get("type"){diff.records.retain(|r|r.coordinate.actual_type.as_str()==ty);}
            if !args.flag("include-source"){for file in &mut diff.files{file.before=None;file.after=None;}}
            response.result=if args.flag("summary"){json!({"base":diff.head_oid,"files":diff.files.len(),"records":diff.records.len(),"semantic_available":diff.semantic_available,"diagnostics":diff.diagnostics})}else{super::json(diff)?};
        }
        "apply"|"check"|"diff"|"schema.check" if input.is_some()=>{
            let request:ChangeSet=serde_json::from_str(input.unwrap_or_default())?;
            let(version,expected)=match &request{ChangeSet::Files{protocol_version,expected_snapshot,..}|ChangeSet::Operations{protocol_version,expected_snapshot,..}=>(*protocol_version,expected_snapshot)};
            if version!=1{return Err(Failure::invalid("unsupported protocol_version"));}
            if expected.as_ref().is_some_and(|e|e!=&snapshot.id){return Err(Failure::conflict("input snapshot changed"));}
            let mut candidate=Candidate::new(project,snapshot)?;
            match request{ChangeSet::Files{changes,..}=>candidate.apply_files(&changes,carrier_dir)?,ChangeSet::Operations{ops,..}=>candidate.apply_operations(&ops)?}
            let(result,diagnostics)=candidate.finish(args,command!="apply")?;response.result=result;response.diagnostics=diagnostics.diagnostics;
        }
        "apply"=>return Err(Failure::invalid("--input is required")),
        command if is_write(command)=>{
            let mut candidate=Candidate::new(project,snapshot)?;
            if command.starts_with("source.")||command.starts_with("project."){
                let operation=match command{
                    "source.write"=>FileOperation::Write{file:args.require("file")?.into(),content:Some(input.ok_or_else(||Failure::invalid("--input is required"))?.into()),content_file:None,expected_fingerprint:args.get("expected-fingerprint").map(str::to_string)},
                    "source.create"=>FileOperation::Create{file:args.require("file")?.into(),content:Some(input.ok_or_else(||Failure::invalid("--input is required"))?.into()),content_file:None},
                    "source.delete"=>FileOperation::Delete{file:args.require("file")?.into(),expected_fingerprint:args.get("expected-fingerprint").map(str::to_string)},
                    "project.add-input"=>FileOperation::AddInput{kind:args.require("kind")?.into(),path:args.require("path")?.into()},
                    _=>FileOperation::RemoveInput{kind:args.require("kind")?.into(),path:args.require("path")?.into()},
                };candidate.apply_files(&[operation],carrier_dir)?;
            }else{
                let record=RecordTarget{actual_type:args.require("type")?.into(),key:args.require("key")?.into()};
                let path=||->Result<Vec<FieldPath>>{public_path(args.require("path")?)};
                let value=||->Result<InputValue>{Ok(InputValue::Expression{source:args.require("value-cfd")?.into()})};
                let operation=match command{
                    "data.create"=>Operation::CreateRecord{record,file:args.require("file")?.into(),fields:serde_json::from_str(input.ok_or_else(||Failure::invalid("--input is required"))?)?},
                    "data.set"=>Operation::SetField{record,path:path()?,value:value()?,expected:None},"data.unset"=>Operation::UnsetField{record,path:path()?},
                    "data.rename"=>Operation::RenameRecord{record,new_key:args.require("new-key")?.into()},"data.delete"=>Operation::DeleteRecord{record},"data.transfer"=>Operation::TransferRecord{record,to_file:args.require("to-file")?.into()},
                    "dimension.set"=>Operation::SetDimensionValue{record,dimension:args.require("dimension")?.into(),variant:args.require("variant")?.into(),field:args.require("field")?.into(),path:args.get("path").map(public_path).transpose()?.unwrap_or_default(),value:value()?,file:args.get("file").map(str::to_string)},
                    _=>Operation::ClearDimensionValue{record,dimension:args.require("dimension")?.into(),variant:args.require("variant")?.into(),field:args.require("field")?.into()},
                };candidate.apply_operations(&[operation])?;
            }
            let(result,diagnostics)=candidate.finish(args,false)?;response.result=result;response.diagnostics=diagnostics.diagnostics;
        }
        _=>return super::read_command(command,project,snapshot,args),
    }
    if !response.diagnostics.is_empty(){response.status=Status::Partial;}
    Ok(response)
}
fn is_write(command:&str)->bool{["source.write","source.create","source.delete","project.add-input","project.remove-input","data.create","data.set","data.unset","data.rename","data.delete","data.transfer","dimension.set","dimension.clear"].contains(&command)}
fn public_path(path:&str)->Result<Vec<FieldPath>>{Ok(super::query::parse_path(path)?.into_iter().map(|s|match s{crate::CfdPathSegment::Field(name)=>FieldPath::Field{name},crate::CfdPathSegment::Index(index)=>FieldPath::Index{index},crate::CfdPathSegment::DictKey(key)=>FieldPath::DictKey{key}}).collect())}
pub fn format_source(source:&str,language:&str)->Result<String>{match language{"cft"=>Ok(coflow_format::format_cft(source)),"cfd"=>Ok(coflow_format::format_cfd(source)),_=>Err(Failure::invalid("language must be cft or cfd"))}}

pub fn explain(code:&str)->Result<Value>{
    // 诊断索引与公开参考使用同一来源，避免维护第二套错误码清单。
    let reference=include_str!("../../../../website/docs/docs/reference/09-diagnostics/02-codes.md");
    let matching:Vec<_>=reference.lines().filter(|line|line.contains(code)).collect();
    if matching.is_empty(){return Err(Failure::missing(format!("unknown diagnostic code `{code}`")));}
    Ok(json!({"code":code,"description":matching.join("\n"),"queries":["check --explain","schema inspect","data get"]}))
}
