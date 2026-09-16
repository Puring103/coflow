//! 函数调用的显式小端值编码；不把 Rust/宿主结构体布局作为线协议。
use super::*;
use coflow_core::{runtime::HostValue, vm::ExecutionError};

fn runtime_handle(identity: u64) -> Result<u64, String> {
    registry()
        .lock()
        .map_err(|_| "handle registry unavailable")?
        .entries
        .iter()
        .find_map(|(handle, entry)| match entry {
            Entry::Runtime(runtime) if runtime.identity() == identity => Some(*handle),
            _ => None,
        })
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
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut reader = Reader { bytes, position: 0 };
    let count = u32::from_le_bytes(reader.take()?) as usize;
    if count > usize::from(u16::MAX) + 1 || count > bytes.len().saturating_sub(4) {
        return Err("argument count exceeds payload".into());
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(match reader.take::<1>()?[0] {
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
            11 => {
                let handle = u64::from_le_bytes(reader.take()?);
                let raw = u64::from_le_bytes(reader.take()?);
                let (runtime, value) = target(handle, raw)?;
                HostValue::Existing {
                    runtime: runtime.identity(),
                    value,
                }
            }
            _ => return Err("unknown argument tag".into()),
        });
    }
    if reader.position != bytes.len() {
        return Err("trailing argument bytes".into());
    }
    Ok(values)
}
pub(super) fn response(value: HostValue) -> Result<Response, String> {
    Ok(match value {
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
