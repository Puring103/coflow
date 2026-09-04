use super::ValueResolver;
use crate::CftErrorCode;
use crate::syntax::ast::DefaultExprKind;

impl ValueResolver<'_, '_> {
    pub(super) fn validate_defaults(&mut self) {
        let previous = self.previous;
        for info in previous.types.values() {
            let module = &info.module;
            let definition = info.def;
            let is_host = super::annotations::has_annotation(&definition.annotations, "Host");
            for field in &definition.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                if !matches!(default.kind, DefaultExprKind::Function { .. })
                    && contains_function_default(default)
                {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        default.span,
                        "function defaults must be used directly on function fields",
                    );
                    continue;
                }
                if is_host && matches!(default.kind, DefaultExprKind::Function { .. }) {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        default.span,
                        "@Host function fields cannot define default implementations",
                    );
                    continue;
                }
                let expected = self
                    .resolve_field_type(module, &field.ty)
                    .value_type()
                    .cloned();
                let Some(expected) = expected else {
                    continue;
                };
                if let Some((_, value)) = self.resolve_static_value(
                    module,
                    default,
                    Some(&expected),
                    &mut Vec::new(),
                ) {
                    self.resolved_defaults.insert(
                        (module.clone(), default.span.start, default.span.end),
                        value,
                    );
                }
            }
        }

        for host in previous
            .types
            .values()
            .filter(|info| super::annotations::has_annotation(&info.def.annotations, "Host"))
        {
            let host_type = &host.name;
            for ancestor in previous
                .ancestry_chain(host_type)
                .into_iter()
                .filter(|info| info.name != *host_type)
            {
                for default in ancestor
                    .def
                    .fields
                    .iter()
                    .filter_map(|field| field.default.as_ref())
                    .filter(|default| matches!(default.kind, DefaultExprKind::Function { .. }))
                {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        &ancestor.module,
                        default.span,
                        format!(
                            "function default inherited by @Host type `{host_type}` is not allowed"
                        ),
                    );
                }
            }
        }
    }
}

fn contains_function_default(expression: &crate::syntax::ast::DefaultExpr) -> bool {
    match &expression.kind {
        DefaultExprKind::Function { .. } => true,
        DefaultExprKind::OptionSome(value)
        | DefaultExprKind::ResultOk(value)
        | DefaultExprKind::ResultErr(value) => contains_function_default(value),
        DefaultExprKind::BitExpr { lhs, rhs, .. } => {
            contains_function_default(lhs) || contains_function_default(rhs)
        }
        DefaultExprKind::Array(values) => values.iter().any(contains_function_default),
        DefaultExprKind::Dictionary(entries) => entries.iter().any(|(key, value)| {
            contains_function_default(key) || contains_function_default(value)
        }),
        DefaultExprKind::Object(fields) | DefaultExprKind::TypedObject { fields, .. } => fields
            .iter()
            .any(|(_, value)| contains_function_default(value)),
        DefaultExprKind::Int(_)
        | DefaultExprKind::Float(_)
        | DefaultExprKind::Bool(_)
        | DefaultExprKind::String(_)
        | DefaultExprKind::FormattedString(_)
        | DefaultExprKind::OptionNone
        | DefaultExprKind::StaticPath(_)
        | DefaultExprKind::RecordReference(_) => false,
    }
}
