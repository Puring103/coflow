//! 只读契约及其二进制格式；加载契约不重新编译 CFT。
use crate::schema::CftSchema;
use bincode::Options;
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};

const MAGIC: &[u8; 8] = b"COFLOWCT";
const VERSION: u32 = 2;
const HEADER: usize = 8 + 4 + 8 + 32;

#[derive(Debug, Clone)]
pub struct Contract {
    schema: Arc<CftSchema>,
    identity: [u8; 32],
    programs: crate::vm::contract_programs::ContractPrograms,
}

#[derive(Debug)]
pub enum ContractError {
    Format(String),
    Compilation(crate::vm::contract_programs::ProgramDiagnostic),
}
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(message) => f.write_str(message),
            Self::Compilation(error) => write!(
                f,
                "{}:{}..{}: {}",
                error.path.as_deref().unwrap_or(error.module.as_str()),
                error.span.start,
                error.span.end,
                error.message
            ),
        }
    }
}
impl std::error::Error for ContractError {}

impl Contract {
    pub fn new(schema: CftSchema) -> Result<Self, ContractError> {
        let programs = crate::vm::contract_programs::ContractPrograms::compile(&schema)
            .map_err(ContractError::Compilation)?;
        let payload = encode(&schema, &programs)?;
        Ok(Self {
            schema: Arc::new(schema),
            identity: Sha256::digest(&payload).into(),
            programs,
        })
    }
    pub fn schema(&self) -> &CftSchema {
        &self.schema
    }
    pub fn programs(&self) -> &crate::vm::contract_programs::ContractPrograms {
        &self.programs
    }
    pub fn identity(&self) -> &[u8; 32] {
        &self.identity
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, ContractError> {
        let payload = encode(&self.schema, &self.programs)?;
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
            return Err(ContractError::Format("invalid contract header".into()));
        }
        let version = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .map_err(|_| ContractError::Format("invalid version".into()))?,
        );
        if version != VERSION {
            return Err(ContractError::Format(format!(
                "unsupported contract version {version}"
            )));
        }
        let length = u64::from_le_bytes(
            bytes[12..20]
                .try_into()
                .map_err(|_| ContractError::Format("invalid length".into()))?,
        );
        if length != (bytes.len() - HEADER) as u64 {
            return Err(ContractError::Format(
                "truncated or trailing contract data".into(),
            ));
        }
        let payload = &bytes[HEADER..];
        let identity: [u8; 32] = Sha256::digest(payload).into();
        if identity.as_slice() != &bytes[20..HEADER] {
            return Err(ContractError::Format("contract digest mismatch".into()));
        }
        let (schema, programs): (CftSchema, crate::vm::contract_programs::ContractPrograms) =
            bincode::DefaultOptions::new()
                .with_fixint_encoding()
                .with_limit(length)
                .reject_trailing_bytes()
                .deserialize(payload)
                .map_err(|e| ContractError::Format(e.to_string()))?;
        for program in programs
            .functions
            .values()
            .chain(programs.checks.iter().map(|check| &check.program))
        {
            program.validate().map_err(ContractError::Format)?;
        }
        Ok(Self {
            schema: Arc::new(schema),
            identity,
            programs,
        })
    }
}
fn encode(
    schema: &CftSchema,
    programs: &crate::vm::contract_programs::ContractPrograms,
) -> Result<Vec<u8>, ContractError> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(&(schema, programs))
        .map_err(|e| ContractError::Format(e.to_string()))
}
