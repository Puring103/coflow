use std::path::Path;
use serde_json::{json,Value};
use crate::{Project,Runtime};
use super::{Arguments,Response,Status,Result,Failure,InputSnapshot,Candidate,ChangeSet,FileOperation,Operation,RecordTarget,InputValue,FieldPath};

pub fn execute(command:&str,project:&Project,snapshot:&InputSnapshot,args:&Arguments,input:Option<&str>,carrier_dir:&Path)->Result<Response>{
    let mut response=Response::new(command);response.project=Some(crate::path_to_slash(project.root_dir()));
    match command {
        "check" if input.is_none()=>{
            let session=Runtime::new().open_data_session(project.clone())?;
            let validation=super::rules::check(&session,args)?;
            response.result=response.set_validation(validation);
            let limit=args.number("limit",usize::MAX)?;response.diagnostics.truncate(limit);
        }
        "format"=>{
            let mut changes=Vec::new();let files=args.all("file");
            for name in &files{snapshot.file(project,name)?;}
            for file in snapshot.files.iter().filter(|f|["schema","data"].contains(&f.kind.as_str())){
                if !files.is_empty()&&!files.iter().any(|p|snapshot.file(project,p).is_ok_and(|f|f.absolute==file.absolute)){continue;}
                let source=String::from_utf8(file.contents.clone()).map_err(|e|Failure::request(e.to_string()))?;
                let formatted=format_source(&source,if file.kind=="schema"{"cft"}else{"cfd"})?;
                if source!=formatted{changes.push(coflow_staging::transaction::FileChange{path:file.absolute.clone(),before:Some(file.contents.clone()),after:Some(formatted.into_bytes())});}
            }
            let preview=args.flag("check")||args.flag("dry-run");
            response.result=json!({"changed":!changes.is_empty(),"written":false,"dry_run":args.flag("dry-run"),"check":args.flag("check"),"written_files":[],
                "files":changes.iter().map(|c|json!({"path":crate::project_path(project.root_dir(),&c.path),"before":c.before.as_ref().map(|b|String::from_utf8_lossy(b)),"after":c.after.as_ref().map(|b|String::from_utf8_lossy(b))})).collect::<Vec<_>>()});
            if !preview&&!changes.is_empty(){
                snapshot.verify(project).map_err(|failure|failure.with_result(response.result.clone()))?;
                let written=coflow_staging::transaction::publish(&changes)
                    .map_err(|error|super::transaction::publication_failure(project.root_dir(),error,response.result.clone()))?;
                response.result["written"]=json!(true);
                response.result["written_files"]=json!(written.iter().map(|path|crate::project_path(project.root_dir(),path)).collect::<Vec<_>>());
            }
            if args.flag("check")&&!changes.is_empty(){response.status=Status::Invalid;}
        }
        "codegen"|"build"=>{
            if command=="build"{let session=Runtime::new().open_data_session(project.clone())?;let mut options=args.clone();options.0.insert("rules".into(),vec!["true".into()]);
                let validation=super::rules::check(&session,&options)?;
                let passed=validation.passed();
                response.result=json!({"validation":response.set_validation(validation)});
                if !passed{return Ok(response);}}
            let outcome=crate::commands::codegen_automation(project,args.flag("check")||args.flag("dry-run"))?;
            match outcome{crate::commands::CommandOutcome::Success(report)=>{if args.flag("check")&&report["changed"]==true{response.status=Status::Invalid;}if response.result.is_null(){response.result=json!({});}response.result["artifacts"]=report;},
                crate::commands::CommandOutcome::Diagnostics(d)=>{response.status=Status::Invalid;response.diagnostics=d.diagnostics;}}
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
            let version=match &request{ChangeSet::Files{protocol_version,..}|ChangeSet::Operations{protocol_version,..}=>*protocol_version};
            if version!=1{return Err(Failure::request("unsupported protocol_version"));}
            let mut candidate=Candidate::new(project,snapshot)?;
            match request{ChangeSet::Files{changes,..}=>candidate.apply_files(&changes,carrier_dir).map_err(|failure|candidate.failure_result(failure,args))?,ChangeSet::Operations{ops,..}=>candidate.apply_operations(&ops).map_err(|failure|candidate.failure_result(failure,args))?}
            let completed = if command == "schema.check" { candidate.preview_schema(args) } else { candidate.finish(args,command!="apply") };
            let(result,validation)=completed.map_err(|failure|candidate.failure_result(failure,args))?;response.set_validation(validation);response.result=result;
            if matches!(command, "check" | "schema.check") { response.diagnostics.truncate(args.number("limit",usize::MAX)?); }
        }
        "apply"=>return Err(Failure::request("--input is required")),
        command if is_write(command)=>{
            let mut candidate=Candidate::new(project,snapshot)?;
            if command.starts_with("source.")||command.starts_with("project."){
                let operation=match command{
                    "source.write"=>FileOperation::Write{file:args.require("file")?.into(),content:Some(input.ok_or_else(||Failure::request("--input is required"))?.into()),content_file:None,expected_fingerprint:args.get("expected-fingerprint").map(str::to_string)},
                    "source.create"=>FileOperation::Create{file:args.require("file")?.into(),content:Some(input.ok_or_else(||Failure::request("--input is required"))?.into()),content_file:None},
                    "source.delete"=>FileOperation::Delete{file:args.require("file")?.into(),expected_fingerprint:args.get("expected-fingerprint").map(str::to_string)},
                    "project.add-input"=>FileOperation::AddInput{kind:args.require("kind")?.into(),path:args.require("path")?.into()},
                    _=>FileOperation::RemoveInput{kind:args.require("kind")?.into(),path:args.require("path")?.into()},
                };candidate.apply_files(&[operation],carrier_dir).map_err(|failure|candidate.failure_result(failure,args))?;
            }else{
                let record=RecordTarget{actual_type:args.require("type")?.into(),key:args.require("key")?.into()};
                let path=||->Result<Vec<FieldPath>>{public_path(args.require("path")?)};
                let value=||->Result<InputValue>{Ok(InputValue::Expression{source:args.require("value-cfd")?.into()})};
                let operation=match command{
                    "data.create"=>Operation::CreateRecord{record,file:args.require("file")?.into(),fields:serde_json::from_str(input.ok_or_else(||Failure::request("--input is required"))?)?},
                    "data.set"=>Operation::SetField{record,path:path()?,value:value()?,expected:None},"data.unset"=>Operation::UnsetField{record,path:path()?},
                    "data.rename"=>Operation::RenameRecord{record,new_key:args.require("new-key")?.into()},"data.delete"=>Operation::DeleteRecord{record},"data.transfer"=>Operation::TransferRecord{record,to_file:args.require("to-file")?.into()},
                    "dimension.set"=>Operation::SetDimensionValue{record,dimension:args.require("dimension")?.into(),variant:args.require("variant")?.into(),field:args.require("field")?.into(),path:args.get("path").map(public_path).transpose()?.unwrap_or_default(),value:value()?,file:args.get("file").map(str::to_string)},
                    _=>Operation::ClearDimensionValue{record,dimension:args.require("dimension")?.into(),variant:args.require("variant")?.into(),field:args.require("field")?.into()},
                };candidate.apply_operations(&[operation]).map_err(|failure|candidate.failure_result(failure,args))?;
            }
            let(result,validation)=candidate.finish(args,false).map_err(|failure|candidate.failure_result(failure,args))?;response.set_validation(validation);response.result=result;
        }
        _=>return super::read_command(command,project,snapshot,args),
    }
    Ok(response)
}
fn is_write(command:&str)->bool{["source.write","source.create","source.delete","project.add-input","project.remove-input","data.create","data.set","data.unset","data.rename","data.delete","data.transfer","dimension.set","dimension.clear"].contains(&command)}
pub(crate) fn public_path(path:&str)->Result<Vec<FieldPath>>{
    super::query::parse_path(path)?.into_iter().map(|s|Ok(match s{
        crate::CfdPathSegment::Field(name)=>FieldPath::Field{name},
        crate::CfdPathSegment::Index(index)=>FieldPath::Index{index},
        crate::CfdPathSegment::DictKey(key)=>FieldPath::DictKey{key:if key.starts_with('"'){
            super::ModelValue::String{value:serde_json::from_str(&key)?}
        }else if key.parse::<i64>().is_ok(){super::ModelValue::Int{value:key}}
        else if let Some((name,variant))=key.split_once("::").or_else(||key.rsplit_once('.')){
            super::ModelValue::Enum{type_name:name.into(),variant:variant.into()}
        }else{return Err(Failure::request("dictionary key requires a typed literal"));}},
    })).collect()
}
pub fn format_source(source:&str,language:&str)->Result<String>{match language{"cft"=>Ok(coflow_format::format_cft(source)),"cfd"=>Ok(coflow_format::format_cfd(source)),_=>Err(Failure::request("language must be cft or cfd"))}}

pub fn explain(code:&str)->Result<Value>{
    super::json(super::explanations::explain(code)?)
}
