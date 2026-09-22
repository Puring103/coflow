//! 草稿与数据模型共用可调用源码载荷，函数或模板的类别由外层值枚举表达。
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct CallableSource {
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub imports: BTreeMap<String, String>,
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub from_default: bool,
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub location: Option<CallableLocation>,
    #[serde(skip)]
    #[cfg_attr(feature = "ts-export", ts(skip))]
    pub constant_origin: Option<String>,
    pub source: String,
}

impl From<&crate::schema::CftCallableSource> for CallableSource {
    fn from(source: &crate::schema::CftCallableSource) -> Self {
        Self {
            imports: BTreeMap::new(),
            from_default: true,
            location: Some(source.into()),
            constant_origin: source.constant_origin.clone(),
            source: source.source.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallableLocation {
    pub module: Option<crate::schema::ModuleId>,
    pub source: String,
    pub span: crate::source::Span,
    pub path: Option<String>,
}

impl From<&crate::schema::CftCallableSource> for CallableLocation {
    fn from(source: &crate::schema::CftCallableSource) -> Self {
        Self {
            module: Some(source.module.clone()),
            source: source.original_source.clone(),
            span: source.span,
            path: None,
        }
    }
}
