//! Schema 查询契约包含声明元数据，诊断通过统一信封返回。
use super::diagnostics::CommandLocation;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct EnumCandidate {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InspectedField {
    #[serde(flatten)]
    pub declaration: crate::SchemaFieldInfo,
    #[serde(default)]
    pub declaring_type: Option<String>,
    #[serde(default)]
    pub location: Option<CommandLocation>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_candidates: Option<Vec<EnumCandidate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concrete_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_query: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "各布尔项对应可组合的语言修饰符，分别暴露声明语义。"
)]
pub struct InspectedType {
    pub kind: String,
    pub module: String,
    pub name: String,
    pub parent: Option<String>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub id_as_enum: Option<String>,
    pub fields: Vec<InspectedField>,
    #[serde(default)]
    pub location: Option<CommandLocation>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub concrete_types: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct OmittedSymbol {
    pub symbol: String,
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InspectedEnum {
    #[serde(flatten)]
    pub declaration: crate::SchemaEnumInfo,
    pub location: Option<CommandLocation>,
    pub label: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct InspectedConst {
    #[serde(flatten)]
    pub declaration: crate::SchemaConstInfo,
    pub location: Option<CommandLocation>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SchemaInspection {
    pub types: Vec<InspectedType>,
    pub enums: Vec<InspectedEnum>,
    pub consts: Vec<InspectedConst>,
    pub dimensions: Vec<crate::SchemaDimensionInfo>,
    #[serde(default)]
    pub omitted: Vec<OmittedSymbol>,
}

#[derive(Debug, JsonSchema)]
#[serde(untagged)]
pub enum SchemaInspectionResult {
    Project(SchemaInspection),
    Field(Box<InspectedField>),
}
