//! 函数调用的显式小端值编码；不把 Rust/宿主结构体布局作为线协议。
use super::*;
use coflow_core::{runtime::HostValue, vm::ExecutionError};

fn runtime_handle(identity: u64) -> Result<u64, String> {
    LOCAL_ENTRIES.try_with(|entries| entries.borrow().0
        .iter()
        .find_map(|(handle, entry)| match entry {
            Entry::Runtime(runtime) if runtime.identity() == identity => Some(*handle),
            _ => None,
        }))
        .map_err(|_| "execution thread is shutting down")?
        .ok_or_else(|| "Runtime handle is no longer registered".into())
}
fn write_text(bytes: &mut Vec<u8>, value: &str) -> Result<(), String> {
    bytes.extend_from_slice(
        &u32::try_from(value.len())
            .map_err(|_| "string too large")?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}
pub(super) fn encode_call(field: &str, args: &[HostValue]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    write_text(&mut bytes, field)?;
    encode_arguments_into(&mut bytes, args)?;
    Ok(bytes)
}
pub(super) fn encode_arguments_into(bytes: &mut Vec<u8>, args: &[HostValue]) -> Result<(), String> {
    bytes.extend_from_slice(
        &u32::try_from(args.len())
            .map_err(|_| "too many arguments")?
            .to_le_bytes(),
    );
    for value in args {
        match value {
            HostValue::Array(values) => { bytes.push(7); encode_arguments_into(bytes, values)?; }
            HostValue::Dictionary(values) => {
                bytes.push(8); bytes.extend_from_slice(&u32::try_from(values.len()).map_err(|_| "too many entries")?.to_le_bytes());
                for (key, value) in values { encode_arguments_into(bytes, &[key.clone(), value.clone()])?; }
            }
            HostValue::Data { type_name, fields } => {
                bytes.push(6); write_text(bytes, type_name)?; bytes.extend_from_slice(&u32::try_from(fields.len()).map_err(|_| "too many fields")?.to_le_bytes());
                for (name, value) in fields { write_text(bytes, name)?; encode_arguments_into(bytes, std::slice::from_ref(value))?; }
            }
            HostValue::None => bytes.push(0),
            HostValue::Bool(value) => {
                bytes.extend_from_slice(&[1, u8::from(*value)]);
            }
            HostValue::Int(value) => {
                bytes.push(2);
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            HostValue::Float(value) => {
                bytes.push(3);
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            HostValue::String(value) => {
                bytes.push(4);
                write_text(bytes, value)?;
            }
            HostValue::Enum { type_name, value } => {
                bytes.push(5);
                write_text(bytes, type_name)?;
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            HostValue::Existing { runtime, value } => {
                bytes.push(11);
                bytes.extend_from_slice(&runtime_handle(*runtime)?.to_le_bytes());
                bytes.extend_from_slice(&(*value as u64 + 1).to_le_bytes());
            }
        }
    }
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let bytes = self
            .bytes
            .get(self.position..self.position.checked_add(N).ok_or("input overflow")?)
            .ok_or("truncated arguments")?;
        self.position += N;
        bytes.try_into().map_err(|_| "invalid width".into())
    }
    fn count(&mut self) -> Result<usize, String> {
        let count = u32::from_le_bytes(self.take()?) as usize;
        if count > 1_000_000 || count > self.bytes.len().saturating_sub(self.position) { return Err("argument count exceeds payload".into()); }
        Ok(count)
    }
    fn string(&mut self) -> Result<String, String> {
        let length = u32::from_le_bytes(self.take()?) as usize;
        let end = self.position.checked_add(length).ok_or("input overflow")?;
        let value = text(
            self.bytes
                .get(self.position..end)
                .ok_or("truncated string")?,
        )?
        .to_string();
        self.position = end;
        Ok(value)
    }
}
pub(super) fn decode_arguments(bytes: &[u8]) -> Result<Vec<HostValue>, String> {
    if bytes.is_empty() { return Ok(Vec::new()); }
    let mut reader = Reader { bytes, position: 0 };
    let values = decode_list(&mut reader, 0)?;
    if reader.position != bytes.len() { return Err("trailing argument bytes".into()); }
    Ok(values)
}
fn decode_list(reader: &mut Reader<'_>, depth: usize) -> Result<Vec<HostValue>, String> {
    if depth >= 128 { return Err("argument nesting limit exceeded".into()); }
    let count = reader.count()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count { values.push(match reader.take::<1>()?[0] {
            0 => HostValue::None,
            1 => match reader.take::<1>()?[0] {
                0 => HostValue::Bool(false),
                1 => HostValue::Bool(true),
                _ => return Err("invalid bool".into()),
            },
            2 => HostValue::Int(i32::from_le_bytes(reader.take()?)),
            3 => HostValue::Float(f32::from_bits(u32::from_le_bytes(reader.take()?))),
            4 => HostValue::String(reader.string()?),
            5 => HostValue::Enum {
                type_name: reader.string()?,
                value: u32::from_le_bytes(reader.take()?),
            },
            6 => {
                let type_name = reader.string()?; let count = reader.count()?;
                let mut fields = Vec::with_capacity(count);
                for _ in 0..count { let name = reader.string()?; let mut values = decode_list(reader, depth + 1)?; if values.len() != 1 { return Err("invalid data field".into()); } fields.push((name, values.remove(0))); }
                HostValue::Data { type_name, fields }
            }
            7 => HostValue::Array(decode_list(reader, depth + 1)?),
            8 => {
                let count = reader.count()?; let mut items = Vec::with_capacity(count);
                for _ in 0..count { let mut values = decode_list(reader, depth + 1)?; if values.len() != 2 { return Err("invalid dictionary entry".into()); } let value = values.pop().unwrap(); items.push((values.pop().unwrap(), value)); }
                HostValue::Dictionary(items)
            }
            11 => {
                let handle = u64::from_le_bytes(reader.take()?);
                let raw = u64::from_le_bytes(reader.take()?);
                let (runtime, value) = target(handle, raw)?;
                HostValue::Existing {
                    runtime: runtime.identity(),
                    value,
                }
            }
            _ => return Err("unknown argument tag".into()), }); }
    Ok(values)
}
pub(super) fn response(value: HostValue) -> Result<Response, String> {
    Ok(match value {
        value @ (HostValue::Data { .. } | HostValue::Array(_) | HostValue::Dictionary(_)) => {
            let mut bytes = Vec::new(); encode_arguments_into(&mut bytes, &[value])?;
            Response { tag: 12, ..buffer(bytes)? }
        }
        HostValue::None => Response::default(),
        HostValue::Bool(value) => Response {
            tag: 1,
            integer: i64::from(value),
            ..Response::default()
        },
        HostValue::Int(value) => Response {
            tag: 2,
            integer: i64::from(value),
            ..Response::default()
        },
        HostValue::Float(value) => Response {
            tag: 3,
            number: f64::from(value),
            ..Response::default()
        },
        HostValue::String(value) => Response {
            tag: 4,
            ..buffer(value.into_bytes())?
        },
        HostValue::Enum { type_name, value } => Response {
            tag: 5,
            integer: i64::from(value),
            ..buffer(type_name.into_bytes())?
        },
        HostValue::Existing { runtime, value } => Response {
            tag: 11,
            handle: runtime_handle(runtime)?,
            length: value as u64 + 1,
            ..Response::default()
        },
    })
}
pub(super) fn host_result(result: Response) -> Result<HostValue, ExecutionError> {
    let invalid = ExecutionError::InvalidAccess;
    match result.tag {
        0 => Ok(HostValue::None),
        1 => match result.integer {
            0 => Ok(HostValue::Bool(false)),
            1 => Ok(HostValue::Bool(true)),
            _ => Err(invalid("invalid Host bool".into())),
        },
        2 => i32::try_from(result.integer)
            .map(HostValue::Int)
            .map_err(|_| invalid("Host int outside i32 range".into())),
        3 => Ok(HostValue::Float(result.number as f32)),
        4 => take_buffer(result.handle)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
            .map(HostValue::String)
            .map_err(invalid),
        5 => Ok(HostValue::Enum {
            type_name: String::from_utf8(take_buffer(result.handle).map_err(invalid)?)
                .map_err(|e| invalid(e.to_string()))?,
            value: u32::try_from(result.integer).map_err(|_| invalid("enum range".into()))?,
        }),
        12 => {
            let bytes = take_buffer(result.handle).map_err(invalid)?;
            let mut values = decode_arguments(&bytes).map_err(invalid)?;
            if values.len() != 1 { return Err(invalid("invalid Host aggregate".into())); }
            Ok(values.remove(0))
        }
        11 => {
            let (runtime, value) = target(result.handle, result.length).map_err(invalid)?;
            Ok(HostValue::Existing {
                runtime: runtime.identity(),
                value,
            })
        }
        _ => Err(invalid("invalid Host data tag".into())),
    }
}
