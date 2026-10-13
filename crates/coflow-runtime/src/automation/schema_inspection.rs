use std::collections::{BTreeSet,VecDeque};
use serde_json::{Value,json};
use coflow_language::cft::CftValueType;
use super::{Result,Failure,Arguments};

pub fn inspect(session:&crate::ProjectSchemaSession,args:&Arguments)->Result<Value>{
    let report=crate::inspect_schema(session,None,false);
    let mut result=super::json(report)?;
    let Some(schema)=session.schema()else{return Ok(result);};
    let mut selected=BTreeSet::new();let mut related_enums=BTreeSet::new();let mut queue=VecDeque::new();let mut omitted=Vec::new();
    if let Some(name)=args.get("type"){
        if schema.resolve_type(name).is_none(){return Err(Failure::missing(format!("unknown type `{name}`")));}
        queue.push_back((name.to_string(),0usize));
        if args.flag("include-derived"){for ty in schema.all_types().filter(|t|schema.is_assignable(&t.name,name)){queue.push_back((ty.name.to_string(),0));}}
    }else{for ty in schema.all_types(){queue.push_back((ty.name.to_string(),0));}}
    let depth=args.number("depth",4)?;let limit=args.number("limit",100)?;
    while let Some((name,level))=queue.pop_front(){
        if selected.contains(&name){continue;}
        if selected.len()>=limit||level>depth{omitted.push(json!({"symbol":name,"query":format!("schema inspect --type {name}")}));continue;}
        selected.insert(name.clone());
        if args.flag("related"){
            if let Some(ty)=schema.resolve_type(&name){
                if let Some(parent)=&ty.parent{queue.push_back((parent.to_string(),level+1));}
                for field in ty.all_fields(){dependencies(&field.value_type,&mut queue,&mut related_enums,level+1);}
            }
        }
    }
    if let Some(types)=result["types"].as_array_mut(){types.retain(|v|v["name"].as_str().is_some_and(|n|selected.contains(n)));
        for item in types{
            let Some(ty)=item["name"].as_str().and_then(|n|schema.resolve_type(n))else{continue;};
            let source=schema.source(&ty.module);
            item["location"]=json!(source.map(|s|super::schema_queries::location(&s.path,&s.source,ty.span)));
            item["label"]=json!(ty.display.as_ref().and_then(|d|d.label.as_ref()));item["description"]=json!(ty.display.as_ref().and_then(|d|d.description.as_ref()));
            item["concrete_types"]=json!(schema.concrete_assignable_types(&ty.name));
            if let Some(fields)=item["fields"].as_array_mut(){for entry in fields{
                let Some(field)=entry["name"].as_str().and_then(|n|schema.field(&ty.name,n))else{continue;};
                let source=schema.resolve_type(&field.declaring_type).and_then(|t|schema.source(&t.module));
                entry["declaring_type"]=json!(field.declaring_type);entry["location"]=json!(source.map(|s|super::schema_queries::location(&s.path,&s.source,field.span)));
                entry["label"]=json!(field.display.as_ref().and_then(|d|d.label.as_ref()));entry["description"]=json!(field.display.as_ref().and_then(|d|d.description.as_ref()));
                let mut expected=&field.value_type;while let CftValueType::Option(inner)=expected{expected=inner;}
                match expected{
                    CftValueType::Enum(name)=>{entry["enum_candidates"]=json!(schema.resolve_enum(name).map(|e|e.variants.iter().map(|v|json!({"name":v.name,"value":v.value.to_string()})).collect::<Vec<_>>()));}
                    CftValueType::Object(name)=>{entry["concrete_types"]=json!(schema.concrete_assignable_types(name));}
                    CftValueType::RecordRef(name)=>{entry["reference_type"]=json!(name);entry["reference_query"]=json!(format!("data list --type {name} --include-derived"));}
                    _=>{}
                }
            }}
        }
    }
    if args.get("type").is_some(){
        if let Some(enums)=result["enums"].as_array_mut(){enums.retain(|v|v["name"].as_str().is_some_and(|n|related_enums.contains(n)));}
    }
    result["omitted"]=json!(omitted);Ok(result)
}
fn dependencies(ty:&CftValueType,queue:&mut VecDeque<(String,usize)>,enums:&mut BTreeSet<String>,level:usize){match ty{
    CftValueType::Object(n)|CftValueType::RecordRef(n)=>queue.push_back((n.to_string(),level)),CftValueType::Enum(n)=>{enums.insert(n.to_string());}
    CftValueType::Array(t)|CftValueType::Option(t)=>dependencies(t,queue,enums,level),CftValueType::Dict(k,v)=>{dependencies(k,queue,enums,level);dependencies(v,queue,enums,level);}_=>{}
}}
