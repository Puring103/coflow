//! 只读契约及其二进制格式；加载契约不重新编译 CFT。
use crate::schema::CftSchema;
use bincode::Options;
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};

const MAGIC: &[u8; 8] = b"COFLOWCT";
const VERSION: u32 = 1;
const HEADER: usize = 8 + 4 + 8 + 32;

#[derive(Debug, Clone)]
pub struct Contract {
    schema: Arc<CftSchema>,
    identity: [u8; 32],
}

#[derive(Debug)]
pub struct ContractError(pub String);
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ContractError {}

impl Contract {
    pub fn new(schema: CftSchema) -> Result<Self, ContractError> {
        let payload = encode(&schema)?;
        Ok(Self {
            schema: Arc::new(schema),
            identity: Sha256::digest(&payload).into(),
        })
    }
    pub fn schema(&self) -> &CftSchema {
        &self.schema
    }
    pub fn identity(&self) -> &[u8; 32] {
        &self.identity
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, ContractError> {
        let payload = encode(&self.schema)?;
        let mut bytes = Vec::with_capacity(HEADER + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&self.identity);
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }
    /// 长度、版本和完整性检查属于格式读取，不重复执行声明类型检查。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ContractError> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC {
            return Err(ContractError("invalid contract header".into()));
        }
        let version = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .map_err(|_| ContractError("invalid version".into()))?,
        );
        if version != VERSION {
            return Err(ContractError(format!(
                "unsupported contract version {version}"
            )));
        }
        let length = u64::from_le_bytes(
            bytes[12..20]
                .try_into()
                .map_err(|_| ContractError("invalid length".into()))?,
        );
        if length != (bytes.len() - HEADER) as u64 {
            return Err(ContractError("truncated or trailing contract data".into()));
        }
        let payload = &bytes[HEADER..];
        let identity: [u8; 32] = Sha256::digest(payload).into();
        if identity.as_slice() != &bytes[20..HEADER] {
            return Err(ContractError("contract digest mismatch".into()));
        }
        let schema = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(length)
            .reject_trailing_bytes()
            .deserialize(payload)
            .map_err(|e| ContractError(e.to_string()))?;
        Ok(Self {
            schema: Arc::new(schema),
            identity,
        })
    }
}
fn encode(schema: &CftSchema) -> Result<Vec<u8>, ContractError> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(schema)
        .map_err(|e| ContractError(e.to_string()))
}
