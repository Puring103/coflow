use super::inferred_type::{types_assignable, InferredType};
use super::state::SymbolKind;
use super::SchemaCompiler;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{DefaultExpr, DefaultExprKind};
use std::collections::BTreeSet;

impl SchemaCompiler<'_> {
    pub(super) fn validate_defaults(&mut self) {
        self.each_type(|this, info| {
            let mut field_names = this
                .collect_ancestor_fields(
                    info.def.parent.as_ref().map(|parent| parent.name.as_str()),
                )
                .into_keys()
                .collect::<BTreeSet<_>>();
            field_names.extend(info.def.fields.iter().map(|field| field.name.clone()));
            for field in &info.def.fields {
                let Some(default) = &field.default else {
                    continue;
                };
                let field_ty = this.resolve_field_type(&field.ty);
                let default_ty = this.default_expr_type(&info.module, default, &field_names);
                if !types_assignable(&field_ty, &default_ty) {
                    this.push_diag(
                        CftErrorCode::DefaultTypeMismatch,
                        &info.module,
                        default.span,
                        "default value does not match field type",
                    );
                }
            }
        });
    }

    fn default_expr_type(
        &mut self,
        module: &ModuleId,
        expr: &DefaultExpr,
        field_names: &BTreeSet<String>,
    ) -> InferredType {
        match &expr.kind {
            DefaultExprKind::Int(_) => InferredType::int(),
            DefaultExprKind::Float(_) => InferredType::float(),
            DefaultExprKind::Bool(_) => InferredType::bool(),
            DefaultExprKind::Null => InferredType::Null,
            DefaultExprKind::String(_) => InferredType::string(),
            DefaultExprKind::Name(name) => {
                if field_names.contains(&name.name) {
                    self.push_diag(
                        CftErrorCode::DefaultReferencesField,
                        module,
                        name.span,
                        "default value cannot reference a field",
                    );
                    return InferredType::Unknown;
                }
                if let Some(info) = self.consts.get(&name.name) {
                    return InferredType::from_const(&info.value);
                }
                self.push_diag(
                    CftErrorCode::UnknownConst,
                    module,
                    name.span,
                    format!("unknown const `{}`", name.name),
                );
                InferredType::Unknown
            }
            DefaultExprKind::EnumVariant { enum_name, variant } => {
                self.default_enum_variant_type(module, enum_name, variant)
            }
            DefaultExprKind::Array(items) => {
                if items.is_empty() {
                    InferredType::EmptyArray
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty array defaults are allowed",
                    );
                    InferredType::Unknown
                }
            }
            DefaultExprKind::Object(fields) => {
                if fields.is_empty() {
                    InferredType::EmptyObject
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty object defaults are allowed",
                    );
                    InferredType::Unknown
                }
            }
        }
    }

    fn default_enum_variant_type(
        &mut self,
        module: &ModuleId,
        enum_name: &crate::syntax::ast::NameRef,
        variant: &crate::syntax::ast::NameRef,
    ) -> InferredType {
        match self.symbols.get(&enum_name.name) {
            Some(symbol) if symbol.kind == SymbolKind::Enum => {
                match self.enums.get(&enum_name.name) {
                    Some(enum_info) if enum_info.variants.contains(&variant.name) => {
                        InferredType::enum_value(crate::EnumName::from_validated(
                            enum_name.name.clone(),
                        ))
                    }
                    Some(_) => {
                        self.push_diag(
                            CftErrorCode::UnknownEnumVariant,
                            module,
                            variant.span,
                            format!("unknown enum variant `{}`", variant.name),
                        );
                        InferredType::Unknown
                    }
                    None => InferredType::Unknown,
                }
            }
            Some(symbol) => {
                self.diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::EnumVariantOnNonEnum,
                        module.clone(),
                        enum_name.span,
                        "enum variant default is used on a non-enum name",
                    )
                    .with_related(
                        symbol.module.clone(),
                        symbol.span,
                        "name is defined here",
                    ),
                );
                InferredType::Unknown
            }
            None => {
                self.push_diag(
                    CftErrorCode::EnumVariantOnNonEnum,
                    module,
                    enum_name.span,
                    "enum variant default is used on an unknown enum",
                );
                InferredType::Unknown
            }
        }
    }
}
