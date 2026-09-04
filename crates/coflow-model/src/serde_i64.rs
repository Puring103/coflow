//! Serde helpers for `i64` values crossing JSON/IPC boundaries.
//!
//! JavaScript cannot represent the full `i64` range as `number`, while the
//! editor's generated TypeScript bindings expose these fields as `bigint`.
//! Serializing as strings preserves the full range and deserializing accepts
//! both decimal strings and numeric JSON.

use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};
use std::fmt;

/// Serialize an `i64` as a decimal string.
///
/// # Errors
///
/// Returns the serializer's error when writing the string fails.
pub fn serialize<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

/// Deserialize an `i64` from either a decimal string or an integer JSON value.
///
/// # Errors
///
/// Returns an error when the input is not a valid `i64`.
pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(I64Visitor)
}

struct I64Visitor;

impl Visitor<'_> for I64Visitor {
    type Value = i64;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an i64 integer or a decimal string")
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        i64::try_from(value).map_err(|_| E::custom(format!("integer `{value}` exceeds i64 range")))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        value
            .parse::<i64>()
            .map_err(|err| E::custom(format!("invalid i64 string `{value}`: {err}")))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_str(&value)
    }
}

pub mod option {
    //! `Option<i64>` variant of the string-preserving serde helper.

    use super::{deserialize as deserialize_i64, serialize as serialize_i64};
    use serde::de::Visitor;
    use serde::{Deserializer, Serializer};
    use std::fmt;

    /// Serialize `Option<i64>`, encoding `Some` as a decimal string.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error when writing the value fails.
    pub fn serialize<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer.serialize_some(&I64String(*value)),
            None => serializer.serialize_none(),
        }
    }

    /// Deserialize `Option<i64>` from null, a decimal string, or an integer.
    ///
    /// # Errors
    ///
    /// Returns an error when the input is not null and is not a valid `i64`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_option(OptionI64Visitor)
    }

    struct I64String(i64);

    impl serde::Serialize for I64String {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serialize_i64(&self.0, serializer)
        }
    }

    struct OptionI64Visitor;

    impl<'de> Visitor<'de> for OptionI64Visitor {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("null, an i64 integer, or a decimal string")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_i64(deserializer).map(Some)
        }
    }
}
