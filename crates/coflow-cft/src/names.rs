use crate::is_cft_identifier;
use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

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

macro_rules! cft_name {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CftNameError> {
                let value = value.into();
                if is_cft_identifier(&value) {
                    Ok(Self(value))
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
    };
}

cft_name!(TypeName, "type name");
cft_name!(FieldName, "field name");
cft_name!(EnumName, "enum name");
cft_name!(EnumVariantName, "enum variant name");
cft_name!(ConstName, "const name");
cft_name!(DimensionName, "dimension name");
cft_name!(BucketName, "dimension bucket");
cft_name!(RecordKey, "record key");

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantName(String);

impl VariantName {
    pub fn new(value: impl Into<String>) -> Result<Self, CftNameError> {
        let value = value.into();
        if value != "default" && is_cft_identifier(&value) {
            Ok(Self(value))
        } else {
            Err(CftNameError::new("dimension variant", &value))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

}

impl AsRef<str> for VariantName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for VariantName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for VariantName {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for VariantName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for VariantName {
    type Err = CftNameError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for VariantName {
    type Error = CftNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for VariantName {
    type Error = CftNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}
