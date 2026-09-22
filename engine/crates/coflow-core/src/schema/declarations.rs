use super::CftValueType;
use crate::source::Span;
use crate::{
    BucketName, CheckName, ConstName, DimensionName, EnumName, EnumVariantName, FieldName, TypeName,
};
use coflow_language::cft::ModuleId;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CftSchemaSource {
    pub path: PathBuf,
    pub source: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftConst {
    pub module: ModuleId,
    pub name: ConstName,
    pub value_type: CftValueType,
    pub value: CftStaticValue,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftTopLevelCheck {
    pub module: ModuleId,
    pub name: CheckName,
    pub block: CftSchemaCheckBlock,
    pub span: Span,
}

/// 常量中的可调用值保留创建坐标；复用时不重新创建或绑定 self。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CftCallableSource {
    pub source: String,
    pub constant_origin: Option<String>,
    pub module: ModuleId,
    pub span: Span,
    pub original_source: String,
}

impl CftCallableSource {
    pub fn literal(source: String, original_source: String, module: ModuleId, span: Span) -> Self {
        Self {
            source,
            constant_origin: None,
            original_source,
            module,
            span,
        }
    }
}

impl std::ops::Deref for CftCallableSource {
    type Target = str;
    fn deref(&self) -> &str {
        &self.source
    }
}

/// 已解析的静态值由常量和字段默认值共用；用途与声明位置由外层结构持有。
/// 空集合直接使用对应集合变体，不保留无类型的空值标记。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CftStaticValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    FormattedString(CftCallableSource),
    Function(CftCallableSource),
    Enum {
        enum_name: EnumName,
        variant: EnumVariantName,
        value: i64,
    },
    OptionNone,
    OptionSome(Box<CftStaticValue>),
    Array(Vec<CftStaticValue>),
    Dictionary(Vec<(CftStaticValue, CftStaticValue)>),
    Object {
        type_name: TypeName,
        fields: Vec<(FieldName, CftStaticValue)>,
    },
    RecordReference {
        type_name: TypeName,
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[allow(clippy::struct_excessive_bools)] // CFT modifiers and annotation semantics are orthogonal.
pub struct CftType {
    pub kind: coflow_language::cft::syntax::ast::TypeKind,
    pub module: ModuleId,
    pub name: TypeName,
    pub parent: Option<TypeName>,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub is_host: bool,
    pub id_as_enum: Option<EnumName>,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub(crate) own_fields: Vec<Arc<CftField>>,
    #[serde(skip)]
    pub(crate) all_fields: Vec<Arc<CftField>>,
    #[serde(skip)]
    pub(crate) field_by_name: BTreeMap<FieldName, usize>,
    pub check: Option<CftSchemaCheckBlock>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CftFieldDimension {
    pub dimension: DimensionName,
    pub bucket: Option<BucketName>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftField {
    pub declaring_type: TypeName,
    pub name: FieldName,
    pub value_type: CftValueType,
    pub default: Option<CftStaticValue>,
    pub dimension: Option<CftFieldDimension>,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

impl CftField {
    /// 公开字段类型保持不变；VM 使用不可在源码中引用的句柄标识编译维度访问。
    pub fn runtime_value_type(&self) -> CftValueType {
        self.dimension.as_ref().map_or_else(
            || self.value_type.clone(),
            |binding| {
                CftValueType::RecordRef(TypeName::from_validated(
                    super::dimensions::dimension_value_marker(
                        binding.dimension.as_str(),
                        self.declaring_type.as_str(),
                        self.name.as_str(),
                    ),
                ))
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftSchemaCheckBlock {
    pub source: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftEnum {
    pub module: ModuleId,
    pub name: EnumName,
    pub variants: Vec<CftEnumVariant>,
    #[serde(skip)]
    pub(crate) variant_by_name: BTreeMap<EnumVariantName, usize>,
    #[serde(skip)]
    pub(crate) variant_by_value: BTreeMap<i64, usize>,
    /// `@flag` 枚举的完整值掩码；预计算避免每次位取反遍历变体。
    #[serde(skip)]
    pub flag_mask: u32,
    pub is_flag: bool,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftEnumVariant {
    pub name: EnumVariantName,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
    pub display: Option<CftDisplayMetadata>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftAnnotation {
    pub name: String,
    pub arguments: Vec<CftAnnotationValue>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CftAnnotationValue {
    Name(String),
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// 展示元数据随声明保存，并参与契约标识计算。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CftDisplayMetadata {
    pub label: Option<String>,
    pub description: Option<String>,
}

impl CftDisplayMetadata {
    #[must_use]
    pub fn summary(&self) -> Option<&str> {
        self.description.as_deref().or(self.label.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CftDimension {
    pub name: DimensionName,
    pub fields: Vec<Arc<CftField>>,
}

#[cfg(feature = "cft-compiler")]
impl CftStaticValue {
    /// 字段名与集合下标组成稳定坐标，已有常量来源保持不变。
    pub(crate) fn assign_constant_origins(&mut self, path: &str) {
        match self {
            Self::Function(value) | Self::FormattedString(value) => {
                if value.constant_origin.is_none() {
                    value.constant_origin = Some(path.to_string());
                }
            }
            Self::OptionSome(value) => value.assign_constant_origins(path),
            Self::Array(values) => {
                for (index, value) in values.iter_mut().enumerate() {
                    value.assign_constant_origins(&format!("{path}[{index}]"));
                }
            }
            Self::Dictionary(values) => {
                for (index, (_, value)) in values.iter_mut().enumerate() {
                    value.assign_constant_origins(&format!("{path}[{index}]"));
                }
            }
            Self::Object { fields, .. } => {
                for (field, value) in fields {
                    value.assign_constant_origins(&format!("{path}.{field}"));
                }
            }
            _ => {}
        }
    }
}
