use crate::is_cft_identifier;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CftNameError {
    kind: &'static str,
    value: String,
}

impl CftNameError {
    fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            value: value.to_string(),
        }
    }
}

impl fmt::Display for CftNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} `{}` is not a valid CFT identifier",
            self.kind, self.value
        )
    }
}

impl std::error::Error for CftNameError {}

fn standard_name(value: &str) -> bool {
    is_cft_identifier(value)
}

fn type_name(value: &str) -> bool {
    standard_name(value)
        && !matches!(
            value,
            "int" | "long" | "float" | "double" | "bool" | "string" | "Option"
        )
}

fn variant_name(value: &str) -> bool {
    is_cft_identifier(value)
}

macro_rules! cft_name {
    ($name:ident, $kind:literal, $validator:path) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        // schema 与模型中的已验证名称共享字节；克隆字段布局不重复分配短字符串。
        pub struct $name(Arc<str>);

        impl $name {
            /// Creates a validated semantic name.
            ///
            /// # Errors
            ///
            /// Returns an error when the value is not valid for this name kind.
            pub fn new(value: impl AsRef<str>) -> Result<Self, CftNameError> {
                let value = value.as_ref();
                if $validator(value) {
                    Ok(Self(Arc::from(value)))
                } else {
                    Err(CftNameError::new($kind, &value))
                }
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }

        impl Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                self.as_str()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = CftNameError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<&str> for $name {
            type Error = CftNameError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = CftNameError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }
    };
    ($name:ident, $kind:literal, $validator:path, trusted) => {
        cft_name!($name, $kind, $validator);

        impl $name {
            pub(in crate::schema) fn from_validated(value: impl AsRef<str>) -> Self {
                Self(Arc::from(value.as_ref()))
            }
        }
    };
}

cft_name!(TypeName, "type name", type_name, trusted);
cft_name!(FieldName, "field name", standard_name, trusted);
cft_name!(EnumName, "enum name", type_name, trusted);
fn enum_variant_name(value: &str) -> bool {
    value == "None" || standard_name(value)
}
cft_name!(
    EnumVariantName,
    "enum variant name",
    enum_variant_name,
    trusted
);
cft_name!(ConstName, "const name", standard_name, trusted);
fn check_name(value: &str) -> bool {
    standard_name(value)
        || value
            .strip_prefix('@')
            .and_then(|value| value.rsplit_once(':'))
            .is_some_and(|(module, offset)| !module.is_empty() && offset.parse::<usize>().is_ok())
}

cft_name!(CheckName, "check name", check_name, trusted);

impl CheckName {
    // @ 不属于源码标识符，自动位置身份不会与用户命名的规则冲突。
    pub(in crate::schema) fn anonymous(module: &crate::ModuleId, span: crate::Span) -> Self {
        Self(format!("@{module}:{}", span.start).into())
    }
}
cft_name!(DimensionName, "dimension name", standard_name, trusted);
cft_name!(RecordId, "record ID", standard_name);
cft_name!(VariantName, "dimension variant", variant_name);
