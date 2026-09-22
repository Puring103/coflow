use super::ValueResolver;
use crate::CftErrorCode;
use coflow_language::cft::syntax::ast::DefaultExprKind;

impl ValueResolver<'_, '_> {
    pub(super) fn validate_defaults(&mut self) {
        let types = self.resolved_types;
        for info in types.types.values() {
            let module = &info.module;
            let definition = info.def;
            let is_host = super::annotations::has_annotation(&definition.annotations, "Host");
            for field in &definition.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                if is_host && matches!(default.kind, DefaultExprKind::Function { .. }) {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        default.span,
                        "@Host function fields cannot define default implementations",
                    );
                    continue;
                }
                let expected = self.resolve_field_type(&field.ty).value_type().cloned();
                let Some(expected) = expected else {
                    continue;
                };
                if let Some((_, value)) =
                    self.resolve_static_value(module, default, Some(&expected), &mut Vec::new())
                {
                    self.resolved_defaults.insert(
                        (module.clone(), default.span.start, default.span.end),
                        value,
                    );
                }
            }
        }

        for host in types
            .types
            .values()
            .filter(|info| super::annotations::has_annotation(&info.def.annotations, "Host"))
        {
            let host_type = &host.name;
            for ancestor in types
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
