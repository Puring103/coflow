use super::state::SymbolKind;
use super::ValueResolver;
use crate::schema::{CftConstValue, CftValueType};
use crate::syntax::ast::{DefaultExpr, DefaultExprKind, QualifiedName};
use crate::{CftErrorCode, EnumName, EnumVariantName, FieldName, ModuleId, TypeName};
use std::collections::BTreeSet;

impl ValueResolver<'_, '_> {
    pub(super) fn resolve_constants(&mut self) {
        let previous = self.previous;
        let mut visiting = Vec::new();
        for name in previous.consts.keys() {
            let _ = self.resolve_constant(name, &mut visiting);
        }
    }

    fn resolve_constant(
        &mut self,
        name: &str,
        visiting: &mut Vec<String>,
    ) -> Option<(CftValueType, CftConstValue)> {
        if let Some(resolved) = self.resolved_constants.get(name) {
            return Some(resolved.clone());
        }
        let info = self.consts.get(name)?;
        if let Some(start) = visiting.iter().position(|entry| entry == name) {
            let mut cycle = visiting[start..].to_vec();
            cycle.push(name.to_string());
            let module = info.module.clone();
            let span = info.def.value.span;
            self.push_diag(
                CftErrorCode::InvalidConstValue,
                &module,
                span,
                format!("constant dependency cycle: {}", cycle.join(" -> ")),
            );
            return None;
        }

        let module = info.module.clone();
        let expression = info.def.value.clone();
        let declared_type = info.def.ty.clone();
        if contains_runtime_default(&expression) {
            self.push_diag(
                CftErrorCode::InvalidConstValue,
                &module,
                expression.span,
                "constants cannot contain formatted strings or functions",
            );
            return None;
        }
        visiting.push(name.to_string());
        let expected = declared_type.as_ref().and_then(|ty| {
            self.resolve_field_type(&module, ty).value_type().cloned()
        });
        if matches!(
            expected,
            Some(CftValueType::Function(_, _) | CftValueType::Unit)
        ) {
            self.push_diag(
                CftErrorCode::InvalidConstValue,
                &module,
                declared_type.as_ref().map_or(expression.span, |ty| ty.span),
                "constants cannot have function or unit type",
            );
            visiting.pop();
            return None;
        }
        let resolved = self.resolve_static_value(&module, &expression, expected.as_ref(), visiting);
        visiting.pop();
        if let Some(resolved) = &resolved {
            self.resolved_constants
                .insert(name.to_string(), resolved.clone());
        }
        resolved
    }

    pub(super) fn resolve_static_value(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        expected: Option<&CftValueType>,
        visiting: &mut Vec<String>,
    ) -> Option<(CftValueType, CftConstValue)> {
        let resolved = match &expression.kind {
            DefaultExprKind::Int(value) => {
                if let Some(CftValueType::Enum(enum_name)) = expected {
                    if self.enums.get(enum_name.as_str()).is_some_and(|info| info.is_flag) {
                        self.resolve_flag_mask(module, expression, enum_name, *value)?
                    } else {
                        (CftValueType::Int, CftConstValue::Int(*value))
                    }
                } else {
                    (CftValueType::Int, CftConstValue::Int(*value))
                }
            }
            DefaultExprKind::Float(value) => {
                (CftValueType::Float, CftConstValue::Float(*value))
            }
            DefaultExprKind::Bool(value) => (CftValueType::Bool, CftConstValue::Bool(*value)),
            DefaultExprKind::String(value) => {
                (CftValueType::String, CftConstValue::String(value.clone()))
            }
            DefaultExprKind::FormattedString(source) => (
                CftValueType::String,
                CftConstValue::FormattedString(source.clone()),
            ),
            DefaultExprKind::Function { signature, source } => {
                let Some(value_type) = self
                    .resolve_field_type(module, signature)
                    .value_type()
                    .cloned()
                else {
                    return None;
                };
                (
                    value_type,
                    CftConstValue::Function(source.clone()),
                )
            }
            DefaultExprKind::BitExpr { op, lhs, rhs } => {
                let Some(CftValueType::Enum(enum_name)) = expected else {
                    return self.cannot_infer_const(module, expression, "flag expression");
                };
                if !self.enums.get(enum_name.as_str()).is_some_and(|info| info.is_flag) {
                    let expected_flag = CftValueType::Enum(enum_name.clone());
                    return self.const_type_mismatch(
                        module,
                        expression,
                        &expected_flag,
                        &CftValueType::Int,
                    );
                }
                let (_, lhs) = self.resolve_static_value(module, lhs, expected, visiting)?;
                let (_, rhs) = self.resolve_static_value(module, rhs, expected, visiting)?;
                let (CftConstValue::Enum { value: lhs, .. }, CftConstValue::Enum { value: rhs, .. }) =
                    (lhs, rhs)
                else {
                    let expected_flag = CftValueType::Enum(enum_name.clone());
                    return self.const_type_mismatch(
                        module,
                        expression,
                        &expected_flag,
                        &CftValueType::Int,
                    );
                };
                let value = match op {
                    crate::syntax::ast::DefaultBitOp::Or => lhs | rhs,
                    crate::syntax::ast::DefaultBitOp::Xor => lhs ^ rhs,
                    crate::syntax::ast::DefaultBitOp::And => lhs & rhs,
                };
                self.resolve_flag_mask(module, expression, enum_name, value)?
            }
            DefaultExprKind::OptionNone => {
                let Some(CftValueType::Option(inner)) = expected else {
                    return self.cannot_infer_const(module, expression, "None");
                };
                (
                    CftValueType::Option(inner.clone()),
                    CftConstValue::OptionNone,
                )
            }
            DefaultExprKind::OptionSome(value) => {
                let inner_expected = match expected {
                    Some(CftValueType::Option(inner)) => Some(inner.as_ref()),
                    Some(_) => None,
                    None => None,
                };
                let (inner_type, inner_value) =
                    self.resolve_static_value(module, value, inner_expected, visiting)?;
                (
                    CftValueType::Option(Box::new(inner_type)),
                    CftConstValue::OptionSome(Box::new(inner_value)),
                )
            }
            DefaultExprKind::ResultOk(value) => {
                let Some(CftValueType::Result(ok_type, error_type)) = expected else {
                    return self.cannot_infer_const(module, expression, "Ok");
                };
                let (_, value) =
                    self.resolve_static_value(module, value, Some(ok_type), visiting)?;
                (
                    CftValueType::Result(ok_type.clone(), error_type.clone()),
                    CftConstValue::ResultOk(Box::new(value)),
                )
            }
            DefaultExprKind::ResultErr(error) => {
                let Some(CftValueType::Result(ok_type, error_type)) = expected else {
                    return self.cannot_infer_const(module, expression, "Err");
                };
                let (_, error) =
                    self.resolve_static_value(module, error, Some(error_type), visiting)?;
                (
                    CftValueType::Result(ok_type.clone(), error_type.clone()),
                    CftConstValue::ResultErr(Box::new(error)),
                )
            }
            DefaultExprKind::StaticPath(path) => {
                self.resolve_static_path_value(module, path, expected, visiting)?
            }
            DefaultExprKind::RecordReference(path) => {
                self.resolve_record_reference(module, path)?
            }
            DefaultExprKind::Array(items) => {
                let expected_item = match expected {
                    Some(CftValueType::Array(item)) => Some(item.as_ref()),
                    _ => None,
                };
                if items.is_empty() && expected_item.is_none() {
                    return self.cannot_infer_const(module, expression, "empty array");
                }
                let mut values = Vec::with_capacity(items.len());
                let mut item_type = expected_item.cloned();
                for item in items {
                    let (resolved_type, value) = self.resolve_static_value(
                        module,
                        item,
                        item_type.as_ref(),
                        visiting,
                    )?;
                    if let Some(expected_item) = &item_type {
                        if !self.default_type_assignable(&resolved_type, expected_item) {
                            return self.const_type_mismatch(module, item, expected_item, &resolved_type);
                        }
                    } else {
                        item_type = Some(resolved_type);
                    }
                    values.push(value);
                }
                (
                    CftValueType::Array(Box::new(item_type.expect("empty array handled above"))),
                    CftConstValue::Array(values),
                )
            }
            DefaultExprKind::Dictionary(entries) => {
                self.resolve_dictionary(module, expression, entries, expected, visiting)?
            }
            DefaultExprKind::Object(fields) => match expected {
                Some(CftValueType::Dict(key, value)) if fields.is_empty() => (
                    CftValueType::Dict(key.clone(), value.clone()),
                    CftConstValue::Dictionary(Vec::new()),
                ),
                Some(CftValueType::Object(type_name)) => {
                    self.resolve_object(module, expression, type_name, fields, visiting)?
                }
                _ => return self.cannot_infer_const(module, expression, "object"),
            },
            DefaultExprKind::TypedObject { type_name, fields } => {
                let resolved_name = self.resolve_name(module, &type_name.canonical());
                if !matches!(
                    self.symbols.get(&resolved_name),
                    Some(symbol) if symbol.kind == SymbolKind::Type
                ) {
                    self.push_diag(
                        CftErrorCode::UnknownNamedType,
                        module,
                        type_name.span,
                        format!("unknown object type `{resolved_name}`"),
                    );
                    return None;
                }
                self.resolve_object(
                    module,
                    expression,
                    &TypeName::from_validated(resolved_name),
                    fields,
                    visiting,
                )?
            }
        };

        if let Some(expected) = expected {
            if !self.default_type_assignable(&resolved.0, expected) {
                return self.const_type_mismatch(module, expression, expected, &resolved.0);
            }
        }
        Some(resolved)
    }

    fn default_type_assignable(&self, actual: &CftValueType, expected: &CftValueType) -> bool {
        if actual == expected {
            return true;
        }
        match (actual, expected) {
            (CftValueType::Object(actual), CftValueType::Object(expected))
            | (CftValueType::RecordRef(actual), CftValueType::RecordRef(expected)) => self
                .ancestry_chain(actual)
                .iter()
                .any(|candidate| candidate.name == expected.as_str()),
            _ => false,
        }
    }

    fn resolve_flag_mask(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        enum_name: &EnumName,
        value: i64,
    ) -> Option<(CftValueType, CftConstValue)> {
        let info = self.enums.get(enum_name.as_str())?;
        let declared_mask = info.values_by_name.values().fold(0_i64, |mask, value| mask | value);
        if value < 0 || value & !declared_mask != 0 {
            self.push_diag(
                CftErrorCode::InvalidConstValue,
                module,
                expression.span,
                format!("flag enum `{enum_name}` default contains undeclared bits"),
            );
            return None;
        }
        let variant = info
            .values_by_name
            .iter()
            .find_map(|(name, candidate)| (*candidate == value).then(|| name.clone()))
            .unwrap_or_else(|| format!("mask_{value}"));
        Some((
            CftValueType::Enum(enum_name.clone()),
            CftConstValue::Enum {
                enum_name: enum_name.clone(),
                variant: EnumVariantName::from_validated(variant),
                value,
            },
        ))
    }

    fn resolve_static_path_value(
        &mut self,
        module: &ModuleId,
        path: &QualifiedName,
        expected: Option<&CftValueType>,
        visiting: &mut Vec<String>,
    ) -> Option<(CftValueType, CftConstValue)> {
        let raw_name = path.canonical();
        let resolved_name = self.resolve_name(module, &raw_name);
        if self.consts.contains_key(&resolved_name) {
            return self.resolve_constant(&resolved_name, visiting);
        }

        if path.segments.len() == 1 {
            if let Some(CftValueType::Enum(enum_name)) = expected {
                return self.resolve_enum_variant(module, enum_name.as_str(), &path.segments[0].name, path.span);
            }
        } else if let Some((variant, owner)) = path.segments.split_last() {
            let owner = owner
                .iter()
                .map(|segment| segment.name.as_str())
                .collect::<Vec<_>>()
                .join("::");
            let enum_name = self.resolve_name(module, &owner);
            if self.enums.contains_key(&enum_name) {
                return self.resolve_enum_variant(module, &enum_name, &variant.name, path.span);
            }
        }

        self.push_diag(
            CftErrorCode::UnknownConst,
            module,
            path.span,
            format!("unknown const or enum variant `{resolved_name}`"),
        );
        None
    }

    fn resolve_enum_variant(
        &mut self,
        module: &ModuleId,
        enum_name: &str,
        variant_name: &str,
        span: crate::source::Span,
    ) -> Option<(CftValueType, CftConstValue)> {
        let Some(info) = self.enums.get(enum_name) else {
            self.push_diag(
                CftErrorCode::EnumVariantOnNonEnum,
                module,
                span,
                format!("unknown enum `{enum_name}`"),
            );
            return None;
        };
        let Some(value) = info.values_by_name.get(variant_name).copied() else {
            self.push_diag(
                CftErrorCode::UnknownEnumVariant,
                module,
                span,
                format!("unknown enum variant `{enum_name}::{variant_name}`"),
            );
            return None;
        };
        let enum_name = EnumName::from_validated(enum_name.to_string());
        Some((
            CftValueType::Enum(enum_name.clone()),
            CftConstValue::Enum {
                enum_name,
                variant: EnumVariantName::from_validated(variant_name.to_string()),
                value,
            },
        ))
    }

    fn resolve_record_reference(
        &mut self,
        module: &ModuleId,
        path: &QualifiedName,
    ) -> Option<(CftValueType, CftConstValue)> {
        let (key, owner) = path.segments.split_last()?;
        let owner = owner
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>()
            .join("::");
        let type_name = self.resolve_name(module, &owner);
        if !matches!(self.symbols.get(&type_name), Some(symbol) if symbol.kind == SymbolKind::Type) {
            self.push_diag(
                CftErrorCode::UnknownNamedType,
                module,
                path.span,
                format!("unknown record type `{type_name}`"),
            );
            return None;
        }
        let type_name = TypeName::from_validated(type_name);
        Some((
            CftValueType::RecordRef(type_name.clone()),
            CftConstValue::RecordReference {
                type_name,
                key: key.name.clone(),
            },
        ))
    }

    fn resolve_dictionary(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        entries: &[(DefaultExpr, DefaultExpr)],
        expected: Option<&CftValueType>,
        visiting: &mut Vec<String>,
    ) -> Option<(CftValueType, CftConstValue)> {
        let (mut key_type, mut value_type) = match expected {
            Some(CftValueType::Dict(key, value)) => (Some((**key).clone()), Some((**value).clone())),
            Some(_) => (None, None),
            None => (None, None),
        };
        if entries.is_empty() && key_type.is_none() {
            return self.cannot_infer_const(module, expression, "empty dictionary");
        }
        let mut values = Vec::with_capacity(entries.len());
        let mut unique = BTreeSet::new();
        for (key, value) in entries {
            let (resolved_key_type, resolved_key) =
                self.resolve_static_value(module, key, key_type.as_ref(), visiting)?;
            let (resolved_value_type, resolved_value) =
                self.resolve_static_value(module, value, value_type.as_ref(), visiting)?;
            if !matches!(resolved_key_type, CftValueType::Int | CftValueType::String | CftValueType::Enum(_)) {
                self.push_diag(
                    CftErrorCode::InvalidDictKeyType,
                    module,
                    key.span,
                    "dictionary constant key must be int, string, or enum",
                );
                return None;
            }
            if let Some(expected_key) = &key_type {
                if expected_key != &resolved_key_type {
                    return self.const_type_mismatch(module, key, expected_key, &resolved_key_type);
                }
            } else {
                key_type = Some(resolved_key_type);
            }
            if let Some(expected_value) = &value_type {
                if !self.default_type_assignable(&resolved_value_type, expected_value) {
                    return self.const_type_mismatch(module, value, expected_value, &resolved_value_type);
                }
            } else {
                value_type = Some(resolved_value_type);
            }
            let identity = format!("{resolved_key:?}");
            if !unique.insert(identity) {
                self.push_diag(
                    CftErrorCode::InvalidConstValue,
                    module,
                    key.span,
                    "duplicate dictionary constant key",
                );
                return None;
            }
            values.push((resolved_key, resolved_value));
        }
        Some((
            CftValueType::Dict(
                Box::new(key_type.expect("empty dictionary handled above")),
                Box::new(value_type.expect("empty dictionary handled above")),
            ),
            CftConstValue::Dictionary(values),
        ))
    }

    fn resolve_object(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        type_name: &TypeName,
        fields: &[(crate::syntax::ast::NameRef, DefaultExpr)],
        visiting: &mut Vec<String>,
    ) -> Option<(CftValueType, CftConstValue)> {
        let Some(field_types) = self.full_fields.get(type_name.as_str()).cloned() else {
            self.push_diag(
                CftErrorCode::UnknownNamedType,
                module,
                expression.span,
                format!("unknown object type `{type_name}`"),
            );
            return None;
        };
        let mut seen = BTreeSet::new();
        let mut values = Vec::with_capacity(field_types.len());
        for (name, value) in fields {
            if !seen.insert(name.name.clone()) {
                self.push_diag(
                    CftErrorCode::InvalidConstValue,
                    module,
                    name.span,
                    format!("duplicate object field `{}`", name.name),
                );
                return None;
            }
            let Some(field) = field_types.get(&name.name) else {
                self.push_diag(
                    CftErrorCode::UnknownField,
                    module,
                    name.span,
                    format!("unknown field `{type_name}.{}`", name.name),
                );
                return None;
            };
            let expected = field.inferred_type.value_type()?.clone();
            let (_, value) =
                self.resolve_static_value(module, value, Some(&expected), visiting)?;
            values.push((FieldName::from_validated(name.name.clone()), value));
        }
        let resolving_constant = visiting.iter().any(|name| self.consts.contains_key(name));
        for (name, field) in &field_types {
            if seen.contains(name) {
                continue;
            }
            if !resolving_constant {
                continue;
            }
            let expected = field.inferred_type.value_type()?.clone();
            let declaring_type = self.types.get(field.declaring_type.as_str())?.clone();
            let default = declaring_type
                .def
                .fields
                .iter()
                .find(|candidate| candidate.name == *name)
                .and_then(|candidate| candidate.default.clone());
            let Some(default) = default else {
                self.push_diag(
                    CftErrorCode::InvalidConstValue,
                    module,
                    expression.span,
                    format!("constant object `{type_name}` is missing field `{name}`"),
                );
                return None;
            };
            let identity = format!("<default:{}.{name}>", field.declaring_type);
            if visiting.contains(&identity) {
                self.push_diag(
                    CftErrorCode::InvalidDefaultExpression,
                    &declaring_type.module,
                    default.span,
                    format!("default dependency cycle at `{}.{name}`", field.declaring_type),
                );
                return None;
            }
            visiting.push(identity);
            let resolved = self.resolve_static_value(
                &declaring_type.module,
                &default,
                Some(&expected),
                visiting,
            );
            visiting.pop();
            let (_, value) = resolved?;
            values.push((FieldName::from_validated(name.clone()), value));
        }
        Some((
            CftValueType::Object(type_name.clone()),
            CftConstValue::Object {
                type_name: type_name.clone(),
                fields: values,
            },
        ))
    }

    fn cannot_infer_const<T>(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        kind: &str,
    ) -> Option<T> {
        self.push_diag(
            CftErrorCode::InvalidConstValue,
            module,
            expression.span,
            format!("cannot infer the type of {kind} constant; add an explicit type"),
        );
        None
    }

    fn const_type_mismatch<T>(
        &mut self,
        module: &ModuleId,
        expression: &DefaultExpr,
        expected: &CftValueType,
        actual: &CftValueType,
    ) -> Option<T> {
        self.push_diag(
            CftErrorCode::InvalidConstValue,
            module,
            expression.span,
            format!("constant value has type `{actual}`, expected `{expected}`"),
        );
        None
    }
}

fn contains_runtime_default(expression: &DefaultExpr) -> bool {
    match &expression.kind {
        DefaultExprKind::FormattedString(_) | DefaultExprKind::Function { .. } => true,
        DefaultExprKind::OptionSome(value)
        | DefaultExprKind::ResultOk(value)
        | DefaultExprKind::ResultErr(value) => contains_runtime_default(value),
        DefaultExprKind::Array(values) => values.iter().any(contains_runtime_default),
        DefaultExprKind::BitExpr { lhs, rhs, .. } => {
            contains_runtime_default(lhs) || contains_runtime_default(rhs)
        }
        DefaultExprKind::Object(fields) | DefaultExprKind::TypedObject { fields, .. } => fields
            .iter()
            .any(|(_, value)| contains_runtime_default(value)),
        DefaultExprKind::Dictionary(entries) => entries.iter().any(|(key, value)| {
            contains_runtime_default(key) || contains_runtime_default(value)
        }),
        DefaultExprKind::Int(_)
        | DefaultExprKind::Float(_)
        | DefaultExprKind::Bool(_)
        | DefaultExprKind::OptionNone
        | DefaultExprKind::String(_)
        | DefaultExprKind::StaticPath(_)
        | DefaultExprKind::RecordReference(_) => false,
    }
}
