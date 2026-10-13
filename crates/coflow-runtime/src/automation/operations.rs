use std::{collections::BTreeMap, path::PathBuf};
use serde::{Serialize,Deserialize};
use serde_json::json;
use coflow_language::{cft::{CftSchema,CftValueType},cfd::{parse_cfd,CfdRecord as AstRecord}};
use crate::{CfdValue,CfdPathSegment,RecordOrigin,Runtime,CfdSource,CfdSourcePath};
use super::{Result,Failure,transaction::Candidate};

#[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordTarget { #[serde(rename="type")] pub actual_type: String, pub key: String }
#[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum FieldPath { Field {name:String}, Index {index:usize}, DictKey {key:String} }
impl FieldPath { fn internal(&self) -> CfdPathSegment { match self {Self::Field{name}=>CfdPathSegment::Field(name.clone()),Self::Index{index}=>CfdPathSegment::Index(*index),Self::DictKey{key}=>CfdPathSegment::DictKey(key.clone())} } }
#[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
#[serde(tag="kind",rename_all="snake_case",deny_unknown_fields)]
pub enum InputValue { Expression {source:String}, Model {#[schemars(with="serde_json::Value")] value:CfdValue} }
impl InputValue {
    fn resolve(&self,schema:&CftSchema,expected:&CftValueType)->Result<CfdValue>{match self{Self::Expression{source}=>super::values::expression(schema,expected,source),Self::Model{value}=>Ok(value.clone())}}
}
#[derive(Debug,Clone,Serialize,Deserialize,schemars::JsonSchema)]
#[serde(tag="op",rename_all="snake_case",deny_unknown_fields)]
pub enum Operation {
    CreateRecord {record:RecordTarget,file:String,#[serde(default)]fields:BTreeMap<String,InputValue>},
    SetField {record:RecordTarget,path:Vec<FieldPath>,value:InputValue,#[serde(default)]expected:Option<InputValue>},
    UnsetField {record:RecordTarget,path:Vec<FieldPath>},
    RenameRecord {record:RecordTarget,new_key:String},
    DeleteRecord {record:RecordTarget},
    TransferRecord {record:RecordTarget,to_file:String},
    SetDimensionValue {record:RecordTarget,dimension:String,variant:String,field:String,#[serde(default)]path:Vec<FieldPath>,value:InputValue,#[serde(default)]file:Option<String>},
    ClearDimensionValue {record:RecordTarget,dimension:String,variant:String,field:String},
    ArrayInsert {record:RecordTarget,path:Vec<FieldPath>,index:usize,value:InputValue},
    ArrayRemove {record:RecordTarget,path:Vec<FieldPath>,index:usize},
    ArrayMove {record:RecordTarget,path:Vec<FieldPath>,from:usize,to:usize},
    DictSet {record:RecordTarget,path:Vec<FieldPath>,key:InputValue,value:InputValue},
    DictRemove {record:RecordTarget,path:Vec<FieldPath>,key:InputValue},
    SetWhere {selector:super::Selector,path:Vec<FieldPath>,value:InputValue,expected_count:usize},
}

impl Candidate {
    pub fn apply_operations(&mut self,operations:&[Operation])->Result<()> {
        for (index,operation) in operations.iter().enumerate() {
            self.apply_operation(operation)?;
            self.operations.push(json!({"index":index,"status":"success","operation":operation}));
        }
        Ok(())
    }
    fn apply_operation(&mut self,operation:&Operation)->Result<()> {
        let project=self.project()?;
        let schema_session=Runtime::open_schema_session(project.clone())?;
        let schema=schema_session.schema().ok_or_else(||Failure::from(schema_session.diagnostics().as_set().clone()))?;
        let writer=crate::cfd_loader::CfdWriter::new();
        match operation {
            Operation::CreateRecord{record,file,fields}=>{
                if find_record(self,record).is_ok(){return Err(Failure::conflict("record already exists"));}
                let meta=schema.resolve_type(&record.actual_type).ok_or_else(||Failure::missing("unknown record type"))?;
                if meta.is_abstract || meta.is_data(){return Err(Failure::invalid("record requires a concrete table or singleton type"));}
                let path=self.resolve(file,true)?;self.track(&path)?;
                if !self.shadow(&path).exists() { self.write(&path,b"")?; }
                let source=CfdSource{location:CfdSourcePath::new(self.shadow(&path)),display_name:file.clone()};
                let fields=fields.iter().map(|(name,value)|{
                    let ty=&schema.field(&record.actual_type,name).ok_or_else(||Failure::missing(format!("unknown field `{name}`")))?.value_type;
                    Ok((name.clone(),value.resolve(schema,ty)?))
                }).collect::<Result<_>>()?;
                writer.insert_record(&crate::InsertRecordRequest{source:&source,record_key:&record.key,actual_type:&record.actual_type,fields:&fields,schema,before:None})?;
                writer.publish()?;self.include(&path)?;
            }
            Operation::SetField{record,path,value,expected}=>{
                let (file,ast,text)=find_record(self,record)?;
                let path=internal_path(path);
                let ty=crate::write_rules::expected_type_for_cfd_path(schema,&record.actual_type,&path,"CLI-PATH","CLI")?;
                let value=value.resolve(schema,&ty)?;
                let current=read_value(schema,record,&ast,&text,&path)?;
                if let Some(expected)=expected {if current.as_ref()!=Some(&expected.resolve(schema,&ty)?){return Err(Failure::conflict("old value precondition failed"));}}
                if current.as_ref()==Some(&value){return Ok(());}
                let top=materialized_top(schema,record,&ast,&text,&path,&value)?;
                writer.write_field(&crate::WriteCellRequest{origin:&RecordOrigin::File{path:file,span:None},record_key:&record.key,actual_type:&record.actual_type,field_path:&path,new_value:&value,materialized_top_level:top.as_ref(),schema})?;
                writer.publish()?;
            }
            Operation::UnsetField{record,path}=>{
                let(file,_,_)=find_record(self,record)?;
                writer.unset_field(&RecordOrigin::File{path:file,span:None},&record.key,&record.actual_type,&internal_path(path),schema)?;writer.publish()?;
            }
            Operation::RenameRecord{record,new_key}=>{
                let(file,_,_)=find_record(self,record)?;
                if find_record(self,&RecordTarget{actual_type:record.actual_type.clone(),key:new_key.clone()}).is_ok(){return Err(Failure::conflict("new record key already exists"));}
                super::rewrite::rename_references(self,schema,record,new_key)?;
                writer.rename_record(&crate::RenameRecordRequest{origin:&RecordOrigin::File{path:file,span:None},old_key:&record.key,new_key,actual_type:&record.actual_type})?;
                rewrite_dimensions(self,&writer,schema,record,Some(new_key))?;writer.publish()?;
                super::rewrite::rename_mapping(self,schema,record,new_key)?;
            }
            Operation::DeleteRecord{record}=>{
                let(file,_,_)=find_record(self,record)?;
                writer.delete_record(&crate::DeleteRecordRequest{origin:&RecordOrigin::File{path:file,span:None},record_key:&record.key,actual_type:&record.actual_type})?;
                rewrite_dimensions(self,&writer,schema,record,None)?;writer.publish()?;
            }
            Operation::TransferRecord{record,to_file}=>{
                let(file,ast,text)=find_record(self,record)?;
                let target=self.resolve(to_file,true)?;self.track(&target)?;
                if self.shadow(&target)==file{return Ok(());}
                if !self.shadow(&target).exists() { self.write(&target,b"")?; }
                let fields=ast.fields.iter().map(|f|{
                    let expected=&schema.field(&record.actual_type,&f.name).ok_or_else(||Failure::missing("unknown source field"))?.value_type;
                    let span=super::query::ast_value_span(&f.value);
                    Ok((f.name.clone(),super::values::expression(schema,expected,&text[span.start..span.end])?))
                }).collect::<Result<_>>()?;
                writer.insert_record(&crate::InsertRecordRequest{source:&CfdSource{location:CfdSourcePath::new(self.shadow(&target)),display_name:to_file.clone()},record_key:&record.key,actual_type:&record.actual_type,fields:&fields,schema,before:None})?;
                writer.delete_record(&crate::DeleteRecordRequest{origin:&RecordOrigin::File{path:file,span:None},record_key:&record.key,actual_type:&record.actual_type})?;
                writer.publish()?;self.include(&target)?;
            }
            Operation::SetDimensionValue{record,dimension,variant,field,path,value,file}=>{
                self.dimension_write(schema,record,dimension,variant,field,path,Some(value),file.as_deref())?;
            }
            Operation::ClearDimensionValue{record,dimension,variant,field}=>{
                self.dimension_write(schema,record,dimension,variant,field,&[],None,None)?;
            }
            Operation::SetWhere{selector,path,value,expected_count}=>{
                let session=Runtime::new().open_data_session(project)?;
                if session.queries().has_diagnostics(){return Err(session.queries().diagnostics().as_set().clone().into());}
                let records=super::query::selected_records(&session,selector)?;
                if records.len()!=*expected_count{return Err(Failure::conflict(format!("expected {expected_count} records, matched {}",records.len())));}
                for record in records{self.apply_operation(&Operation::SetField{record:RecordTarget{actual_type:record.actual_type().into(),key:record.key().into()},path:path.clone(),value:value.clone(),expected:None})?;}
            }
            other=>self.collection(schema,other)?,
        }
        Ok(())
    }
    fn collection(&mut self,schema:&CftSchema,operation:&Operation)->Result<()> {
        let(record,path)=match operation{
            Operation::ArrayInsert{record,path,..}|Operation::ArrayRemove{record,path,..}|Operation::ArrayMove{record,path,..}|Operation::DictSet{record,path,..}|Operation::DictRemove{record,path,..}=>(record,path),
            _=>return Err(Failure::invalid("operation is not a collection edit")),
        };
        let(_,ast,text)=find_record(self,record)?;
        let internal=internal_path(path);
        let expected=crate::write_rules::expected_type_for_cfd_path(schema,&record.actual_type,&internal,"CLI-PATH","CLI")?;
        let mut value=read_value(schema,record,&ast,&text,&internal)?.ok_or_else(||Failure::missing("collection value is missing"))?;
        match(operation,&mut value,&expected){
            (Operation::ArrayInsert{index,value,..},CfdValue::Array(items),CftValueType::Array(inner))=>{if *index>items.len(){return Err(Failure::invalid("array index out of bounds"));}items.insert(*index,value.resolve(schema,inner)?);}
            (Operation::ArrayRemove{index,..},CfdValue::Array(items),_)=>{if *index>=items.len(){return Err(Failure::invalid("array index out of bounds"));}items.remove(*index);}
            (Operation::ArrayMove{from,to,..},CfdValue::Array(items),_)=>{if *from>=items.len()||*to>=items.len(){return Err(Failure::invalid("array index out of bounds"));}let item=items.remove(*from);items.insert(*to,item);}
            (Operation::DictSet{key,value,..},CfdValue::Dict(items),CftValueType::Dict(key_ty,value_ty))=>{let key=as_key(key.resolve(schema,key_ty)?)?;let value=value.resolve(schema,value_ty)?;if let Some((_,v))=items.iter_mut().find(|(k,_)|k==&key){*v=value;}else{items.push((key,value));}}
            (Operation::DictRemove{key,..},CfdValue::Dict(items),CftValueType::Dict(key_ty,_))=>{let key=as_key(key.resolve(schema,key_ty)?)?;let index=items.iter().position(|(k,_)|k==&key).ok_or_else(||Failure::missing("dictionary key does not exist"))?;items.remove(index);}
            _=>return Err(Failure::invalid("collection operation does not match the field type")),
        }
        self.apply_operation(&Operation::SetField{record:record.clone(),path:path.clone(),value:InputValue::Model{value},expected:None})
    }
    fn dimension_write(&mut self,schema:&CftSchema,record:&RecordTarget,dimension:&str,variant:&str,field:&str,path:&[FieldPath],value:Option<&InputValue>,file:Option<&str>)->Result<()> {
        let(owner,_,_)=find_record(self,record)?;
        let source_type=schema.resolve_type(&record.actual_type).ok_or_else(||Failure::missing("unknown record type"))?;
        let source_field=schema.field(&record.actual_type,field).ok_or_else(||Failure::missing("unknown field"))?;
        if !source_field.dimension.as_ref().is_some_and(|d|d.dimension.as_str()==dimension){return Err(Failure::invalid("field is not bound to this dimension"));}
        let dimension_schema=schema.all_dimensions().find(|d|d.name.as_str()==dimension).ok_or_else(||Failure::missing("unknown dimension"))?;
        let project=self.project()?;
        let snapshot=super::InputSnapshot::capture(&project)?;
        let mut existing=None;
        for source in snapshot.files.iter().filter(|s|s.kind=="data"){
            let text=String::from_utf8(source.contents.clone()).map_err(|e|Failure::invalid(e.to_string()))?;
            let(ast,_)=parse_cfd(&text);
            for block in ast.dimensions.iter().filter(|b|b.dimension==dimension&&b.variant==variant){
                for target in block.records.iter().filter(|r|r.key==record.key&&schema.inheritance_root(&r.type_name)==schema.inheritance_root(&record.actual_type)){
                    if let Some(f)=target.fields.iter().find(|f|f.name==field){
                        if existing.is_some(){return Err(Failure::conflict("dimension field has multiple sources"));}
                        let span=super::query::ast_value_span(&f.value);
                        let value=super::values::expression(schema,&source_field.value_type,&text[span.start..span.end])?;
                        existing=Some((source.absolute.clone(),value));
                    }
                }
            }
        }
        if existing.is_none()&&value.is_none(){return Ok(());}
        let destination=if let Some((path,_))=&existing{path.clone()}else if let Some(file)=file{
            let original=self.resolve(file,true)?;self.track(&original)?;self.shadow(&original)
        }else{
            let dir=project.config().dimensions.get(dimension).and_then(|d|d.dir.as_ref()).ok_or_else(||Failure::invalid(format!("configure dimensions.{dimension}.dir before creating overrides")))?;
            let relative=owner.strip_prefix(project.root_dir()).map_err(|_|Failure::invalid("automatic dimension files require a project-relative owner"))?;
            project.resolve_path(dir).join(relative)
        };
        let new_value=if let Some(value)=value{
            if path.is_empty(){Some(value.resolve(schema,&source_field.value_type)?)}else{
                let mut current=existing.as_ref().map(|(_,v)|v.clone()).ok_or_else(||Failure::missing("set the complete dimension field before editing a nested value"))?;
                let mut full=vec![CfdPathSegment::Field(field.into())];full.extend(internal_path(path));
                let expected=crate::write_rules::expected_type_for_cfd_path(schema,&record.actual_type,&full,"CLI-PATH","CLI")?;
                replace_nested(&mut current,&internal_path(path),value.resolve(schema,&expected)?)?;Some(current)
            }
        }else{None};
        if existing.as_ref().map(|(_,v)|v)==new_value.as_ref(){return Ok(());}
        let original=self.unshadow(&destination);self.track(&original)?;
        let writer=crate::cfd_loader::CfdWriter::new();
        writer.write_dimension_value(&crate::WriteDimensionValueRequest{source:&CfdSource{location:CfdSourcePath::new(destination),display_name:field.into()},
            schema:crate::DimensionSourceSchema{schema,dimension:dimension_schema,source_type,source_field},
            source_key:&crate::RecordKey::new(&record.key).map_err(|e|Failure::invalid(e.to_string()))?,
            variant:&crate::VariantName::new(variant).map_err(|e|Failure::invalid(e.to_string()))?,new_value:new_value.as_ref()})?;
        writer.publish()?;self.include(&original)?;Ok(())
    }
}

fn as_key(value:CfdValue)->Result<crate::CfdDictKey>{Ok(match value{CfdValue::Int(v)=>crate::CfdDictKey::Int(v),CfdValue::String(v)=>crate::CfdDictKey::String(v),CfdValue::Enum(v)=>crate::CfdDictKey::Enum(v),_=>return Err(Failure::invalid("invalid dictionary key type"))})}
pub fn internal_path(path:&[FieldPath])->Vec<CfdPathSegment>{path.iter().map(FieldPath::internal).collect()}

pub fn find_record(candidate:&Candidate,record:&RecordTarget)->Result<(PathBuf,AstRecord,String)> {
    let project=candidate.project()?;
    let snapshot=super::InputSnapshot::capture(&project)?;
    let mut found=None;
    for file in snapshot.files.iter().filter(|f|f.kind=="data"){
        let text=String::from_utf8(file.contents.clone()).map_err(|e|Failure::invalid(e.to_string()))?;
        let(ast,errors)=parse_cfd(&text);
        if !errors.is_empty(){return Err(Failure::invalid(format!("cannot edit malformed CFD `{}`",file.path)));}
        for item in ast.records{
            if item.type_name==record.actual_type&&item.key==record.key{
                if found.is_some(){return Err(Failure::conflict("record identity is ambiguous"));}
                found=Some((file.absolute.clone(),item,text.clone()));
            }
        }
    }
    found.ok_or_else(||Failure::missing(format!("record `{}::{}` does not exist",record.actual_type,record.key)))
}

pub fn read_value(schema:&CftSchema,record:&RecordTarget,ast:&AstRecord,text:&str,path:&[CfdPathSegment])->Result<Option<CfdValue>> {
    let Some(CfdPathSegment::Field(name))=path.first()else{return Err(Failure::invalid("path must start with a field"));};
    let field=schema.field(&record.actual_type,name).ok_or_else(||Failure::missing("unknown field"))?;
    let mut value=if let Some(f)=ast.fields.iter().find(|f|&f.name==name){let span=super::query::ast_value_span(&f.value);super::values::expression(schema,&field.value_type,&text[span.start..span.end])?}
        else {if field.default.is_none()&&!matches!(field.value_type,CftValueType::Option(_)){return Ok(None);}
            let draft=crate::mutation::defaults::create_record_draft_for_type(schema,&record.actual_type)?;
            let Some(value)=draft.fields.into_iter().find(|f|f.name==*name).and_then(|f|f.value)else{return Ok(None);};value};
    for segment in &path[1..]{while let CfdValue::OptionSome(inner)=value{value=*inner;}
        value=match(segment,&value){(CfdPathSegment::Field(name),CfdValue::Object(v))=>v.field(name).cloned(),(CfdPathSegment::Index(i),CfdValue::Array(v))=>v.get(*i).cloned(),
            (CfdPathSegment::DictKey(key),CfdValue::Dict(v))=>v.iter().find(|(k,_)|crate::data_model::format_cfd_dict_key(k)==*key).map(|(_,v)|v.clone()),_=>None}.ok_or_else(||Failure::missing("nested value is unreachable"))?;
    }
    Ok(Some(value))
}
fn materialized_top(schema:&CftSchema,record:&RecordTarget,ast:&AstRecord,text:&str,path:&[CfdPathSegment],value:&CfdValue)->Result<Option<CfdValue>>{
    if path.len()<2{return Ok(None);}
    let mut top=read_value(schema,record,ast,text,&path[..1])?.ok_or_else(||Failure::missing("top-level value is missing"))?;
    replace_nested(&mut top,&path[1..],value.clone())?;Ok(Some(top))
}
pub fn replace_nested(current:&mut CfdValue,path:&[CfdPathSegment],value:CfdValue)->Result<()> {
    if path.is_empty(){*current=value;return Ok(());}
    if let CfdValue::OptionSome(inner)=current{return replace_nested(inner,path,value);}
    let next=match(&path[0],current){(CfdPathSegment::Field(name),CfdValue::Object(v))=>v.fields.get_mut(name.as_str()),(CfdPathSegment::Index(i),CfdValue::Array(v))=>v.get_mut(*i),
        (CfdPathSegment::DictKey(key),CfdValue::Dict(v))=>v.iter_mut().find(|(k,_)|crate::data_model::format_cfd_dict_key(k)==*key).map(|(_,v)|v),_=>None}.ok_or_else(||Failure::missing("nested value is unreachable"))?;
    replace_nested(next,&path[1..],value)
}

fn rewrite_dimensions(candidate:&Candidate,writer:&crate::cfd_loader::CfdWriter,schema:&CftSchema,record:&RecordTarget,new_key:Option<&str>)->Result<()> {
    let snapshot=super::InputSnapshot::capture(&candidate.project()?)?;
    let old=crate::RecordKey::new(&record.key).map_err(|e|Failure::invalid(e.to_string()))?;
    let new=new_key.map(crate::RecordKey::new).transpose().map_err(|e|Failure::invalid(e.to_string()))?;
    for file in snapshot.files.iter().filter(|f|f.kind=="data"){
        writer.rewrite_dimension_record(&crate::RewriteDimensionRecordRequest{source:&CfdSource{location:CfdSourcePath::new(file.absolute.clone()),display_name:file.path.clone()},schema,actual_type:&record.actual_type,old_key:&old,new_key:new.as_ref()})?;
    }Ok(())
}
