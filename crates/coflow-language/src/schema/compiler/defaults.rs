use super::SchemaCompiler;

impl SchemaCompiler<'_> {
    pub(super) fn validate_defaults(&mut self) {
        let types = self
            .types
            .values()
            .map(|info| (info.module.clone(), info.def.clone()))
            .collect::<Vec<_>>();
        for (module, definition) in types {
            for field in &definition.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                let expected = self
                    .resolve_field_type(&module, &field.ty)
                    .value_type()
                    .cloned();
                let Some(expected) = expected else {
                    continue;
                };
                if let Some((_, value)) = self.resolve_static_value(
                    &module,
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
    }
}
