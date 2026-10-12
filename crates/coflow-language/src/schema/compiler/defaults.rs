use super::ValueResolver;

impl ValueResolver<'_, '_> {
    pub(super) fn validate_defaults(&mut self) {
        let types = self.resolved_types;
        for info in types.types.values() {
            let module = &info.module;
            let definition = info.def;
            for field in &definition.fields {
                let Some(default) = &field.default else {
                    continue;
                };
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
    }
}
