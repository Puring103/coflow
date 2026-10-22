use super::{Failure, Result};
use crate::{CfdDictKey, CfdValue, LoadedDictKeyDraft, LoadedValueDraft};
use coflow_language::cft::{CftSchema, CftValueType};

pub fn expression(schema: &CftSchema, expected: &CftValueType, source: &str) -> Result<CfdValue> {
    let value = crate::cfd_loader::parse_expression(schema, expected, source)?;
    convert(schema, expected, value)
}

pub(super) fn convert(
    schema: &CftSchema,
    expected: &CftValueType,
    value: LoadedValueDraft,
) -> Result<CfdValue> {
    crate::value_drafts::convert(schema, expected, value).map_err(Failure::request)
}

pub(super) fn dict_key(schema: &CftSchema, value: LoadedDictKeyDraft) -> Result<CfdDictKey> {
    crate::value_drafts::dict_key(schema, value).map_err(Failure::request)
}
