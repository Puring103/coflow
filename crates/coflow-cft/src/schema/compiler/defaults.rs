use super::SchemaCompiler;
use crate::ast::{DefaultExpr, DefaultExprKind};
use crate::container::ModuleId;
use crate::error::{CftDiagnostic, CftErrorCode};
use crate::schema::support::{types_assignable, SymbolKind, Ty};
use std::collections::BTreeSet;

impl<'a> SchemaCompiler<'a> {
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
    ) -> Ty {
        match &expr.kind {
            DefaultExprKind::Int(_) => Ty::Int,
            DefaultExprKind::Float(_) => Ty::Float,
            DefaultExprKind::Bool(_) => Ty::Bool,
            DefaultExprKind::Null => Ty::Null,
            DefaultExprKind::String(_) => Ty::String,
            DefaultExprKind::Name(name) => {
                if field_names.contains(&name.name) {
                    self.push_diag(
                        CftErrorCode::DefaultReferencesField,
                        module,
                        name.span,
                        "default value cannot reference a field",
                    );
                    return Ty::Unknown;
                }
                if let Some(info) = self.consts.get(&name.name) {
                    return Ty::from_const(&info.value);
                }
                self.push_diag(
                    CftErrorCode::UnknownConst,
                    module,
                    name.span,
                    format!("unknown const `{}`", name.name),
                );
                Ty::Unknown
            }
            DefaultExprKind::EnumVariant { enum_name, variant } => {
                self.default_enum_variant_type(module, enum_name, variant)
            }
            DefaultExprKind::Array(items) => {
                if items.is_empty() {
                    Ty::EmptyArray
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty array defaults are allowed",
                    );
                    Ty::Unknown
                }
            }
            DefaultExprKind::Object(fields) => {
                if fields.is_empty() {
                    Ty::EmptyObject
                } else {
                    self.push_diag(
                        CftErrorCode::InvalidDefaultExpression,
                        module,
                        expr.span,
                        "only empty object defaults are allowed",
                    );
                    Ty::Unknown
                }
            }
        }
    }

    fn default_enum_variant_type(
        &mut self,
        module: &ModuleId,
        enum_name: &crate::ast::NameRef,
        variant: &crate::ast::NameRef,
    ) -> Ty {
        match self.symbols.get(&enum_name.name) {
            Some(symbol) if symbol.kind == SymbolKind::Enum => {
                match self.enums.get(&enum_name.name) {
                    Some(enum_info) if enum_info.variants.contains(&variant.name) => {
                        Ty::Enum(enum_name.name.clone())
                    }
                    Some(_) => {
                        self.push_diag(
                            CftErrorCode::UnknownEnumVariant,
                            module,
                            variant.span,
                            format!("unknown enum variant `{}`", variant.name),
                        );
                        Ty::Unknown
                    }
                    None => Ty::Unknown,
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
                Ty::Unknown
            }
            None => {
                self.push_diag(
                    CftErrorCode::EnumVariantOnNonEnum,
                    module,
                    enum_name.span,
                    "enum variant default is used on an unknown enum",
                );
                Ty::Unknown
            }
        }
    }
}
