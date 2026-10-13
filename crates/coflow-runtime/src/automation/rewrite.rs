use std::fs;
use coflow_language::{cfd::{CfdValue as A,CfdField},cft::{CftSchema,CftValueType},source::Span};
use super::{Result,Failure,Candidate,RecordTarget,InputSnapshot};

pub fn rename_references(candidate:&Candidate,schema:&CftSchema,record:&RecordTarget,new_key:&str)->Result<()> {
    let snapshot=InputSnapshot::capture(&candidate.project()?)?;
    for file in snapshot.files.iter().filter(|f|f.kind=="data"){
        let mut text=String::from_utf8(file.contents.clone()).map_err(|e|Failure::invalid(e.to_string()))?;
        let(ast,_)=coflow_language::cfd::parse_cfd(&text);
        let mut patches=Vec::new();
        for r in ast.records.iter().chain(ast.dimensions.iter().flat_map(|b|&b.records)){
            fields(schema,&r.type_name,&r.fields,record,new_key,&mut patches);
        }
        patches.sort_by_key(|(span,_)|std::cmp::Reverse(span.start));
        for(span,value)in patches{text.replace_range(span.start..span.end,&value);}
        if text.as_bytes()!=file.contents{fs::write(&file.absolute,text)?;}
    }Ok(())
}
fn fields(schema:&CftSchema,ty:&str,fields:&[CfdField],record:&RecordTarget,new_key:&str,patches:&mut Vec<(Span,String)>){
    for field in fields{if let Some(meta)=schema.field(ty,&field.name){visit(schema,&meta.value_type,&field.value,record,new_key,patches);}}
}
fn visit(schema:&CftSchema,ty:&CftValueType,value:&A,record:&RecordTarget,new_key:&str,patches:&mut Vec<(Span,String)>){
    if let CftValueType::Option(inner)=ty{if let A::OptionSome(value,_)=value{visit(schema,inner,value,record,new_key,patches);}else{visit(schema,inner,value,record,new_key,patches);}return;}
    match(ty,value){
        (CftValueType::RecordRef(target),A::Ref(reference))if reference.key.0==record.key&&schema.is_assignable(&record.actual_type,target)=>{patches.push((reference.key.1,new_key.into()));}
        (CftValueType::Object(expected),A::Block(block))=>{let actual=block.type_marker.as_ref().map_or(expected.as_str(),|(ty,_)|ty.as_str());fields(schema,actual,&block.fields,record,new_key,patches);}
        (CftValueType::Array(inner),A::Array(items,_))=>{for item in items{visit(schema,inner,item,record,new_key,patches);}}
        (CftValueType::Dict(_,inner),A::Block(block))=>{for field in &block.fields{visit(schema,inner,&field.value,record,new_key,patches);}}
        (_,A::FormattedString(formatted))=>{for segment in &formatted.segments{if let coflow_language::cfd::CfdFormatSegment::Reference(reference)=segment{
            if reference.key.as_deref()==Some(&record.key)&&reference.type_name.as_ref().is_some_and(|ty|schema.is_assignable(&record.actual_type,ty)){
                let target=reference.type_name.as_deref().unwrap_or(&record.actual_type);
                patches.push((reference.span,format!("&{target}::{new_key}.{}",reference.path.join("."))));
            }
        }}}
        _=>{}
    }
}

pub fn rename_mapping(candidate:&Candidate,schema:&CftSchema,record:&RecordTarget,new_key:&str)->Result<()> {
    let Some(name)=schema.inherited_id_as_enum(&record.actual_type)else{return Ok(());};
    let path=candidate.project()?.root_dir().join("coflow.enum.lock.json");
    if !path.exists(){return Ok(());}
    let mut values:serde_json::Value=serde_json::from_slice(&fs::read(&path)?)?;
    if let Some(entries)=values.get_mut(name.as_str()).and_then(serde_json::Value::as_object_mut){
        if let Some(value)=entries.remove(&record.key){entries.insert(new_key.into(),value);fs::write(path,serde_json::to_vec_pretty(&values)?)?;}
    }Ok(())
}
