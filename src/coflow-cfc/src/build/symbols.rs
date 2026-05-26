use super::{BuildCtx, EnumInfo, TypeInfo, UnionInfo};
use crate::ast::{EnumDef, Expr, ExprKind, Item, TypeDef, TypeName, TypeRef};
use crate::build::support::build_error;
use crate::container::ModuleId;
use crate::error::{BuildError, BuildErrorKind};
use crate::span::Span;
use std::collections::{HashMap, HashSet};

impl BuildCtx<'_> {
    pub(super) fn collect_symbols(&mut self) {
        let module_ids = self.module_ids.clone();
        for module_id in &module_ids {
            let Some(module) = self.container.modules.get(module_id) else {
                self.errors
                    .push(build_error(format!("unknown module `{module_id}`")));
                continue;
            };
            let mut local_names = HashSet::new();
            for import in &module.imports {
                if !local_names.insert(import.alias.clone()) {
                    self.errors.push(BuildError::new(
                        BuildErrorKind::DuplicateName,
                        format!("duplicate name `{}`", import.alias),
                        Some(import.span),
                    ));
                }
            }
            for item in &module.ast.items {
                match item {
                    Item::Type(def) => {
                        if !local_names.insert(def.name.clone()) {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::DuplicateName,
                                format!("duplicate name `{}`", def.name),
                                Some(def.span),
                            ));
                        }
                        if let Some(alias) = &def.alias {
                            if let Some(branches) = union_branches(alias) {
                                self.symbols.unions.insert(
                                    (module_id.clone(), def.name.clone()),
                                    UnionInfo {
                                        module: module_id.clone(),
                                        name: def.name.clone(),
                                        branches,
                                    },
                                );
                            } else {
                                self.errors.push(BuildError::new(
                                    BuildErrorKind::UnknownType,
                                    format!("union `{}` must alias named types", def.name),
                                    Some(def.span),
                                ));
                            }
                        } else {
                            self.symbols.types.insert(
                                (module_id.clone(), def.name.clone()),
                                TypeInfo {
                                    module: module_id.clone(),
                                    def: def.clone(),
                                },
                            );
                        }
                    }
                    Item::Enum(def) => {
                        if !local_names.insert(def.name.clone()) {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::DuplicateName,
                                format!("duplicate name `{}`", def.name),
                                Some(def.span),
                            ));
                        }
                        let info = self.build_enum(def);
                        self.symbols
                            .enums
                            .insert((module_id.clone(), def.name.clone()), info);
                    }
                    Item::Data(def) => {
                        if !local_names.insert(def.name.clone()) {
                            self.errors.push(BuildError::new(
                                BuildErrorKind::DuplicateName,
                                format!("duplicate name `{}`", def.name),
                                Some(def.span),
                            ));
                        }
                        self.symbols
                            .data
                            .insert((module_id.clone(), def.name.clone()), def.clone());
                    }
                    Item::Check(block) => {
                        let _ = block.span;
                    }
                }
            }
        }
        self.validate_type_defs();
    }

    fn validate_type_defs(&mut self) {
        let defs: Vec<_> = self
            .symbols
            .types
            .values()
            .map(|info| (info.module.clone(), info.def.clone()))
            .collect();
        for (module, def) in defs {
            self.validate_type_def(&module, &def);
        }
        let unions: Vec<_> = self.symbols.unions.values().cloned().collect();
        for union in unions {
            self.validate_union_def(&union);
        }
    }

    fn build_enum(&mut self, def: &EnumDef) -> EnumInfo {
        let mut next = 0;
        let mut used = HashSet::new();
        let mut values = HashMap::new();
        let mut names = HashSet::new();
        for variant in &def.variants {
            let value = variant.value.unwrap_or(next);
            if !names.insert(variant.name.clone()) {
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateEnumVariant,
                    format!("duplicate enum variant `{}`", variant.name),
                    Some(variant.span),
                ));
            }
            if !used.insert(value) {
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateEnumValue,
                    format!("duplicate enum value `{value}`"),
                    Some(variant.span),
                ));
            }
            values.insert(variant.name.clone(), value);
            next = value + 1;
        }
        EnumInfo { values }
    }

    fn validate_type_def(&mut self, module: &ModuleId, def: &TypeDef) {
        let mut names = HashSet::new();
        for field in &def.fields {
            if !names.insert(field.name.clone()) {
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateField,
                    format!("duplicate field `{}`", field.name),
                    Some(field.span),
                ));
            }
            self.validate_type_ref(module, &field.ty, field.span);
            if let Some(default) = &field.default {
                if !self.is_default_constant(module, default) {
                    self.errors.push(BuildError::new(
                        BuildErrorKind::InvalidDefault,
                        format!(
                            "default value for field `{}` must be a constant",
                            field.name
                        ),
                        Some(default.span),
                    ));
                }
            }
        }
    }

    fn validate_union_def(&mut self, union: &UnionInfo) {
        let mut seen = HashSet::new();
        for branch in &union.branches {
            let Some((target_module, target_name)) =
                self.resolve_type_name(&union.module, branch, Span::new(0, 0))
            else {
                continue;
            };
            if !seen.insert((target_module.clone(), target_name.clone())) {
                self.errors.push(BuildError::new(
                    BuildErrorKind::DuplicateName,
                    format!("duplicate union branch `{target_name}`"),
                    None,
                ));
            }
            if !self
                .symbols
                .types
                .contains_key(&(target_module.clone(), target_name.clone()))
            {
                self.errors.push(BuildError::new(
                    BuildErrorKind::UnknownType,
                    format!(
                        "union `{}` branch `{target_name}` must name a type",
                        union.name
                    ),
                    None,
                ));
            }
        }
    }

    fn is_default_constant(&self, module: &ModuleId, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::String(_) => true,
            ExprKind::Qualified(parts) => self.is_enum_constant(module, parts),
            ExprKind::Array(items) => items
                .iter()
                .all(|item| self.is_default_constant(module, item)),
            ExprKind::Object(fields) => fields
                .iter()
                .all(|field| self.is_default_constant(module, &field.value)),
            ExprKind::TypedObject { fields, .. } => fields
                .iter()
                .all(|field| self.is_default_constant(module, &field.value)),
            ExprKind::Dict(entries) => entries.iter().all(|(key, value)| {
                self.is_default_constant(module, key) && self.is_default_constant(module, value)
            }),
            ExprKind::Name(_) | ExprKind::Path { .. } => false,
        }
    }

    fn is_enum_constant(&self, module: &ModuleId, parts: &[String]) -> bool {
        match parts {
            [enum_name, variant] => self
                .symbols
                .enums
                .get(&(module.clone(), enum_name.clone()))
                .is_some_and(|info| info.values.contains_key(variant)),
            [alias, enum_name, variant] => self
                .try_resolve_import(module, alias)
                .and_then(|dep| self.symbols.enums.get(&(dep, enum_name.clone())))
                .is_some_and(|info| info.values.contains_key(variant)),
            _ => false,
        }
    }

    fn validate_type_ref(&mut self, module: &ModuleId, ty: &TypeRef, span: Span) {
        match ty {
            TypeRef::Int
            | TypeRef::Float
            | TypeRef::Bool
            | TypeRef::String
            | TypeRef::StringLiteral(_)
            | TypeRef::Any => {}
            TypeRef::Array(inner) => self.validate_type_ref(module, inner, span),
            TypeRef::Dict(key, value) => {
                self.validate_dict_key_type(module, key, span);
                self.validate_type_ref(module, value, span);
            }
            TypeRef::Union(items) => {
                for item in items {
                    self.validate_type_ref(module, item, span);
                }
            }
            TypeRef::Named(name) => {
                if let Some((target_module, target_name)) =
                    self.resolve_type_name(module, name, span)
                {
                    if !self
                        .symbols
                        .types
                        .contains_key(&(target_module.clone(), target_name.clone()))
                        && !self
                            .symbols
                            .unions
                            .contains_key(&(target_module.clone(), target_name.clone()))
                        && !self
                            .symbols
                            .enums
                            .contains_key(&(target_module, target_name.clone()))
                    {
                        self.errors.push(BuildError::new(
                            BuildErrorKind::UnknownType,
                            format!("unknown type `{target_name}`"),
                            Some(span),
                        ));
                    }
                }
            }
        }
    }

    fn validate_dict_key_type(&mut self, module: &ModuleId, ty: &TypeRef, span: Span) {
        match ty {
            TypeRef::String | TypeRef::Int | TypeRef::StringLiteral(_) => {}
            TypeRef::Named(name) => {
                if let Some((target_module, target_name)) =
                    self.resolve_type_name(module, name, span)
                {
                    if !self
                        .symbols
                        .enums
                        .contains_key(&(target_module.clone(), target_name.clone()))
                    {
                        self.errors.push(BuildError::new(
                            BuildErrorKind::InvalidDictKeyType,
                            format!("dict key type `{target_name}` must be string, int, or enum"),
                            Some(span),
                        ));
                    }
                }
            }
            _ => {
                self.errors.push(BuildError::new(
                    BuildErrorKind::InvalidDictKeyType,
                    "dict key type must be string, int, or enum",
                    Some(span),
                ));
            }
        }
    }
}

fn union_branches(alias: &TypeRef) -> Option<Vec<TypeName>> {
    let TypeRef::Union(items) = alias else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            TypeRef::Named(name) => Some(name.clone()),
            _ => None,
        })
        .collect()
}
