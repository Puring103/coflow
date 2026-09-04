use super::inferred_type::InferredType;
use super::state::SymbolKind;
use super::ResolvedTypes;
use crate::diagnostics::{CftDiagnostic, CftErrorCode};
use crate::module::ModuleId;
use crate::syntax::ast::{Annotation, AnnotationArg, FieldDef};
use crate::source::Span;
use std::collections::BTreeMap;

impl ResolvedTypes<'_> {
    pub(super) fn validate_annotations(&mut self) {
        let mut diagnostics = Vec::new();
        for info in self.enums.values() {
            self.validate_annotation_list(
                &info.module,
                AnnotationTarget::Enum,
                &info.def.annotations,
                &mut diagnostics,
            );
            for variant in &info.def.variants {
                self.validate_annotation_list(
                    &info.module,
                    AnnotationTarget::EnumVariant,
                    &variant.annotations,
                    &mut diagnostics,
                );
            }
        }

        let mut id_as_enum_names = BTreeMap::<String, (ModuleId, Span)>::new();
        for info in self.types.values() {
            self.validate_annotation_list(
                &info.module,
                AnnotationTarget::Type,
                &info.def.annotations,
                &mut diagnostics,
            );
            if let Some(annotation) = find_annotation(&info.def.annotations, "struct") {
                if !info.def.is_sealed {
                    push_diag(
                        &mut diagnostics,
                        CftErrorCode::StructRequiresSealedType,
                        &info.module,
                        annotation.span,
                        "@struct requires a sealed type",
                    );
                }
            }
            if let Some(singleton) = find_annotation(&info.def.annotations, "singleton") {
                if info.def.is_abstract {
                    push_diag(
                        &mut diagnostics,
                        CftErrorCode::SingletonOnAbstractType,
                        &info.module,
                        singleton.span,
                        "@singleton cannot be applied to an abstract type",
                    );
                }
                if find_annotation(&info.def.annotations, "idAsEnum").is_some() {
                    push_diag(
                        &mut diagnostics,
                        CftErrorCode::SingletonIdAsEnumConflict,
                        &info.module,
                        singleton.span,
                        "@singleton cannot be combined with @idAsEnum",
                    );
                }
            }
            if let Some(host) = find_annotation(&info.def.annotations, "Host") {
                if info.def.is_abstract || find_annotation(&info.def.annotations, "singleton").is_none() {
                    push_diag(
                        &mut diagnostics,
                        CftErrorCode::InvalidAnnotatedFieldType,
                        &info.module,
                        host.span,
                        "@Host requires a concrete type with an explicit @singleton annotation",
                    );
                }
            }
            if let Some(annotation) = find_annotation(&info.def.annotations, "idAsEnum") {
                if let Some(AnnotationArg::Name(enum_name)) = annotation.args.first() {
                    let resolved_name = self.resolve_name(&info.module, &enum_name.name);
                    self.validate_id_as_enum_name(
                        &info.module,
                        &resolved_name,
                        enum_name.span,
                        &mut diagnostics,
                    );
                    Self::register_id_as_enum_name(
                        &mut id_as_enum_names,
                        &info.module,
                        annotation,
                        &resolved_name,
                        &mut diagnostics,
                    );
                }
            }
        }

        for info in self.types.values() {
            for field in &info.def.fields {
                self.validate_annotation_list(
                    &info.module,
                    AnnotationTarget::Field,
                    &field.annotations,
                    &mut diagnostics,
                );
                self.validate_field_annotations(
                    &info.module,
                    field,
                    info.def.is_sealed,
                    &mut diagnostics,
                );
            }
        }
        self.diagnostics.extend(diagnostics);
    }

    fn register_id_as_enum_name(
        id_as_enum_names: &mut BTreeMap<String, (ModuleId, Span)>,
        module: &ModuleId,
        annotation: &Annotation,
        enum_name: &str,
        diagnostics: &mut Vec<CftDiagnostic>,
    ) {
        if let Some((first_module, first_span)) = id_as_enum_names.get(enum_name) {
            diagnostics.push(
                CftDiagnostic::error(
                    CftErrorCode::DuplicateGlobalName,
                    module.clone(),
                    annotation.span,
                    format!("duplicate @idAsEnum enum name `{enum_name}`"),
                )
                .with_related(
                    first_module.clone(),
                    *first_span,
                    "first @idAsEnum enum name is here",
                ),
            );
        } else {
            id_as_enum_names.insert(enum_name.to_string(), (module.clone(), annotation.span));
        }
    }

    fn validate_annotation_list(
        &self,
        module: &ModuleId,
        target: AnnotationTarget,
        annotations: &[Annotation],
        diagnostics: &mut Vec<CftDiagnostic>,
    ) {
        let mut seen = BTreeMap::<&str, Span>::new();
        for annotation in annotations {
            if let Some(first) = seen.get(annotation.name.as_str()) {
                diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::DuplicateAnnotation,
                        module.clone(),
                        annotation.span,
                        format!("duplicate annotation `{}`", annotation.name),
                    )
                    .with_related(
                        module.clone(),
                        *first,
                        "first annotation is here",
                    ),
                );
            } else {
                seen.insert(&annotation.name, annotation.span);
            }
            let Some(spec) = AnnotationSpec::for_name(&annotation.name) else {
                continue;
            };
            if !spec.targets.contains(&target) {
                let code = if annotation.name == "localized" {
                    CftErrorCode::LocalizedOnInvalidTarget
                } else {
                    CftErrorCode::InvalidAnnotationTarget
                };
                push_diag(
                    diagnostics,
                    code,
                    module,
                    annotation.span,
                    format!("@{} cannot be applied to this target", annotation.name),
                );
            }
            if !spec.args_valid(annotation) {
                push_diag(
                    diagnostics,
                    CftErrorCode::InvalidAnnotationArgument,
                    module,
                    annotation.span,
                    format!("@{} has invalid arguments", annotation.name),
                );
            }
        }
    }

    fn validate_field_annotations(
        &self,
        module: &ModuleId,
        field: &FieldDef,
        owner_is_sealed: bool,
        diagnostics: &mut Vec<CftDiagnostic>,
    ) {
        let localized = find_annotation(&field.annotations, "localized");
        let dimension = find_annotation(&field.annotations, "dimension");
        if owner_is_sealed {
            if let Some(annotation) = localized {
                push_diag(
                    diagnostics,
                    CftErrorCode::LocalizedOnInvalidTarget,
                    module,
                    annotation.span,
                    "@localized can only appear on top-level type fields, not inside sealed types",
                );
                return;
            }
            if let Some(annotation) = dimension {
                push_diag(
                    diagnostics,
                    CftErrorCode::DimensionOnInvalidTarget,
                    module,
                    annotation.span,
                    "@dimension can only appear on top-level type fields, not inside sealed types",
                );
                return;
            }
        }
        if let Some(annotation) = localized {
            if let Some(AnnotationArg::String(bucket, span)) = annotation.args.first() {
                if !crate::is_cft_identifier(bucket) {
                    push_diag(
                        diagnostics,
                        CftErrorCode::LocalizedBucketNotIdentifier,
                        module,
                        *span,
                        format!("@localized bucket `{bucket}` is not a valid CFT identifier"),
                    );
                }
            }
        }
        if let Some(annotation) = dimension {
            if let Some(AnnotationArg::String(dim_name, span)) = annotation.args.first() {
                if !crate::is_cft_identifier(dim_name) {
                    push_diag(
                        diagnostics,
                        CftErrorCode::DimensionNameNotIdentifier,
                        module,
                        *span,
                        format!("@dimension name `{dim_name}` is not a valid CFT identifier"),
                    );
                }
            }
        }
        if let (Some(localized), Some(_dimension)) = (localized, dimension) {
            push_diag(
                diagnostics,
                CftErrorCode::DuplicateAnnotation,
                module,
                localized.span,
                "field can only declare one dimension annotation",
            );
        }
        if let Some(annotation) = find_annotation(&field.annotations, "expand") {
            // @expand requires an inline concrete object field. Arrays, dicts,
            // primitives, enums, option wrappers, refs, abstract objects, and
            // singleton objects don't make sense because the loader needs one
            // known inline set of inner field names to consume from adjacent
            // header columns.
            let resolved = self.resolve_field_type(module, &field.ty);
            if !self.expand_target_is_concrete_inline_object(&resolved) {
                push_diag(
                    diagnostics,
                    CftErrorCode::InvalidAnnotatedFieldType,
                    module,
                    annotation.span,
                    "@expand fields must reference an inline concrete type (no refs, abstract/singleton types, Option, arrays, dicts, enums, or primitives)",
                );
            }
        }
    }

    fn expand_target_is_concrete_inline_object(&self, ty: &InferredType) -> bool {
        if ty.is_unknown() {
            return true;
        }
        ty.object_name().is_some_and(|name| {
            self.types.get(name.as_str()).is_some_and(|info| {
                !info.def.is_abstract && !has_annotation(&info.def.annotations, "singleton")
            })
        })
    }

    fn validate_id_as_enum_name(
        &self,
        module: &ModuleId,
        enum_name: &str,
        enum_name_span: Span,
        diagnostics: &mut Vec<CftDiagnostic>,
    ) {
        match self.symbols.get(enum_name) {
            Some(symbol) if symbol.kind == SymbolKind::Enum => {
                if let Some(info) = self.enums.get(enum_name) {
                    if !info.def.variants.is_empty() {
                        diagnostics.push(
                            CftDiagnostic::error(
                                CftErrorCode::IdAsEnumRequiresEmptyEnum,
                                module.clone(),
                                enum_name_span,
                                format!(
                                    "@idAsEnum enum `{enum_name}` must be declared with no variants"
                                ),
                            )
                            .with_related(
                                info.module.clone(),
                                info.def.name_span,
                                "enum placeholder is defined here",
                            ),
                        );
                    }
                }
            }
            Some(symbol) => {
                diagnostics.push(
                    CftDiagnostic::error(
                        CftErrorCode::IdAsEnumRequiresEmptyEnum,
                        module.clone(),
                        enum_name_span,
                        format!("@idAsEnum argument `{enum_name}` must name an enum"),
                    )
                    .with_related(
                        symbol.module.clone(),
                        symbol.span,
                        "name is defined here",
                    ),
                );
            }
            None => {
                push_diag(
                    diagnostics,
                    CftErrorCode::UnknownNamedType,
                    module,
                    enum_name_span,
                    format!("unknown @idAsEnum enum `{enum_name}`"),
                );
            }
        }
    }
}

fn push_diag(
    diagnostics: &mut Vec<CftDiagnostic>,
    code: CftErrorCode,
    module: &ModuleId,
    span: Span,
    message: impl Into<String>,
) {
    diagnostics.push(CftDiagnostic::error(code, module.clone(), span, message));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnnotationTarget {
    Type,
    Enum,
    EnumVariant,
    Field,
}

#[derive(Debug, Clone)]
pub(super) struct AnnotationSpec {
    pub(super) targets: &'static [AnnotationTarget],
    args: AnnotationArgs,
}

impl AnnotationSpec {
    pub(super) fn for_name(name: &str) -> Option<Self> {
        Some(match name {
            "struct" | "singleton" | "Host" => Self {
                targets: &[AnnotationTarget::Type],
                args: AnnotationArgs::None,
            },
            "idAsEnum" => Self {
                targets: &[AnnotationTarget::Type],
                args: AnnotationArgs::OneName,
            },
            "flag" => Self {
                targets: &[AnnotationTarget::Enum],
                args: AnnotationArgs::None,
            },
            "label" | "description" => Self {
                targets: &[
                    AnnotationTarget::Type,
                    AnnotationTarget::Enum,
                    AnnotationTarget::EnumVariant,
                    AnnotationTarget::Field,
                ],
                args: AnnotationArgs::OneString,
            },
            "expand" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::None,
            },
            "localized" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::NoneOrOneString,
            },
            "dimension" => Self {
                targets: &[AnnotationTarget::Field],
                args: AnnotationArgs::OneString,
            },
            _ => return None,
        })
    }

    pub(super) fn args_valid(&self, annotation: &Annotation) -> bool {
        match self.args {
            AnnotationArgs::None => annotation.args.is_empty(),
            AnnotationArgs::NoneOrOneString => {
                annotation.args.is_empty()
                    || matches!(annotation.args.as_slice(), [AnnotationArg::String(_, _)])
            }
            AnnotationArgs::OneName => {
                matches!(annotation.args.as_slice(), [AnnotationArg::Name(_)])
            }
            AnnotationArgs::OneString => {
                matches!(annotation.args.as_slice(), [AnnotationArg::String(_, _)])
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum AnnotationArgs {
    None,
    NoneOrOneString,
    OneName,
    OneString,
}
pub(super) fn has_annotation(annotations: &[Annotation], name: &str) -> bool {
    find_annotation(annotations, name).is_some()
}

pub(super) fn find_annotation<'a>(
    annotations: &'a [Annotation],
    name: &str,
) -> Option<&'a Annotation> {
    annotations
        .iter()
        .find(|annotation| annotation.name == name)
}

pub(super) fn field_dimension_name(field: &FieldDef) -> Option<crate::DimensionName> {
    if find_annotation(&field.annotations, "localized").is_some() {
        return Some(crate::DimensionName::from_validated("language"));
    }
    let annotation = find_annotation(&field.annotations, "dimension")?;
    let Some(AnnotationArg::String(name, _)) = annotation.args.first() else {
        return None;
    };
    Some(crate::DimensionName::from_validated(name.clone()))
}
