//! 只读类型契约及其二进制格式；可执行程序在 Runtime 绑定 CFD 时生成。
use crate::schema::CftSchema;
use bincode::Options;
use sha2::{Digest, Sha256};
use std::{fmt, sync::Arc};

const MAGIC: &[u8; 8] = b"COFLOWCT";
const VERSION: u32 = 5;
const HEADER: usize = 8 + 4 + 8 + 32;

#[derive(Debug, Clone)]
pub struct Contract {
    schema: Arc<CftSchema>,
    ir: crate::vm::contract_programs::ContractIr,
    identity: [u8; 32],
}

#[derive(Debug)]
pub enum ContractError {
    Format(String),
    Semantic(crate::vm::contract_programs::ProgramDiagnostic),
}
impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(message) => f.write_str(message),
            Self::Semantic(error) => write!(f, "{}:{}..{}: {}", error.module, error.span.start, error.span.end, error.message),
        }
    }
}
impl std::error::Error for ContractError {}

impl Contract {
    pub fn new(schema: CftSchema) -> Result<Self, ContractError> {
        let ir = crate::vm::contract_programs::ContractIr::compile(&schema).map_err(ContractError::Semantic)?;
        ir.validate(&schema).map_err(ContractError::Semantic)?;
        let payload = encode(&schema, &ir)?;
        Ok(Self {
            schema: Arc::new(schema),
            ir,
            identity: Sha256::digest(&payload).into(),
        })
    }
    pub(crate) fn ir(&self) -> &crate::vm::contract_programs::ContractIr {
        &self.ir
    }
    pub fn schema(&self) -> &CftSchema {
        &self.schema
    }
    pub fn identity(&self) -> &[u8; 32] {
        &self.identity
    }
    pub fn to_bytes(&self) -> Result<Vec<u8>, ContractError> {
        let payload = encode(&self.schema, &self.ir)?;
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
        let (schema, ir): (CftSchema, crate::vm::contract_programs::ContractIr) = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(length)
            .reject_trailing_bytes()
            .deserialize(payload)
            .map_err(|e| ContractError::Format(e.to_string()))?;
        // 摘要不证明 IR 可信，解码后仍验证结构和控制流。
        ir.validate(&schema).map_err(ContractError::Semantic)?;
        Ok(Self {
            schema: Arc::new(schema),
            ir,
            identity,
        })
    }
}
fn encode(schema: &CftSchema, ir: &crate::vm::contract_programs::ContractIr) -> Result<Vec<u8>, ContractError> {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialize(&(schema, ir))
        .map_err(|e| ContractError::Format(e.to_string()))
}

#[cfg(all(test, feature = "cft-compiler"))]
mod tests {
    use super::*;
    use crate::{runtime::{HostValue, RuntimeBuilder}, schema::{build_schema, parse_modules, CftFile, ModuleId}, vm::{executor::ExecutionLimits, ir::{LocationId, Operation}}};

    fn contract() -> Contract {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"),
            "table Item { value: int; run: fn() -> int => { self.value + 1 }; }")]);
        Contract::new(build_schema(&modules).unwrap()).unwrap()
    }

    fn resign(contract: &Contract) -> Vec<u8> {
        let payload = encode(&contract.schema, &contract.ir).unwrap();
        let mut bytes = Vec::from(MAGIC.as_slice());
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&Sha256::digest(&payload));
        bytes.extend_from_slice(&payload);
        bytes
    }

    #[test]
    fn rehashed_invalid_ir_is_rejected_before_execution() {
        let mut wrong_type = contract();
        let function = Arc::make_mut(wrong_type.ir.functions.values_mut().next().unwrap());
        function.result = crate::schema::CftValueType::Bool;
        assert!(Contract::from_bytes(&resign(&wrong_type)).is_err());

        let mut bad_branch = contract();
        let function = Arc::make_mut(bad_branch.ir.functions.values_mut().next().unwrap());
        function.body[0].operation = Operation::Jump(LocationId(u32::MAX));
        assert!(Contract::from_bytes(&resign(&bad_branch)).is_err());
    }

    #[test]
    fn rehashed_symbol_and_builtin_type_forgery_is_rejected() {
        for operation in [
            Operation::Reference("Missing::record".into()),
            Operation::Reference("Item::record".into()),
            Operation::Reference("$const::Missing".into()),
            Operation::Builtin { name: "len".into(), receiver: crate::vm::ir::ValueId(0), arguments: vec![] },
            Operation::Builtin { name: "$records::Item".into(), receiver: crate::vm::ir::ValueId(0), arguments: vec![] },
        ] {
            let mut forged = contract();
            let function = Arc::make_mut(forged.ir.functions.values_mut().next().unwrap());
            // 原节点结果是 int；引用和内建操作不能自行宣称任意结果类型。
            function.body[0].operation = operation;
            assert!(Contract::from_bytes(&resign(&forged)).is_err());
        }
    }

    #[test]
    fn rehashed_unguarded_narrowing_is_rejected() {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("guard"),
            "table Item { run: fn(value: int?) -> int => { if value is Some(found) { found } else { 0 } }; }")]);
        let original = Contract::new(build_schema(&modules).unwrap()).unwrap();
        Contract::from_bytes(&resign(&original)).unwrap();
        let mut forged = original.clone();
        let function = Arc::make_mut(forged.ir.functions.values_mut().next().unwrap());
        for node in &mut function.body {
            if matches!(node.operation, Operation::IsSome(_)) {
                node.operation = Operation::Constant(crate::vm::bytecode::Constant::Bool(true));
            }
        }
        assert!(Contract::from_bytes(&resign(&forged)).is_err());
    }

    #[test]
    fn execution_uses_ir_even_when_tool_source_is_unparseable() {
        let mut original = contract();
        // 工具源码不是执行输入；更改诊断文本不改变已绑定的语义节点。
        for function in original.ir.functions.values_mut() {
            Arc::make_mut(function).source = "not a function".into();
        }
        let mut schema = serde_json::to_value(original.schema.as_ref()).unwrap();
        for source in schema["sources"].as_object_mut().unwrap().values_mut() {
            let length = source["source"].as_str().unwrap().len();
            source["source"] = serde_json::Value::String("?".repeat(length));
        }
        original.schema = Arc::new(serde_json::from_value(schema).unwrap());
        let loaded = Arc::new(Contract::from_bytes(&resign(&original)).unwrap());
        for (value, expected) in [(4, 5), (17, 18)] {
            let mut builder = RuntimeBuilder::new(loaded.clone());
            builder.add_text(&format!("a: Item {{ value: {value} }}"), None);
            let runtime = builder.build().runtime.unwrap();
            let function = runtime.field(runtime.record("Item", "a").unwrap(), "run").unwrap();
            assert!(matches!(runtime.invoke(function, &[], ExecutionLimits::default()).unwrap(), HostValue::Int(result) if result == expected));
        }
    }

}
