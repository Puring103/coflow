use crate::{CftDiagnostic, CftDiagnostics, CftErrorCode};
use coflow_language::cft::syntax::ast::*;
use coflow_language::cft::{CftModuleSet, ModuleId};
use std::collections::{BTreeMap, BTreeSet};

/// 名称解析只改写编译副本；导入不会改变源码、模块路径或记录 key。
pub(super) fn resolve(modules: &CftModuleSet) -> Result<CftModuleSet, CftDiagnostics> {
    let mut names = BTreeSet::new();
    for (_, module) in modules.modules() {
        if let Some(ast) = module.ast() {
            let ns = ast
                .namespace
                .as_ref()
                .map(NamePath::canonical)
                .unwrap_or_default();
            for item in &ast.items {
                names.insert(qualify(&ns, item_name(item)));
            }
        }
    }
    let mut diagnostics = Vec::new();
    let resolved = modules.map_asts(|id, ast| {
        let ns = ast
            .namespace
            .as_ref()
            .map(NamePath::canonical)
            .unwrap_or_default();
        if ns.split("::").next() == Some("Coflow") {
            diagnostics.push(diag(
                id,
                ast.namespace
                    .as_ref()
                    .map_or(crate::Span::default(), |p| p.span),
                "Coflow namespace is reserved",
            ));
        }
        let mut imports = BTreeMap::new();
        for import in &ast.imports {
            let full = import.path.canonical();
            let short = full.rsplit("::").next().unwrap_or(&full).to_string();
            let member = full
                .rsplit_once("::")
                .is_some_and(|(owner, _)| names.contains(owner));
            let system = matches!(
                full.as_str(),
                "Coflow::Check::require" | "Coflow::Check::records"
            );
            if (!names.contains(&full) && !member && !system)
                || (names.contains(&full) && member)
                || imports.contains_key(&short)
                || names.contains(&qualify(&ns, &short))
            {
                diagnostics.push(diag(
                    id,
                    import.path.span,
                    "unknown, ambiguous or conflicting import",
                ));
            }
            imports.insert(short, full);
        }
        let resolve = |name: &str| -> String {
            let (head, tail) = name.split_once("::").map_or((name, ""), |(a, b)| (a, b));
            if let Some(import) = imports.get(head) {
                return if tail.is_empty() {
                    import.clone()
                } else {
                    format!("{import}::{tail}")
                };
            }
            let local = qualify(&ns, name);
            if names.contains(&local) || names.contains(&qualify(&ns, head)) {
                local
            } else {
                name.to_string()
            }
        };
        for item in &mut ast.items {
            match item {
                Item::Type(def) => {
                    def.name = qualify(&ns, &def.name);
                    if let Some(parent) = &mut def.parent {
                        parent.name = resolve(&parent.name);
                    }
                    let context = (def.kind != TypeKind::Data).then_some(def.name.as_str());
                    annotations(&mut def.annotations, &resolve);
                    for field in &mut def.fields {
                        ty(&mut field.ty, &resolve);
                        annotations(&mut field.annotations, &resolve);
                        if let Some(default) = &mut field.default {
                            value(default, context, &resolve);
                        }
                    }
                }
                Item::Enum(def) => {
                    def.name = qualify(&ns, &def.name);
                }
                Item::TypeAlias(def) => {
                    def.name = qualify(&ns, &def.name);
                    ty(&mut def.target, &resolve);
                }
                Item::Const(def) => {
                    def.name = qualify(&ns, &def.name);
                    if let Some(t) = &mut def.ty {
                        ty(t, &resolve);
                    }
                    value(&mut def.value, None, &resolve);
                }
                Item::Check(def) => {
                    def.name = qualify(&ns, &def.name);
                }
            }
        }
    });
    if diagnostics.is_empty() {
        Ok(resolved)
    } else {
        Err(CftDiagnostics::new(diagnostics))
    }
}

fn diag(id: &ModuleId, span: crate::Span, message: &str) -> CftDiagnostic {
    CftDiagnostic::error(CftErrorCode::UnknownNamedType, id.clone(), span, message)
}
fn item_name(item: &Item) -> &str {
    match item {
        Item::Type(d) => &d.name,
        Item::Enum(d) => &d.name,
        Item::Const(d) => &d.name,
        Item::TypeAlias(d) => &d.name,
        Item::Check(d) => &d.name,
    }
}
fn qualify(ns: &str, name: &str) -> String {
    if ns.is_empty() {
        name.to_string()
    } else {
        format!("{ns}::{name}")
    }
}
fn annotations(items: &mut [Annotation], resolve: &impl Fn(&str) -> String) {
    for item in items {
        for arg in &mut item.args {
            if let AnnotationArg::Name(name) = arg {
                name.name = resolve(&name.name);
            }
        }
    }
}
fn ty(t: &mut TypeRef, resolve: &impl Fn(&str) -> String) {
    match &mut t.kind {
        TypeRefKind::Named(name) => *name = resolve(name),
        TypeRefKind::Array(t) | TypeRefKind::Option(t) => ty(t, resolve),
        TypeRefKind::Dict(k, v) => {
            ty(k, resolve);
            ty(v, resolve);
        }
        TypeRefKind::Function(args, result) => {
            for a in args {
                ty(&mut a.value_type, resolve);
            }
            ty(result, resolve);
        }
        _ => {}
    }
}
fn rewrite(path: &mut NamePath, full: String) {
    path.segments = full
        .split("::")
        .map(|name| NameRef {
            name: name.to_string(),
            span: path.span,
        })
        .collect();
}
fn value(v: &mut DefaultExpr, context: Option<&str>, resolve: &impl Fn(&str) -> String) {
    match &mut v.kind {
        DefaultExprKind::RecordReference(path) => {
            let original = path.canonical();
            let full = if let Some((owner, key)) = original.rsplit_once("::") {
                format!("{}::{key}", resolve(owner))
            } else if let Some(owner) = context {
                format!("{owner}::{original}")
            } else {
                original
            };
            rewrite(path, full);
        }
        DefaultExprKind::StaticPath(path) => rewrite(path, resolve(&path.canonical())),
        DefaultExprKind::TypedObject { type_name, fields } => {
            rewrite(type_name, resolve(&type_name.canonical()));
            for (_, v) in fields {
                value(v, None, resolve);
            }
        }
        DefaultExprKind::Object(fields) => {
            for (_, v) in fields {
                value(v, None, resolve);
            }
        }
        DefaultExprKind::Array(items) => {
            for v in items {
                value(v, context, resolve);
            }
        }
        DefaultExprKind::Dictionary(items) => {
            for (k, v) in items {
                value(k, context, resolve);
                value(v, context, resolve);
            }
        }
        DefaultExprKind::BitExpr { lhs, rhs, .. } => {
            value(lhs, context, resolve);
            value(rhs, context, resolve);
        }
        DefaultExprKind::Function { signature, .. } => ty(signature, resolve),
        _ => {}
    }
}
