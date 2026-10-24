use super::annotations::has_annotation;
use super::SymbolTable;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::source::Span;
use std::collections::{BTreeMap, BTreeSet};

impl SymbolTable<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
    )]
    pub(super) fn validate_enums(&mut self) {
        let names = self.enums.keys().cloned().collect::<Vec<_>>();
        for name in names {
            let Some(info) = self.enums.get(&name).cloned() else {
                continue;
            };
            let is_int64 = has_annotation(&info.def.annotations, "int64");
            let mut next = Some(0_i64);
            let mut variant_names: BTreeMap<String, (ModuleId, Span)> = BTreeMap::new();
            let mut values: BTreeMap<i64, (String, ModuleId, Span)> = BTreeMap::new();
            let mut variants = BTreeSet::new();
            let mut values_by_name = BTreeMap::new();
            for variant in &info.def.variants {
                self.validate_identifier(&variant.name, &info.module, variant.name_span);
                if info.is_flag && matches!(variant.name.as_str(), "None" | "Everything") {
                    self.push_diag(
                        CftErrorCode::DuplicateEnumVariant,
                        &info.module,
                        variant.name_span,
                        "None and Everything are built-in @flag members",
                    );
                }
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
                let Some(value) = variant.value.as_ref().map(|value| value.value).or(next) else {
                    self.push_diag(CftErrorCode::InvalidEnumValueSequence, &info.module, variant.span,
                        "enum auto numbering exceeds declared width; use an explicit value or @int64");
                    continue;
                };
                if !is_int64 && i32::try_from(value).is_err() {
                    self.push_diag(
                        CftErrorCode::InvalidEnumValueSequence,
                        &info.module,
                        variant.span,
                        "enum value exceeds int range; add @int64 to use long",
                    );
                }
                // 显式值重新开始编号；只对真正依赖前值的下一个隐式成员报告溢出。
                next = value
                    .checked_add(1)
                    .filter(|value| is_int64 || i32::try_from(*value).is_ok());
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
            // 内建成员参与同一份符号表，默认值、检查表达式和代码生成保持一致。
            if info.is_flag {
                let everything = values_by_name.values().fold(0, |mask, value| mask | value);
                for (member, value) in [("None", 0), ("Everything", everything)] {
                    variants.insert(member.to_string());
                    values_by_name.insert(member.to_string(), value);
                    values.entry(value).or_insert_with(|| {
                        (member.to_string(), info.module.clone(), info.def.span)
                    });
                }
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
