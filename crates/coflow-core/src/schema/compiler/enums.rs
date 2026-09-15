use super::SymbolTable;
use crate::source::Span;
use coflow_language::cft::ModuleId;
use coflow_language::diagnostics::{CftDiagnostic, CftErrorCode};
use std::collections::{BTreeMap, BTreeSet};

impl SymbolTable<'_> {
    pub(super) fn validate_enums(&mut self) {
        let names = self.enums.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let Some(info) = self.enums.get(&name).cloned() else {
                continue;
            };
            let mut next = if info.is_flag { 1_i64 } else { 0_i64 };
            let mut variant_names: BTreeMap<String, (ModuleId, Span)> = BTreeMap::new();
            let mut values: BTreeMap<i64, (String, ModuleId, Span)> = BTreeMap::new();
            let mut variants = BTreeSet::new();
            let mut values_by_name = BTreeMap::new();
            for (index, variant) in info.def.variants.iter().enumerate() {
                self.validate_identifier(&variant.name, &info.module, variant.name_span);
                if let Some(first) = variant_names.get(&variant.name) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateEnumVariant,
                            info.module.clone(),
                            variant.name_span,
                            format!("duplicate enum variant `{}`", variant.name),
                        )
                        .with_related(
                            first.0.clone(),
                            first.1,
                            "first variant is here",
                        ),
                    );
                } else {
                    variant_names.insert(
                        variant.name.clone(),
                        (info.module.clone(), variant.name_span),
                    );
                }
                if info.is_flag {
                    while values.contains_key(&next) && next <= i64::from(u32::MAX) {
                        next *= 2;
                    }
                }
                let value = variant.value.as_ref().map_or(next, |value| value.value);
                let maximum = if info.is_flag {
                    i64::from(u32::MAX)
                } else {
                    i64::from(i32::MAX)
                };
                if value < 0 || value > maximum {
                    self.push_diag(
                        CftErrorCode::InvalidEnumValueSequence,
                        &info.module,
                        variant.span,
                        "enum value out of 32-bit range",
                    );
                }
                if value == i64::MAX
                    && info
                        .def
                        .variants
                        .iter()
                        .skip(index + 1)
                        .any(|next_variant| next_variant.value.is_none())
                {
                    self.push_diag(
                        CftErrorCode::InvalidEnumValueSequence,
                        &info.module,
                        variant.span,
                        "enum auto numbering overflowed",
                    );
                }
                if !info.is_flag {
                    next = value.saturating_add(1);
                }
                if let Some(first) = values.get(&value) {
                    self.diagnostics.push(
                        CftDiagnostic::error(
                            CftErrorCode::DuplicateEnumValue,
                            info.module.clone(),
                            variant.span,
                            format!("duplicate enum value `{value}`"),
                        )
                        .with_related(
                            first.1.clone(),
                            first.2,
                            "first value is here",
                        ),
                    );
                } else {
                    values.insert(
                        value,
                        (variant.name.clone(), info.module.clone(), variant.span),
                    );
                }
                if info.is_flag && value != 0 && !is_i64_power_of_two(value) {
                    self.push_diag(
                        CftErrorCode::InvalidFlagEnumValue,
                        &info.module,
                        variant.span,
                        "@flag enum values must be powers of two, except zero",
                    );
                }
                variants.insert(variant.name.clone());
                // First definition wins on name collisions; later duplicates
                // already raised `DuplicateEnumVariant` above.
                values_by_name.entry(variant.name.clone()).or_insert(value);
            }
            if let Some(stored) = self.enums.get_mut(&name) {
                stored.variants = variants;
                stored.values = values
                    .into_iter()
                    .map(|(value, (_, module, span))| (value, (module, span)))
                    .collect();
                stored.values_by_name = values_by_name;
            }
        }
    }
}

fn is_i64_power_of_two(value: i64) -> bool {
    value > 0 && (value & (value - 1)) == 0
}
