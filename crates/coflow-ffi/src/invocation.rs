//! 函数调用的显式小端值编码；不把 Rust/宿主结构体布局作为线协议。
use super::*;
use coflow_core::{runtime::HostValue, vm::ExecutionError};

use super::handles::runtime_handle;
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
    for value in args { encode_value(bytes, value)?; }
    Ok(())
}
fn encode_value(bytes: &mut Vec<u8>, value: &HostValue) -> Result<(), String> {
        match value {
            HostValue::Array(values) => { bytes.push(7); encode_arguments_into(bytes, values)?; }
            HostValue::Dictionary(values) => {
                bytes.push(8); bytes.extend_from_slice(&u32::try_from(values.len()).map_err(|_| "too many entries")?.to_le_bytes());
                for (key, value) in values {
                    bytes.extend_from_slice(&2u32.to_le_bytes());
                    encode_value(bytes, key)?; encode_value(bytes, value)?;
                }
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
    Ok(())
}
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    remaining_allocation: usize,
}
impl Reader<'_> {
    // 解码图有独立上限，长度前缀不能绕过 VM 入口前的内存约束。
    fn charge_allocation(&mut self, bytes: usize) -> Result<(), String> {
        self.remaining_allocation = self.remaining_allocation.checked_sub(bytes).ok_or("argument allocation budget exceeded")?;
        Ok(())
    }
    fn reserve<T>(&mut self, count: usize) -> Result<Vec<T>, String> {
        self.charge_allocation(count.checked_mul(std::mem::size_of::<T>()).ok_or("argument allocation overflow")?)?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| "argument allocation failed")?;
        Ok(values)
    }
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
        self.charge_allocation(length)?;
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
    let mut reader = Reader { bytes, position: 0, remaining_allocation: coflow_core::vm::ExecutionLimits::default().max_heap_bytes };
    let values = decode_list(&mut reader, 0)?;
    if reader.position != bytes.len() { return Err("trailing argument bytes".into()); }
    Ok(values)
}
fn decode_list(reader: &mut Reader<'_>, depth: usize) -> Result<Vec<HostValue>, String> {
    if depth >= 128 { return Err("argument nesting limit exceeded".into()); }
    let count = reader.count()?;
    let mut values = reader.reserve(count)?;
    for _ in 0..count { values.push(decode_value(reader, depth)?); }
    Ok(values)
}
fn decode_value(reader: &mut Reader<'_>, depth: usize) -> Result<HostValue, String> {
    if depth >= 128 { return Err("argument nesting limit exceeded".into()); }
    Ok(match reader.take::<1>()?[0] {
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
                let mut fields = reader.reserve(count)?;
                for _ in 0..count { let name = reader.string()?; if reader.count()? != 1 { return Err("invalid data field".into()); } fields.push((name, decode_value(reader, depth + 1)?)); }
                HostValue::Data { type_name, fields }
            }
            7 => HostValue::Array(decode_list(reader, depth + 1)?),
            8 => {
                let count = reader.count()?; let mut items = reader.reserve(count)?;
                for _ in 0..count { if reader.count()? != 2 { return Err("invalid dictionary entry".into()); } let key = decode_value(reader, depth + 1)?; let value = decode_value(reader, depth + 1)?; items.push((key, value)); }
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
            _ => return Err("unknown argument tag".into()), })
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
