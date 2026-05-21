use std::collections::HashMap;

use crate::ast::{ClassDecl, EnumDecl, Item, Module, TypeExpr};
use crate::hir::{BuiltinId, ClassId, EnumId, FunctionId, GlobalId, VariantId};
use crate::span::Span;

use super::{Diagnostic, SemaErrorKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleSymbols {
    pub globals: Vec<(String, GlobalEntry)>,
    pub classes: Vec<ClassInfo>,
    pub enums: Vec<EnumInfo>,
    pub imports: Vec<ImportInfo>,
    pub builtins: BuiltinSymbols,
}

impl ModuleSymbols {
    pub fn get_global(&self, name: &str) -> Option<&GlobalEntry> {
        self.globals
            .iter()
            .find_map(|(entry_name, entry)| (entry_name == name).then_some(entry))
    }

    pub fn global_name(&self, id: GlobalId) -> Option<&str> {
        self.globals.get(id.0).map(|(name, _)| name.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GlobalEntry {
    Config {
        id: GlobalId,
        ty: Option<TypeExpr>,
        ast_index: usize,
    },
    Var {
        id: GlobalId,
        ty: Option<TypeExpr>,
        ast_index: usize,
    },
    Function {
        id: GlobalId,
        fn_id: FunctionId,
        is_iter: bool,
        ast_index: usize,
    },
    Class {
        id: GlobalId,
        class_id: ClassId,
        ast_index: usize,
    },
    Enum {
        id: GlobalId,
        enum_id: EnumId,
        ast_index: usize,
    },
    Import {
        id: GlobalId,
        import_id: usize,
        ast_index: usize,
    },
    Builtin {
        id: GlobalId,
        builtin_id: BuiltinId,
    },
}

impl GlobalEntry {
    pub fn id(&self) -> GlobalId {
        match self {
            GlobalEntry::Config { id, .. }
            | GlobalEntry::Var { id, .. }
            | GlobalEntry::Function { id, .. }
            | GlobalEntry::Class { id, .. }
            | GlobalEntry::Enum { id, .. }
            | GlobalEntry::Import { id, .. }
            | GlobalEntry::Builtin { id, .. } => *id,
        }
    }

    pub fn readonly(&self) -> bool {
        !matches!(self, GlobalEntry::Var { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassInfo {
    pub name: String,
    pub local: bool,
    pub fields: Vec<ClassFieldInfo>,
    pub has_check: bool,
    pub span: Span,
    pub ast_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassFieldInfo {
    pub name: String,
    pub ty: TypeExpr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumInfo {
    pub name: String,
    pub local: bool,
    pub variants: Vec<EnumVariantInfo>,
    pub span: Span,
    pub ast_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariantInfo {
    pub name: String,
    pub value: i64,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportInfo {
    pub name: String,
    pub span: Span,
    pub ast_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSymbols {
    pub entries: Vec<BuiltinInfo>,
}

impl BuiltinSymbols {
    pub fn id_of(&self, name: &str) -> Option<BuiltinId> {
        self.entries
            .iter()
            .find_map(|entry| (entry.name == name).then_some(entry.id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinInfo {
    pub id: BuiltinId,
    pub name: String,
}

pub fn collect_module_symbols(module: &Module) -> (ModuleSymbols, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut globals = Vec::new();
    let mut classes = Vec::new();
    let mut enums = Vec::new();
    let mut imports = Vec::new();
    let mut names = HashMap::<String, Span>::new();

    for (idx, item) in module.items.iter().enumerate() {
        match item {
            Item::Import(import) => {
                let name = import
                    .alias
                    .as_ref()
                    .or_else(|| import.module.segments.last())
                    .map(|ident| ident.text.clone())
                    .unwrap_or_default();
                let id = GlobalId(globals.len());
                if insert_name(&mut names, &name, import.span, &mut diagnostics) {
                    let import_id = imports.len();
                    imports.push(ImportInfo {
                        name: name.clone(),
                        span: import.span,
                        ast_index: idx,
                    });
                    globals.push((
                        name,
                        GlobalEntry::Import {
                            id,
                            import_id,
                            ast_index: idx,
                        },
                    ));
                }
            }
            Item::Class(class) => {
                let id = GlobalId(globals.len());
                let class_id = ClassId(classes.len());
                if insert_name(
                    &mut names,
                    &class.name.text,
                    class.name.span,
                    &mut diagnostics,
                ) {
                    classes.push(collect_class_info(class, idx, &mut diagnostics));
                    globals.push((
                        class.name.text.clone(),
                        GlobalEntry::Class {
                            id,
                            class_id,
                            ast_index: idx,
                        },
                    ));
                }
            }
            Item::Enum(enum_decl) => {
                let id = GlobalId(globals.len());
                let enum_id = EnumId(enums.len());
                if insert_name(
                    &mut names,
                    &enum_decl.name.text,
                    enum_decl.name.span,
                    &mut diagnostics,
                ) {
                    enums.push(collect_enum_info(enum_decl, idx, &mut diagnostics));
                    globals.push((
                        enum_decl.name.text.clone(),
                        GlobalEntry::Enum {
                            id,
                            enum_id,
                            ast_index: idx,
                        },
                    ));
                }
            }
            Item::Function(func) => {
                let id = GlobalId(globals.len());
                let fn_id = FunctionId(usize::MAX);
                if insert_name(
                    &mut names,
                    &func.name.text,
                    func.name.span,
                    &mut diagnostics,
                ) {
                    globals.push((
                        func.name.text.clone(),
                        GlobalEntry::Function {
                            id,
                            fn_id,
                            is_iter: func.iter,
                            ast_index: idx,
                        },
                    ));
                }
            }
            Item::Var(var) => {
                let id = GlobalId(globals.len());
                if insert_name(&mut names, &var.name.text, var.name.span, &mut diagnostics) {
                    globals.push((
                        var.name.text.clone(),
                        GlobalEntry::Var {
                            id,
                            ty: var.ty.clone(),
                            ast_index: idx,
                        },
                    ));
                }
            }
            Item::Config(config) => {
                let id = GlobalId(globals.len());
                if insert_name(
                    &mut names,
                    &config.name.text,
                    config.name.span,
                    &mut diagnostics,
                ) {
                    globals.push((
                        config.name.text.clone(),
                        GlobalEntry::Config {
                            id,
                            ty: config.ty.clone(),
                            ast_index: idx,
                        },
                    ));
                }
            }
        }
    }

    let builtins = BuiltinSymbols {
        entries: ["error", "iter", "range", "print"]
            .into_iter()
            .enumerate()
            .map(|(idx, name)| BuiltinInfo {
                id: BuiltinId(idx),
                name: name.to_string(),
            })
            .collect(),
    };

    for builtin in &builtins.entries {
        if let Some(span) = names.get(&builtin.name) {
            diagnostics.push(Diagnostic::Sema(SemaErrorKind::DuplicateTopLevel, *span));
            continue;
        }
        let id = GlobalId(globals.len());
        globals.push((
            builtin.name.clone(),
            GlobalEntry::Builtin {
                id,
                builtin_id: builtin.id,
            },
        ));
    }

    (
        ModuleSymbols {
            globals,
            classes,
            enums,
            imports,
            builtins,
        },
        diagnostics,
    )
}

fn insert_name(
    names: &mut HashMap<String, Span>,
    name: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if names.insert(name.to_string(), span).is_some() {
        diagnostics.push(Diagnostic::Sema(SemaErrorKind::DuplicateTopLevel, span));
        false
    } else {
        true
    }
}

fn collect_class_info(
    class: &ClassDecl,
    ast_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> ClassInfo {
    let mut seen = HashMap::<String, Span>::new();
    let mut fields = Vec::new();
    for field in &class.fields {
        if seen
            .insert(field.name.text.clone(), field.name.span)
            .is_some()
        {
            diagnostics.push(Diagnostic::Sema(
                SemaErrorKind::DuplicateField,
                field.name.span,
            ));
            continue;
        }
        fields.push(ClassFieldInfo {
            name: field.name.text.clone(),
            ty: field.ty.clone(),
            span: field.span,
        });
    }

    ClassInfo {
        name: class.name.text.clone(),
        local: class.local,
        fields,
        has_check: !class.checks.is_empty(),
        span: class.span,
        ast_index,
    }
}

fn collect_enum_info(
    enum_decl: &EnumDecl,
    ast_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> EnumInfo {
    let mut seen = HashMap::<String, Span>::new();
    let mut variants = Vec::new();
    let mut next_value = 0i64;
    for variant in &enum_decl.variants {
        if seen
            .insert(variant.name.text.clone(), variant.name.span)
            .is_some()
        {
            diagnostics.push(Diagnostic::Sema(
                SemaErrorKind::DuplicateVariant,
                variant.name.span,
            ));
            continue;
        }

        let value = variant.value.unwrap_or(next_value);
        next_value = value.saturating_add(1);
        variants.push(EnumVariantInfo {
            name: variant.name.text.clone(),
            value,
            span: variant.span,
        });
    }

    let _ = VariantId(0);
    EnumInfo {
        name: enum_decl.name.text.clone(),
        local: enum_decl.local,
        variants,
        span: enum_decl.span,
        ast_index,
    }
}
