//! 版本化批量值传输。引用采用值身份，托管端按需读取记录并保持对象唯一性。
use coflow_core::runtime::{Runtime, Value, ValueId};
use std::collections::{BTreeSet, VecDeque};

pub(super) fn encode_dynamic(runtime: &Runtime, root: ValueId) -> Result<Vec<u8>, String> {
    let mut output = Writer(b"CFVI".to_vec());
    output.u32(1)?;
    let count_offset = output.0.len();
    output.u32(0)?;
    let count = runtime.visit_value_graph(Some(root), |id, value| {
        encode_value(&mut output, id, value).map_err(coflow_core::vm::ExecutionError::InvalidAccess)
    }).map_err(|error| error.to_string())?;
    let count = u32::try_from(count).map_err(|_| "value image length overflow")?;
    output.0[count_offset..count_offset + 4].copy_from_slice(&count.to_le_bytes());
    Ok(output.0)
}

/// 固定记录只传当前记录和内联值；其他记录保留身份，由托管端按需读取。
pub(super) fn encode_record(runtime: &Runtime, root: ValueId) -> Result<Vec<u8>, String> {
    let mut singletons = BTreeSet::new();
    for ty in runtime.contract().schema().all_types() {
        if let Ok(id) = runtime.singleton(&ty.name) { singletons.insert(id); }
    }
    let mut output = Writer(b"CFVI".to_vec());
    output.u32(1)?;
    let count_offset = output.0.len();
    output.u32(0)?;
    let mut pending = VecDeque::from([root]);
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop_front() {
        if !visited.insert(id) { continue; }
        let value = runtime.stored_value(id).map_err(|error| error.to_string())?;
        output.id(id)?;
        let reference = id != root && (singletons.contains(&id)
            || matches!(value.as_ref(), Value::Object { key: Some(_), .. }));
        encode_record_value(&mut output, value.as_ref(), reference, &mut pending)?;
    }
    let count = u32::try_from(visited.len()).map_err(|_| "record image length overflow")?;
    output.0[count_offset..count_offset + 4].copy_from_slice(&count.to_le_bytes());
    Ok(output.0)
}

fn encode_record_value(output: &mut Writer, value: &Value, reference: bool, pending: &mut VecDeque<ValueId>) -> Result<(), String> {
    if reference {
        let Value::Object { type_name, key, bases, .. } = value else { return Err("record identity is not an object".into()); };
        output.byte(13)?;
        output.text(type_name)?;
        output.byte(u8::from(key.is_some()))?;
        if let Some(key) = key { output.text(key)?; }
        output.count(bases.len())?;
        for (name, id) in bases { output.text(name)?; output.id(*id)?; pending.push_back(*id); }
        return Ok(());
    }
    match value {
        Value::None => output.byte(0)?,
        Value::Bool(value) => { output.byte(1)?; output.byte(u8::from(*value))?; }
        Value::Int(value) => { output.byte(2)?; output.u32(*value as u32)?; }
        Value::Float(value) => { output.byte(3)?; output.u32(value.to_bits())?; }
        Value::String(value) => { output.byte(4)?; output.text(value)?; }
        Value::Enum { type_name, value } => { output.byte(5)?; output.text(type_name)?; output.u32(*value)?; }
        Value::Object { type_name, key, fields, bases, .. } => {
            output.byte(6)?; output.text(type_name)?;
            output.byte(u8::from(key.is_some()))?; if let Some(key) = key { output.text(key)?; }
            output.count(fields.len())?;
            for (name, id) in fields { output.text(name)?; output.id(*id)?; pending.push_back(*id); }
            output.count(bases.len())?;
            for (name, id) in bases { output.text(name)?; output.id(*id)?; pending.push_back(*id); }
        }
        Value::Array(items) => { output.byte(7)?; output.count(items.len())?; for id in items { output.id(id)?; pending.push_back(id); } }
        Value::Dict(items) => { output.byte(8)?; output.count(items.len())?; for (key, value) in items.values() { output.id(*key)?; output.id(*value)?; pending.push_back(*key); pending.push_back(*value); } }
        Value::Function { source, .. } => { output.byte(9)?; output.text(source)?; }
        Value::Template { source, .. } => { output.byte(10)?; output.text(source)?; }
        Value::Dimension { default, variants, explicit } => {
            output.byte(11)?; output.id(*default)?; pending.push_back(*default); output.count(variants.len())?;
            for (name, id) in variants { output.text(name)?; output.id(*id)?; output.byte(u8::from(explicit.contains(name)))?; pending.push_back(*id); }
        }
        Value::HostData { .. } => output.byte(12)?,
    }
    Ok(())
}
fn encode_value(output: &mut Writer, id: ValueId, value: &Value) -> Result<(), String> {
    output.id(id)?;
        match value {
            Value::None => output.byte(0)?,
            Value::Bool(value) => { output.byte(1)?; output.byte(u8::from(*value))?; },
            Value::Int(value) => { output.byte(2)?; output.u32(*value as u32)?; },
            Value::Float(value) => { output.byte(3)?; output.u32(value.to_bits())?; },
            Value::String(value) => { output.byte(4)?; output.text(value)?; },
            Value::Enum { type_name, value } => { output.byte(5)?; output.text(type_name)?; output.u32(*value)?; },
            Value::Object { type_name, key, fields, bases, .. } => {
                output.byte(6)?; output.text(type_name)?;
                output.byte(u8::from(key.is_some()))?; if let Some(key) = key { output.text(key)?; }
                output.count(fields.len())?;
                for (name, id) in fields { output.text(name)?; output.id(*id)?; }
                output.count(bases.len())?;
                for (name, id) in bases { output.text(name)?; output.id(*id)?; }
            },
            Value::Array(items) => { output.byte(7)?; output.count(items.len())?; for id in items { output.id(id)?; } },
            Value::Dict(items) => { output.byte(8)?; output.count(items.len())?; for (key, value) in items.values() { output.id(*key)?; output.id(*value)?; } },
            Value::Function { source, .. } => { output.byte(9)?; output.text(source)?; },
            Value::Template { source, .. } => { output.byte(10)?; output.text(source)?; },
            Value::Dimension { default, variants, explicit } => {
                output.byte(11)?; output.id(*default)?; output.count(variants.len())?;
                for (name, id) in variants { output.text(name)?; output.id(*id)?; output.byte(u8::from(explicit.contains(name)))?; }
            },
            Value::HostData { .. } => output.byte(12)?,
        }
    Ok(())
}
struct Writer(Vec<u8>);
impl Writer {
    fn extend(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.0.try_reserve(bytes.len()).map_err(|_| "value image allocation failed")?;
        self.0.extend_from_slice(bytes); Ok(())
    }
    fn byte(&mut self, value: u8) -> Result<(), String> { self.extend(&[value]) }
    fn u32(&mut self, value: u32) -> Result<(), String> { self.extend(&value.to_le_bytes()) }
    fn count(&mut self, value: usize) -> Result<(), String> { self.u32(u32::try_from(value).map_err(|_| "value image length overflow")?) }
    fn id(&mut self, id: ValueId) -> Result<(), String> { self.extend(&u64::try_from(id).map_err(|_| "value identity overflow")?.checked_add(1).ok_or("value identity overflow")?.to_le_bytes()) }
    fn text(&mut self, value: &str) -> Result<(), String> { self.count(value.len())?; self.extend(value.as_bytes()) }
}
