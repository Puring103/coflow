use super::inferred_type::InferredType;
use crate::module::ModuleId;
use crate::schema::CftConstValue;
use crate::syntax::ast::{ConstDef, EnumDef, TopLevelCheckDef, TypeDef};
use crate::syntax::Span;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(super) struct ConstInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a ConstDef,
    pub(super) value: CftConstValue,
}

#[derive(Debug, Clone)]
pub(super) struct TypeInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a TypeDef,
}

#[derive(Debug, Clone)]
pub(super) struct CheckInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a TopLevelCheckDef,
}

#[derive(Debug, Clone)]
pub(super) struct EnumInfo<'a> {
    pub(super) module: ModuleId,
    pub(super) def: &'a EnumDef,
    pub(super) variants: BTreeSet<String>,
    pub(super) values: BTreeMap<i64, (ModuleId, Span)>,
    pub(super) values_by_name: BTreeMap<String, i64>,
    pub(super) is_flag: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FieldInfo {
    pub(super) inferred_type: InferredType,
    pub(super) dimension: Option<crate::DimensionName>,
}

#[derive(Debug, Clone)]
pub(super) struct FieldOrigin {
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

#[derive(Debug, Clone)]
pub(super) struct Symbol {
    pub(super) kind: SymbolKind,
    pub(super) module: ModuleId,
    pub(super) span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SymbolKind {
    Const,
    Type,
    Enum,
}
