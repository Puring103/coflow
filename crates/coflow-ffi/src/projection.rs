//! 版本化批量只读投影。引用采用值身份，先传节点再由托管端连边，支持记录环。
use coflow_core::runtime::{Runtime, Value, ValueId};

pub(super) fn encode(runtime: &Runtime, root: Option<ValueId>) -> Result<Vec<u8>, String> {
    let mut output = Writer(b"CFSP".to_vec());
    output.u32(1)?;
    let mut tables = Vec::new();
    if root.is_none() {
        for ty in runtime.contract().schema().all_types() {
            if let Ok(ids) = runtime.table_values(&ty.name) {
                tables.push((ty.name.to_string(), false, ids.to_vec()));
            } else if let Ok(id) = runtime.singleton(&ty.name) {
                tables.push((ty.name.to_string(), true, vec![id]));
            }
        }
    }
    output.count(tables.len())?;
    for (name, singleton, ids) in tables {
        output.text(&name)?; output.byte(u8::from(singleton))?; output.count(ids.len())?;
        for id in ids { output.id(id)?; }
    }
    let count_offset = output.0.len();
    output.u32(0)?;
    let count = runtime.visit_projection(root, |id, value| {
        encode_value(&mut output, id, value).map_err(coflow_core::vm::ExecutionError::InvalidAccess)
    }).map_err(|error| error.to_string())?;
    let count = u32::try_from(count).map_err(|_| "projection length overflow")?;
    output.0[count_offset..count_offset + 4].copy_from_slice(&count.to_le_bytes());
    Ok(output.0)
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
        self.0.try_reserve(bytes.len()).map_err(|_| "projection allocation failed")?;
        self.0.extend_from_slice(bytes); Ok(())
    }
    fn byte(&mut self, value: u8) -> Result<(), String> { self.extend(&[value]) }
    fn u32(&mut self, value: u32) -> Result<(), String> { self.extend(&value.to_le_bytes()) }
    fn count(&mut self, value: usize) -> Result<(), String> { self.u32(u32::try_from(value).map_err(|_| "projection length overflow")?) }
    fn id(&mut self, id: ValueId) -> Result<(), String> { self.extend(&u64::try_from(id).map_err(|_| "projection identity overflow")?.checked_add(1).ok_or("projection identity overflow")?.to_le_bytes()) }
    fn text(&mut self, value: &str) -> Result<(), String> { self.count(value.len())?; self.extend(value.as_bytes()) }
}
